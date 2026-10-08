//! Voice acknowledgements belong to the capture/actor that scheduled them.
use super::*;
use bumblebee_audio::SignalKey;
use std::sync::Weak;

// Legacy Nucleus listened for 12 seconds plus a 4-second no-speech allowance.
const NO_SPEECH_LIMIT: Duration = Duration::from_secs(16);
const POST_SPEECH_STALL_LIMIT: Duration = Duration::from_secs(20);
const FINALIZE_RECEIPT_LIMIT: Duration = Duration::from_secs(2);
const CAPTURE_LIMIT: Duration = Duration::from_secs(120);
// Match Nucleus trimVoicePreroll: only the last 1500ms of the keyword
// detector's 3-second rolling buffer belongs in the voice-command handoff.
const WAKE_PREROLL_BYTES: usize = 1_500 * 16_000 * 2 / 1_000;

pub(super) fn expired_captures(
	captures: &mut HashMap<String, Capture>,
	now: tokio::time::Instant,
) -> Vec<(String, Capture, bool)> {
	let expired: Vec<_> = captures
		.iter()
		.filter_map(|(user, capture)| {
			let cancelled = capture.cancel.is_cancelled() || capture.actor_scope.cancel.is_cancelled();
			let age = now.saturating_duration_since(capture.started_at);
			let stalled = capture
				.forced_finalize_at
				.map(|requested| now.saturating_duration_since(requested) >= FINALIZE_RECEIPT_LIMIT)
				.unwrap_or(capture.first_voice_at.is_none() && age >= NO_SPEECH_LIMIT);
			(cancelled || stalled || age >= CAPTURE_LIMIT).then(|| (user.clone(), !cancelled))
		})
		.collect();
	expired
		.into_iter()
		.filter_map(|(user, timed_out)| {
			captures
				.remove(&user)
				.map(|capture| (user, capture, timed_out))
		})
		.collect()
}

/// The original deadline is 20s after the first voiced frame, not 20s of silence.
/// Ask the native owner to finalize once so useful audio reaches transcription.
pub(super) fn finalize_due_captures(
	captures: &mut HashMap<String, Capture>,
	now: tokio::time::Instant,
) -> Vec<String> {
	captures
		.iter_mut()
		.filter_map(|(user, capture)| {
			if capture.cancel.is_cancelled()
				|| capture.actor_scope.cancel.is_cancelled()
				|| capture.forced_finalize_at.is_some()
			{
				return None;
			}
			if capture
				.first_voice_at
				.is_some_and(|first| now.saturating_duration_since(first) >= POST_SPEECH_STALL_LIMIT)
			{
				capture.forced_finalize_at = Some(now);
				Some(user.clone())
			} else {
				None
			}
		})
		.collect()
}

impl Capture {
	pub(super) fn seed_wake_preroll(&mut self, verification_pcm: &[u8]) {
		if !self.pcm.is_empty()
			|| self.cancel.is_cancelled()
			|| self.actor_scope.cancel.is_cancelled()
		{
			return;
		}
		// Native keyword audio is PCM16 mono at 16kHz. Keep whole samples even
		// if a malformed event has a trailing byte; never pad or synthesize audio.
		let end = verification_pcm.len() & !1;
		let start = end.saturating_sub(WAKE_PREROLL_BYTES);
		self.pcm.extend_from_slice(&verification_pcm[start..end]);
		// This buffer includes the wake phrase itself. Do not count it as new
		// post-wake VAD or extend the no-speech/first-speech deadlines.
	}

	pub(super) fn note_voice_activity(&mut self, active: bool, now: tokio::time::Instant) {
		if active && self.first_voice_at.is_none() {
			self.first_voice_at = Some(now);
		}
	}
}

impl Engine {
	pub(super) async fn accept_voice_wake(
		&self,
		user_id: &str,
		username: String,
		listen_allowed: bool,
		requests: &mut HashMap<String, Weak<queue::ActorScope>>,
	) -> Result<Option<Capture>> {
		if !listen_allowed || !self.voice_agent_enabled(user_id)? {
			return Ok(None);
		}
		requests.retain(|_, request| {
			request
				.upgrade()
				.is_some_and(|scope| !scope.cancel.is_cancelled())
		});
		// Repeated keyword hits never start a second request for the same ongoing turn.
		if requests.contains_key(user_id) {
			return Ok(None);
		}
		let owner = user_id == self.store.settings()?.owner_discord_id;
		let playback_active = self.speech_lock.try_lock().is_err();
		if !owner && (!requests.is_empty() || playback_active) {
			return Ok(None);
		}
		if owner && (!requests.is_empty() || playback_active) {
			for other in requests.keys() {
				self.cancel_actor(&format!("discord:{other}")).await;
			}
			requests.clear();
			// Wake interrupts voice/readout, never unrelated platform tool requests.
			self.cancel_signals();
			if let Some(session) = self.session.lock().await.as_mut() {
				session.speech_cancel.cancel();
				session.speech_cancel = session.cancel.child_token();
			}
			self.preview_cancel.lock().await.cancel();
			if let Some(audio) = self.audio().await {
				audio.interrupt().await;
			}
			self.emit(OverlayEvent::StopSpeech);
		}
		let capture = self
			.begin_voice_capture(user_id, username, listen_allowed)
			.await?;
		if let Some(capture) = &capture {
			requests.insert(user_id.into(), Arc::downgrade(&capture.actor_scope));
		}
		Ok(capture)
	}

