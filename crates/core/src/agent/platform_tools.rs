//! Retained streaming tools. Discovery returns IDs; mutations bind those exact IDs
//! before confirmation and recheck local grants and provider permissions at execution.
mod discord;
mod twitch;
mod youtube;

use super::ToolDefinition;
use crate::{model::ChatMessage, runtime::Engine};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct UncertainOutcome(pub String);
impl std::fmt::Display for UncertainOutcome {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}
impl std::error::Error for UncertainOutcome {}

pub fn required_twitch_scopes(groups: &[String]) -> Vec<&'static str> {
	let mut scopes = vec!["user:read:chat", "user:write:chat"];
	for group in groups {
		match group.as_str() {
			"twitch_broadcast" => scopes.extend([
				"channel:manage:broadcast",
				"channel:manage:raids",
				"channel:edit:commercial",
				"clips:edit",
			]),
			"twitch_moderation" => scopes.extend([
				"moderator:manage:banned_users",
				"moderator:manage:chat_settings",
				"moderator:manage:shoutouts",
			]),
			"twitch_polls" => scopes.push("channel:manage:polls"),
			_ => {}
		}
	}
	scopes.sort_unstable();
	scopes.dedup();
	scopes
}

fn string(max: usize) -> Value {
	json!({"type":"string","minLength":1,"maxLength":max})
}
fn id() -> Value {
	json!({"type":"string","pattern":"^[0-9]{1,20}$","maxLength":20})
}
fn optional(value: Value) -> Value {
	json!({"anyOf":[value,{"type":"null"}]})
}
fn integer(min: i64, max: i64) -> Value {
	json!({"type":"integer","minimum":min,"maximum":max})
}
fn boolean() -> Value {
	json!({"type":"boolean"})
}
fn choice(values: &[&str]) -> Value {
	json!({"type":"string","enum":values})
}
fn definition(
	name: &str,
	description: &str,
	fields: Vec<(&str, Value)>,
	confirmation: bool,
	external: bool,
) -> ToolDefinition {
	let required: Vec<_> = fields.iter().map(|(k, _)| *k).collect();
	let properties: serde_json::Map<_, _> = fields
		.iter()
		.map(|(k, v)| (k.to_string(), v.clone()))
		.collect();
	ToolDefinition {
		name: name.into(),
		description: description.into(),
		parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
		owner_only: true,
		requires_confirmation: confirmation,
		external_effect: external,
	}
}
pub fn definitions() -> Vec<ToolDefinition> {
	let mut tools = vec![definition(
		"resolveUserTarget",
		"Resolve platform user IDs for subsequent actions. Results may be ambiguous; ask the requester to choose rather than guessing.",
		vec![
			("platform", choice(&["twitch", "youtube", "discord"])),
			("query", string(100)),
		],
		false,
		false,
	)];
	tools.extend(discord::definitions());
	tools.extend(twitch::definitions());
	tools.extend(youtube::definitions());
	tools.push(definition("createPoll","Create one poll on the selected platform. durationSeconds and channelPointsPerVote apply only to Twitch; pass null for YouTube. Compose separate calls for multiple platforms, preserving each receipt independently.",vec![
        ("platform",choice(&["twitch","youtube"])),("channelId",string(256)),("question",string(100)),
        ("options",json!({"type":"array","items":string(100),"minItems":2,"maxItems":4})),
        ("durationSeconds",optional(integer(15,1800))),("channelPointsPerVote",optional(integer(0,1000000)))],true,true));
	tools.push(definition("closePoll","Close the exact discovered poll; a changed active poll does not retarget this action. YouTube always shows its poll result (showResults=true).",vec![
        ("platform",choice(&["twitch","youtube"])),("channelId",string(256)),("pollId",string(256)),("showResults",boolean())],true,true));
	tools
}

pub async fn execute(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	super::require_owner(engine, source).await?;
	ensure!(
		!cancel.is_cancelled(),
		"Tool execution canceled before starting"
	);
	if name == "resolveUserTarget" {
		return match text(args, "platform", 20)? {
			"discord" => discord::resolve(engine, source, args, cancel).await,
			"twitch" => twitch::resolve(engine, source, args, cancel).await,
			"youtube" => youtube::resolve(engine, source, args, cancel).await,
			_ => anyhow::bail!("Unsupported platform"),
		};
	}
	if matches!(name, "createPoll" | "closePoll") {
		return match text(args, "platform", 20)? {
			"twitch" => twitch::execute(engine, source, name, args, cancel).await,
			"youtube" => youtube::execute(engine, source, name, args, cancel).await,
			_ => anyhow::bail!("Unsupported poll platform"),
		};
	}
	if name.contains("Discord") || name == "discordManageRole" || name == "discordStageControlUser" {
		discord::execute(engine, source, name, args, cancel).await
	} else if name.contains("Twitch") || name == "twitchShoutout" {
		twitch::execute(engine, source, name, args, cancel).await
	} else if name.contains("Youtube") {
		youtube::execute(engine, source, name, args, cancel).await
	} else {
		anyhow::bail!("Unknown platform tool")
	}
}

