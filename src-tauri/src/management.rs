//! Local owner commands, deliberately absent from the OBS HTTP router.
use crate::Runtime;
use bumblebee_core::{
	agent_storage::{Memory, Reminder},
	model::{ChatAccess, ChatMessage},
	providers::discovery::{DiscordOption, DiscordOptions},
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
	e.to_string()
}

#[derive(Serialize)]
pub struct Library {
	memories: Vec<Memory>,
	reminders: Vec<Reminder>,
}

#[tauri::command]
pub async fn get_library(state: State<'_, Arc<Runtime>>) -> Result<Library> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || {
		Ok(Library {
			memories: store.memories("preview:owner", true).map_err(err)?,
			reminders: store.reminders("preview:owner", true).map_err(err)?,
		})
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn save_memory(
	state: State<'_, Arc<Runtime>>,
	id: Option<String>,
	content: String,
) -> Result<()> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || {
		if let Some(id) = id {
			store
				.update_memory(&id, "preview:owner", true, &content)
				.map_err(err)
		} else {
			store
				.remember("preview:owner", &content)
				.map(|_| ())
				.map_err(err)
		}
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn delete_memory(state: State<'_, Arc<Runtime>>, id: String) -> Result<()> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || store.delete_memory(&id, "preview:owner", true).map_err(err))
		.await
		.map_err(err)?
}

#[tauri::command]
pub async fn save_reminder(
	state: State<'_, Arc<Runtime>>,
	id: Option<String>,
	content: String,
	due_at: i64,
) -> Result<()> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || {
		if let Some(id) = id {
			return store
				.update_reminder(&id, "preview:owner", true, &content, due_at)
				.map_err(err);
		}
		let settings = store.settings().map_err(err)?;
		if settings.owner_discord_id.is_empty() {
			return Err("Configure your Discord user ID before adding a reminder.".into());
		}
		let source = ChatMessage {
			platform: "discord".into(),
			user_id: settings.owner_discord_id,
			display_name: "Streamer".into(),
			channel_id: settings.discord_text_channel_id,
			message_id: uuid::Uuid::new_v4().to_string(),
			text: content.clone(),
			is_owner: true,
			access: ChatAccess::default(),
		};
		store
			.create_reminder(&source, &content, due_at)
			.map(|_| ())
			.map_err(err)
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn cancel_reminder(state: State<'_, Arc<Runtime>>, id: String) -> Result<()> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || {
		store
			.cancel_reminder(&id, "preview:owner", true)
			.map_err(err)
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn discord_guilds(state: State<'_, Arc<Runtime>>) -> Result<Vec<DiscordOption>> {
	state.providers.discord_guilds().await.map_err(err)
}
#[tauri::command]
pub async fn discord_options(
	state: State<'_, Arc<Runtime>>,
	guild_id: String,
) -> Result<DiscordOptions> {
	state
		.providers
		.discord_options(&guild_id)
		.await
		.map_err(err)
}
#[tauri::command]
pub async fn openai_models(state: State<'_, Arc<Runtime>>) -> Result<Vec<String>> {
	state.providers.openai_models().await.map_err(err)
}

/// Reset behavior only. Account identity, credentials and durable user data are never reset here.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetScope {
	Audio,
	Ai,
	Discord,
	Twitch,
	Youtube,
	All,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPreferences {
	settings: bumblebee_core::model::Settings,
	overlay_settings: Option<bumblebee_core::model::OverlaySettings>,
}

fn reset_patch(scope: ResetScope) -> serde_json::Value {
	use serde_json::{Map, Value};
	let defaults =
		serde_json::to_value(bumblebee_core::model::Settings::default()).expect("Settings serialize");
	let mut patch = Map::new();
	let audio = [
		"bumblebeeVoice",
		"readChat",
		"audioOutput",
		"masterVolume",
		"bumblebeeTtsVolume",
		"puppetTtsVolume",
		"wakeChirpVolume",
		"thinkingSoundVolume",
		"chatTtsWaitingToneVolume",
		"chatTtsWaitingToneEnabled",
		"chatTtsSpeakerIntroCooldownSeconds",
		"chatTtsInterruptSilenceMs",
		"chatTtsQueueExpirationMs",
		"chatTtsBlockedWords",
		"chatAiDictationEnabled",
		"voiceMentionsEnabled",
		"wakeWord",
		"replayEnabled",
		"replaySeconds",
		"wakeKeywordSensitivity",
		"stopKeywordSensitivity",
		"cancelKeywordSensitivity",
	];
	let ai = [
		"aiEnabled",
		"openaiModel",
		"openaiVoiceModel",
		"openaiReasoningEffort",
		"openaiVoiceReasoningEffort",
		"imageModel",
		"streamerTranscriptionModel",
		"voiceTranscriptionModel",
		"aiWebSearchEnabled",
		"aiCodeInterpreterEnabled",
		"aiImageGenerationEnabled",
		"aiMemoriesEnabled",
		"aiRemindersEnabled",
		"enabledToolGroups",
	];
	let discord = [
		"discordListenEveryone",
		"discordListenRoleIds",
		"discordListenAllowedUserIds",
		"discordListenBlockedUserIds",
		"voiceCaptionsEnabled",
	];
	if matches!(scope, ResetScope::Audio | ResetScope::All) {
		for key in audio {
			patch.insert(key.into(), defaults[key].clone());
		}
	}
	if matches!(scope, ResetScope::Ai | ResetScope::All) {
		for key in ai {
			patch.insert(key.into(), defaults[key].clone());
		}
	}
	if matches!(scope, ResetScope::Discord | ResetScope::All) {
		for key in discord {
			patch.insert(key.into(), defaults[key].clone());
		}
	}
	let platforms: &[&str] = match scope {
		ResetScope::Discord => &["discord"],
		ResetScope::Twitch => &["twitch"],
		ResetScope::Youtube => &["youtube"],
		ResetScope::All => &["discord", "twitch", "youtube"],
		_ => &[],
	};
	if !platforms.is_empty() {
		patch.insert(
			"chatPlatforms".into(),
			Value::Object(
				platforms
					.iter()
					.map(|name| ((*name).to_owned(), defaults["chatPlatforms"][*name].clone()))
					.collect(),
			),
		);
	}
	if matches!(scope, ResetScope::All) {
		patch.insert(
			"customImagesEnabled".into(),
			defaults["customImagesEnabled"].clone(),
		);
	}
	Value::Object(patch)
}

#[tauri::command]
pub async fn reset_preferences(
	state: State<'_, Arc<Runtime>>,
	scope: ResetScope,
) -> Result<ResetPreferences> {
	let _configuration = state.configuration.lock().await;
	let previous = state.store.settings().map_err(err)?;
	let patch = reset_patch(scope);
	let store = state.store.clone();
	let (settings, overlay_settings) = tokio::task::spawn_blocking(move || {
		store
			.patch_preferences(&patch, matches!(scope, ResetScope::All))
			.map_err(err)
	})
	.await
	.map_err(err)??;
	if let Some(overlay) = &overlay_settings {
		state
			.engine
			.overlay_settings_changed(overlay)
			.map_err(err)?;
		state
			.engine
			.emit(bumblebee_core::model::OverlayEvent::OverlaySettings {
				settings: overlay.clone(),
			});
	}
	state
		.engine
		.settings_changed(&previous)
		.await
		.map_err(|error| {
			format!("Preferences were reset, but applying the runtime change failed: {error}")
		})?;
	Ok(ResetPreferences {
		settings,
		overlay_settings,
	})
}

#[cfg(test)]
mod reset_tests {
	use super::*;
	use bumblebee_core::{model::Settings, storage::Store};
	#[test]
	fn reset_scopes_reject_unknown_values() {
		for value in ["factory", "credentials", "Audio", "", "overlay"] {
			assert!(serde_json::from_value::<ResetScope>(serde_json::json!(value)).is_err());
		}
	}
	#[test]
	fn all_reset_preserves_connections_and_durable_state() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::open(&dir.path().join("reset.sqlite3")).unwrap();
		let original = serde_json::json!({"overlayPort":2999,"azureRegion":"westus","twitchClientId":"client","twitchChannel":"channel","googleClientId":"google","youtubeLiveChatId":"youtube-chat","discordGuildId":"123","discordTextChannelId":"456","discordVoiceChannelId":"789","ownerDiscordId":"321","masterVolume":0.25,"enabledToolGroups":["twitch_polls"],"voiceCaptionsEnabled":true});
		store.patch_settings(&original).unwrap();
		let token = store.overlay_token().unwrap();
		let chatter = store.ensure_chatter("discord", "321", "Streamer").unwrap();
		let message = ChatMessage {
			platform: "discord".into(),
			user_id: "321".into(),
			display_name: "Streamer".into(),
			channel_id: "456".into(),
			message_id: "reset-fixture".into(),
			text: "Preserve reminder".into(),
			is_owner: true,
			access: ChatAccess::default(),
		};
		store
			.create_reminder(
				&message,
				"Preserve reminder",
				bumblebee_core::now_ms() + 3_600_000,
			)
			.unwrap();
		store.remember("preview:owner", "Keep this memory").unwrap();
		store
			.set(
				"settings_undo",
				&serde_json::json!({"unrelated":"preserved"}),
			)
			.unwrap();
		let (reset, overlay) = store
			.patch_preferences(&reset_patch(ResetScope::All), true)
			.unwrap();
		let reset_json = serde_json::to_value(&reset).unwrap();
		for key in [
			"overlayPort",
			"azureRegion",
			"twitchClientId",
			"twitchChannel",
			"googleClientId",
			"youtubeLiveChatId",
			"discordGuildId",
			"discordTextChannelId",
			"discordVoiceChannelId",
			"ownerDiscordId",
		] {
			assert_eq!(reset_json[key], original[key], "Preserve {key}");
		}
		assert_eq!(reset.master_volume, Settings::default().master_volume);
		assert!(reset.enabled_tool_groups.is_empty());
		assert!(!reset.voice_captions_enabled);
		assert!(overlay.is_some());
		assert_eq!(store.overlay_token().unwrap(), token);
		assert_eq!(store.memories("preview:owner", true).unwrap().len(), 1);
		assert_eq!(store.reminders("preview:owner", true).unwrap().len(), 1);
		assert_eq!(
			serde_json::to_value(store.chatter("discord", "321").unwrap()).unwrap(),
			serde_json::to_value(chatter).unwrap()
		);
		assert_eq!(
			store
				.get::<serde_json::Value>("settings_undo")
				.unwrap()
				.unwrap()["unrelated"],
			"preserved"
		);
	}
	#[test]
	fn section_reset_does_not_touch_other_behavior_groups() {
		let current=Settings::default().patched(&serde_json::json!({"masterVolume":0.25,"openaiModel":"custom-model","chatPlatforms":{"discord":{"relay":true},"twitch":{"relay":true}}})).unwrap();
		let audio = current.patched(&reset_patch(ResetScope::Audio)).unwrap();
		assert_eq!(audio.master_volume, Settings::default().master_volume);
		assert_eq!(audio.openai_model, "custom-model");
		assert!(audio.chat_platforms.discord.relay);
		let discord = current.patched(&reset_patch(ResetScope::Discord)).unwrap();
		assert!(!discord.chat_platforms.discord.relay);
		assert!(discord.chat_platforms.twitch.relay);
		assert_eq!(discord.master_volume, 0.25);
	}
}
