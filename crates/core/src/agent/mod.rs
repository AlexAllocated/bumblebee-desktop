//! One durable tool loop shared by chat and Discord voice.
mod delivery;
mod recovery;
pub(crate) use recovery::{recover_interrupted, recoverable_source};
mod access;
mod managed;
mod model;
mod receipts;
pub use receipts::RecoveryReceipt;
pub(crate) use receipts::{delivery_failure_receipt, send_parts};
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
	/// Only audited observations may be deliberately requested again after a
	/// crashed read. Local writes (memories, reminders, settings) are not reads,
	/// even when they do not have a remote external_effect.
	fn call_effect(&self) -> crate::storage::CallEffect {
		use crate::storage::CallEffect;
		if !self.external_effect
			&& matches!(
				self.name.as_str(),
				"discoverConnectedCapabilities"
					| "listMemories"
					| "listReminders"
					| "getCurrentSettings"
					| "getSettingOptions"
					| "getRuntimeStatus"
					| "getRecentChatContext"
					| "getOverlayLayout"
					| "listChatterProfiles"
					| "listGeneratedArtifacts"
					| "resolveUserTarget"
					| "inspectDiscordResources"
					| "inspectTwitchResources"
					| "inspectYoutubeResources"
			) {
			CallEffect::ReadOnly
		} else {
			CallEffect::MayMutate
		}
	}
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
	#[serde(default)]
	pub owner_context: bool,
	#[serde(default)]
	pub requester_was_owner: Option<bool>,
	#[serde(default)]
	pub access_bindings: access::AccessBindings,
	#[serde(default)]
	pub pending_final: Option<FinalReply>,
	#[serde(default)]
	pub failure_final: bool,
	#[serde(default)]
	pub recovery_eligible: bool,
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
	if !store.settings()?.ai_enabled
		&& !is_desktop_answer(message)
		&& !is_pending_cancel(&message.text)
		&& !message.text.trim().starts_with("!answer ")
	{
		return Ok(false);
	}
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
	ensure!(
		is_pending_cancel(answer) || store.settings()?.ai_enabled,
		"Enable the agent to answer this pending request"
	);
	Ok(ChatMessage {
		platform: "desktop_answer".into(),
		user_id: "owner".into(),
		display_name: "Streamer".into(),
		message_id: uuid::Uuid::new_v4().to_string(),
		channel_id: id.into(),
		text: format!("!answer {id} {answer}"),
		is_owner: true,
		access: Default::default(),
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

	let scope = durable::conversation_scope(&message);
	let overrides = host
		.store()
		.chatter(durable::platform(&message.platform), &message.user_id)
		.map(|c| c.overrides)
		.unwrap_or_default();
	let selected_model = settings.selected_model(&message, &overrides);
	ensure!(
		!selected_model.is_empty(),
		"Choose an OpenAI model in Settings"
	);
	let verified_owner = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled before memory access"),owner=host.owner(&message)=>owner?};
	let (memories, owner_context) = if settings.ai_memories_enabled {
		context_memories(host.store(), &message, verified_owner)?
	} else {
		(Vec::new(), false)
	};
	let context = json!({"history":host.store().history(&scope)?,"memories":memories,"displayName":message.display_name,"platform":message.platform,"request":message.text});
	let voice_channel_id =
		(message.platform == "discord_voice").then_some(settings.discord_voice_channel_id.clone());
	let mut cp = Checkpoint {
		id: uuid::Uuid::new_v4().to_string(),
		source: message,
		model: selected_model,
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
		owner_context,
		requester_was_owner: Some(verified_owner),
		access_bindings: Default::default(),
		pending_final: None,
		failure_final: false,
		recovery_eligible: true,
	};
	cp.access_bindings = access::AccessBindings::snapshot_source(&settings, &cp.source)?;
	tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled before binding its source"),result=access::bind_source(&host.engine,&cp.source,&mut cp.access_bindings,&cancel)=>result?};
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

fn context_memories(
	store: &Store,
	source: &ChatMessage,
	verified_owner: bool,
) -> Result<(Vec<durable::Memory>, bool)> {
	let actor = durable::actor(source);
	let mut memories = store.memories(&actor, false)?;
	let mut owner_context = false;
	if verified_owner && actor != "preview:owner" {
		let shared = store.memories("preview:owner", false)?;
		owner_context = !shared.is_empty();
		memories.extend(shared);
	}
	Ok((memories, owner_context))
}
fn require_context_owner(cp: &Checkpoint, owner: bool) -> Result<()> {
	ensure!(
		!(cp.owner_context || cp.requester_was_owner.unwrap_or(true)) || owner,
		"Owner identity changed; private owner context cannot continue under this identity"
	);
	Ok(())
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
	fn validate_checkpoint(
		&self,
		_cp: &Checkpoint,
		_cancel: &CancellationToken,
	) -> impl Future<Output = Result<()>> + Send {
		async { Ok(()) }
	}

	fn catalog(&self) -> Vec<ToolDefinition> {
		self.definitions()
	}
	fn preflight(
		&self,
		_cp: &mut Checkpoint,
		_call: &ToolCall,
		_args: &Value,
		_cancel: &CancellationToken,
	) -> impl Future<Output = Result<Option<access::AccessBlocker>>> + Send {
		async { Ok(None) }
	}
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
/// Nonverbal processing audio follows spoken delivery, not public text progress.
/// Before classification only a wake-triggered voice request gets neutral feedback.
pub(crate) fn with_thinking_feedback<'a, T: 'a, F>(
	engine: &'a Engine,
	source: &'a ChatMessage,
	delivery: Option<&'a Delivery>,
	variant: String,
	cancel: CancellationToken,
	failure_cue: bool,
	request: F,
) -> impl Future<Output = Result<T>> + 'a
where
	F: Future<Output = Result<T>> + 'a,
{
	// The real tool executor sits below queue, turn and cue futures. Heap-own
	// its state once so each orchestration layer does not copy it into its poll
	// frame; the full queue must fit Tokio's ordinary worker stack.
	let request = Box::pin(request);
	async move {
		let private_dm = source.platform == "discord"
			&& source.channel_id != engine.store.settings()?.discord_text_channel_id;
		let enabled = !private_dm
			&& delivery
				.map(|d| d.speech && d.targets.iter().any(|target| target == "source"))
				.unwrap_or(source.platform == "discord_voice");
		if failure_cue && source.platform == "discord_voice" && enabled {
			engine
				.while_voice_processing(variant, cancel, request)
				.await
		} else {
			engine
				.while_thinking(enabled, variant, cancel, request)
				.await
		}
	}
}

struct RuntimeHost {
	engine: Arc<Engine>,
}
impl RuntimeHost {
	fn require_enabled(&self) -> Result<()> {
		ensure!(
			self.engine.store.settings()?.ai_enabled,
			"The agent is disabled; enable it before requesting more work"
		);
		Ok(())
	}
}
impl Host for RuntimeHost {
	async fn validate_checkpoint(&self, cp: &Checkpoint, cancel: &CancellationToken) -> Result<()> {
		access::verify_bound_source(&self.engine, &cp.source, &cp.access_bindings, cancel).await?;
		self.require_enabled()
	}

	async fn reply_route(&self, cp: &Checkpoint, owner: bool) -> Result<ReplyRoute> {
		self.require_enabled()?;
		require_context_owner(cp, self.owner(&cp.source).await?)?;
		delivery::reply_route(&self.engine, cp, owner).await
	}
	fn catalog(&self) -> Vec<ToolDefinition> {
		tools::definitions()
	}
	async fn preflight(
		&self,
		cp: &mut Checkpoint,
		call: &ToolCall,
		args: &Value,
		cancel: &CancellationToken,
	) -> Result<Option<access::AccessBlocker>> {
		self.require_enabled()?;
		access::inspect(
			&self.engine,
			&cp.source,
			&call.name,
			args,
			&mut cp.access_bindings,
			cancel,
		)
		.await
	}

	fn store(&self) -> &Store {
		&self.engine.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		self
			.engine
			.store
			.settings()
			.map(|settings| {
				tools::definitions()
					.into_iter()
					.filter(|d| crate::settings::tool_enabled(&settings, &d.name))
					.collect()
			})
			.unwrap_or_default()
	}
	async fn owner(&self, source: &ChatMessage) -> Result<bool> {
		self.require_enabled()?;
		let owner = is_owner(&self.engine, source).await?;
		self.require_enabled()?;
		let settings = self.engine.store.settings()?;
		let overrides = self
			.engine
			.store
			.chatter(durable::platform(&source.platform), &source.user_id)
			.map(|c| c.overrides)
			.unwrap_or_default();
		ensure!(
			settings.permits_ai(source, &overrides, owner),
			"This requester no longer has permission to use the agent"
		);
		Ok(owner)
	}
	async fn request(
		&self,
		cp: &Checkpoint,
		defs: &[ToolDefinition],
		_owner: bool,
		cancel: CancellationToken,
	) -> Result<Value> {
		self.require_enabled()?;
		validate_voice_context(&self.engine, cp).await?;
		let owner = self.owner(&cp.source).await?;
		require_context_owner(cp, owner)?;
		self.validate_checkpoint(cp, &cancel).await?;
		self.require_enabled()?;
		with_thinking_feedback(
			&self.engine,
			&cp.source,
			cp.delivery.as_ref(),
			cp.id.clone(),
			cancel.clone(),
			true,
			model::request(&self.engine, cp, defs, owner, cancel),
		)
		.await
	}
	async fn execute(
		&self,
		cp: &mut Checkpoint,
		call: &ToolCall,
		args: &Value,
		cancel: CancellationToken,
	) -> Result<Value> {
		self.require_enabled()?;
		validate_voice_context(&self.engine, cp).await?;
		let owner = self.owner(&cp.source).await?;
		require_context_owner(cp, owner)?;
		self.validate_checkpoint(cp, &cancel).await?;
		self.require_enabled()?;
		// Progress is itself user-facing playback, not background tool work.
		if call.name == "progressUpdate" {
			return tools::execute(&self.engine, cp, call, args, cancel).await;
		}
		let source = cp.source.clone();
		// Classification is already known when this tool is dispatched. Honor a
		// silent/private proposal immediately, including its DM creation request.
		let delivery = if call.name == "configureTurnDelivery" {
			Some(
				serde_json::from_value::<Delivery>(args.clone()).unwrap_or(Delivery {
					speech: false,
					public_progress: false,
					targets: vec![],
					discord_dm_user_id: None,
					dm_channel: None,
				}),
			)
		} else {
			cp.delivery.clone()
		};
		with_thinking_feedback(
			&self.engine,
			&source,
			delivery.as_ref(),
			cp.id.clone(),
			cancel.clone(),
			false,
			tools::execute(&self.engine, cp, call, args, cancel),
		)
		.await
	}
	async fn prompt(
		&self,
		cp: &Checkpoint,
		pending: &PendingInput,
		cancel: CancellationToken,
	) -> Result<()> {
		self.require_enabled()?;
		require_context_owner(cp, self.owner(&cp.source).await?)?;
		self.validate_checkpoint(cp, &cancel).await?;
		delivery::prompt(&self.engine, cp, pending, cancel).await
	}
	async fn finish(
		&self,
		cp: &Checkpoint,
		reply: &FinalReply,
		cancel: CancellationToken,
	) -> Result<Value> {
		self.require_enabled()?;
		validate_voice_context(&self.engine, cp).await?;
		let owner = self.owner(&cp.source).await?;
		require_context_owner(cp, owner)?;
		self.validate_checkpoint(cp, &cancel).await?;
		self.require_enabled()?;
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
	// Canceling an already-owned pending request never requires model/provider work.
	if is_pending_cancel(answer) {
		host.store().cancel_agent_turn(&cp.id)?;
		return Ok(true);
	}
	ensure!(
		host.store().settings()?.ai_enabled,
		"Enable the agent to answer this pending request"
	);
	cp.source.access = if !is_desktop_answer(message)
		&& durable::actor(message) == durable::actor(&cp.source)
		&& durable::channel(message) == durable::channel(&cp.source)
	{
		message.access.clone()
	} else {
		Default::default()
	};
	cp.source.is_owner = false;
	tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled before continuing"),result=host.validate_checkpoint(&cp,&cancel)=>result?};
	let owner = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled during permission check"),result=host.owner(&cp.source)=>result?};
	require_context_owner(&cp, owner)?;
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
	let resolved = if pending.kind == "confirmation" {
		let answer = if message.platform == "discord_voice" {
			spoken_confirmation(answer, &host.store().settings()?.wake_word)
		} else {
			answer
		};
		match answer.to_ascii_lowercase().as_str() {
			"yes" | "confirm" | "approve" => "yes".to_string(),
			"no" | "reject" | "decline" => "no".to_string(),
			_ => bail!("Reply yes or no, or use cancel to stop the remaining request"),
		}
	} else if pending.choices.is_empty() {
		answer.to_owned()
	} else {
		let settings = host.store().settings()?;
		let spoken = (message.platform == "discord_voice")
			.then(|| spoken_answer_body(answer, &settings.wake_word));
		// Literal choices win, including punctuation that is part of a saved label.
		pending
			.choices
			.iter()
			.find(|choice| choice.eq_ignore_ascii_case(answer))
			.or_else(|| {
				spoken.and_then(|body| {
					pending
						.choices
						.iter()
						.find(|choice| choice.eq_ignore_ascii_case(body))
				})
			})
			.or_else(|| {
				spoken.and_then(|body| {
					pending
						.choices
						.iter()
						.find(|choice| choice.eq_ignore_ascii_case(trim_spoken_punctuation(body)))
				})
			})
			.context("Choose one of the saved choices, or use cancel")?
			.clone()
	};
	cp.answer = if pending.kind == "access" {
		None
	} else {
		Some(resolved.clone())
	};
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
fn is_pending_cancel(answer: &str) -> bool {
	matches!(
		answer.trim().to_ascii_lowercase().as_str(),
		"cancel" | "never mind" | "!cancel"
	)
}
/// Speech recognition can retain the wake address and add sentence punctuation.
/// Only an otherwise exact confirmation counts; never extract "yes" from a longer statement.
fn spoken_confirmation<'a>(answer: &'a str, wake_word: &str) -> &'a str {
	trim_spoken_punctuation(spoken_answer_body(answer, wake_word))
}
fn trim_spoken_punctuation(answer: &str) -> &str {
	answer.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!'))
}
fn spoken_answer_body<'a>(answer: &'a str, wake_word: &str) -> &'a str {
	let wake = if wake_word == "bumblebee" {
		"bumblebee"
	} else {
		"hey bumblebee"
	};
	let answer = answer.trim();
	match answer.get(..wake.len()) {
		Some(prefix) if prefix.eq_ignore_ascii_case(wake) => {
			let rest = &answer[wake.len()..];
			if rest.starts_with(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '.' | '!')) {
				rest.trim_start_matches(|c: char| {
					c.is_whitespace() || matches!(c, ',' | ':' | '.' | '!')
				})
			} else {
				answer
			}
		}
		_ => answer,
	}
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
					"cancelled"
				} else {
					"failed"
				},
				&serde_json::to_value(&cp)?,
			)?;
		}
	}
	result
}
fn suspend_access<H: Host>(
	host: &H,
	cp: &mut Checkpoint,
	blocker: access::AccessBlocker,
	owner_required: bool,
) -> Result<()> {
	let pending = PendingInput {
		id: uuid::Uuid::new_v4().simple().to_string()[..8].into(),
		turn_id: cp.id.clone(),
		actor: durable::actor(&cp.source),
		channel: durable::channel(&cp.source),
		kind: "access".into(),
		prompt: blocker.prompt,
		choices: vec!["continue".into(), "cancel".into()],
		owner_required,
		expires_at: crate::now_ms() + 86_400_000,
	};
	cp.reply_route = Some(ReplyRoute::Dashboard);
	cp.pending = Some(pending.clone());
	host
		.store()
		.suspend_turn(&pending, &serde_json::to_value(&cp)?)
}
async fn owner_before_dispatch<H: Host>(
	host: &H,
	cp: &mut Checkpoint,
	cancel: &CancellationToken,
) -> Result<Option<bool>> {
	let result = async {
		let owner = host.owner(&cp.source).await?;
		require_context_owner(cp, owner)?;
		host.validate_checkpoint(cp, cancel).await?;
		Ok::<_, anyhow::Error>(owner)
	};
	match tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled during permission check"),result=result=>result}
	{
		Ok(owner) => Ok(Some(owner)),
		Err(error)
			if error
				.downcast_ref::<crate::providers::AuthorizationRequired>()
				.is_some() =>
		{
			ensure!(
				!cancel.is_cancelled(),
				"Request cancelled during authorization check"
			);
			suspend_access(
				host,
				cp,
				access::AccessBlocker::repair(error.to_string()),
				cp.requester_was_owner.unwrap_or(true),
			)?;
			Ok(None)
		}
		Err(error) => Err(error),
	}
}
async fn drive<H: Host>(host: &H, cp: &mut Checkpoint, cancel: CancellationToken) -> Result<()> {
	loop {
		ensure!(!cancel.is_cancelled(), "Request cancelled");
		if cp.pending_final.is_some() {
			return deliver_final(host, cp, cancel.clone()).await;
		}
		if cp.cursor < cp.calls.len() {
			let call = cp.calls[cp.cursor].clone();
			if recovery::apply_saved_call(host, cp, &call)? {
				continue;
			}
			let defs = host.catalog();
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

			let Some(owner) = owner_before_dispatch(host, cp, &cancel).await? else {
				return Ok(());
			};
			require_context_owner(cp, owner)?;
			ensure!(
				!cancel.is_cancelled(),
				"Request cancelled after permission check"
			);
			let denied = def.owner_only && !owner;
			if !denied && !(def.requires_confirmation && cp.answer.as_deref() == Some("no")) {
				if let Some(blocker) = tokio::select! {biased;_=cancel.cancelled()=>bail!("Request cancelled"),result=host.preflight(cp,&call,&args,&cancel)=>result?}
				{
					suspend_access(host, cp, blocker, def.owner_only)?;
					return Ok(());
				}
				checkpoint(host, cp, "running")?;
			}
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
			let cached = host.store().begin_call(
				&cp.id,
				&call.id,
				&call.name,
				&call.arguments,
				def.call_effect(),
			)?;
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
							  if def.call_effect()==crate::storage::CallEffect::MayMutate{host.store().mark_call_uncertain(&cp.id,&call.id)?;}else{host.store().finish_call(&cp.id,&call.id,&json!({"status":"cancelled"}).to_string())?;}
							  bail!("Request cancelled; an in-flight external action may have an unknown outcome")
						 },
						 result=Box::pin(host.execute(cp,&call,&args,cancel.clone()))=>result,
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
					Err(error) => match delivery_failure_receipt(&error) {
						Some(receipt) => {
							json!({"status":if receipt["status"]=="failed"{"failed"}else{"unknown"},"receipt":receipt,"error":error.to_string()})
						}
						None => json!({"status":"failed","error":error.to_string()}),
					},
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
		let Some(owner) = owner_before_dispatch(host, cp, &cancel).await? else {
			return Ok(());
		};
		require_context_owner(cp, owner)?;
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
		let (items, calls, reply) = match model::parse_response(response) {
			Ok(parsed) => parsed,
			Err(error)
				if cp.delivery.is_some()
					&& error
						.downcast_ref::<model::InvalidFinalResponse>()
						.is_some() =>
			{
				cp.pending_final = Some(model::safe_failure_reply());
				cp.failure_final = true;
				checkpoint(host, cp, "running")?;
				continue;
			}
			Err(error) => return Err(error),
		};
		cp.items.extend(items);
		cp.calls = calls;
		cp.cursor = 0;
		cp.pending_final = reply;
		checkpoint(host, cp, "running")?;
		if !cp.calls.is_empty() {
			continue;
		}
		ensure!(
			cp.delivery.is_some(),
			"Model finished without configuring delivery"
		);
		ensure!(
			cp.pending_final.is_some(),
			"Model did not return a terminal response"
		);
	}
}

