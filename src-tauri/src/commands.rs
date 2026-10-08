use crate::{Runtime, shutdown_app};
use bumblebee_core::{
	agent_storage::PendingInput,
	catalog,
	model::*,
	providers::{ProviderStatus, oauth::Authorization},
};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};
use tauri::{Emitter, State};
type CommandResult<T> = Result<T, String>;
fn err(error: impl std::fmt::Display) -> String {
	error.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
	overlay_settings: OverlaySettings,
	settings: Settings,
	puppets: Vec<Puppet>,
	voices: Vec<Voice>,
	pending_images: Vec<ImageSubmission>,
	chatters: Vec<Chatter>,
	overlay_url: String,
	secrets: BTreeMap<String, bool>,
	statuses: Vec<ProviderStatus>,
	active: bool,
	credential_store_error: Option<String>,
	overlay_error: Option<String>,
	pending_inputs: Vec<PendingInput>,
	interrupted_turns: Vec<RecoveryTurn>,
	artifacts: Vec<bumblebee_core::agent::Artifact>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryTurn {
	id: String,
	actor: String,
	state: String,
	updated_at: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
	pending_inputs: Vec<PendingInput>,
	interrupted_turns: Vec<RecoveryTurn>,
	pending_images: Vec<ImageSubmission>,
	active: bool,
	artifacts: Vec<bumblebee_core::agent::Artifact>,
}
#[tauri::command]
pub async fn get_activity(state: State<'_, Arc<Runtime>>) -> CommandResult<Activity> {
	let state = state.inner().clone();
	tokio::task::spawn_blocking(move || {
		Ok(Activity {
			pending_inputs: state.store.pending_inputs().map_err(err)?,
			interrupted_turns: state
				.store
				.interrupted_turns()
				.map_err(err)?
				.into_iter()
				.map(|t| RecoveryTurn {
					id: t.id,
					actor: t.actor,
					state: t.state,
					updated_at: t.updated_at,
				})
				.collect(),
			pending_images: state.store.pending_images().map_err(err)?,
			active: state.engine.is_active(),
			artifacts: crate::artifacts::list(&state).map_err(err)?,
		})
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, Arc<Runtime>>) -> CommandResult<Snapshot> {
	let state = state.inner().clone();
	tokio::task::spawn_blocking(move || {
		let mut secrets = BTreeMap::new();
		let mut credential_store_error = None;
		for id in [
			"azure_speech",
			"openai",
			"discord_bot",
			"google_client_secret",
			"twitch_tokens",
			"google_tokens",
		] {
			match state.providers.secrets.get(id) {
				Ok(value) => {
					secrets.insert(id.to_owned(), value.is_some_and(|v| !v.trim().is_empty()));
				}
				Err(error) => {
					secrets.insert(id.to_owned(), false);
					credential_store_error = Some(error.to_string());
				}
			}
		}
		let interrupted_turns = state
			.store
			.interrupted_turns()
			.map_err(err)?
			.into_iter()
			.map(|t| RecoveryTurn {
				id: t.id,
				actor: t.actor,
				state: t.state,
				updated_at: t.updated_at,
			})
			.collect();
		Ok(Snapshot {
			overlay_settings: state
				.store
				.get("overlay_settings")
				.map_err(err)?
				.unwrap_or_default(),
			settings: state.store.settings().map_err(err)?,
			puppets: catalog::puppets(),
			voices: state.store.catalog_voices().map_err(err)?,
			pending_images: state.store.pending_images().map_err(err)?,
			chatters: state.store.chatters("").map_err(err)?,
			overlay_url: state.transport.url(),
			secrets,
			statuses: state.providers.statuses(),
			active: state.engine.is_active(),
			credential_store_error,
			overlay_error: state.overlay_error.clone(),
			pending_inputs: state.store.pending_inputs().map_err(err)?,
			interrupted_turns,
			artifacts: crate::artifacts::list(&state).map_err(err)?,
		})
	})
	.await
	.map_err(err)?
}

#[tauri::command]
pub async fn save_overlay_settings(
	state: State<'_, Arc<Runtime>>,
	settings: OverlaySettings,
) -> CommandResult<OverlaySettings> {
	let settings = state
		.store
		.patch_overlay(&serde_json::to_value(settings).map_err(err)?)
		.map_err(err)?;
	state
		.engine
		.overlay_settings_changed(&settings)
		.map_err(err)?;
	let _ = state.transport.events.send(OverlayEvent::OverlaySettings {
		settings: settings.clone(),
	});
	Ok(settings)
}

#[tauri::command]
pub async fn patch_overlay_settings(
	state: State<'_, Arc<Runtime>>,
	patch: serde_json::Value,
) -> CommandResult<OverlaySettings> {
	let store = state.store.clone();
	let settings = tokio::task::spawn_blocking(move || store.patch_overlay(&patch).map_err(err))
		.await
		.map_err(err)??;
	state
		.engine
		.overlay_settings_changed(&settings)
		.map_err(err)?;
	state.engine.emit(OverlayEvent::OverlaySettings {
		settings: settings.clone(),
	});
	Ok(settings)
}

#[tauri::command]
pub async fn patch_settings(
	state: State<'_, Arc<Runtime>>,
	patch: serde_json::Value,
) -> CommandResult<Settings> {
	apply_settings_patch(state.inner(), patch).await
}

async fn apply_settings_patch(
	state: &Arc<Runtime>,
	patch: serde_json::Value,
) -> CommandResult<Settings> {
	let _configuration = state.configuration.lock().await;
	let previous = state.store.settings().map_err(err)?;
	previous.patched(&patch).map_err(err)?;
	let previous_json = serde_json::to_value(&previous).map_err(err)?;
	let connection_change = [
		"twitchClientId",
		"twitchChannel",
		"googleClientId",
		"youtubeLiveChatId",
		"discordGuildId",
		"ownerDiscordId",
	]
	.iter()
	.any(|key| {
		patch
			.get(key)
			.is_some_and(|value| value != &previous_json[key])
	});
	let restart = connection_change && state.engine.is_active();
	if connection_change {
		state.providers.cancel_authorizations();
	}
	if restart {
		state.engine.cancel().await;
		state.engine.stop().await.map_err(err)?;
	}
	let store = state.store.clone();
	let settings = tokio::task::spawn_blocking(move || store.patch_settings(&patch).map_err(err))
		.await
		.map_err(err)??;
	if restart {
		state.engine.start().await.map_err(|error| {
			format!("Settings saved, but the session could not reconnect: {error}")
		})?;
	} else {
		state
			.engine
			.settings_changed(&previous)
			.await
			.map_err(err)?;
	}
	Ok(settings)
}

#[tauri::command]
pub async fn save_settings(
	state: State<'_, Arc<Runtime>>,
	settings: Settings,
) -> CommandResult<Settings> {
	settings.validate().map_err(err)?;
	apply_settings_patch(state.inner(), serde_json::to_value(settings).map_err(err)?).await
}
#[tauri::command]
pub async fn set_secret(
	state: State<'_, Arc<Runtime>>,
	name: String,
	value: String,
) -> CommandResult<()> {
	let _configuration = state.configuration.lock().await;
	if ![
		"azure_speech",
		"openai",
		"discord_bot",
		"google_client_secret",
	]
	.contains(&name.as_str())
	{
		return Err("This credential must be obtained through authorization".into());
	}
	if value.trim().is_empty() || value.len() > 16384 {
		return Err("Enter a valid credential".into());
	}
	// Replacing a credential retires every connection using the old credential.
	state.engine.cancel().await;
	state.engine.stop().await.map_err(err)?;
	state
		.providers
		.set_credential(&name, value.trim())
		.await
		.map_err(err)
}
#[tauri::command]
pub async fn delete_secret(state: State<'_, Arc<Runtime>>, name: String) -> CommandResult<()> {
	let _configuration = state.configuration.lock().await;
	state.engine.cancel().await;
	state.engine.stop().await.map_err(err)?;
	state.providers.delete_credential(&name).await.map_err(err)
}
#[tauri::command]
pub async fn validate_provider(
	state: State<'_, Arc<Runtime>>,
	provider: String,
) -> CommandResult<()> {
	let p = state.providers.clone();
	let result: anyhow::Result<()> = async {
		match provider.as_str() {
			"azure_speech" => {
				p.refresh_voices().await?;
			}
			"openai" => {
				p.openai_models().await?;
			}
			"discord" => {
				let key = p.secret("discord_bot")?;
				let response = p
					.http
					.get("https://discord.com/api/v10/users/@me")
					.header("Authorization", format!("Bot {key}"))
					.send()
					.await?;
				bumblebee_core::providers::check_response("discord", &response)?;
				p.status(
					"discord",
					"authorized",
					"Bot token validated; start a session to connect",
				);
			}
			_ => anyhow::bail!("Authorize this provider in your browser"),
		}
		Ok(())
	}
	.await;
	if let Err(error) = &result {
		p.status(&provider, "configuration_error", error.to_string());
	}
	result.map_err(err)
}
#[tauri::command]
pub async fn authorize_provider(
	state: State<'_, Arc<Runtime>>,
	provider: String,
) -> CommandResult<Authorization> {
	let _configuration = state.configuration.lock().await;
	state.engine.cancel().await;
	state.engine.stop().await.map_err(err)?;
	match provider.as_str() {
		"twitch" => state
			.providers
			.authorize_twitch(state.shutdown.child_token())
			.await
			.map_err(err),
		"youtube" => state
			.providers
			.authorize_google(state.shutdown.child_token())
			.await
			.map_err(err),
		_ => Err("This provider uses an API credential".into()),
	}
}
#[tauri::command]
pub async fn start_session(state: State<'_, Arc<Runtime>>) -> CommandResult<()> {
	let _configuration = state.configuration.lock().await;
	if state
		.providers
		.statuses()
		.iter()
		.any(|s| s.state == "authorizing")
	{
		return Err("Finish browser authorization before starting the session".into());
	}
	state.engine.start().await.map_err(err)
}
#[tauri::command]
pub async fn stop_session(state: State<'_, Arc<Runtime>>) -> CommandResult<()> {
	let _configuration = state.configuration.lock().await;
	state.engine.stop().await.map_err(err)
}
#[tauri::command]
pub async fn preview_speech(state: State<'_, Arc<Runtime>>, text: String) -> CommandResult<()> {
	state.engine.say_preview(text).await.map_err(err)
}
#[tauri::command]
pub async fn cancel_speech(state: State<'_, Arc<Runtime>>) -> CommandResult<()> {
	state.engine.cancel().await;
	Ok(())
}
#[tauri::command]
pub async fn search_chatters(
	state: State<'_, Arc<Runtime>>,
	search: String,
) -> CommandResult<Vec<Chatter>> {
	let store = state.store.clone();
	tokio::task::spawn_blocking(move || store.chatters(&search).map_err(err))
		.await
		.map_err(err)?
}
#[tauri::command]
pub async fn review_image(
	app: tauri::AppHandle,
	state: State<'_, Arc<Runtime>>,
	id: String,
	approve: bool,
) -> CommandResult<Chatter> {
	let store = state.store.clone();
	let chatter = tokio::task::spawn_blocking(move || store.review_image(&id, approve).map_err(err))
		.await
		.map_err(err)??;
	let _ = state.transport.events.send(OverlayEvent::ChatterChanged {
		chatter: chatter.clone(),
	});
	let _ = app.emit("bumblebee:profiles-changed", ());
	Ok(chatter)
}
#[tauri::command]
pub async fn update_chatter(
	app: tauri::AppHandle,
	state: State<'_, Arc<Runtime>>,
	platform: String,
	user_id: String,
	puppet_id: Option<String>,
	voice_id: Option<String>,
	blocked: Option<bool>,
	overrides: Option<ChatterOverrides>,
) -> CommandResult<Chatter> {
	let permissions_changed = overrides.is_some();
	let store = state.store.clone();
	let chatter = tokio::task::spawn_blocking(move || {
		if let Some(overrides) = overrides {
			store
				.set_chatter_overrides(&platform, &user_id, &overrides)
				.map_err(err)?;
		}
		if let Some(puppet) = puppet_id {
			store
				.set_chatter_puppet(&platform, &user_id, &puppet)
				.map_err(err)?;
		}
		if let Some(voice) = voice_id {
			store
				.set_chatter_voice(&platform, &user_id, &voice)
				.map_err(err)?;
		}
		if let Some(blocked) = blocked {
			store
				.block_customization(&platform, &user_id, blocked)
				.map_err(err)?;
		}
		store.chatter(&platform, &user_id).map_err(err)
	})
	.await
	.map_err(err)??;
	if permissions_changed {
		state
			.engine
			.chatter_settings_changed(&chatter.platform, &chatter.user_id)
			.await
			.map_err(err)?;
	}
	let _ = state.transport.events.send(OverlayEvent::ChatterChanged {
		chatter: chatter.clone(),
	});
	let _ = app.emit("bumblebee:profiles-changed", ());
	Ok(chatter)
}
#[tauri::command]
pub async fn rotate_overlay_token(state: State<'_, Arc<Runtime>>) -> CommandResult<String> {
	let token = format!(
		"{}{}",
		uuid::Uuid::new_v4().simple(),
		uuid::Uuid::new_v4().simple()
	);
	state.store.set("overlay_token", &token).map_err(err)?;
	state.transport.token.send_replace(token);
	Ok(state.transport.url())
}
#[tauri::command]
pub async fn quit_app(app: tauri::AppHandle, state: State<'_, Arc<Runtime>>) -> CommandResult<()> {
	shutdown_app(app, state.inner().clone()).await;
	Ok(())
}

#[tauri::command]
pub async fn answer_pending(
	state: State<'_, Arc<Runtime>>,
	id: String,
	answer: String,
) -> CommandResult<()> {
	if !state.engine.is_active() {
		return Err("Start a session before answering a pending request".into());
	}
	let message =
		bumblebee_core::agent::desktop_answer_message(&state.store, &id, &answer).map_err(err)?;
	state.engine.handle_chat(message).await.map_err(err)
}
#[tauri::command]
pub async fn dismiss_interrupted(state: State<'_, Arc<Runtime>>, id: String) -> CommandResult<()> {
	if !state
		.store
		.interrupted_turns()
		.map_err(err)?
		.iter()
		.any(|turn| turn.id == id)
	{
		return Err("This turn is not awaiting interruption review".into());
	}
	state.store.cancel_agent_turn(&id).map_err(err)
}
