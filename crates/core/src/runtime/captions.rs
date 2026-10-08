//! Explicit streamer captions never grant guest transcription or agent invocation.
use super::*;
use crate::model::OverlaySettings;

pub(super) struct CaptionCapture {
	pub pcm: Vec<u8>,
	pub cancel: CancellationToken,
}
fn permitted(settings: &Settings, overlay: &OverlaySettings, user_id: &str) -> bool {
	settings.ai_enabled
		&& settings.voice_captions_enabled
		&& !settings.owner_discord_id.is_empty()
		&& user_id == settings.owner_discord_id
		&& !settings
			.discord_listen_blocked_user_ids
			.iter()
			.any(|id| id == user_id)
		&& overlay.streamer_voice_bubble.enabled
}
impl Engine {
	pub(super) fn clear_caption(&self, user_id: &str) {
		self.emit(OverlayEvent::VoiceTranscript {
			user_id: user_id.into(),
			is_owner: true,
			text: String::new(),
			r#final: true,
		});
	}
	pub fn clear_streamer_caption(&self) -> Result<()> {
		if let Ok(mut token) = self.caption_cancel.lock() {
			token.cancel();
			*token = CancellationToken::new();
		}
		self.clear_caption(&self.store.settings()?.owner_discord_id);
		Ok(())
	}
	pub fn overlay_settings_changed(&self, saved: &OverlaySettings) -> Result<()> {
		if !saved.streamer_voice_bubble.enabled {
			self.clear_streamer_caption()?;
		}
		Ok(())
	}

	pub(super) fn caption_allowed_cached(
		&self,
		audio: &AudioRuntime,
		user_id: &str,
	) -> Result<bool> {
		if !audio.participants().iter().any(|p| p.user_id == user_id) {
			return Ok(false);
		}
		self.caption_allowed(user_id)
	}
	fn caption_allowed(&self, user_id: &str) -> Result<bool> {
		Ok(permitted(
			&self.store.settings()?,
			&self.store.get("overlay_settings")?.unwrap_or_default(),
			user_id,
		))
	}
	pub(super) async fn begin_caption(
		&self,
		audio: &AudioRuntime,
		user_id: &str,
		_username: &str,
	) -> Result<Option<CaptionCapture>> {
		if !self.caption_allowed_cached(audio, user_id)? {
			return Ok(None);
		}
		let session = self.session.lock().await;
		let Some(_session) = session.as_ref().filter(|s| !s.cancel.is_cancelled()) else {
			return Ok(None);
		};
		Ok(Some(CaptionCapture {
			pcm: Vec::new(),
			cancel: self
				.caption_cancel
				.lock()
				.map_err(|_| anyhow::anyhow!("Caption lock poisoned"))?
				.child_token(),
		}))
	}
	pub(super) async fn transcribe_caption(
		&self,
		user_id: String,
		capture: CaptionCapture,
	) -> Result<()> {
		if capture.cancel.is_cancelled() || capture.pcm.len() < 1600 {
			return Ok(());
		}
		let Some(audio) = self.audio().await else {
			return Ok(());
		};
		let member = tokio::select! {_=capture.cancel.cancelled()=>return Ok(()),member=audio.revalidate_participant(&user_id)=>member?};
		if member.is_none() || !self.caption_allowed(&user_id)? {
			return Ok(());
		}
		let model = self.store.settings()?.streamer_transcription_model;
		// Re-read consent after awaited live membership, immediately before the paid request.
		if !self.caption_allowed(&user_id)? {
			return Ok(());
		}
		let text = tokio::select! {biased;_=capture.cancel.cancelled()=>return Ok(()),text=self.providers.transcribe_audio(&model,pcm_wav(&capture.pcm),None,&capture.cancel)=>text?};
		let member = tokio::select! {_=capture.cancel.cancelled()=>return Ok(()),member=audio.revalidate_participant(&user_id)=>member?};
		if capture.cancel.is_cancelled() || member.is_none() || !self.caption_allowed(&user_id)? {
			return Ok(());
		}
		let text = text.trim();
		if !text.is_empty() && text.len() <= 16000 {
			self.emit(OverlayEvent::VoiceTranscript {
				user_id,
				is_owner: true,
				text: text.to_owned(),
				r#final: true,
			});
		}
		Ok(())
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn captions_require_explicit_owner_consent_and_honor_revocation() {
		let mut s = Settings {
			ai_enabled: true,
			voice_captions_enabled: true,
			owner_discord_id: "123".into(),
			..Default::default()
		};
		let mut o = OverlaySettings::default();
		o.streamer_voice_bubble.enabled = true;
		assert!(permitted(&s, &o, "123"));
		assert!(!permitted(&s, &o, "456"));
		s.discord_listen_everyone = true;
		assert!(!permitted(&s, &o, "456"));
		s.discord_listen_blocked_user_ids = vec!["123".into()];
		assert!(!permitted(&s, &o, "123"));
		s.discord_listen_blocked_user_ids.clear();
		s.voice_captions_enabled = false;
		assert!(!permitted(&s, &o, "123"));
		s.voice_captions_enabled = true;
		s.ai_enabled = false;
		assert!(!permitted(&s, &o, "123"));
		s.ai_enabled = true;
		o.streamer_voice_bubble.enabled = false;
		assert!(!permitted(&s, &o, "123"));
		o.streamer_voice_bubble.enabled = true;
		s.owner_discord_id = "456".into();
		assert!(!permitted(&s, &o, "123"));
	}
}
