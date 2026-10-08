//! Native cue synthesis shared by Discord output and the local OBS renderer.
use super::*;
use bumblebee_audio::SignalKey;
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
	pub(super) async fn play_cue(&self, key: SignalKey, cancel: CancellationToken) -> Result<()> {
		let root = self
			.cue_cancel
			.lock()
			.map_err(|_| anyhow::anyhow!("Cue lock poisoned"))?
			.clone();
		if cancel.is_cancelled() || root.is_cancelled() {
			return Ok(());
		}
		// Cues never interrupt active spoken words or each other.
		let Ok(_speech) = self.speech_lock.try_lock() else {
			return Ok(());
		};
		let settings = self.store.settings()?;
		let (kind, level) = match key {
			SignalKey::ThinkingLoop => ("thinking", settings.thinking_sound_volume),
			SignalKey::CallWaiting => ("waiting", settings.chat_tts_waiting_tone_volume),
			_ => ("wake", settings.wake_chirp_volume),
		};
		let gain = (settings.master_volume * level).clamp(0., 2.);
		if gain == 0.0 {
			return Ok(());
		}
		let wav = bumblebee_audio::signal_wav(key)?;
		let (path, duration) = self.speech.publish_signal(wav).await?;
		if cancel.is_cancelled() || root.is_cancelled() {
			return Ok(());
		}
		let id = uuid::Uuid::new_v4().to_string();
		self.cue_kind.store(
			match kind {
				"thinking" => 2,
				"waiting" => 3,
				_ => 1,
			},
			Ordering::SeqCst,
		);
		let _guard = CueGuard {
			kind: &self.cue_kind,
			events: self.events.clone(),
			id: id.clone(),
		};
		self.emit(OverlayEvent::Signal {
			id,
			audio_path: path,
			gain,
			kind: kind.into(),
			looping: false,
			audible: settings.audio_output == "overlay",
		});
		if settings.audio_output == "discord" {
			if let Some(audio) = self.audio().await {
				if audio.has_voice_presence() {
					tokio::select! {biased;_=root.cancelled()=>{},_=cancel.cancelled()=>{},r=audio.play_signal_gain(key,gain,cancel.clone())=>{r?;}};
				}
			}
		} else {
			tokio::select! {biased;_=root.cancelled()=>{},_=cancel.cancelled()=>{},_=tokio::time::sleep(Duration::from_millis(duration))=>{}};
		}
		Ok(())
	}
	pub(crate) async fn while_thinking<T, F>(
		&self,
		enabled: bool,
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
		let result = async {
			let result = request.await;
			stop.cancel();
			result
		};
		let cues = async {
			// Short requests need no progress sound.
			tokio::select! {biased;_=stop.cancelled()=>return, _=tokio::time::sleep(Duration::from_millis(800))=>{}}
			while !stop.is_cancelled() {
				if self
					.play_cue(SignalKey::ThinkingLoop, stop.clone())
					.await
					.is_err()
				{
					break;
				}
				tokio::select! {biased;_=stop.cancelled()=>break,_=tokio::time::sleep(Duration::from_millis(350))=>{}}
			}
		};
		let (result, ()) = tokio::join!(result, cues);
		result
	}
}