pub(super) async fn preflight_discord(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	cancel: &CancellationToken,
) -> Result<Option<super::access::AccessBlocker>> {
	match discord::preflight(engine, source, name, args, cancel.clone()).await {
		Ok(()) => Ok(None),
		Err(error) => {
			if let Some(missing) = error.downcast_ref::<super::access::PermissionRequired>() {
				Ok(Some(super::access::AccessBlocker::repair(&missing.0)))
			} else {
				Err(error)
			}
		}
	}
}

pub async fn send_discord_message(
	engine: &Engine,
	source: &ChatMessage,
	channel_id: &str,
	text: &str,
) -> Result<Value> {
	super::require_owner(engine, source).await?;
	discord::send_message(engine, source, channel_id, text).await
}
fn grant(engine: &Engine, group: &str) -> Result<()> {
	ensure!(
		engine
			.store
			.settings()?
			.enabled_tool_groups
			.iter()
			.any(|s| s == group),
		"Enable the {group} tool group in Settings before using this action"
	);
	Ok(())
}
fn text<'a>(args: &'a Value, key: &str, max: usize) -> Result<&'a str> {
	let s = args[key]
		.as_str()
		.with_context(|| format!("{key} must be text"))?;
	ensure!(
		!s.trim().is_empty() && s.chars().count() <= max,
		"{key} must contain 1 to {max} characters"
	);
	Ok(s)
}
fn optional_text<'a>(args: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>> {
	if args[key].is_null() {
		Ok(None)
	} else {
		let s = args[key]
			.as_str()
			.with_context(|| format!("{key} must be text or null"))?;
		ensure!(s.chars().count() <= max, "{key} is too long");
		Ok(Some(s))
	}
}
fn number(args: &Value, key: &str, min: i64, max: i64) -> Result<i64> {
	let n = args[key]
		.as_i64()
		.with_context(|| format!("{key} must be an integer"))?;
	ensure!(
		(min..=max).contains(&n),
		"{key} must be between {min} and {max}"
	);
	Ok(n)
}
fn snowflake<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
	let s = text(args, key, 20)?;
	ensure!(
		s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u64>().is_ok_and(|id| id > 0),
		"{key} must be an exact provider ID from discovery"
	);
	Ok(s)
}
fn receipt(status: &str, data: Value) -> Value {
	json!({"status":status,"data":data})
}

