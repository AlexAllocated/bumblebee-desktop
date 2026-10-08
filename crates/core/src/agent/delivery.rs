use super::{
	Artifact, Checkpoint, Delivery, FinalReply, ReplyRoute, platform_tools::UncertainOutcome,
};
use crate::{
	agent_storage::PendingInput,
	model::{ChatMessage, OverlayEvent},
	runtime::{Engine, split_message},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct Upload {
	pub filename: String,
	pub media_type: String,
	pub bytes: Vec<u8>,
}

pub async fn configure(engine: &Engine, cp: &Checkpoint, args: &Value) -> Result<Delivery> {
	ensure!(
		cp.delivery.is_none(),
		"The original request's delivery policy is already fixed"
	);
	let mut delivery: Delivery = serde_json::from_value(args.clone())?;
	ensure!(
		delivery.targets.len() <= 2
			&& delivery
				.targets
				.iter()
				.all(|t| matches!(t.as_str(), "source" | "discord_dm")),
		"Invalid delivery destination"
	);
	delivery.targets.sort();
	delivery.targets.dedup();
	if cp.source.platform == "discord"
		&& cp.source.channel_id != engine.store.settings()?.discord_text_channel_id
	{
		delivery.speech = false;
		delivery.public_progress = false;
	}
	if delivery.targets.iter().any(|t| t == "discord_dm") {
		let requester = if crate::agent_storage::platform(&cp.source.platform) == "discord" {
			cp.source.user_id.clone()
		} else {
			super::require_owner(engine, &cp.source).await?;
			engine.store.settings()?.owner_discord_id
		};
		let recipient = delivery
			.discord_dm_user_id
			.clone()
			.unwrap_or(requester.clone());
		if recipient != requester {
			super::require_owner(engine, &cp.source).await?;
		}
		delivery.dm_channel = Some(create_dm(engine, &recipient).await?);
		delivery.discord_dm_user_id = Some(recipient);
		// A private final destination never silently falls back to the public stream.
		delivery.public_progress = false;
		if !delivery.targets.iter().any(|t| t == "source") {
			delivery.speech = false;
		}
	}
	if delivery.targets.is_empty() {
		delivery.speech = false;
		delivery.public_progress = false;
	}
	Ok(delivery)
}

#[derive(Debug, PartialEq, Eq)]
enum PromptDestination {
	Source,
	Dashboard,
	RequesterDm { user_id: String, owner_link: bool },
}
fn prompt_destination(
	source: &ChatMessage,
	delivery: &Delivery,
	owner: bool,
	owner_discord_id: &str,
) -> PromptDestination {
	if delivery.targets.is_empty() {
		return PromptDestination::Dashboard;
	}
	if delivery.targets.iter().any(|t| t == "discord_dm") {
		// Final delivery to somebody else does not give them the requester's authority.
		if crate::agent_storage::platform(&source.platform) == "discord" {
			return PromptDestination::RequesterDm {
				user_id: source.user_id.clone(),
				owner_link: false,
			};
		}
		if owner && !owner_discord_id.is_empty() {
			return PromptDestination::RequesterDm {
				user_id: owner_discord_id.into(),
				owner_link: true,
			};
		}
		// No verified cross-platform identity: keep the question in trusted local UI.
		return PromptDestination::Dashboard;
	}
	if delivery.targets.iter().any(|t| t == "source") {
		PromptDestination::Source
	} else {
		PromptDestination::Dashboard
	}
}
pub(super) async fn reply_route(
	engine: &Engine,
	cp: &Checkpoint,
	owner: bool,
) -> Result<ReplyRoute> {
	let policy = cp.delivery.as_ref().context("Delivery is not configured")?;
	Ok(
		match prompt_destination(
			&cp.source,
			policy,
			owner,
			&engine.store.settings()?.owner_discord_id,
		) {
			PromptDestination::Dashboard => ReplyRoute::Dashboard,
			PromptDestination::Source => ReplyRoute::Source,
			PromptDestination::RequesterDm {
				user_id,
				owner_link,
			} => {
				let channel = if policy.discord_dm_user_id.as_deref() == Some(user_id.as_str()) {
					policy.dm_channel.clone()
				} else {
					None
				};
				let channel = match channel {
					Some(channel) => Ok(channel),
					None => create_dm(engine, &user_id).await,
				};
				match channel {
					Ok(channel_id) => ReplyRoute::DiscordDm {
						user_id,
						channel_id,
						owner_link,
					},
					// A failed private route must never fall back to a public prompt.
					Err(_) => ReplyRoute::Dashboard,
				}
			}
		},
	)
}

pub async fn prompt(
	engine: &Engine,
	cp: &Checkpoint,
	pending: &PendingInput,
	cancel: CancellationToken,
) -> Result<()> {
	let choices = if pending.choices.is_empty() {
		String::new()
	} else {
		format!(" Choices: {}.", pending.choices.join(" / "))
	};
	let text = format!(
		"{}{} Reply with your answer, or !answer {} <answer>. Use cancel to stop this request.",
		pending.prompt, choices, pending.id
	);
	let policy = cp.delivery.as_ref().context("Delivery is not configured")?;
	match cp
		.reply_route
		.as_ref()
		.context("Pending reply route is missing")?
	{
		ReplyRoute::Dashboard => {} // The durable pending record is already visible in the desktop UI.
		ReplyRoute::DiscordDm { channel_id, .. } => {
			send_discord(engine, channel_id, &text, &[]).await?;
		}
		ReplyRoute::Source => {
			ensure!(
				policy.targets.iter().any(|t| t == "source"),
				"Public prompts are not permitted by this delivery policy"
			);
			engine.send_message(&cp.source, &text).await?;
			if policy.speech && cp.source.platform == "discord_voice" {
				engine.speak(&text, None, cancel).await?;
			}
		}
	}
	Ok(())
}

pub async fn finish(
	engine: &Engine,
	cp: &Checkpoint,
	reply: &FinalReply,
	cancel: CancellationToken,
) -> Result<Value> {
	let policy = cp.delivery.as_ref().context("Delivery is not configured")?;
	let groups = reply.messages.clone().unwrap_or_else(|| {
		vec![super::FinalMessage {
			text: reply.text.clone(),
			artifact_ids: vec![],
		}]
	});
	let mut receipts = Vec::new();
	for group in groups {
		ensure!(!cancel.is_cancelled(), "Delivery cancelled");
		if group.text.is_empty() && group.artifact_ids.is_empty() {
			continue;
		}
		let uploads = load_uploads(engine, cp, &group.artifact_ids).await?;
		for target in &policy.targets {
			let receipt = match target.as_str() {
				"source" => send_source(engine, &cp.source, &group.text, &uploads).await?,
				"discord_dm" => {
					send_discord(
						engine,
						policy
							.dm_channel
							.as_deref()
							.context("Private destination is unavailable")?,
						&group.text,
						&uploads,
					)
					.await?
				}
				_ => anyhow::bail!("Unknown final destination"),
			};
			receipts.push(json!({"target":target,"receipt":receipt}));
		}
		if policy.speech && !group.text.is_empty() {
			engine.speak(&group.text, None, cancel.clone()).await?;
		}
	}
	Ok(json!({"status":"delivered","receipts":receipts}))
}

pub async fn deliver(engine: &Engine, cp: &Checkpoint, args: &Value) -> Result<Value> {
	let target = args["target"]
		.as_str()
		.context("A delivery target is required")?;
	let text = args["text"].as_str().context("Message text is required")?;
	let ids = args["artifactIds"]
		.as_array()
		.context("Artifact IDs must be an array")?
		.iter()
		.map(|v| v.as_str().context("Invalid artifact ID").map(str::to_owned))
		.collect::<Result<Vec<_>>>()?;
	let uploads = load_uploads(engine, cp, &ids).await?;
	let destination = args["destinationId"].as_str();
	let receipt = match target {
		"source" => send_source(engine, &cp.source, text, &uploads).await?,
		"discord_dm" => {
			let source_discord = crate::agent_storage::platform(&cp.source.platform) == "discord";
			let recipient = destination
				.or(if source_discord {
					Some(cp.source.user_id.as_str())
				} else {
					None
				})
				.context("Resolve an exact Discord recipient ID first")?;
			if !source_discord || recipient != cp.source.user_id {
				super::require_owner(engine, &cp.source).await?;
			}
			send_dm(engine, recipient, text, &uploads).await?
		}
		"discord_channel" => {
			super::require_owner(engine, &cp.source).await?;
			let channel = destination.context("An exact Discord channel ID is required")?;
			ensure!(
				uploads.is_empty(),
				"Send file attachments to the source Discord channel or a requester DM"
			);
			super::platform_tools::send_discord_message(engine, &cp.source, channel, text).await?
		}
		"twitch" | "youtube" => {
			super::require_owner(engine, &cp.source).await?;
			ensure!(
				uploads.is_empty(),
				"Twitch and YouTube chat do not support file attachments; use the dashboard or Discord"
			);
			let channel = destination.context("An exact destination ID is required")?;
			let settings = engine.store.settings()?;
			if target == "youtube" {
				ensure!(
					channel == settings.youtube_live_chat_id,
					"Destination must be the connected YouTube live chat"
				);
			} else {
				ensure!(
					channel == engine.providers.tokens("twitch").await?.account_id,
					"Destination must be the authorized Twitch channel"
				);
			}
			let mut source = cp.source.clone();
			source.platform = target.into();
			source.channel_id = channel.into();
			json!({"messageIds":engine.send_message(&source,text).await.map_err(|_|UncertainOutcome("Message delivery did not return a complete receipt; inspect chat before retrying".into()))?})
		}
		_ => anyhow::bail!("Unknown delivery target"),
	};
	Ok(json!({"status":"delivered","receipt":receipt}))
}

async fn send_source(
	engine: &Engine,
	source: &ChatMessage,
	text: &str,
	uploads: &[Upload],
) -> Result<Value> {
	if source.platform == "preview" {
		if !text.is_empty() {
			engine.emit(OverlayEvent::Status {
				message: text.into(),
			});
		}
		return Ok(
			json!({"status":"available_in_dashboard","files":uploads.iter().map(|u|u.filename.as_str()).collect::<Vec<_>>()}),
		);
	}
	if !uploads.is_empty() {
		ensure!(
			crate::agent_storage::platform(&source.platform) == "discord",
			"Twitch and YouTube cannot receive file attachments; the generated files are available in the desktop dashboard"
		);
		return send_discord(engine, &source.channel_id, text, uploads).await;
	}
	Ok(
		json!({"messageIds":engine.send_message(source,text).await.map_err(|_|UncertainOutcome("Message delivery did not return a complete receipt; inspect chat before retrying".into()))?}),
	)
}

pub async fn create_dm(engine: &Engine, recipient: &str) -> Result<String> {
	ensure!(
		recipient.parse::<u64>().is_ok_and(|n| n > 0),
		"Resolve an exact Discord recipient ID first"
	);
	let response = engine
		.providers
		.http
		.post("https://discord.com/api/v10/users/@me/channels")
		.header(
			reqwest::header::AUTHORIZATION,
			format!("Bot {}", engine.providers.secret("discord_bot")?),
		)
		.json(&json!({"recipient_id":recipient}))
		.send()
		.await
		.context("Cannot create Discord DM")?;
	crate::providers::check_response("discord", &response)?;
	let value: Value = response
		.json()
		.await
		.context("Discord DM response was invalid")?;
	let id = value["id"]
		.as_str()
		.context("Discord returned no DM channel ID")?;
	ensure!(
		id.parse::<u64>().is_ok_and(|n| n > 0),
		"Discord returned an invalid DM channel ID"
	);
	Ok(id.into())
}
pub async fn send_dm(
	engine: &Engine,
	recipient: &str,
	text: &str,
	uploads: &[Upload],
) -> Result<Value> {
	let channel = create_dm(engine, recipient).await?;
	send_discord(engine, &channel, text, uploads).await
}
async fn send_discord(
	engine: &Engine,
	channel: &str,
	text: &str,
	uploads: &[Upload],
) -> Result<Value> {
	ensure!(
		channel.parse::<u64>().is_ok_and(|n| n > 0),
		"Invalid Discord channel ID"
	);
	ensure!(
		uploads.len() <= 10,
		"Discord supports at most ten files per message"
	);
	let token = engine.providers.secret("discord_bot")?;
	let parts = if text.is_empty() {
		vec![String::new()]
	} else {
		split_message(text, 1900)
	};
	let mut ids = Vec::new();
	for (part_number, part) in parts.iter().enumerate() {
		let mut body = json!({"content":part,"allowed_mentions":{"parse":[]}});
		let mut request = engine
			.providers
			.http
			.post(format!(
				"https://discord.com/api/v10/channels/{channel}/messages"
			))
			.header(reqwest::header::AUTHORIZATION, format!("Bot {token}"));
		if part_number == 0 && !uploads.is_empty() {
			body["attachments"] = json!(
				uploads
					.iter()
					.enumerate()
					.map(|(i, u)| json!({"id":i,"filename":u.filename}))
					.collect::<Vec<_>>()
			);
			let mut form = reqwest::multipart::Form::new().text("payload_json", body.to_string());
			for (index, upload) in uploads.iter().enumerate() {
				ensure!(
					upload.bytes.len() <= 8 * 1024 * 1024,
					"Attachment exceeds the 8 MiB delivery limit; open it in the desktop dashboard"
				);
				form = form.part(
					format!("files[{index}]"),
					reqwest::multipart::Part::bytes(upload.bytes.clone())
						.file_name(upload.filename.clone())
						.mime_str(&upload.media_type)?,
				);
			}
			request = request.multipart(form);
		} else {
			request = request.json(&body);
		}
		let response = request.send().await.map_err(|_| {
			UncertainOutcome(
				"Discord message outcome is unknown; inspect the destination before retrying".into(),
			)
		})?;
		if !response.status().is_success() {
			let code = response.status().as_u16();
			if !ids.is_empty() || code >= 500 || code == 408 {
				return Err(UncertainOutcome(format!("Discord delivery was partial or uncertain (HTTP {code}); inspect the destination before retrying")).into());
			}
			crate::providers::check_response("discord", &response)?;
		}
		let response: Value = response.json().await.map_err(|_| {
			UncertainOutcome("Discord acknowledged the message but its receipt was unreadable".into())
		})?;
		ids.push(
			response["id"]
				.as_str()
				.context("Discord receipt has no message ID")
				.map_err(|_| {
					UncertainOutcome(
						"Discord acknowledged delivery without an identifiable message receipt".into(),
					)
				})?
				.to_owned(),
		);
	}
	Ok(json!({"channelId":channel,"messageIds":ids}))
}

async fn load_uploads(engine: &Engine, cp: &Checkpoint, ids: &[String]) -> Result<Vec<Upload>> {
	ensure!(ids.len() <= 10, "Too many attachments");
	let mut result = Vec::new();
	for id in ids {
		let artifact = cp
			.artifacts
			.iter()
			.find(|a| a.id == *id)
			.context("This artifact does not belong to the current request")?;
		result.push(load_artifact(engine, artifact).await?);
	}
	Ok(result)
}
pub async fn load_artifact(engine: &Engine, artifact: &Artifact) -> Result<Upload> {
	ensure!(
		!artifact.filename.is_empty()
			&& artifact
				.filename
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
			&& !artifact.filename.starts_with('.'),
		"Invalid artifact filename"
	);
	let path = engine
		.paths
		.data_dir
		.join("artifacts")
		.join(&artifact.filename);
	let metadata = tokio::fs::metadata(&path)
		.await
		.context("Generated file is missing")?;
	ensure!(
		metadata.is_file() && metadata.len() <= 8 * 1024 * 1024,
		"File exceeds the 8 MiB attachment limit"
	);
	Ok(Upload {
		filename: artifact.filename.clone(),
		media_type: artifact.media_type.clone(),
		bytes: tokio::fs::read(path).await?,
	})
}

#[cfg(test)]
mod routing_tests {
	use super::*;
	fn source(platform: &str) -> ChatMessage {
		ChatMessage {
			platform: platform.into(),
			user_id: "123".into(),
			channel_id: "10".into(),
			display_name: "Requester".into(),
			message_id: "request".into(),
			text: "private request".into(),
			is_owner: true,
		}
	}
	fn private() -> Delivery {
		Delivery {
			speech: false,
			public_progress: false,
			targets: vec!["discord_dm".into()],
			discord_dm_user_id: Some("777".into()),
			dm_channel: Some("88".into()),
		}
	}
	#[test]
	fn final_third_party_recipient_never_inherits_prompt_authority() {
		for platform in ["discord", "discord_voice"] {
			assert_eq!(
				prompt_destination(&source(platform), &private(), false, "999"),
				PromptDestination::RequesterDm {
					user_id: "123".into(),
					owner_link: false
				}
			);
		}
		for platform in ["twitch", "youtube"] {
			assert_eq!(
				prompt_destination(&source(platform), &private(), true, "999"),
				PromptDestination::RequesterDm {
					user_id: "999".into(),
					owner_link: true
				}
			);
			assert_eq!(
				prompt_destination(&source(platform), &private(), false, "999"),
				PromptDestination::Dashboard
			);
			assert_eq!(
				prompt_destination(&source(platform), &private(), true, ""),
				PromptDestination::Dashboard
			);
		}
	}
	#[test]
	fn silent_delivery_cannot_fall_back_to_public_question() {
		let mut policy = private();
		policy.targets.clear();
		assert_eq!(
			prompt_destination(&source("twitch"), &policy, true, "999"),
			PromptDestination::Dashboard
		);
		policy.targets = vec!["source".into()];
		assert_eq!(
			prompt_destination(&source("twitch"), &policy, true, "999"),
			PromptDestination::Source
		);
	}
}
