use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SongbirdPlaybackInputType {
	Raw,
	OggOpus,
	Encoded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SongbirdPlaybackMode {
	Normal,
	Urgent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SongbirdPlaybackClass {
	Speech,
	Thinking,
	Cue,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SongbirdSignalKey {
	WakeChirp,
	HeardChirp,
	TimeoutChirp,
	CancelChirp,
	CallWaiting,
	ThinkingLoop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceWakeWord {
	Bumblebee,
	HeyBumblebee,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceKeywordSensitivity {
	Strict,
	Balanced,
	Loose,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SongbirdPresenceConfig {
	pub guild_id: String,
	pub channel_id: String,
	pub require_speak: bool,
	pub listen_everyone: bool,
	pub listen_role_ids: Vec<String>,
	pub listen_allowed_user_ids: Vec<String>,
	pub listen_blocked_user_ids: Vec<String>,
	pub owner_discord_id: Option<String>,
	pub replay_buffer_enabled: bool,
	pub replay_buffer_seconds: u32,
	pub wake_word: VoiceWakeWord,
	pub wake_keyword_sensitivity: VoiceKeywordSensitivity,
	pub stop_keyword_sensitivity: VoiceKeywordSensitivity,
	pub cancel_keyword_sensitivity: VoiceKeywordSensitivity,
}

impl SongbirdPresenceConfig {
	/// Safe first-run policy: only the configured streamer can wake the agent.
	pub fn owner_only(guild_id: String, channel_id: String, owner_id: String) -> Self {
		Self {
			guild_id,
			channel_id,
			require_speak: true,
			listen_everyone: false,
			listen_role_ids: Vec::new(),
			listen_allowed_user_ids: vec![owner_id.clone()],
			listen_blocked_user_ids: Vec::new(),
			owner_discord_id: Some(owner_id),
			replay_buffer_enabled: false,
			replay_buffer_seconds: 20,
			wake_word: VoiceWakeWord::HeyBumblebee,
			wake_keyword_sensitivity: VoiceKeywordSensitivity::Balanced,
			stop_keyword_sensitivity: VoiceKeywordSensitivity::Balanced,
			cancel_keyword_sensitivity: VoiceKeywordSensitivity::Balanced,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
	tag = "type",
	rename_all = "snake_case",
	rename_all_fields = "camelCase"
)]
pub enum SongbirdEvent {
	KeywordDetected {
		user_id: String,
		username: String,
		owner: bool,
		keyword_text: String,
		keyword_kind: String,
		model_name: String,
	},
	ListenStarted {
		user_id: String,
		username: String,
		owner: bool,
		duration_ms: u64,
		reason: String,
	},
	AudioFrame {
		user_id: String,
		username: String,
		sample_rate_hz: u32,
		channels: u8,
		frame_ms: u32,
		#[serde(skip_serializing_if = "Option::is_none")]
		vad_active: Option<bool>,
		#[serde(skip_serializing_if = "Option::is_none")]
		rms: Option<f32>,
	},
	VoiceActivity {
		user_id: String,
		username: String,
		frame_ms: u32,
		vad_active: bool,
		rms: f32,
	},
	ParticipantJoined {
		user_id: String,
		username: String,
		owner: bool,
	},
	ParticipantLeft {
		user_id: String,
	},
	SpeakingStarted {
		user_id: String,
		username: String,
	},
	SpeakingEnded {
		user_id: String,
		username: String,
	},
	NoSpeech {
		user_id: String,
		username: String,
	},
	UtteranceFinalized {
		user_id: String,
		username: String,
		reason: String,
		duration_ms: u64,
		voiced_ms: u64,
	},
	SourceUp {
		guild_id: String,
		channel_id: String,
	},
	SourceDown {
		#[serde(skip_serializing_if = "Option::is_none")]
		reason: Option<String>,
	},
	PlaybackLag {
		message: String,
		#[serde(skip_serializing_if = "Option::is_none")]
		meta: Option<Value>,
	},
	PlaybackStarted {
		playback_id: u64,
		playback_class: SongbirdPlaybackClass,
		#[serde(skip_serializing_if = "Option::is_none")]
		debug_meta: Option<Value>,
	},
	ReplaySaved {
		replay_id: String,
		duration_ms: u64,
		sample_rate_hz: u32,
		channels: u8,
	},
	SessionError {
		message: String,
	},
}
