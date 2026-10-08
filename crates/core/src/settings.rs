//! Retained streamer controls and the policy shared by chat, voice and the agent.
use crate::model::{ChatMessage, Settings};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ChatAccess {
	pub role_ids: Vec<String>,
	pub moderator: bool,
	pub subscriber: bool,
	pub vip: bool,
	pub member: bool,
	pub follower: Option<bool>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct AudiencePolicy {
	pub everyone: bool,
	pub role_ids: Vec<String>,
	pub followers: bool,
	pub subscribers: bool,
	pub vips: bool,
	pub moderators: bool,
	pub members: bool,
}
impl AudiencePolicy {
	pub fn permits(&self, access: &ChatAccess) -> bool {
		self.everyone
			|| self.moderators && access.moderator
			|| self.subscribers && access.subscriber
			|| self.vips && access.vip
			|| self.members && access.member
			|| self.followers && access.follower == Some(true)
			|| self.role_ids.iter().any(|id| access.role_ids.contains(id))
	}
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PlatformPolicy {
	pub monitor: bool,
	pub relay: bool,
	pub mentions: AudiencePolicy,
	pub readout: AudiencePolicy,
}
impl Default for PlatformPolicy {
	fn default() -> Self {
		Self {
			monitor: true,
			relay: false,
			mentions: AudiencePolicy::default(),
			readout: AudiencePolicy {
				everyone: true,
				..Default::default()
			},
		}
	}
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ChatPlatforms {
	pub discord: PlatformPolicy,
	pub twitch: PlatformPolicy,
	pub youtube: PlatformPolicy,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Override {
	#[default]
	Inherit,
	Allow,
	Block,
}
impl Override {
	pub fn permits(self, inherited: bool) -> bool {
		match self {
			Self::Inherit => inherited,
			Self::Allow => true,
			Self::Block => false,
		}
	}
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ChatterOverrides {
	pub chat_puppet: Override,
	pub relay: Override,
	pub tts_wait: Override,
	pub ai_access: Override,
	pub text_model: Option<String>,
	pub voice_model: Option<String>,
}
fn model_name(value: &str) -> bool {
	value.len() <= 200
		&& value
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
fn ids(values: &[String]) -> bool {
	values.len() <= 200 && values.iter().all(|v| v.parse::<u64>().is_ok_and(|n| n > 0))
}
impl ChatterOverrides {
	pub fn validate(&self) -> Result<()> {
		for model in [&self.text_model, &self.voice_model].into_iter().flatten() {
			ensure!(
				!model.is_empty() && model_name(model),
				"Invalid per-user model name"
			);
		}
		Ok(())
	}
}
impl Settings {
	pub fn platform_policy(&self, platform: &str) -> Option<&PlatformPolicy> {
		match platform {
			"discord" | "discord_voice" => Some(&self.chat_platforms.discord),
			"twitch" => Some(&self.chat_platforms.twitch),
			"youtube" => Some(&self.chat_platforms.youtube),
			_ => None,
		}
	}
	pub fn permits_ai(
		&self,
		source: &ChatMessage,
		overrides: &ChatterOverrides,
		owner: bool,
	) -> bool {
		if !self.ai_enabled {
			return false;
		}
		if source.platform == "preview" {
			return owner;
		}
		let inherited = if source.platform == "discord_voice" {
			// Native voice admission is revalidated independently at every agent boundary.
			self.voice_mentions_enabled
				&& !self
					.discord_listen_blocked_user_ids
					.contains(&source.user_id)
		} else {
			self
				.platform_policy(&source.platform)
				.is_some_and(|p| p.monitor && (owner || p.mentions.permits(&source.access)))
		};
		// Per-user AI allow cannot bypass the master switch, voice consent or monitoring.
		let source_enabled = if source.platform == "discord_voice" {
			self.voice_mentions_enabled && inherited
		} else {
			self
				.platform_policy(&source.platform)
				.is_some_and(|p| p.monitor)
		};
		source_enabled && overrides.ai_access.permits(inherited)
	}
	pub fn readout_allowed(
		&self,
		source: &ChatMessage,
		overrides: &ChatterOverrides,
		owner: bool,
	) -> bool {
		self.read_chat
			&& self
				.platform_policy(&source.platform)
				.is_none_or(|p| p.monitor)
			&& overrides.chat_puppet.permits(
				self
					.platform_policy(&source.platform)
					.is_none_or(|p| owner || p.readout.permits(&source.access)),
			) && !self.contains_blocked_word(&source.text)
	}
	pub fn contains_blocked_word(&self, text: &str) -> bool {
		let words: Vec<String> = text
			.split(|c: char| !c.is_alphanumeric() && c != '\'')
			.filter(|w| !w.is_empty())
			.map(str::to_lowercase)
			.collect();
		self.chat_tts_blocked_words.iter().any(|blocked| {
			words
				.iter()
				.any(|word| word == &blocked.trim().to_lowercase())
		})
	}
	pub fn selected_model(&self, source: &ChatMessage, overrides: &ChatterOverrides) -> String {
		if source.platform == "discord_voice" {
			overrides.voice_model.clone().unwrap_or_else(|| {
				if self.openai_voice_model.is_empty() {
					self.openai_model.clone()
				} else {
					self.openai_voice_model.clone()
				}
			})
		} else {
			overrides
				.text_model
				.clone()
				.unwrap_or_else(|| self.openai_model.clone())
		}
	}
	pub fn validate_retained(&self) -> Result<()> {
		ensure!(
			matches!(self.audio_output.as_str(), "overlay" | "discord"),
			"Choose OBS overlay or Discord for audio output"
		);
		for volume in [
			self.master_volume,
			self.bumblebee_tts_volume,
			self.puppet_tts_volume,
			self.wake_chirp_volume,
			self.thinking_sound_volume,
			self.flying_sound_volume,
			self.chat_tts_waiting_tone_volume,
		] {
			ensure!(
				volume.is_finite() && (0.0..=2.0).contains(&volume),
				"Audio volumes must be between 0 and 2"
			);
		}
		ensure!(
			self.chat_tts_speaker_intro_cooldown_seconds <= 600,
			"Speaker introduction cooldown must be at most 600 seconds"
		);
		ensure!(
			self.chat_tts_interrupt_silence_ms <= 10_000,
			"Interrupt patience must be at most 10 seconds"
		);
		ensure!(
			(1_000..=600_000).contains(&self.chat_tts_queue_expiration_ms),
			"Speech queue expiration must be 1–600 seconds"
		);
		ensure!(
			self.chat_tts_blocked_words.len() <= 500
				&& self
					.chat_tts_blocked_words
					.iter()
					.all(|word| !word.trim().is_empty()
						&& word.len() <= 100
						&& !word.chars().any(char::is_whitespace)),
			"Blocked words must be single words, at most 100 bytes each and 500 entries"
		);
		for sensitivity in [
			&self.wake_keyword_sensitivity,
			&self.stop_keyword_sensitivity,
			&self.cancel_keyword_sensitivity,
		] {
			ensure!(
				matches!(sensitivity.as_str(), "strict" | "balanced" | "loose"),
				"Invalid keyword sensitivity"
			);
		}
		for effort in [
			&self.openai_reasoning_effort,
			&self.openai_voice_reasoning_effort,
		] {
			ensure!(
				matches!(
					effort.as_str(),
					"default" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
				),
				"Invalid reasoning effort"
			);
		}
		for model in [
			&self.openai_model,
			&self.openai_voice_model,
			&self.image_model,
			&self.streamer_transcription_model,
			&self.voice_transcription_model,
		] {
			ensure!(model_name(model), "Invalid model name");
		}
		ensure!(
			!self.image_model.is_empty()
				&& !self.streamer_transcription_model.is_empty()
				&& !self.voice_transcription_model.is_empty(),
			"Image and transcription model names cannot be empty"
		);
		for p in [
			&self.chat_platforms.discord,
			&self.chat_platforms.twitch,
			&self.chat_platforms.youtube,
		] {
			ensure!(
				ids(&p.mentions.role_ids) && ids(&p.readout.role_ids),
				"Audience role lists must contain at most 200 numeric IDs"
			);
		}
		Ok(())
	}
}

/// Apply a sparse, strictly known-field patch. Null is the agent's "unchanged" slot.
pub fn merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) -> Result<()> {
	use anyhow::Context;
	for (key, value) in patch
		.as_object()
		.context("Settings patch must be an object")?
	{
		let current = target.get_mut(key).context("Unknown setting in patch")?;
		if value.is_null() {
			continue;
		}
		if current.is_object() && value.is_object() {
			merge_patch(current, value)?;
		} else {
			*current = value.clone();
		}
	}
	Ok(())
}

impl Settings {
	pub fn patched(&self, patch: &serde_json::Value) -> Result<Self> {
		fn contains_null(v: &serde_json::Value) -> bool {
			v.is_null()
				|| v.as_object().is_some_and(|m| m.values().any(contains_null))
				|| v.as_array().is_some_and(|a| a.iter().any(contains_null))
		}
		ensure!(
			!contains_null(patch),
			"Settings patches cannot contain null"
		);
		let mut next = serde_json::to_value(self)?;
		merge_patch(&mut next, patch)?;
		let next: Settings = serde_json::from_value(next)?;
		next.validate()?;
		Ok(next)
	}
}

pub fn tool_enabled(settings: &Settings, name: &str) -> bool {
	match name {
		"researchWeb" => settings.ai_web_search_enabled,
		"analyzeWithCodeInterpreter" => settings.ai_code_interpreter_enabled,
		"generateImage" | "editImage" => settings.ai_image_generation_enabled,
		"rememberMemory" | "listMemories" | "updateMemory" | "deleteMemory" => {
			settings.ai_memories_enabled
		}
		"createReminder" | "listReminders" | "cancelReminder" => settings.ai_reminders_enabled,
		_ => true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	fn source(platform: &str) -> ChatMessage {
		ChatMessage {
			platform: platform.into(),
			user_id: "42".into(),
			display_name: "Viewer".into(),
			message_id: "m".into(),
			channel_id: "room".into(),
			text: "hi bumblebee".into(),
			is_owner: false,
			access: Default::default(),
		}
	}
	#[test]
	fn audience_defaults_and_explicit_blocks_prevent_ungranted_ai_cost() {
		let mut settings = Settings {
			ai_enabled: true,
			..Default::default()
		};
		let mut viewer = source("twitch");
		let mut overrides = ChatterOverrides::default();
		assert!(!settings.permits_ai(&viewer, &overrides, false));
		settings.chat_platforms.twitch.mentions.followers = true;
		assert!(!settings.permits_ai(&viewer, &overrides, false));
		viewer.access.follower = Some(true);
		assert!(settings.permits_ai(&viewer, &overrides, false));
		overrides.ai_access = Override::Block;
		assert!(!settings.permits_ai(&viewer, &overrides, true));
		overrides.ai_access = Override::Allow;
		settings.chat_platforms.twitch.monitor = false;
		assert!(!settings.permits_ai(&viewer, &overrides, true));
		settings.chat_platforms.twitch.monitor = true;
		settings.ai_enabled = false;
		assert!(!settings.permits_ai(&viewer, &overrides, true));
	}
	#[test]
	fn readout_block_words_are_case_insensitive_whole_words_and_never_override_master() {
		let mut settings = Settings {
			chat_tts_blocked_words: vec!["blocked".into()],
			..Default::default()
		};
		let mut viewer = source("youtube");
		let overrides = ChatterOverrides {
			chat_puppet: Override::Allow,
			..Default::default()
		};
		viewer.text = "Unblocked is okay".into();
		assert!(settings.readout_allowed(&viewer, &overrides, false));
		viewer.text = "This is BLOCKED!".into();
		assert!(!settings.readout_allowed(&viewer, &overrides, true));
		viewer.text = "okay".into();
		settings.read_chat = false;
		assert!(!settings.readout_allowed(&viewer, &overrides, true));
	}
	#[test]
	fn partial_patches_preserve_unrelated_fields_and_reject_unknown_nested_keys() {
		let original = Settings {
			twitch_client_id: "client".into(),
			owner_discord_id: "42".into(),
			..Default::default()
		};
		let next = original
			.patched(
				&json!({"chatPlatforms":{"twitch":{"mentions":{"subscribers":true}}},"masterVolume":0.2}),
			)
			.unwrap();
		assert!(next.chat_platforms.twitch.mentions.subscribers);
		assert!(next.chat_platforms.twitch.readout.everyone);
		assert_eq!(next.twitch_client_id, "client");
		assert_eq!(next.owner_discord_id, "42");
		assert!(
			original
				.patched(&json!({"chatPlatforms":{"twitch":{"mentions":{"admin":true}}}}))
				.is_err()
		);
		assert!(original.patched(&json!({"aiEnabled":null})).is_err());
		assert!(original.patched(&json!({"masterVolume":3})).is_err());
	}
	#[test]
	fn transcription_defaults_preserve_existing_caption_choices_and_validate_new_voice_choice() {
		let defaults = Settings::default();
		assert_eq!(defaults.openai_model, "gpt-6-astra");
		assert!(defaults.openai_voice_model.is_empty());
		assert_eq!(defaults.image_model, "gpt-image-2.5-sunburst");
		assert_eq!(defaults.voice_transcription_model, "gpt-transcribe");
		assert_eq!(defaults.streamer_transcription_model, "gpt-transcribe");
		let old: Settings = serde_json::from_value(json!({"streamerTranscriptionModel":"whisper-1","openaiModel":"gpt-5.6-sol","imageModel":"gpt-image-2"})).unwrap();
		assert_eq!(old.openai_model, "gpt-5.6-sol");
		assert_eq!(old.image_model, "gpt-image-2");
		assert_eq!(old.streamer_transcription_model, "whisper-1");
		assert_eq!(old.voice_transcription_model, "gpt-transcribe");
		let selected = old
			.patched(&json!({"voiceTranscriptionModel":"gpt-4o-transcribe"}))
			.unwrap();
		assert_eq!(selected.voice_transcription_model, "gpt-4o-transcribe");
		assert_eq!(selected.streamer_transcription_model, "whisper-1");
		assert!(old.patched(&json!({"voiceTranscriptionModel":""})).is_err());
		assert!(
			old.patched(&json!({"voiceTranscriptionModel":"bad model"}))
				.is_err()
		);
	}

	#[test]
	fn voice_models_and_managed_capabilities_follow_current_settings() {
		let mut s = Settings {
			openai_model: "text".into(),
			openai_voice_model: "voice".into(),
			..Default::default()
		};
		assert_eq!(
			s.selected_model(&source("discord_voice"), &ChatterOverrides::default()),
			"voice"
		);
		assert_eq!(
			s.selected_model(
				&source("twitch"),
				&ChatterOverrides {
					text_model: Some("special".into()),
					..Default::default()
				}
			),
			"special"
		);
		for tool in [
			"researchWeb",
			"analyzeWithCodeInterpreter",
			"generateImage",
			"editImage",
		] {
			assert!(!tool_enabled(&s, tool));
		}
		s.ai_image_generation_enabled = true;
		assert!(tool_enabled(&s, "editImage"));
		s.ai_memories_enabled = false;
		assert!(!tool_enabled(&s, "rememberMemory"));
	}
}
