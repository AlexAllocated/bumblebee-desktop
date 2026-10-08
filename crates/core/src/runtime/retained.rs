use super::*;
use crate::model::Override;
use std::time::Instant;

#[derive(Default)]
pub(super) struct SpeakerIntros {
	users: HashMap<String, (String, Instant)>,
	voices: HashMap<String, (String, Instant)>,
}
impl SpeakerIntros {
	fn should_introduce(&mut self, chatter: &Chatter, cooldown: u32, now: Instant) -> bool {
		self
			.users
			.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(600));
		self
			.voices
			.retain(|_, (_, at)| now.duration_since(*at) < Duration::from_secs(600));
		let actor = format!("{}:{}", chatter.platform, chatter.user_id);
		let distinct = self.users.get(&actor).is_some_and(|(voice, at)| {
			voice == &chatter.voice_id
				&& now.duration_since(*at) < Duration::from_secs(cooldown.into())
		}) && self
			.voices
			.get(&chatter.voice_id)
			.is_some_and(|(user, at)| {
				user == &actor && now.duration_since(*at) < Duration::from_secs(cooldown.into())
			});
		!distinct
	}
	fn record(&mut self, chatter: &Chatter, now: Instant) {
		let actor = format!("{}:{}", chatter.platform, chatter.user_id);
		self
			.users
			.insert(actor.clone(), (chatter.voice_id.clone(), now));
		self.voices.insert(chatter.voice_id.clone(), (actor, now));
	}
}
pub(super) fn sensitivity(value: &str) -> bumblebee_audio::VoiceKeywordSensitivity {
	match value {
		"strict" => bumblebee_audio::VoiceKeywordSensitivity::Strict,
		"loose" => bumblebee_audio::VoiceKeywordSensitivity::Loose,
		_ => bumblebee_audio::VoiceKeywordSensitivity::Balanced,
	}
}
impl Engine {
	/// Apply runtime policy without bouncing unrelated providers or canceling volume changes.
	pub async fn settings_changed(&self, previous: &Settings) -> Result<()> {
		self.apply_settings_change(previous, true).await
	}
	/// A setting tool must return its completed local effect before the turn's next
	/// permission boundary stops it. Canceling its own actor during this await would
	/// lose the known-success receipt. All subsequent dispatches revalidate policy.
	pub(crate) async fn settings_changed_from_agent(&self, previous: &Settings) -> Result<()> {
		self.apply_settings_change(previous, false).await
	}
	async fn apply_settings_change(&self, previous: &Settings, cancel_agents: bool) -> Result<()> {
		let next = self.store.settings()?;
		self.emit(OverlayEvent::AudioSettings {
			settings: next.audio_mix(),
		});
		let policy_changed = previous.ai_enabled != next.ai_enabled
			|| previous.chat_platforms != next.chat_platforms
			|| previous.enabled_tool_groups != next.enabled_tool_groups
			|| previous.ai_web_search_enabled != next.ai_web_search_enabled
			|| previous.ai_code_interpreter_enabled != next.ai_code_interpreter_enabled
			|| previous.ai_image_generation_enabled != next.ai_image_generation_enabled
			|| previous.ai_memories_enabled != next.ai_memories_enabled
			|| previous.ai_reminders_enabled != next.ai_reminders_enabled;
		if policy_changed {
			self.cancel_signals();
			if cancel_agents {
				if let Some(session) = self.session.lock().await.as_mut() {
					let epoch = session.agent_scopes.cancel_all(&session.cancel);
					session.input_epoch.send_replace(epoch);
				}
			}
		}
		if previous.audio_output != next.audio_output {
			self.cancel_signals();
		}
		if previous.read_chat != next.read_chat
			|| previous.chat_platforms != next.chat_platforms
			|| previous.chat_tts_blocked_words != next.chat_tts_blocked_words
			|| previous.audio_output != next.audio_output
		{
			if let Some(session) = self.session.lock().await.as_mut() {
				session.speech_cancel.cancel();
				session.speech_cancel = session.cancel.child_token();
			}
			if let Some(audio) = self.audio().await {
				audio.interrupt().await;
			}
			self.emit(OverlayEvent::StopSpeech);
		}
		if let Some(audio) = self.audio().await {
			let gain = (next.master_volume
				* if self.speech_is_chatter.load(Ordering::SeqCst) {
					next.puppet_tts_volume
				} else {
					next.bumblebee_tts_volume
				})
			.clamp(0., 2.);
			audio.set_speech_gain(gain)?;
			let cue = match self.cue_kind.load(Ordering::SeqCst) {
				2 => next.thinking_sound_volume,
				3 => next.chat_tts_waiting_tone_volume,
				_ => next.wake_chirp_volume,
			};
			audio.set_cue_gain((next.master_volume * cue).clamp(0., 2.))?;
		}
		let voice_changed = previous.discord_voice_channel_id != next.discord_voice_channel_id
			|| previous.discord_listen_everyone != next.discord_listen_everyone
			|| previous.discord_listen_role_ids != next.discord_listen_role_ids
			|| previous.discord_listen_allowed_user_ids != next.discord_listen_allowed_user_ids
			|| previous.discord_listen_blocked_user_ids != next.discord_listen_blocked_user_ids
			|| previous.wake_word != next.wake_word
			|| previous.wake_keyword_sensitivity != next.wake_keyword_sensitivity
			|| previous.stop_keyword_sensitivity != next.stop_keyword_sensitivity
			|| previous.cancel_keyword_sensitivity != next.cancel_keyword_sensitivity
			|| previous.replay_enabled != next.replay_enabled
			|| previous.replay_seconds != next.replay_seconds
			|| previous.voice_mentions_enabled != next.voice_mentions_enabled
			|| previous.voice_captions_enabled != next.voice_captions_enabled
			|| previous.ai_enabled != next.ai_enabled;
		if voice_changed {
			self.clear_caption(&previous.owner_discord_id);
			self.clear_streamer_caption()?;
			self.refresh_voice_settings().await?;
		}
		if previous.chat_tts_speaker_intro_cooldown_seconds
			!= next.chat_tts_speaker_intro_cooldown_seconds
		{
			*self.speaker_intros.lock().await = Default::default();
		}
		Ok(())
	}
	pub(super) async fn run_speech_job(self: &Arc<Self>, job: SpeechJob) -> Result<()> {
		let mut courtesy = false;
		loop {
			if job.cancel.is_cancelled() {
				return Ok(());
			}
			let settings = self.store.settings()?;
			if job.queued_at.elapsed()
				>= Duration::from_millis(settings.chat_tts_queue_expiration_ms.into())
			{
				return Ok(());
			}
			let chatter = self.store.chatter(
				crate::agent_storage::platform(&job.source.platform),
				&job.source.user_id,
			)?;
			if !settings.readout_allowed(&job.source, &chatter.overrides, job.source.is_owner) {
				return Ok(());
			}
			let now = Instant::now();
			let wait = settings.chat_tts_interrupt_silence_ms > 0
				&& self.voice_activity.lock().await.iter().any(|(id, at)| {
					now.duration_since(*at)
						< Duration::from_millis(settings.chat_tts_interrupt_silence_ms.into())
						&& self
							.store
							.chatter("discord", id)
							.is_ok_and(|p| p.overrides.tts_wait != Override::Block)
				});
			if !wait {
				let intro = self.speaker_intros.lock().await.should_introduce(
					&chatter,
					settings.chat_tts_speaker_intro_cooldown_seconds,
					now,
				);
				if intro {
					self
						.speak(
							&format!("{} says:", chatter.display_name),
							None,
							job.cancel.clone(),
						)
						.await?;
				}
				if job.cancel.is_cancelled()
					|| !self.store.settings()?.readout_allowed(
						&job.source,
						&self
							.store
							.chatter(
								crate::agent_storage::platform(&job.source.platform),
								&job.source.user_id,
							)?
							.overrides,
						job.source.is_owner,
					) {
					return Ok(());
				}
				self
					.speak(&job.source.text, Some(chatter.clone()), job.cancel)
					.await?;
				self
					.speaker_intros
					.lock()
					.await
					.record(&chatter, Instant::now());
				return Ok(());
			}
			if !courtesy && settings.chat_tts_waiting_tone_enabled {
				courtesy = true;
				self
					.play_cue(bumblebee_audio::SignalKey::CallWaiting, job.cancel.clone())
					.await?;
			}
			tokio::select! {_=job.cancel.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(40))=>{}}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn chatter(id: &str, voice: &str) -> Chatter {
		Chatter {
			platform: "twitch".into(),
			user_id: id.into(),
			display_name: id.into(),
			voice_id: voice.into(),
			puppet_id: "dandy".into(),
			image_hash: None,
			customization_blocked: false,
			overrides: Default::default(),
		}
	}
	#[test]
	fn shared_voices_and_voice_changes_reintroduce_the_speaker() {
		let mut intros = SpeakerIntros::default();
		let now = Instant::now();
		let a = chatter("alice", "dandy");
		let b = chatter("bob", "dandy");
		assert!(intros.should_introduce(&a, 45, now));
		intros.record(&a, now);
		assert!(!intros.should_introduce(&a, 45, now + Duration::from_secs(5)));
		assert!(intros.should_introduce(&b, 45, now + Duration::from_secs(6)));
		intros.record(&b, now + Duration::from_secs(6));
		assert!(intros.should_introduce(&a, 45, now + Duration::from_secs(7)));
		assert!(intros.should_introduce(
			&chatter("alice", "goblin"),
			45,
			now + Duration::from_secs(8)
		));
		assert!(intros.should_introduce(&b, 45, now + Duration::from_secs(60)));
	}
}

impl Engine {
	pub(super) async fn relay_chat(
		&self,
		source: &ChatMessage,
		chatter: &Chatter,
		cancel: &CancellationToken,
	) -> Result<()> {
		let settings = self.store.settings()?;
		let Some(policy) = settings.platform_policy(&source.platform) else {
			return Ok(());
		};
		if !policy.monitor || !chatter.overrides.relay.permits(policy.relay) {
			return Ok(());
		}
		let label = match source.platform.as_str() {
			"discord" => "Discord",
			"twitch" => "Twitch",
			"youtube" => "YouTube",
			_ => return Ok(()),
		};
		let name: String = source
			.display_name
			.chars()
			.filter(|c| !c.is_control())
			.take(80)
			.collect();
		for target in ["discord", "twitch", "youtube"] {
			if target == source.platform || cancel.is_cancelled() {
				continue;
			}
			let current = self.store.settings()?;
			if !current.platform_policy(target).is_some_and(|p| p.monitor) {
				continue;
			}
			let latest = self.store.chatter(&source.platform, &source.user_id)?;
			if !latest.overrides.relay.permits(
				current
					.platform_policy(&source.platform)
					.is_some_and(|p| p.relay),
			) {
				break;
			}
			let destination = if target == "discord" {
				if current.discord_text_channel_id.is_empty() || self.audio().await.is_none() {
					continue;
				}
				current.discord_text_channel_id
			} else {
				let Some(channel) = self.providers.stream_chat_destination(target) else {
					continue;
				};
				channel
			};
			let limit = match target {
				"twitch" => 500,
				"youtube" => 200,
				_ => 2000,
			};
			let text: String = format!("[{label}] {name}: {}", source.text)
				.chars()
				.take(limit)
				.collect();
			let relay = ChatMessage {
				platform: target.into(),
				channel_id: destination,
				user_id: String::new(),
				display_name: String::new(),
				message_id: String::new(),
				text: String::new(),
				is_owner: false,
				access: Default::default(),
			};
			let result = tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),r=self.send_message(&relay,&text)=>r};
			if let Err(error) = result {
				self.emit(OverlayEvent::Status {
					message: format!(
						"Chat relay to {target} was not confirmed; it will not be retried: {error}"
					),
				});
			}
		}
		Ok(())
	}
	pub async fn chatter_settings_changed(&self, platform: &str, user_id: &str) -> Result<()> {
		self.cancel_actor(&format!("{platform}:{user_id}")).await;
		self.cancel_signals();
		self.clear_streamer_caption()?;
		if let Some(session) = self.session.lock().await.as_mut() {
			session.speech_cancel.cancel();
			session.speech_cancel = session.cancel.child_token();
			session.voice_cancel.cancel();
			session.voice_cancel = session.cancel.child_token();
		}
		Ok(())
	}
}
