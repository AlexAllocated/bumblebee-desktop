//! Native cue synthesis shared by Discord output and the local OBS renderer.
use super::*;
use bumblebee_audio::SignalKey;

pub(super) struct CuePlan {
	key: SignalKey,
	cancel: CancellationToken,
	actor: Option<Arc<queue::ActorScope>>,
	epoch: CancellationToken,
	wait: bool,
	variant: Option<String>,
}
struct CueGuard<'a> {
	kind: &'a AtomicU8,
	events: broadcast::Sender<OverlayEvent>,
	id: String,
}
impl Drop for CueGuard<'_> {
	fn drop(&mut self) {
		self.kind.store(0, Ordering::SeqCst);
		let _ = self.events.send(OverlayEvent::StopSignal {
			id: self.id.clone(),
		});
	}
}
impl Engine {
	pub(super) fn cancel_signals(&self) {
		if let Ok(mut token) = self.cue_cancel.lock() {
			token.cancel();
			*token = CancellationToken::new();
		}
	}
	pub(super) fn cue_plan(
		&self,
		key: SignalKey,
		cancel: CancellationToken,
		actor: Option<Arc<queue::ActorScope>>,
		wait: bool,
		variant: Option<String>,
	) -> Result<CuePlan> {
		Ok(CuePlan {
			key,
			cancel,
			actor,
			wait,
			variant,
			epoch: self
				.cue_cancel
				.lock()
				.map_err(|_| anyhow::anyhow!("Cue lock poisoned"))?
				.clone(),
		})
	}
	pub(super) async fn play_cue(&self, key: SignalKey, cancel: CancellationToken) -> Result<()> {
		self
			.play_planned_cue(self.cue_plan(key, cancel, None, false, None)?)
			.await
	}
	pub(super) async fn play_planned_cue(&self, plan: CuePlan) -> Result<()> {
		let actor_cancel = plan
			.actor
			.as_ref()
			.map(|actor| actor.cancel.clone())
			.unwrap_or_default();
		tokio::select! { biased;
			 _=plan.cancel.cancelled()=>Ok(()),
			 _=plan.epoch.cancelled()=>Ok(()),
			 _=actor_cancel.cancelled()=>Ok(()),
			 result=self.play_cue_inner(&plan)=>result,
		}
	}
	async fn play_cue_inner(&self, plan: &CuePlan) -> Result<()> {
		// Acknowledgements may wait briefly for interrupted playback to release its guard.
		// All scopes remain watched during this wait, including before the task's first poll.
		let _speech = if plan.wait {
			match tokio::time::timeout(Duration::from_millis(750), self.speech_lock.lock()).await {
				Ok(lock) => lock,
				Err(_) => return Ok(()),
			}
		} else {
			let Ok(lock) = self.speech_lock.try_lock() else {
				return Ok(());
			};
			lock
		};
		let settings = self.store.settings()?;
		let (kind, level, id, multiplier) = match plan.key {
			SignalKey::WakeChirp => ("wake", settings.wake_chirp_volume, 1, 0.20),
			SignalKey::HeardChirp => ("heard", settings.wake_chirp_volume, 4, 0.32),
			SignalKey::TimeoutChirp => ("timeout", settings.wake_chirp_volume, 5, 1.0),
			SignalKey::CancelChirp => ("cancel", settings.wake_chirp_volume, 6, 1.0),
			SignalKey::ThinkingLoop => ("thinking", settings.thinking_sound_volume, 2, 1.0),
			SignalKey::CallWaiting => ("waiting", settings.chat_tts_waiting_tone_volume, 3, 1.0),
		};
		let gain = (settings.master_volume * level * multiplier).clamp(0., 2.);
		if gain == 0.0 {
			return Ok(());
		}
		let repeats = if plan.key == SignalKey::ThinkingLoop {
			2
		} else {
			1
		};
		let wav =
			bumblebee_audio::signal_wav_with_options(plan.key, repeats, plan.variant.as_deref())?;
		let (path, duration) = self.speech.publish_signal(wav).await?;
		// Cancellation may have won while the filesystem publication worker was running.
		if plan.cancel.is_cancelled()
			|| plan.epoch.is_cancelled()
			|| plan.actor.as_ref().is_some_and(|a| a.cancel.is_cancelled())
		{
			return Ok(());
		}
		let signal_id = uuid::Uuid::new_v4().to_string();
		self.cue_kind.store(id, Ordering::SeqCst);
		let _guard = CueGuard {
			kind: &self.cue_kind,
			events: self.events.clone(),
			id: signal_id.clone(),
		};
		self.emit(OverlayEvent::Signal {
			id: signal_id,
			audio_path: path,
			gain,
			kind: kind.into(),
			looping: false,
			audible: settings.audio_output == "overlay",
		});
		if settings.audio_output == "discord" {
			if let Some(audio) = self.audio().await {
				if audio.has_voice_presence() {
					audio
						.play_signal_gain_with_options(
							plan.key,
							gain,
							repeats,
							plan.variant.as_deref(),
							plan.cancel.clone(),
						)
						.await?;
				}
			}
		} else {
			tokio::time::sleep(Duration::from_millis(duration)).await;
		}
		Ok(())
	}
	/// Neutral feedback begins alongside wake/acknowledgement, before speech can
	/// be classified. It never carries request content. Failures use the legacy
	/// timeout sound; canceled/revoked scopes remain silent.
	pub(crate) async fn while_voice_processing<T, F>(
		&self,
		variant: String,
		cancel: CancellationToken,
		request: F,
	) -> Result<T>
	where
		F: std::future::Future<Output = Result<T>>,
	{
		let failure = self.cue_plan(SignalKey::TimeoutChirp, cancel.clone(), None, true, None)?;
		let result = self.while_thinking(true, variant, cancel, request).await;
		if result.is_err() {
			let _ = self.play_planned_cue(failure).await;
		}
		result
	}
	pub(crate) async fn while_thinking<T, F>(
		&self,
		enabled: bool,
		variant: String,
		cancel: CancellationToken,
		request: F,
	) -> Result<T>
	where
		F: std::future::Future<Output = Result<T>>,
	{
		if !enabled {
			return request.await;
		}
		let stop = cancel.child_token();
		let epoch = self
			.cue_cancel
			.lock()
			.map_err(|_| anyhow::anyhow!("Cue lock poisoned"))?
			.clone();
		let result = async {
			let result = request.await;
			stop.cancel();
			result
		};
		let cues = async {
			while !stop.is_cancelled() && !epoch.is_cancelled() {
				let started = tokio::time::Instant::now();
				let plan = CuePlan {
					key: SignalKey::ThinkingLoop,
					cancel: stop.clone(),
					actor: None,
					epoch: epoch.clone(),
					wait: false,
					variant: Some(variant.clone()),
				};
				if self.play_planned_cue(plan).await.is_err() {
					break;
				}
				// Normally the two-clip batch itself paces this loop. If spoken playback
				// owns the clock, wait before retrying instead of spinning.
				if started.elapsed() < Duration::from_millis(100) {
					tokio::select! {_=stop.cancelled()=>break,_=epoch.cancelled()=>break,_=tokio::time::sleep(Duration::from_millis(100))=>{}};
				}
				if self
					.store
					.settings()
					.map(|s| s.master_volume * s.thinking_sound_volume == 0.)
					.unwrap_or(true)
				{
					break;
				}
			}
		};
		let (result, ()) = tokio::join!(result, cues);
		result
	}
}