	pub(super) fn voice_cue(&self, key: SignalKey, capture: &Capture) -> Result<signals::CuePlan> {
		self.cue_plan(
			key,
			capture.cancel.clone(),
			Some(capture.actor_scope.clone()),
			true,
			None,
		)
	}

	pub(super) async fn cancel_voice_cue(
		&self,
		user_id: &str,
	) -> Result<Option<(signals::CuePlan, CancellationToken)>> {
		let mut session = self.session.lock().await;
		let Some(session) = session
			.as_mut()
			.filter(|session| !session.cancel.is_cancelled())
		else {
			return Ok(None);
		};
		// Mint acknowledgement ownership only after the interrupted scopes were invalidated.
		let token = session.voice_cancel.child_token();
		let actor = session.agent_scopes.lease(&format!("discord:{user_id}"));
		Ok(Some((
			self.cue_plan(
				SignalKey::CancelChirp,
				token.clone(),
				Some(actor),
				true,
				None,
			)?,
			token,
		)))
	}

	pub(super) fn spawn_voice_cue(self: &Arc<Self>, jobs: &mut JoinSet<()>, cue: signals::CuePlan) {
		// Do not grow an unbounded queue of delayed acknowledgements during keyword bursts.
		if jobs.len() >= 8 {
			return;
		}
		let engine = self.clone();
		jobs.spawn(async move {
			if let Err(error) = engine.play_voice_cue(cue).await {
				engine.emit(OverlayEvent::Status {
					message: error.to_string(),
				});
			}
		});
	}

	pub(super) async fn play_voice_cue(&self, cue: signals::CuePlan) -> Result<()> {
		self.play_planned_cue(cue).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn capture() -> Capture {
		Capture {
			username: "Owner".into(),
			pcm: vec![],
			cancel: CancellationToken::new(),
			actor_scope: Arc::new(queue::ActorScope {
				cancel: CancellationToken::new(),
			}),
			started_at: tokio::time::Instant::now(),
			first_voice_at: None,
			forced_finalize_at: None,
		}
	}
	fn pcm(sample: i16, milliseconds: usize) -> Vec<u8> {
		(0..milliseconds * 16)
			.flat_map(|_| sample.to_le_bytes())
			.collect()
	}
	#[test]
	fn delayed_wake_handoff_preserves_immediate_command_audio_and_legacy_tail_limit() {
		let preceding_conversation = pcm(101, 1500);
		let full_wake_phrase = pcm(202, 1000);
		let immediate_command_words = pcm(303, 500);
		let post_detection_stream = pcm(404, 1000);
		// The asynchronous recognizer reports the wake after the beginning of
		// the command has already entered its rolling buffer.
		let verification = [
			preceding_conversation,
			full_wake_phrase.clone(),
			immediate_command_words.clone(),
		]
		.concat();
		let mut capture = capture();
		capture.seed_wake_preroll(&verification);
		capture.pcm.extend_from_slice(&post_detection_stream);
		assert_eq!(
			capture.pcm,
			[
				full_wake_phrase,
				immediate_command_words,
				post_detection_stream
			]
			.concat()
		);
		assert_eq!(capture.pcm.len(), 16_000 * 2 * 2500 / 1000);
		assert!(
			capture.first_voice_at.is_none(),
			"The wake phrase alone must not restart VAD deadlines"
		);
		// Repeated handoff cannot overwrite live frames or duplicate the phrase.
		let expected = capture.pcm.clone();
		capture.seed_wake_preroll(&verification);
		assert_eq!(capture.pcm, expected);
	}
	#[test]
	fn short_preroll_is_complete_bounded_sample_aligned_and_cancelled_captures_keep_nothing() {
		let mut short = capture();
		let pcm = pcm(505, 300);
		short.seed_wake_preroll(&pcm);
		assert_eq!(short.pcm, pcm);
		let mut malformed = capture();
		malformed.seed_wake_preroll(&[1, 2, 3]);
		assert_eq!(malformed.pcm, vec![1, 2]);
		for actor_cancel in [false, true] {
			let mut stopped = capture();
			if actor_cancel {
				stopped.actor_scope.cancel.cancel();
			} else {
				stopped.cancel.cancel();
			}
			stopped.seed_wake_preroll(&pcm);
			assert!(stopped.pcm.is_empty());
		}
	}
}
