pub use crate::settings::{
	AudiencePolicy, ChatAccess, ChatPlatforms, ChatterOverrides, Override, PlatformPolicy,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Puppet {
	pub id: String,
	pub name: String,
	#[serde(default)]
	pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
	pub id: String,
	pub voice_name: String,
	#[serde(default = "azure")]
	pub provider: String,
	pub name: Option<String>,
	#[serde(default = "one")]
	pub rate: String,
	#[serde(default = "one")]
	pub pitch: String,
	#[serde(default = "normal")]
	pub expression: String,
	#[serde(default = "puppet")]
	pub role: String,
}
fn azure() -> String {
	"azure_speech".into()
}
fn one() -> String {
	"1.0".into()
}
fn normal() -> String {
	"default".into()
}
fn puppet() -> String {
	"puppet".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chatter {
	pub platform: String,
	pub user_id: String,
	pub display_name: String,
	pub puppet_id: String,
	pub voice_id: String,
	pub image_hash: Option<String>,
	pub customization_blocked: bool,
	#[serde(default)]
	pub overrides: ChatterOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSubmission {
	pub id: String,
	pub platform: String,
	pub user_id: String,
	pub display_name: String,
	pub image_hash: String,
	pub submitted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
	pub overlay_port: u16,
	pub azure_region: String,
	pub bumblebee_voice: String,
	pub openai_model: String,
	pub twitch_client_id: String,
	pub twitch_channel: String,
	pub google_client_id: String,
	pub youtube_live_chat_id: String,
	pub discord_guild_id: String,
	pub discord_text_channel_id: String,
	pub discord_voice_channel_id: String,
	pub owner_discord_id: String,
	pub enabled_tool_groups: Vec<String>,
	pub discord_listen_everyone: bool,
	pub discord_listen_role_ids: Vec<String>,
	pub discord_listen_allowed_user_ids: Vec<String>,
	pub discord_listen_blocked_user_ids: Vec<String>,
	pub wake_word: String,
	pub replay_enabled: bool,
	pub replay_seconds: u32,
	pub read_chat: bool,
	pub ai_enabled: bool,
	pub custom_images_enabled: bool,
	pub audio_output: String,
	pub master_volume: f32,
	pub bumblebee_tts_volume: f32,
	pub puppet_tts_volume: f32,
	pub wake_chirp_volume: f32,
	pub thinking_sound_volume: f32,
	pub flying_sound_volume: f32,
	pub chat_tts_waiting_tone_volume: f32,
	pub chat_tts_waiting_tone_enabled: bool,
	pub chat_tts_speaker_intro_cooldown_seconds: u32,
	pub chat_tts_interrupt_silence_ms: u32,
	pub chat_tts_queue_expiration_ms: u32,
	pub chat_tts_blocked_words: Vec<String>,
	pub chat_ai_dictation_enabled: bool,
	pub voice_mentions_enabled: bool,
	pub wake_keyword_sensitivity: String,
	pub stop_keyword_sensitivity: String,
	pub cancel_keyword_sensitivity: String,
	pub openai_voice_model: String,
	pub openai_reasoning_effort: String,
	pub openai_voice_reasoning_effort: String,
	pub image_model: String,
	pub ai_web_search_enabled: bool,
	pub ai_code_interpreter_enabled: bool,
	pub ai_image_generation_enabled: bool,
	pub ai_memories_enabled: bool,
	pub ai_reminders_enabled: bool,
	pub chat_platforms: ChatPlatforms,
	pub voice_captions_enabled: bool,
	pub streamer_transcription_model: String,
	pub voice_transcription_model: String,
}
impl Default for Settings {
	fn default() -> Self {
		Self {
			overlay_port: 2899,
			azure_region: "eastus".into(),
			bumblebee_voice: "bumblebee-buddy".into(),
			openai_model: "gpt-6-astra".into(),
			twitch_client_id: String::new(),
			twitch_channel: String::new(),
			google_client_id: String::new(),
			youtube_live_chat_id: String::new(),
			discord_guild_id: String::new(),
			discord_text_channel_id: String::new(),
			discord_voice_channel_id: String::new(),
			owner_discord_id: String::new(),
			enabled_tool_groups: Vec::new(),
			discord_listen_everyone: false,
			discord_listen_role_ids: Vec::new(),
			discord_listen_allowed_user_ids: Vec::new(),
			discord_listen_blocked_user_ids: Vec::new(),
			wake_word: "hey_bumblebee".into(),
			replay_enabled: false,
			replay_seconds: 30,
			read_chat: true,
			ai_enabled: false,
			custom_images_enabled: true,
			audio_output: "overlay".into(),
			master_volume: 1.,
			bumblebee_tts_volume: 1.,
			puppet_tts_volume: 1.,
			wake_chirp_volume: 1.,
			thinking_sound_volume: 1.,
			flying_sound_volume: 1.,
			chat_tts_waiting_tone_volume: 1.,
			chat_tts_waiting_tone_enabled: true,
			chat_tts_speaker_intro_cooldown_seconds: 45,
			chat_tts_interrupt_silence_ms: 450,
			chat_tts_queue_expiration_ms: 60_000,
			chat_tts_blocked_words: Vec::new(),
			chat_ai_dictation_enabled: false,
			voice_mentions_enabled: true,
			wake_keyword_sensitivity: "balanced".into(),
			stop_keyword_sensitivity: "balanced".into(),
			cancel_keyword_sensitivity: "balanced".into(),
			openai_voice_model: String::new(),
			openai_reasoning_effort: "default".into(),
			openai_voice_reasoning_effort: "default".into(),
			image_model: "gpt-image-2.5-sunburst".into(),
			ai_web_search_enabled: false,
			ai_code_interpreter_enabled: false,
			ai_image_generation_enabled: false,
			ai_memories_enabled: true,
			ai_reminders_enabled: true,
			chat_platforms: ChatPlatforms::default(),
			voice_captions_enabled: false,
			streamer_transcription_model: "gpt-transcribe".into(),
			voice_transcription_model: "gpt-transcribe".into(),
		}
	}
}

impl Settings {
	pub fn validate(&self) -> anyhow::Result<()> {
		self.validate_retained()?;
		use anyhow::ensure;
		ensure!(
			self.overlay_port >= 1024,
			"OBS port must be between 1024 and 65535"
		);
		ensure!(
			!self.azure_region.is_empty()
				&& self.azure_region.len() < 40
				&& self
					.azure_region
					.bytes()
					.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
			"Invalid Azure Speech region"
		);
		ensure!(
			crate::catalog::voices()
				.iter()
				.any(|v| v.id == self.bumblebee_voice && v.role == "bumblebee"),
			"Select a built-in Bumblebee voice"
		);
		ensure!(
			matches!(self.wake_word.as_str(), "bumblebee" | "hey_bumblebee"),
			"Unknown wake word"
		);
		ensure!(
			(5..=120).contains(&self.replay_seconds),
			"Replay buffer must be 5-120 seconds"
		);
		let groups = [
			"discord_resources",
			"discord_moderation",
			"twitch_broadcast",
			"twitch_moderation",
			"twitch_polls",
			"youtube_moderation",
			"youtube_polls",
		];
		ensure!(
			self
				.enabled_tool_groups
				.iter()
				.all(|group| groups.contains(&group.as_str())),
			"Unknown tool permission group"
		);
		fn discord_id(value: &str) -> bool {
			value.parse::<u64>().is_ok_and(|id| id > 0)
		}
		for value in [
			&self.discord_guild_id,
			&self.discord_text_channel_id,
			&self.discord_voice_channel_id,
			&self.owner_discord_id,
		] {
			ensure!(
				value.is_empty() || discord_id(value),
				"Discord IDs must be positive numeric IDs"
			);
		}
		for values in [
			&self.discord_listen_role_ids,
			&self.discord_listen_allowed_user_ids,
			&self.discord_listen_blocked_user_ids,
		] {
			ensure!(
				values.len() <= 200 && values.iter().all(|v| discord_id(v)),
				"Discord permission lists support up to 200 numeric IDs"
			);
		}
		for value in [
			&self.openai_model,
			&self.twitch_client_id,
			&self.twitch_channel,
			&self.google_client_id,
			&self.youtube_live_chat_id,
		] {
			ensure!(
				value.len() <= 512 && !value.chars().any(char::is_control),
				"A provider setting is too long or contains control characters"
			);
		}
		Ok(())
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlayEvent {
	AudioSettings {
		settings: AudioMix,
	},
	Signal {
		id: String,
		audio_path: String,
		gain: f32,
		kind: String,
		looping: bool,
		audible: bool,
	},
	StopSignal {
		id: String,
	},
	VoiceTranscript {
		#[serde(rename = "userId")]
		user_id: String,
		#[serde(rename = "isOwner")]
		is_owner: bool,
		text: String,
		r#final: bool,
	},
	OverlaySettings {
		settings: OverlaySettings,
	},
	Image {
		id: String,
		image_path: String,
		title: String,
	},
	HideImage,
	Presentation {
		title: String,
		text: String,
	},
	ChatterChanged {
		chatter: Chatter,
	},
	Chat {
		chatter: Chatter,
		text: String,
	},
	Speech {
		id: String,
		chatter: Option<Chatter>,
		text: String,
		audio_path: String,
		audible: bool,
		gain: f32,
		words: Vec<WordTiming>,
	},
	StopSpeech,
	Status {
		message: String,
	},
}

pub use crate::overlay::OverlaySettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordTiming {
	pub text: String,
	pub start_ms: u64,
	pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
	pub platform: String,
	pub user_id: String,
	pub display_name: String,
	pub message_id: String,
	pub channel_id: String,
	pub text: String,
	pub is_owner: bool,
	#[serde(default)]
	pub access: ChatAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMix {
	pub output: String,
	pub master_volume: f32,
	pub bumblebee_tts_volume: f32,
	pub puppet_tts_volume: f32,
	pub flying_sound_volume: f32,
	pub wake_chirp_volume: f32,
	pub thinking_sound_volume: f32,
	pub chat_tts_waiting_tone_volume: f32,
}
impl Settings {
	pub fn audio_mix(&self) -> AudioMix {
		AudioMix {
			output: self.audio_output.clone(),
			master_volume: self.master_volume,
			bumblebee_tts_volume: self.bumblebee_tts_volume,
			puppet_tts_volume: self.puppet_tts_volume,
			flying_sound_volume: self.flying_sound_volume,
			wake_chirp_volume: self.wake_chirp_volume,
			thinking_sound_volume: self.thinking_sound_volume,
			chat_tts_waiting_tone_volume: self.chat_tts_waiting_tone_volume,
		}
	}
}
