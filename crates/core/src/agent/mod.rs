//! One durable tool loop shared by chat and Discord voice.
mod delivery;
mod managed;
mod model;
pub mod platform_tools;
#[cfg(feature = "live-probes")]
pub mod probe;
mod tools;

use crate::{
	agent_storage::{self as durable, PendingInput},
	model::{ChatMessage, OverlayEvent},
	runtime::Engine,
	storage::Store,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct ToolDefinition {
	pub name: String,
	pub description: String,
	pub parameters: Value,
	pub owner_only: bool,
	pub requires_confirmation: bool,
	pub external_effect: bool,
}
impl ToolDefinition {
	fn wire(&self) -> Value {
		json!({"type":"function","name":self.name,"description":self.description,"parameters":self.parameters,"strict":true})
	}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
	pub id: String,
	pub name: String,
	pub arguments: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Delivery {
	pub speech: bool,
	pub public_progress: bool,
	pub targets: Vec<String>,
	pub discord_dm_user_id: Option<String>,
	#[serde(default)]
	pub dm_channel: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Artifact {
	pub id: String,
	pub filename: String,
	pub media_type: String,
	pub label: String,
}
/// The transport allowed to answer a saved question. This never changes the turn's actor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyRoute {
	Source,
	Dashboard,
	DiscordDm {
		user_id: String,
		channel_id: String,
		owner_link: bool,
	},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
	pub id: String,
	pub source: ChatMessage,
	pub model: String,
	pub items: Vec<Value>,
	pub rounds: usize,
	pub executed: usize,
	pub calls: Vec<ToolCall>,
	pub cursor: usize,
	pub delivery: Option<Delivery>,
	pub artifacts: Vec<Artifact>,
	pub pending: Option<PendingInput>,
	#[serde(default)]
	pub reply_route: Option<ReplyRoute>,
	pub answer: Option<String>,
	pub approved_call: Option<String>,
	#[serde(default)]
	pub voice_channel_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FinalMessage {
	pub text: String,
	pub artifact_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalReply {
	pub text: String,
	pub messages: Option<Vec<FinalMessage>>,
}

pub async fn is_owner(engine: &Engine, source: &ChatMessage) -> Result<bool> {
	if source.platform == "discord_voice" {
		ensure!(
			engine
				.audio()
				.await
				.context("Discord voice is disconnected")?
				.revalidate_listener(&source.user_id)
				.await?,
			"Voice listening permission changed; this request is stopped"
		);
	}
	Ok(match durable::platform(&source.platform) {
		"preview" => source.user_id == "owner",
		"discord" => {
			!engine.store.settings()?.owner_discord_id.is_empty()
				&& source.user_id == engine.store.settings()?.owner_discord_id
		}
		"twitch" => engine.providers.tokens("twitch").await?.account_id == source.user_id,
		"youtube" => engine.providers.tokens("google").await?.account_id == source.user_id,
		_ => false,
	})
}
pub(super) async fn require_owner(engine: &Engine, source: &ChatMessage) -> Result<()> {
	ensure!(
		is_owner(engine, source).await?,
		"This action requires the currently authorized streamer's permission"
	);
	Ok(())
}

/// Only trusted desktop IPC creates this reserved platform. Provider adapters use fixed names.
pub fn is_desktop_answer(message: &ChatMessage) -> bool {
	message.platform == "desktop_answer" && message.user_id == "owner"
}
fn matches_reply(store: &Store, pending: &PendingInput, message: &ChatMessage) -> Result<bool> {
	if is_desktop_answer(message) {
		return Ok(message.channel_id == pending.id);
	}
	// Most chat messages cannot answer this request; avoid loading unrelated saved model context.
	let original =
		pending.actor == durable::actor(message) && pending.channel == durable::channel(message);
	let possible_dm = message.platform == "discord"
		&& (pending.actor == durable::actor(message)
			|| message.user_id == store.settings()?.owner_discord_id);
	if !original && !possible_dm {
		return Ok(false);
	}
	let checkpoint = store.turn(&pending.turn_id)?.checkpoint;
	let route: Option<ReplyRoute> = checkpoint
		.get("reply_route")
		.filter(|v| !v.is_null())
		.map(|v| serde_json::from_value(v.clone()))
		.transpose()?;
	Ok(match route.as_ref().unwrap_or(&ReplyRoute::Source) {
		ReplyRoute::Dashboard => false,
		ReplyRoute::Source => {
			pending.actor == durable::actor(message) && pending.channel == durable::channel(message)
		}
		ReplyRoute::DiscordDm {
			user_id,
			channel_id,
			owner_link,
		} => {
			message.platform == "discord"
				&& &message.user_id == user_id
				&& &message.channel_id == channel_id
				&& (!owner_link || store.settings()?.owner_discord_id == *user_id)
		}
	})
}
fn matching_pending(store: &Store, message: &ChatMessage) -> Result<Vec<PendingInput>> {
	store
		.pending_inputs()?
		.into_iter()
		.filter_map(|p| match matches_reply(store, &p, message) {
			Ok(true) => Some(Ok(p)),
			Ok(false) => None,
			Err(e) => Some(Err(e)),
		})
		.collect()
}
pub fn pending_for_message(store: &Store, message: &ChatMessage) -> Result<bool> {
	Ok(!matching_pending(store, message)?.is_empty())
}
pub fn pending_actor_for_message(store: &Store, message: &ChatMessage) -> Result<Option<String>> {
	let candidates = matching_pending(store, message)?;
	let code = message
		.text
		.trim()
		.strip_prefix("!answer ")
		.and_then(|s| s.split_whitespace().next());
	Ok(candidates
		.iter()
		.find(|p| code.is_some_and(|id| p.id.eq_ignore_ascii_case(id)))
		.or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
		.map(|p| p.actor.clone()))
}
pub fn pending_to_cancel(store: &Store, message: &ChatMessage) -> Result<Vec<PendingInput>> {
	store
		.pending_inputs()?
		.into_iter()
		.filter_map(|p| {
			if p.actor == durable::actor(message) && p.channel == durable::channel(message) {
				return Some(Ok(p));
			}
			match matches_reply(store, &p, message) {
				Ok(true) => Some(Ok(p)),
				Ok(false) => None,
				Err(e) => Some(Err(e)),
			}
		})
		.collect()
}
pub fn desktop_answer_message(store: &Store, id: &str, answer: &str) -> Result<ChatMessage> {
	ensure!(
		store.pending_inputs()?.iter().any(|p| p.id == id),
		"This question expired or was already answered"
	);
	Ok(ChatMessage {
		platform: "desktop_answer".into(),
		user_id: "owner".into(),
		display_name: "Streamer".into(),
		message_id: uuid::Uuid::new_v4().to_string(),
		channel_id: id.into(),
		text: format!("!answer {id} {answer}"),
		is_owner: true,
	})
}
pub async fn run(
	engine: Arc<Engine>,
	message: ChatMessage,
	cancel: CancellationToken,
) -> Result<()> {
	ensure!(!cancel.is_cancelled(), "Request cancelled before starting");
	let host = RuntimeHost { engine };
	if resume(&host, &message, cancel.clone()).await? {
		return Ok(());
	}
	ensure!(
		!is_desktop_answer(&message),
		"This question expired or was already answered"
	);
	ensure!(
		host.engine.store.settings()?.ai_enabled,
		"AI is disabled in Settings"
	);
	let settings = host.engine.store.settings()?;
	ensure!(
		!settings.openai_model.is_empty(),
		"Choose an OpenAI model in Settings"
	);
	let scope = durable::conversation_scope(&message);
	let context = json!({"history":host.store().history(&scope)?,"memories":host.store().memories(&durable::actor(&message),false)?,"displayName":message.display_name,"platform":message.platform,"request":message.text});
	let voice_channel_id =
		(message.platform == "discord_voice").then_some(settings.discord_voice_channel_id.clone());
	let mut cp = Checkpoint {
		id: uuid::Uuid::new_v4().to_string(),
		source: message,
		model: settings.openai_model,
		items: vec![json!({"role":"user","content":context.to_string()})],
		rounds: 0,
		executed: 0,
		calls: vec![],
		cursor: 0,
		delivery: None,
		artifacts: vec![],
		pending: None,
		reply_route: None,
		answer: None,
		approved_call: None,
		voice_channel_id,
	};
	host.store().create_turn(
		&cp.id,
		&durable::actor(&cp.source),
		&serde_json::to_value(&cp)?,
	)?;
	host
		.store()
		.append_history(&scope, "user", &cp.source.text)?;
	run_guarded(&host, &mut cp, cancel).await
}

pub async fn handle_pending(
	engine: Arc<Engine>,
	message: &ChatMessage,
	cancel: CancellationToken,
) -> Result<bool> {
	resume(&RuntimeHost { engine }, message, cancel).await
}

trait Host: Sync {
	fn reply_route(
		&self,
		cp: &Checkpoint,
		_owner: bool,
	) -> impl Future<Output = Result<ReplyRoute>> + Send {
		async {
			Ok(
				if cp.delivery.as_ref().is_some_and(|d| d.targets.is_empty()) {
					ReplyRoute::Dashboard
				} else {
					ReplyRoute::Source
				},
			)
		}
	}
	fn store(&self) -> &Store;
	fn definitions(&self) -> Vec<ToolDefinition>;
	fn owner(&self, source: &ChatMessage) -> impl Future<Output = Result<bool>> + Send;
	fn request(
		&self,
		cp: &Checkpoint,
		definitions: &[ToolDefinition],
		owner: bool,
		cancel: CancellationToken,
	) -> impl Future<Output = Result<Value>> + Send;
	fn execute(
		&self,
		cp: &mut Checkpoint,
		call: &ToolCall,
		args: &Value,
		cancel: CancellationToken,
	) -> impl Future<Output = Result<Value>> + Send;
	fn prompt(
		&self,
		cp: &Checkpoint,
		pending: &PendingInput,
		cancel: CancellationToken,
	) -> impl Future<Output = Result<()>> + Send;
	fn finish(
		&self,
		cp: &Checkpoint,
		reply: &FinalReply,
		cancel: CancellationToken,
	) -> impl Future<Output = Result<Value>> + Send;
}
struct RuntimeHost {
	engine: Arc<Engine>,
}
impl Host for RuntimeHost {
	async fn reply_route(&self, cp: &Checkpoint, owner: bool) -> Result<ReplyRoute> {
		delivery::reply_route(&self.engine, cp, owner).await
	}
	fn store(&self) -> &Store {
		&self.engine.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		tools::definitions()
	}
	async fn owner(&self, source: &ChatMessage) -> Result<bool> {
		is_owner(&self.engine, source).await
	}
	async fn request(
		&self,
		cp: &Checkpoint,
		defs: &[ToolDefinition],
		owner: bool,
		cancel: CancellationToken,
	) -> Result<Value> {
		validate_voice_context(&self.engine, cp).await?;
		model::request(&self.engine, cp, defs, owner, cancel).await
	}
	async fn execute(
		&self,
		cp: &mut Checkpoint,
		call: &ToolCall,
		args: &Value,
		cancel: CancellationToken,
	) -> Result<Value> {
		validate_voice_context(&self.engine, cp).await?;
		tools::execute(&self.engine, cp, call, args, cancel).await
	}
	async fn prompt(
		&self,
		cp: &Checkpoint,
		pending: &PendingInput,
		cancel: CancellationToken,
	) -> Result<()> {
		delivery::prompt(&self.engine, cp, pending, cancel).await
	}
	async fn finish(
		&self,
		cp: &Checkpoint,
		reply: &FinalReply,
		cancel: CancellationToken,
	) -> Result<Value> {
		validate_voice_context(&self.engine, cp).await?;
		delivery::finish(&self.engine, cp, reply, cancel).await
	}
}
async fn validate_voice_context(engine: &Engine, cp: &Checkpoint) -> Result<()> {
	if cp.source.platform == "discord_voice" {
		ensure!(
			cp.voice_channel_id.as_deref()
				== Some(engine.store.settings()?.discord_voice_channel_id.as_str()),
			"The voice destination changed; this saved request cannot continue automatically"
		);
		let _ = is_owner(engine, &cp.source).await?;
	}
	Ok(())
}

async fn resume<H: Host>(
	host: &H,
	message: &ChatMessage,
	cancel: CancellationToken,
) -> Result<bool> {
	ensure!(!cancel.is_cancelled(), "Request cancelled before answering");
	let candidates = matching_pending(host.store(), message)?;
	if candidates.is_empty() {
		return Ok(false);
	}
	let text = message.text.trim();
	let (code, answer) = if let Some(rest) = text.strip_prefix("!answer ") {
		let (code, answer) = rest
			.split_once(char::is_whitespace)
			.context("Use !answer <question code> <answer>")?;
		(Some(code), answer.trim())
	} else {
		(None, text)
	};
	let pending = match code {
		Some(code) => candidates
			.iter()
			.find(|p| p.id.eq_ignore_ascii_case(code))
			.context("No pending question matches that code")?,
		None if candidates.len() == 1 => &candidates[0],
		_ => bail!("Several questions are pending. Use !answer <question code> <answer>."),
	};
	let mut cp: Checkpoint =
		serde_json::from_value(host.store().turn(&pending.turn_id)?.checkpoint)?;
	ensure!(
		cp.pending.as_ref().is_some_and(|p| p.id == pending.id),
		"Pending question checkpoint does not match"
	);
	let owner = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled during permission check"),result=host.owner(&cp.source)=>result?};
	ensure!(
		!cancel.is_cancelled(),
		"Request cancelled after permission check"
	);
	if let Some(ReplyRoute::DiscordDm {
		user_id,
		owner_link: true,
		..
	}) = &cp.reply_route
	{
		ensure!(
			owner && host.store().settings()?.owner_discord_id == *user_id,
			"The linked streamer identity changed; this private request cannot continue"
		);
	}
	if matches!(
		answer.to_ascii_lowercase().as_str(),
		"cancel" | "never mind" | "!cancel"
	) {
		host.store().cancel_agent_turn(&cp.id)?;
		return Ok(true);
	}
	let resolved = if pending.kind == "confirmation" {
		match answer.to_ascii_lowercase().as_str() {
			"yes" | "confirm" | "approve" => "yes".to_string(),
			"no" | "reject" | "decline" => "no".to_string(),
			_ => bail!("Reply yes or no, or use cancel to stop the remaining request"),
		}
	} else if pending.choices.is_empty() {
		answer.to_owned()
	} else if let Some(choice) = pending
		.choices
		.iter()
		.find(|choice| choice.eq_ignore_ascii_case(answer))
	{
		choice.clone()
	} else {
		bail!("Choose one of the saved choices, or use cancel")
	};
	cp.answer = Some(resolved.clone());
	if pending.kind == "confirmation" && resolved == "yes" {
		cp.approved_call = cp.calls.get(cp.cursor).map(|c| c.id.clone());
	}
	cp.pending = None;
	ensure!(
		!cancel.is_cancelled(),
		"Request cancelled before consuming answer"
	);
	host.store().consume_pending(
		&pending.id,
		&pending.actor,
		&pending.channel,
		owner,
		&resolved,
		&serde_json::to_value(&cp)?,
	)?;
	run_guarded(host, &mut cp, cancel).await?;
	Ok(true)
}
async fn run_guarded<H: Host>(
	host: &H,
	cp: &mut Checkpoint,
	cancel: CancellationToken,
) -> Result<()> {
	let result = drive(host, cp, cancel.clone()).await;
	if result.is_err() {
		let current = host.store().turn(&cp.id)?;
		if current.state == "waiting" && cancel.is_cancelled() {
			host.store().cancel_agent_turn(&cp.id)?;
		} else if current.state == "running" {
			host.store().checkpoint_turn(
				&cp.id,
				if cancel.is_cancelled() {
					"interrupted"
				} else {
					"failed"
				},
				&serde_json::to_value(&cp)?,
			)?;
		}
	}
	result
}
async fn drive<H: Host>(host: &H, cp: &mut Checkpoint, cancel: CancellationToken) -> Result<()> {
	loop {
		ensure!(!cancel.is_cancelled(), "Request cancelled");
		if cp.cursor < cp.calls.len() {
			let call = cp.calls[cp.cursor].clone();
			let defs = host.definitions();
			let def = defs
				.iter()
				.find(|d| d.name == call.name)
				.context("Model requested an unadvertised tool")?;
			let args: Value =
				serde_json::from_str(&call.arguments).context("Tool arguments were not valid JSON")?;
			model::validate(&def.parameters, &args)?;
			ensure!(
				cp.delivery.is_some() || call.name == "configureTurnDelivery",
				"Delivery must be configured before executing actions"
			);
			let owner = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled during permission check"),result=host.owner(&cp.source)=>result?};
			ensure!(
				!cancel.is_cancelled(),
				"Request cancelled after permission check"
			);
			let denied = def.owner_only && !owner;
			if !denied
				&& ((def.requires_confirmation
					&& cp.approved_call.as_deref() != Some(&call.id)
					&& cp.answer.as_deref() != Some("no"))
					|| (call.name == "requestUserInput" && cp.answer.is_none()))
			{
				let confirmation = call.name != "requestUserInput";
				let prompt = if confirmation {
					format!("Approve {} with these exact details: {}?", call.name, args)
				} else {
					args["question"]
						.as_str()
						.context("A question is required")?
						.into()
				};
				let choices = if confirmation {
					vec!["yes".into(), "no".into()]
				} else {
					args["choices"]
						.as_array()
						.context("Choices must be an array")?
						.iter()
						.map(|v| v.as_str().context("Invalid choice").map(str::to_owned))
						.collect::<Result<Vec<_>>>()?
				};
				cp.reply_route = Some(tokio::select! { biased;
					 _=cancel.cancelled()=>bail!("Request cancelled before choosing a prompt destination"),
					 result=host.reply_route(cp,owner)=>result?,
				});
				let pending = PendingInput {
					id: uuid::Uuid::new_v4().simple().to_string()[..8].into(),
					turn_id: cp.id.clone(),
					actor: durable::actor(&cp.source),
					channel: durable::channel(&cp.source),
					kind: if confirmation {
						"confirmation"
					} else {
						"question"
					}
					.into(),
					prompt,
					choices,
					owner_required: def.owner_only,
					expires_at: crate::now_ms() + 86_400_000,
				};
				cp.pending = Some(pending.clone());
				host
					.store()
					.suspend_turn(&pending, &serde_json::to_value(&cp)?)?;
				tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled before question delivery"),result=host.prompt(cp,&pending,cancel.clone())=>result?};
				ensure!(
					!cancel.is_cancelled(),
					"Request cancelled while asking a question"
				);
				return Ok(());
			}
			ensure!(
				cp.executed < 96,
				"This request reached its tool execution limit"
			);
			ensure!(!cancel.is_cancelled(), "Request cancelled before dispatch");
			let cached = host
				.store()
				.begin_call(&cp.id, &call.id, &call.name, &call.arguments)?;
			let output = if let Some(result) = cached {
				serde_json::from_str(&result).context("Saved tool result was invalid")?
			} else {
				cp.executed += 1;
				checkpoint(host, cp, "running")?;
				let result = if denied {
					Ok(json!({"status":"rejected","error":"Current owner permission is required"}))
				} else if def.requires_confirmation && cp.answer.as_deref() == Some("no") {
					Ok(json!({"status":"declined","executed":false}))
				} else if call.name == "requestUserInput" {
					Ok(
						json!({"status":"answered","answer":cp.answer.take().context("Saved answer missing")?}),
					)
				} else {
					tokio::select! {
						 biased;
						 _=cancel.cancelled()=>{
							  if def.external_effect{host.store().mark_call_uncertain(&cp.id,&call.id)?;}else{host.store().finish_call(&cp.id,&call.id,&json!({"status":"cancelled"}).to_string())?;}
							  bail!("Request cancelled; an in-flight external action may have an unknown outcome")
						 },
						 result=host.execute(cp,&call,&args,cancel.clone())=>result,
					}
				};
				let output = match result {
					Ok(output) => output,
					Err(error)
						if error
							.downcast_ref::<platform_tools::UncertainOutcome>()
							.is_some() =>
					{
						json!({"status":"unknown","error":error.to_string()})
					}
					Err(error) => json!({"status":"failed","error":error.to_string()}),
				};
				if output["status"] == "unknown" {
					host
						.store()
						.record_unknown_call(&cp.id, &call.id, &output.to_string())?;
				} else {
					host
						.store()
						.finish_call(&cp.id, &call.id, &output.to_string())?;
				}
				output
			};
			tools::apply_result(cp, &output)?;
			cp.answer = None;
			cp.approved_call = None;
			cp.items.push(json!({"type":"function_call_output","call_id":call.id,"output":json!({"untrustedToolResult":output}).to_string()}));
			cp.cursor += 1;
			checkpoint(host, cp, "running")?;
			continue;
		}
		ensure!(cp.rounds < 24, "This request reached its model round limit");
		let owner = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled during permission check"),result=host.owner(&cp.source)=>result?};
		ensure!(
			!cancel.is_cancelled(),
			"Request cancelled after permission check"
		);
		let defs: Vec<_> = host
			.definitions()
			.into_iter()
			.filter(|d| !d.owner_only || owner)
			.collect();
		cp.rounds += 1;
		checkpoint(host, cp, "running")?;
		let response = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled"),response=host.request(cp,&defs,owner,cancel.clone())=>response?};
		let (items, calls, reply) = model::parse_response(response)?;
		cp.items.extend(items);
		cp.calls = calls;
		cp.cursor = 0;
		checkpoint(host, cp, "running")?;
		if !cp.calls.is_empty() {
			continue;
		}
		ensure!(
			cp.delivery.is_some(),
			"Model finished without configuring delivery"
		);
		let reply = reply.context("Model did not return a terminal response")?;
		ensure!(
			!cancel.is_cancelled(),
			"Request cancelled before final delivery"
		);
		checkpoint(host, cp, "delivering")?;
		let result = tokio::select! {biased;_=cancel.cancelled()=>Err(anyhow::anyhow!("Final delivery interrupted; it will not be repeated automatically")),result=host.finish(cp,&reply,cancel.clone())=>result};
		match result {
			Ok(receipts) => {
				host.store().append_history(
					&durable::conversation_scope(&cp.source),
					"assistant",
					&reply.text,
				)?;
				let mut final_checkpoint = serde_json::to_value(&cp)?;
				final_checkpoint["deliveryReceipts"] = receipts;
				host
					.store()
					.checkpoint_turn(&cp.id, "completed", &final_checkpoint)?;
				return Ok(());
			}
			Err(error) => {
				checkpoint(host, cp, "unknown")?;
				return Err(error);
			}
		}
	}
}
fn checkpoint<H: Host>(host: &H, cp: &Checkpoint, state: &str) -> Result<()> {
	let value = serde_json::to_value(cp)?;
	ensure!(
		serde_json::to_vec(&value)?.len() <= 8 * 1024 * 1024,
		"This request reached its saved context limit; no further actions were dispatched"
	);
	host.store().checkpoint_turn(&cp.id, state, &value)
}

pub async fn reminder_loop(engine: Arc<Engine>, cancel: CancellationToken) {
	let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
	loop {
		tokio::select! {_=cancel.cancelled()=>break,_=tick.tick()=>{}}
		for _ in 0..16 {
			if cancel.is_cancelled() {
				return;
			}
			let reminder = match engine.store.claim_due_reminder(crate::now_ms()) {
				Ok(Some(r)) => r,
				Ok(None) => break,
				Err(e) => {
					engine.emit(OverlayEvent::Status {
						message: e.to_string(),
					});
					break;
				}
			};
			let result = tokio::select! {_=cancel.cancelled()=>Err(anyhow::anyhow!("Reminder delivery interrupted")),result=delivery::send_dm(&engine,&reminder.destination.user_id,&reminder.content,&[])=>result};
			match result {
				Ok(receipt) => {
					let _ = engine
						.store
						.finish_reminder(&reminder.id, Some(&receipt.to_string()));
				}
				Err(error) => {
					let _ = engine.store.finish_reminder(&reminder.id, None);
					engine.emit(OverlayEvent::Status {
						message: format!(
							"Reminder delivery outcome is unknown; inspect it before retrying: {error}"
						),
					});
				}
			}
		}
	}
}
#[cfg(test)]
mod tests;