/// A successful HTTP response is accepted evidence, not proof of read-back state.
/// Provider rejections are definite failures; lost responses after a write are
/// uncertain and must never trigger an automatic retry with a new call ID.
async fn request(
	builder: reqwest::RequestBuilder,
	provider: &str,
	write: bool,
	cancel: &CancellationToken,
) -> Result<Value> {
	ensure!(
		!cancel.is_cancelled(),
		"Tool execution canceled before request"
	);
	let result = tokio::select! {biased;_=cancel.cancelled()=>{if write{return Err(UncertainOutcome(format!("{provider} write was interrupted; inspect provider state before attempting another action")).into())}else{anyhow::bail!("Provider read canceled")}},r=builder.send()=>r};
	let response = match result {
		Ok(r) => r,
		Err(_) => {
			if write {
				return Err(
					UncertainOutcome(format!(
						"{provider} write outcome is unknown; do not retry automatically"
					))
					.into(),
				);
			} else {
				anyhow::bail!("Cannot reach {provider}")
			}
		}
	};
	let status = response.status();
	if !status.is_success() {
		if write && (status.is_server_error() || status.as_u16() == 408) {
			return Err(
				UncertainOutcome(format!(
					"{provider} returned HTTP {}; the write outcome is unknown",
					status.as_u16()
				))
				.into(),
			);
		}
		let reason = match status.as_u16() {
			401 => "Authorization expired",
			403 => "Provider permission denied",
			404 => "Resource no longer exists",
			429 => "Provider rate limit reached",
			_ => "Provider rejected request",
		};
		anyhow::bail!("{provider}: {reason} (HTTP {})", status.as_u16())
	}
	if status.as_u16() == 204 {
		return Ok(Value::Null);
	}
	use futures_util::StreamExt;
	let failed_body = |reason: &str| -> anyhow::Error {
		if write {
			UncertainOutcome(format!(
				"{provider} acknowledged a write but {reason}; inspect provider state before retrying"
			))
			.into()
		} else {
			anyhow::anyhow!("{provider} {reason}")
		}
	};
	if response
		.content_length()
		.is_some_and(|len| len > 4 * 1024 * 1024)
	{
		return Err(failed_body("its response exceeds 4 MiB"));
	}
	let mut stream = response.bytes_stream();
	let mut body = Vec::new();
	loop {
		let chunk = tokio::select! {biased;_=cancel.cancelled()=>return Err(failed_body("its response was interrupted")),chunk=stream.next()=>chunk};
		let Some(chunk) = chunk else { break };
		let chunk = chunk.map_err(|_| failed_body("its response failed"))?;
		if body.len() + chunk.len() > 4 * 1024 * 1024 {
			return Err(failed_body("its response exceeds 4 MiB"));
		}
		body.extend_from_slice(&chunk);
	}
	if body.is_empty() {
		return Ok(Value::Null);
	}
	serde_json::from_slice(&body).map_err(|_| {
		if write {
			UncertainOutcome(format!(
				"{provider} acknowledged a write but returned an unreadable receipt"
			))
			.into()
		} else {
			anyhow::anyhow!("Invalid {provider} response")
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn oauth_grants_are_explicit_and_minimal() {
		assert_eq!(
			required_twitch_scopes(&[]),
			vec!["user:read:chat", "user:write:chat"]
		);
		let scopes = required_twitch_scopes(&["twitch_polls".into()]);
		assert!(scopes.contains(&"channel:manage:polls"));
		assert!(!scopes.contains(&"moderator:manage:banned_users"));
	}
	#[test]
	fn exact_ids_reject_paths_and_zero() {
		for value in ["../members", "1?x=y", "0", "-2", "123abc"] {
			assert!(snowflake(&json!({"id":value}), "id").is_err())
		}
	}
	#[test]
	fn definitions_are_unique_strict_and_effects_are_owner_only() {
		let defs = definitions();
		let mut names = std::collections::HashSet::new();
		for def in defs {
			assert!(names.insert(def.name));
			assert!(def.owner_only);
			assert_eq!(def.parameters["additionalProperties"], false);
			let props = def.parameters["properties"].as_object().unwrap();
			assert_eq!(
				def.parameters["required"].as_array().unwrap().len(),
				props.len()
			);
		}
	}
	async fn mock_response(
		status: u16,
		body: String,
	) -> (
		String,
		std::sync::Arc<std::sync::atomic::AtomicUsize>,
		tokio::task::JoinHandle<()>,
	) {
		use axum::{Router, http::StatusCode, routing::any};
		let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let counter = calls.clone();
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let app = Router::new().fallback(any(move || {
			let counter = counter.clone();
			let body = body.clone();
			async move {
				counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
				(StatusCode::from_u16(status).unwrap(), body)
			}
		}));
		let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
		(format!("http://{address}/write"), calls, task)
	}
	#[tokio::test]
	async fn acknowledged_or_lost_writes_are_never_automatically_retried() {
		use std::sync::atomic::Ordering;
		for (status, body, uncertain) in [
			(503, "server failure".into(), true),
			(200, "unreadable receipt".into(), true),
			(403, "denied".into(), false),
			(200, "x".repeat(4 * 1024 * 1024 + 1), true),
		] {
			let (url, calls, server) = mock_response(status, body).await;
			let result = request(
				reqwest::Client::new().post(url),
				"Mock",
				true,
				&CancellationToken::new(),
			)
			.await
			.unwrap_err();
			assert_eq!(
				result.downcast_ref::<UncertainOutcome>().is_some(),
				uncertain,
				"{result}"
			);
			assert_eq!(calls.load(Ordering::SeqCst), 1);
			server.abort();
		}
		let (url, calls, server) = mock_response(204, String::new()).await;
		assert_eq!(
			request(
				reqwest::Client::new().post(url),
				"Mock",
				true,
				&CancellationToken::new()
			)
			.await
			.unwrap(),
			Value::Null
		);
		assert_eq!(calls.load(Ordering::SeqCst), 1);
		server.abort();
	}
	#[tokio::test]
	async fn cancel_after_server_received_write_leaves_unknown_outcome() {
		use axum::{Router, routing::post};
		let observed = std::sync::Arc::new(tokio::sync::Notify::new());
		let notify = observed.clone();
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let address = listener.local_addr().unwrap();
		let app = Router::new().route(
			"/write",
			post(move || {
				let notify = notify.clone();
				async move {
					notify.notify_one();
					std::future::pending::<()>().await;
					"{}"
				}
			}),
		);
		let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
		let cancel = CancellationToken::new();
		let child = cancel.clone();
		let writing = tokio::spawn(async move {
			request(
				reqwest::Client::new().post(format!("http://{address}/write")),
				"Mock",
				true,
				&child,
			)
			.await
		});
		tokio::time::timeout(std::time::Duration::from_secs(2), observed.notified())
			.await
			.unwrap();
		cancel.cancel();
		let error = writing.await.unwrap().unwrap_err();
		assert!(error.downcast_ref::<UncertainOutcome>().is_some());
		server.abort();
	}
}
