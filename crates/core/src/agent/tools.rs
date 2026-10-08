use super::{Artifact, Checkpoint, ToolCall, ToolDefinition, delivery, managed, platform_tools};
use crate::{
	agent_storage as durable,
	model::{OverlayEvent, OverlaySettings, Settings},
	runtime::Engine,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn string(max: u64) -> Value {
	json!({"type":"string","maxLength":max})
}
fn boolean() -> Value {
	json!({"type":"boolean"})
}
fn nullable(value: Value) -> Value {
	json!({"anyOf":[value,{"type":"null"}]})
}
fn array(item: Value, max: u64) -> Value {
	json!({"type":"array","items":item,"maxItems":max})
}
fn number(min: f64, max: f64) -> Value {
	json!({"type":"number","minimum":min,"maximum":max})
}
fn enumeration(values: &[&str]) -> Value {
	json!({"type":"string","enum":values})
}
fn definition(
	name: &str,
	description: &str,
	fields: Vec<(&str, Value)>,
	owner: bool,
	confirm: bool,
	effect: bool,
) -> ToolDefinition {
	let required: Vec<_> = fields.iter().map(|(key, _)| *key).collect();
	let props: serde_json::Map<String, Value> = fields
		.iter()
		.map(|(key, value)| (key.to_string(), value.clone()))
		.collect();
	ToolDefinition {
		name: name.into(),
		description: description.into(),
		parameters: json!({"type":"object","properties":props,"required":required,"additionalProperties":false}),
		owner_only: owner,
		requires_confirmation: confirm,
		external_effect: effect,
	}
}
pub fn definitions() -> Vec<ToolDefinition> {
	let mut tools = vec![
		definition(
			"configureTurnDelivery",
			"First call of every turn. Fix whether to speak, allow public progress and send the final answer to source chat and/or a Discord DM. Private source DMs always remain silent on stream.",
			vec![
				("speech", boolean()),
				("publicProgress", boolean()),
				("targets", array(enumeration(&["source", "discord_dm"]), 2)),
				("discordDmUserId", nullable(string(20))),
			],
			false,
			false,
			false,
		),
		definition(
			"requestUserInput",
			"Suspend this request for an exact answer. Use for every question that blocks further work. Empty choices allow free text; otherwise provide the discovered choices.",
			vec![
				("question", string(3000)),
				("choices", array(string(200), 20)),
			],
			false,
			false,
			false,
		),
		definition(
			"progressUpdate",
			"Send concise meaningful progress only if the original delivery policy permits it. This is not a final result.",
			vec![("text", string(1000))],
			false,
			false,
			true,
		),
		definition(
			"discoverConnectedCapabilities",
			"Inspect the actual executable tool catalog. Discovery is not a permission grant; current provider and streamer permissions are checked at execution.",
			vec![
				("query", nullable(string(200))),
				(
					"offset",
					json!({"type":"integer","minimum":0,"maximum":1000}),
				),
				("limit", json!({"type":"integer","minimum":1,"maximum":50})),
			],
			false,
			false,
			false,
		),
		definition(
			"deliverMessage",
			"Deliver an explicitly requested message and selected artifact IDs. Source and own Discord DM work for viewers; other destinations require the streamer. Twitch/YouTube cannot receive files. Local files stay available in the dashboard.",
			vec![
				(
					"target",
					enumeration(&[
						"source",
						"discord_dm",
						"discord_channel",
						"twitch",
						"youtube",
					]),
				),
				("destinationId", nullable(string(256))),
				("text", string(40000)),
				("artifactIds", array(string(100), 10)),
			],
			false,
			false,
			true,
		),
		definition(
			"rememberMemory",
			"Remember a concise durable fact for this requester. Do not save credentials, sensitive identifiers or transient conversation filler.",
			vec![("content", string(4000))],
			false,
			false,
			false,
		),
		definition(
			"listMemories",
			"List saved memories. Viewers see their own; the streamer can inspect all.",
			vec![],
			false,
			false,
			false,
		),
		definition(
			"updateMemory",
			"Correct a saved memory by exact ID. Viewers can update only their own memories.",
			vec![("id", string(100)), ("content", string(4000))],
			false,
			false,
			false,
		),
		definition(
			"deleteMemory",
			"Delete an exact memory ID. Viewers can delete only their own.",
			vec![("id", string(100))],
			false,
			false,
			false,
		),
		definition(
			"createReminder",
			"Schedule a Discord DM to the current requester. dueAt is ISO8601 with explicit UTC offset, in the next year. Delivery requires this app to be running; interrupted deliveries are never automatically repeated.",
			vec![("content", string(2000)), ("dueAt", string(80))],
			false,
			false,
			false,
		),
		definition(
			"listReminders",
			"List reminders and their pending, delivered or uncertain delivery states.",
			vec![],
			false,
			false,
			false,
		),
		definition(
			"cancelReminder",
			"Cancel an exact pending reminder ID. It cannot retract a message already dispatched.",
			vec![("id", string(100))],
			false,
			false,
			false,
		),
		definition(
			"clearConversationHistory",
			"Clear this requester's history in this conversation. This does not delete durable memories.",
			vec![],
			false,
			false,
			false,
		),
		definition(
			"getCurrentSettings",
			"Read current application and overlay settings, without credentials.",
			vec![],
			true,
			false,
			false,
		),
		definition(
			"getSettingOptions",
			"List built-in puppets, separate curated Bumblebee voices, eligible chatter voices and supported wake words.",
			vec![],
			true,
			false,
			false,
		),
		definition(
			"getRuntimeStatus",
			"Read current local session, connection and Discord audio status. Status describes observed runtime state, not hypothetical provider capabilities.",
			vec![],
			true,
			false,
			false,
		),
		definition(
			"getRecentChatContext",
			"Read this requester's recent messages in this exact conversation. Private and public histories never share a scope.",
			vec![],
			false,
			false,
			false,
		),
		definition(
			"getOverlayLayout",
			"Read the persisted renderer layout shared by preview and OBS.",
			vec![],
			true,
			false,
			false,
		),
        ToolDefinition { name:"setOverlaySettings".into(), description:"Patch retained preview/OBS widgets. Null leaves a field unchanged. Positions are 0–100 percent; sizes are fractions. Hidden controls never enable voice capture or permissions.".into(), parameters:crate::overlay::patch_schema(),owner_only:true,requires_confirmation:false,external_effect:false },
		definition(
			"setBumblebeeVoice",
			"Select one of Bumblebee's own curated voice IDs. Chatter voice customization is separate.",
			vec![("voiceId", string(100))],
			true,
			false,
			false,
		),
		definition(
			"setChatTtsSettings",
			"Patch chat speech readout and queue behavior. Null leaves a field unchanged.",
            vec![("enabled",nullable(boolean())),("chatAiDictationEnabled",nullable(boolean())),("chatTtsWaitingToneEnabled",nullable(boolean())),("chatTtsSpeakerIntroCooldownSeconds",nullable(json!({"type":"integer","minimum":0,"maximum":600}))),("chatTtsInterruptSilenceMs",nullable(json!({"type":"integer","minimum":0,"maximum":10000}))),("chatTtsQueueExpirationMs",nullable(json!({"type":"integer","minimum":1000,"maximum":600000}))),("chatTtsBlockedWords",nullable(array(string(100),500)))],
			true,
			false,
			false,
		),
		definition(
			"setAiSettings",
			"Change the OpenAI model or enable/disable AI. Null leaves a field unchanged. This tool cannot enable platform write grants or change credentials.",
			vec![
				("model", nullable(string(200))),
                ("enabled", nullable(boolean())),
                ("openaiVoiceModel",nullable(string(200))),
                ("openaiReasoningEffort",nullable(enumeration(&["default","none","minimal","low","medium","high","xhigh"]))),
                ("openaiVoiceReasoningEffort",nullable(enumeration(&["default","none","minimal","low","medium","high","xhigh"]))),
                ("imageModel",nullable(string(200))),
                ("aiWebSearchEnabled",nullable(boolean())),("aiCodeInterpreterEnabled",nullable(boolean())),("aiImageGenerationEnabled",nullable(boolean())),("aiMemoriesEnabled",nullable(boolean())),("aiRemindersEnabled",nullable(boolean())) ,
			],
			true,
			false,
			false,
		),
        definition("setAudioSettings","Patch audio routing and volume. Null leaves a field unchanged. Discord output requires an active voice connection.",vec![("audioOutput",nullable(enumeration(&["overlay","discord"]))),("masterVolume",nullable(number(0.,2.))),("bumblebeeTtsVolume",nullable(number(0.,2.))),("puppetTtsVolume",nullable(number(0.,2.))),("wakeChirpVolume",nullable(number(0.,2.))),("thinkingSoundVolume",nullable(number(0.,2.))),("chatTtsWaitingToneVolume",nullable(number(0.,2.)))],true,false,false),
		definition(
			"setVoiceMentionSettings",
			"Patch Discord listening and replay settings. Permission widening requires explicit approval; a block always overrides an allow entry. Null leaves a field unchanged.",
			vec![
				("enabled",nullable(boolean())),
                ("wakeKeywordSensitivity",nullable(enumeration(&["strict","balanced","loose"]))),
                ("stopKeywordSensitivity",nullable(enumeration(&["strict","balanced","loose"]))),
                ("cancelKeywordSensitivity",nullable(enumeration(&["strict","balanced","loose"]))),
                ("listenEveryone", nullable(boolean())),
				("listenRoleIds", nullable(array(string(20), 200))),
				("allowedUserIds", nullable(array(string(20), 200))),
				("blockedUserIds", nullable(array(string(20), 200))),
				(
					"wakeWord",
					nullable(enumeration(&["bumblebee", "hey_bumblebee"])),
				),
				("replayEnabled", nullable(boolean())),
				(
					"replaySeconds",
					nullable(json!({"type":"integer","minimum":5,"maximum":120})),
				),
			],
			true,
			true,
			false,
		),
		definition(
			"setDiscordConnectionSettings",
			"Change configured Discord guild/text/voice destinations by exact IDs. Null leaves a field unchanged; empty voiceChannelId leaves voice. Does not change the owner identity.",
			vec![
				("guildId", nullable(string(20))),
				("textChannelId", nullable(string(20))),
				("voiceChannelId", nullable(string(20))),
			],
			true,
			true,
			false,
		),
		definition(
			"undoLastAction",
			"Undo the most recent local settings change only if those settings still match that action's saved result. External platform actions are not silently reversed.",
			vec![],
			true,
			true,
			false,
		),
		definition(
			"listChatterProfiles",
			"Search local chatter profiles by display name or stable platform user ID.",
			vec![("search", string(200))],
			true,
			false,
			false,
		),
		definition(
			"resetChatterProfile",
			"Restore an exact local chatter profile to random built-in puppet and voice, invalidating pending custom images. Its block status remains unchanged.",
			vec![
				("platform", enumeration(&["twitch", "youtube", "discord"])),
				("userId", string(128)),
			],
			true,
			false,
			false,
		),
		definition(
			"blockChatterCustomization",
			"Block or unblock an exact viewer's customization. Blocking cancels pending images; existing approved appearance remains until reset.",
			vec![
				("platform", enumeration(&["twitch", "youtube", "discord"])),
				("userId", string(128)),
				("blocked", boolean()),
			],
			true,
			false,
			false,
		),
		definition(
			"saveVoiceReplay",
			"Save the current permitted Discord replay buffer as a local WAV artifact. Replay must already be enabled; never invent a recording when it is disabled.",
			vec![],
			true,
			false,
			false,
		),
		definition(
			"listGeneratedArtifacts",
			"List local generated artifacts available for exact-image edits or explicit attachment delivery. Files remain private unless explicitly delivered or shown on stream.",
			vec![],
			true,
			false,
			false,
		),
		definition(
			"researchWeb",
			"Perform web research using OpenAI's web tool and return source citations to this same turn. Full on-stream presentation requires explicit streamer intent.",
			vec![("query", string(12000)), ("showOnStream", boolean())],
			false,
			false,
			true,
		),
		definition(
			"analyzeWithCodeInterpreter",
			"Run code-backed analysis in OpenAI's hosted code interpreter, not on this computer. Return observed output and generated file artifacts to this turn.",
			vec![("task", string(12000)), ("showOnStream", boolean())],
			false,
			false,
			true,
		),
		definition(
			"generateImage",
			"Generate one requested image, save its bytes locally, and return an artifact ID. Call separately for distinct images. Full on-stream display requires explicit streamer intent.",
			vec![
				("prompt", string(12000)),
				("background", enumeration(&["opaque", "transparent"])),
				("showOnStream", boolean()),
			],
			false,
			false,
			true,
		),
		definition(
			"editImage",
			"Edit exact available image artifact IDs using their saved original bytes. Ask for an original when it is missing; never substitute text-only generation. Streamer can select previous local artifacts; viewers can edit this turn's artifacts.",
			vec![
				("prompt", string(12000)),
				("sourceImageIds", array(string(100), 5)),
				("background", enumeration(&["opaque", "transparent"])),
				("showOnStream", boolean()),
			],
			false,
			false,
			true,
		),
	];
	tools.extend(platform_tools::definitions());
	tools
}

pub fn apply_result(cp: &mut Checkpoint, output: &Value) -> Result<()> {
	if let Some(value) = output.get("delivery") {
		cp.delivery = Some(serde_json::from_value(value.clone())?);
	}
	if let Some(artifacts) = output["artifacts"].as_array() {
		for value in artifacts {
			let artifact: Artifact = serde_json::from_value(value.clone())?;
			if !cp.artifacts.iter().any(|a| a.id == artifact.id) {
				cp.artifacts.push(artifact);
			}
		}
	}
	Ok(())
}

pub async fn execute(
	engine: &Engine,
	cp: &mut Checkpoint,
	call: &ToolCall,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	let owner = super::is_owner(engine, &cp.source).await?;
	ensure!(
		crate::settings::tool_enabled(&engine.store.settings()?, &call.name),
		"This capability is disabled in Settings"
	);
	let actor = durable::actor(&cp.source);
	let text = |key: &str| -> Result<&str> {
		args[key]
			.as_str()
			.with_context(|| format!("{key} is required"))
	};
	let result = match call.name.as_str() {
		"configureTurnDelivery" => {
			json!({"status":"configured","delivery":delivery::configure(engine,cp,args).await?})
		}
		"progressUpdate" => {
			let policy = cp.delivery.as_ref().context("Delivery is not configured")?;
			if !policy.public_progress {
				json!({"status":"suppressed"})
			} else {
				let receipt = engine
					.send_message(&cp.source, text("text")?)
					.await
					.map_err(|_| {
						platform_tools::UncertainOutcome("Progress delivery outcome is unknown".into())
					})?;
				json!({"status":"delivered","messageIds":receipt})
			}
		}
		"discoverConnectedCapabilities" => {
			let query = args["query"].as_str().unwrap_or("").to_lowercase();
			let offset = args["offset"].as_u64().unwrap_or(0) as usize;
			let limit = args["limit"].as_u64().unwrap_or(20) as usize;
			let enabled = engine.store.settings()?;
			let all: Vec<_> = definitions()
				.into_iter()
				.filter(|d| crate::settings::tool_enabled(&enabled, &d.name))
				.filter(|d| {
					(!d.owner_only || owner)
						&& format!("{} {}", d.name, d.description)
							.to_lowercase()
							.contains(&query)
				})
				.collect();
			json!({"status":"advertised","total":all.len(),"nextOffset":if offset+limit<all.len(){Some(offset+limit)}else{None},"tools":all.iter().skip(offset).take(limit).map(|d|json!({"name":d.name,"description":d.description,"parameters":d.parameters})).collect::<Vec<_>>()})
		}
		"deliverMessage" => delivery::deliver(engine, cp, args).await?,
		"rememberMemory" => {
			json!({"status":"saved","memory":engine.store.remember(&actor,text("content")?)?})
		}
		"listMemories" => {
			if owner {
				cp.owner_context = true;
			}
			json!({"status":"observed","memories":engine.store.memories(&actor,owner)?})
		}
		"updateMemory" => {
			engine
				.store
				.update_memory(text("id")?, &actor, owner, text("content")?)?;
			json!({"status":"updated"})
		}
		"deleteMemory" => {
			engine.store.delete_memory(text("id")?, &actor, owner)?;
			json!({"status":"deleted"})
		}
		"createReminder" => {
			let due = chrono::DateTime::parse_from_rfc3339(text("dueAt")?)
				.context("Use ISO8601 with an explicit UTC offset for the reminder")?
				.timestamp_millis();
			json!({"status":"scheduled","reminder":engine.store.create_reminder(&cp.source,text("content")?,due)?})
		}
		"listReminders" => {
			json!({"status":"observed","reminders":engine.store.reminders(&actor,owner)?})
		}
		"cancelReminder" => {
			engine.store.cancel_reminder(text("id")?, &actor, owner)?;
			json!({"status":"cancelled"})
		}
		"clearConversationHistory" => {
			json!({"status":"cleared","count":engine.store.clear_history(&durable::conversation_scope(&cp.source))?})
		}
		"getCurrentSettings" => {
			json!({"status":"observed","settings":engine.store.settings()?,"overlay":overlay(engine)?})
		}
		"getSettingOptions" => {
			json!({"status":"observed","bumblebeeVoices":crate::catalog::voices().into_iter().filter(|v|v.role=="bumblebee").collect::<Vec<_>>(),"chatterVoices":crate::catalog::eligible_voices(&engine.store.catalog_voices()?),"puppets":crate::catalog::puppets(),"wakeWords":["bumblebee","hey_bumblebee"]})
		}
		"getRuntimeStatus" => {
			json!({"status":"observed","sessionActive":engine.is_active(),"connections":engine.providers.statuses(),"audio":if let Some(audio)=engine.audio().await{audio.diagnostics()}else{Value::Null}})
		}
		"getRecentChatContext" => {
			json!({"status":"observed","history":engine.store.history(&durable::conversation_scope(&cp.source))?})
		}
		"getOverlayLayout" => json!({"status":"observed","settings":overlay(engine)?}),
		"setOverlaySettings" => {
			let before = serde_json::to_value(overlay(engine)?)?;
			let mut next = before.clone();
			patch(&mut next, args)?;
			let settings: OverlaySettings = serde_json::from_value(next.clone())?;
			settings.validate()?;
			save_setting(engine, "overlay_settings", &before, &next)?;
			engine.overlay_settings_changed(&settings)?;
			engine.emit(OverlayEvent::OverlaySettings { settings });
			json!({"status":"applied","settings":next})
		}
		"setBumblebeeVoice"
		| "setChatTtsSettings"
		| "setAudioSettings"
		| "setAiSettings"
		| "setVoiceMentionSettings"
		| "setDiscordConnectionSettings" => {
			let before = serde_json::to_value(engine.store.settings()?)?;
			let mut next = before.clone();
			match call.name.as_str() {
				"setBumblebeeVoice" => next["bumblebeeVoice"] = args["voiceId"].clone(),
				"setAudioSettings" => patch(&mut next, args)?,
				"setChatTtsSettings" => {
					if !args["enabled"].is_null() {
						next["readChat"] = args["enabled"].clone();
					}
					for key in [
						"chatAiDictationEnabled",
						"chatTtsWaitingToneEnabled",
						"chatTtsSpeakerIntroCooldownSeconds",
						"chatTtsInterruptSilenceMs",
						"chatTtsQueueExpirationMs",
						"chatTtsBlockedWords",
					] {
						if !args[key].is_null() {
							next[key] = args[key].clone();
						}
					}
				}
				"setAiSettings" => {
					for key in [
						"openaiVoiceModel",
						"openaiReasoningEffort",
						"openaiVoiceReasoningEffort",
						"imageModel",
						"aiWebSearchEnabled",
						"aiCodeInterpreterEnabled",
						"aiImageGenerationEnabled",
						"aiMemoriesEnabled",
						"aiRemindersEnabled",
					] {
						if !args[key].is_null() {
							next[key] = args[key].clone();
						}
					}
					if !args["model"].is_null() {
						next["openaiModel"] = args["model"].clone();
					}
					if !args["enabled"].is_null() {
						next["aiEnabled"] = args["enabled"].clone();
					}
				}
				"setVoiceMentionSettings" => {
					for (from, to) in [
						("enabled", "voiceMentionsEnabled"),
						("wakeKeywordSensitivity", "wakeKeywordSensitivity"),
						("stopKeywordSensitivity", "stopKeywordSensitivity"),
						("cancelKeywordSensitivity", "cancelKeywordSensitivity"),
						("listenEveryone", "discordListenEveryone"),
						("listenRoleIds", "discordListenRoleIds"),
						("allowedUserIds", "discordListenAllowedUserIds"),
						("blockedUserIds", "discordListenBlockedUserIds"),
						("wakeWord", "wakeWord"),
						("replayEnabled", "replayEnabled"),
						("replaySeconds", "replaySeconds"),
					] {
						if !args[from].is_null() {
							next[to] = args[from].clone();
						}
					}
				}
				"setDiscordConnectionSettings" => {
					for (from, to) in [
						("guildId", "discordGuildId"),
						("textChannelId", "discordTextChannelId"),
						("voiceChannelId", "discordVoiceChannelId"),
					] {
						if !args[from].is_null() {
							next[to] = args[from].clone();
						}
					}
				}
				_ => {}
			}
			let settings: Settings = serde_json::from_value(next.clone())?;
			settings.validate()?;
			save_setting(engine, "installation", &before, &next)?;
			let refresh = engine
				.settings_changed_from_agent(&serde_json::from_value(before)?)
				.await;
			match refresh {
				Ok(()) => json!({"status":"applied","settings":next}),
				Err(error) => {
					json!({"status":"runtime_pending","settingsSaved":true,"error":error.to_string()})
				}
			}
		}
		"undoLastAction" => {
			let undo: Value = engine
				.store
				.get("settings_undo")?
				.context("There is no local settings change to undo")?;
			let key = undo["key"].as_str().context("Invalid undo record")?;
			ensure!(
				matches!(key, "installation" | "overlay_settings"),
				"Invalid undo setting"
			);
			save_setting(engine, key, &undo["after"], &undo["before"])?;
			if key == "overlay_settings" {
				engine.overlay_settings_changed(&serde_json::from_value(undo["before"].clone())?)?;
				engine.emit(OverlayEvent::OverlaySettings {
					settings: serde_json::from_value(undo["before"].clone())?,
				});
			} else {
				engine
					.settings_changed_from_agent(&serde_json::from_value(undo["after"].clone())?)
					.await?;
			}
			json!({"status":"restored","settings":undo["before"]})
		}
		"listChatterProfiles" => {
			json!({"status":"observed","chatters":engine.store.chatters(text("search")?)?})
		}
		"resetChatterProfile" => {
			let chatter = engine
				.store
				.reset_chatter(text("platform")?, text("userId")?)?;
			engine.emit(OverlayEvent::ChatterChanged {
				chatter: chatter.clone(),
			});
			json!({"status":"reset","chatter":chatter})
		}
		"blockChatterCustomization" => {
			engine.store.block_customization(
				text("platform")?,
				text("userId")?,
				args["blocked"]
					.as_bool()
					.context("blocked must be boolean")?,
			)?;
			let chatter = engine.store.chatter(text("platform")?, text("userId")?)?;
			engine.emit(OverlayEvent::ChatterChanged {
				chatter: chatter.clone(),
			});
			json!({"status":"updated","chatter":chatter})
		}
		"saveVoiceReplay" => {
			let replay = engine
				.audio()
				.await
				.context("Discord voice is disconnected")?
				.replay_snapshot()?;
			let artifact = managed::save_artifact(
				engine,
				&replay.wav,
				"wav",
				"audio/wav",
				"Discord voice replay",
			)
			.await?;
			json!({"status":"saved","artifacts":[artifact],"durationMs":replay.duration_ms,"sourcePacketLossFrames":replay.source_packet_loss_frames})
		}
		"listGeneratedArtifacts" => {
			json!({"status":"observed","artifacts":engine.store.get::<Vec<Artifact>>("generated_artifacts")?.unwrap_or_default()})
		}
		"researchWeb" | "analyzeWithCodeInterpreter" | "generateImage" | "editImage" => {
			managed::execute(engine, cp, &call.name, args, cancel).await?
		}
		_ => platform_tools::execute(engine, &cp.source, &call.name, args, cancel).await?,
	};
	Ok(result)
}
fn overlay(engine: &Engine) -> Result<OverlaySettings> {
	Ok(engine.store.get("overlay_settings")?.unwrap_or_default())
}
fn patch(target: &mut Value, value: &Value) -> Result<()> {
	crate::settings::merge_patch(target, value)
}

fn save_setting(engine: &Engine, key: &str, before: &Value, after: &Value) -> Result<()> {
	let mut db = engine.store.db()?;
	let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
	let current: Option<String> = tx
		.query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
			r.get(0)
		})
		.optional()?;
	let current = match current {
		Some(value) => serde_json::from_str::<Value>(&value)?,
		None if key == "installation" => serde_json::to_value(Settings::default())?,
		None => serde_json::to_value(OverlaySettings::default())?,
	};
	ensure!(
		current == *before,
		"Settings changed since this action read them; inspect current settings before trying again"
	);
	tx.execute("INSERT INTO settings(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,after.to_string()])?;
	tx.execute("INSERT INTO settings(key,value) VALUES ('settings_undo',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[json!({"key":key,"before":before,"after":after}).to_string()])?;
	tx.commit()?;
	Ok(())
}