async fn deliver_final<H: Host>(
	host: &H,
	cp: &mut Checkpoint,
	cancel: CancellationToken,
) -> Result<()> {
	let reply = cp
		.pending_final
		.clone()
		.context("Saved final response is missing")?;
	ensure!(cp.delivery.is_some(), "Delivery is not configured");
	ensure!(
		!cancel.is_cancelled(),
		"Request cancelled before final delivery"
	);
	let Some(owner) = owner_before_dispatch(host, cp, &cancel).await? else {
		return Ok(());
	};
	require_context_owner(cp, owner)?;
	checkpoint(host, cp, "delivering")?;
	let result = tokio::select! {biased;_=cancel.cancelled()=>Err(anyhow::anyhow!("Final delivery interrupted; it will not be repeated automatically")),result=host.finish(cp,&reply,cancel.clone())=>result};
	match result {
		Ok(receipts) => {
			if !cp.failure_final {
				host.store().append_history(
					&durable::conversation_scope(&cp.source),
					"assistant",
					&reply.text,
				)?;
			}
			let mut saved = serde_json::to_value(&cp)?;
			saved["deliveryReceipts"] = receipts;
			host.store().checkpoint_turn(
				&cp.id,
				if cp.failure_final {
					"failed"
				} else {
					"completed"
				},
				&saved,
			)?;
			Ok(())
		}
		Err(error) => {
			let mut saved = serde_json::to_value(&cp)?;
			if let Some(receipt) = delivery_failure_receipt(&error) {
				saved["partialDelivery"] = receipt;
			}
			host.store().checkpoint_turn(&cp.id, "unknown", &saved)?;
			Err(error)
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
			if !engine
				.store
				.settings()
				.is_ok_and(|s| s.ai_enabled && s.ai_reminders_enabled)
			{
				break;
			}
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

#[cfg(test)]
mod dispatch_tests;
