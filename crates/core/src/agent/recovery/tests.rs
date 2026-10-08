use super::*;
use std::{
	collections::VecDeque,
	sync::{
		Mutex,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
};
struct TestHost {
	store: Store,
	owner: AtomicBool,
	blocked: AtomicBool,
	preflights: AtomicUsize,
	expired: AtomicBool,
	partial_tool: AtomicBool,
	partial_final: AtomicBool,
	responses: Mutex<VecDeque<Value>>,
	executed: Mutex<Vec<String>>,
	requests: Mutex<Vec<Checkpoint>>,
	finals: Mutex<Vec<FinalReply>>,
}
impl TestHost {
	fn open(path: &std::path::Path, responses: Vec<Value>) -> Self {
		Self {
			store: Store::open(path).unwrap(),
			owner: AtomicBool::new(true),
			blocked: AtomicBool::new(false),
			preflights: AtomicUsize::new(0),
			expired: AtomicBool::new(false),
			partial_tool: AtomicBool::new(false),
			partial_final: AtomicBool::new(false),
			responses: Mutex::new(responses.into()),
			executed: Mutex::new(vec![]),
			requests: Mutex::new(vec![]),
			finals: Mutex::new(vec![]),
		}
	}
	fn defs() -> Vec<ToolDefinition> {
		["write","inspectDiscordResources","requestUserInput","rememberMemory","destructive"].into_iter().map(|name|ToolDefinition{name:name.into(),description:name.into(),parameters:if name=="requestUserInput"{json!({"type":"object","properties":{"question":{"type":"string"},"choices":{"type":"array","items":{"type":"string"}}},"required":["question","choices"],"additionalProperties":false})}else{json!({"type":"object","properties":{},"additionalProperties":false})},owner_only:matches!(name,"write"|"destructive"),requires_confirmation:name=="destructive",external_effect:matches!(name,"write"|"destructive")}).collect()
	}
}
impl Host for TestHost {
	fn store(&self) -> &Store {
		&self.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		if self.blocked.load(Ordering::SeqCst) {
			vec![]
		} else {
			Self::defs()
		}
	}
	fn catalog(&self) -> Vec<ToolDefinition> {
		Self::defs()
	}
	async fn preflight(
		&self,
		_: &mut Checkpoint,
		call: &ToolCall,
		_: &Value,
		_: &CancellationToken,
	) -> Result<Option<access::AccessBlocker>> {
		if matches!(call.name.as_str(), "write" | "destructive") {
			self.preflights.fetch_add(1, Ordering::SeqCst);
		}
		Ok((matches!(call.name.as_str(), "write" | "destructive")
			&& self.blocked.load(Ordering::SeqCst))
		.then(|| access::AccessBlocker {
			prompt: "Enable the retained tool permission, then continue".into(),
		}))
	}
	async fn owner(&self, _: &ChatMessage) -> Result<bool> {
		if self.expired.load(Ordering::SeqCst) {
			return Err(crate::providers::AuthorizationRequired::new("twitch").into());
		}
		Ok(self.owner.load(Ordering::SeqCst))
	}
	async fn request(
		&self,
		cp: &Checkpoint,
		_: &[ToolDefinition],
		_: bool,
		_: CancellationToken,
	) -> Result<Value> {
		self.requests.lock().unwrap().push(cp.clone());
		self
			.responses
			.lock()
			.unwrap()
			.pop_front()
			.context("Unexpected model request")
	}
	async fn execute(
		&self,
		cp: &mut Checkpoint,
		call: &ToolCall,
		_: &Value,
		cancel: CancellationToken,
	) -> Result<Value> {
		self.executed.lock().unwrap().push(call.name.clone());
		if call.name == "rememberMemory" {
			self.store.remember(
				&durable::actor(&cp.source),
				"Committed memory before interrupted receipt",
			)?;
			cancel.cancel();
			std::future::pending::<()>().await;
		}
		if call.name == "write" && self.partial_tool.load(Ordering::SeqCst) {
			return Err(receipts::DeliveryFailure{receipt:json!({"status":"partial","messageIds":["part-one"],"completedParts":1,"totalParts":2})}.into());
		}
		Ok(json!({"status":"verified","receipt":call.id}))
	}
	async fn prompt(&self, _: &Checkpoint, _: &PendingInput, _: CancellationToken) -> Result<()> {
		Ok(())
	}
	async fn finish(
		&self,
		_: &Checkpoint,
		reply: &FinalReply,
		_: CancellationToken,
	) -> Result<Value> {
		self.finals.lock().unwrap().push(reply.clone());
		if self.partial_final.load(Ordering::SeqCst) {
			return Err(receipts::DeliveryFailure{receipt:json!({"status":"partial","messageIds":["final-part-one"],"completedParts":1,"totalParts":2})}.into());
		}
		Ok(json!({"messageIds":["final-receipt"]}))
	}
}
fn terminal() -> Value {
	json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":json!({"text":"Completed the remaining inspection.","messages":null}).to_string()}]}]})
}
fn checkpoint(host: &TestHost) -> Checkpoint {
	let mut settings = host.store.settings().unwrap();
	settings.ai_enabled = true;
	host.store.set("installation", &settings).unwrap();
	let cp = Checkpoint {
		id: "saved-turn".into(),
		source: ChatMessage {
			platform: "preview".into(),
			user_id: "owner".into(),
			display_name: "Owner".into(),
			channel_id: "room".into(),
			message_id: "original-message".into(),
			text: "Do the action, then inspect".into(),
			is_owner: true,
			access: Default::default(),
		},
		model: "fixture".into(),
		items: vec![json!({"role":"user","content":"original request"})],
		rounds: 3,
		executed: 1,
		calls: vec![
			ToolCall {
				id: "action".into(),
				name: "write".into(),
				arguments: "{}".into(),
			},
			ToolCall {
				id: "inspect".into(),
				name: "inspectDiscordResources".into(),
				arguments: "{}".into(),
			},
		],
		cursor: 0,
		delivery: Some(Delivery {
			speech: false,
			public_progress: false,
			targets: vec!["source".into()],
			discord_dm_user_id: None,
			dm_channel: None,
		}),
		artifacts: vec![],
		pending: None,
		reply_route: None,
		answer: None,
		approved_call: None,
		voice_channel_id: None,
		owner_context: false,
		requester_was_owner: Some(true),
		access_bindings: Default::default(),
		pending_final: None,
		failure_final: false,
		recovery_eligible: true,
	};
	host
		.store
		.create_turn(
			&cp.id,
			&durable::actor(&cp.source),
			&serde_json::to_value(&cp).unwrap(),
		)
		.unwrap();
	cp
}
#[tokio::test]
async fn restart_reuses_completed_receipt_and_continues_remaining_batch_with_saved_budgets() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("state.db");
	{
		let host = TestHost::open(&path, vec![]);
		let cp = checkpoint(&host);
		host
			.store
			.begin_call(
				&cp.id,
				"action",
				"write",
				"{}",
				crate::storage::CallEffect::MayMutate,
			)
			.unwrap();
		host
			.store
			.finish_call(
				&cp.id,
				"action",
				"{\"status\":\"verified\",\"receipt\":\"saved\"}",
			)
			.unwrap();
	}
	let host = TestHost::open(&path, vec![terminal()]);
	host.store.recover_interrupted().unwrap();
	assert_eq!(host.store.recovery_candidates().unwrap().len(), 1);
	assert!(
		host
			.store
			.stage_recovery("saved-turn", "preview:owner")
			.unwrap()
	);
	recover_with_host(&host, "saved-turn", CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["inspectDiscordResources"]
	);
	let record = host.store.turn("saved-turn").unwrap();
	assert_eq!(record.state, "completed");
	let cp: Checkpoint = serde_json::from_value(record.checkpoint).unwrap();
	assert_eq!(cp.rounds, 4);
	assert_eq!(cp.executed, 2);
	assert!(
		cp.items.iter().any(|item| item["call_id"] == "action"
			&& item["output"].as_str().unwrap_or("").contains("saved"))
	);
}
#[tokio::test]
async fn restart_observes_unknown_write_without_replay_then_allows_inspection() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("state.db");
	{
		let host = TestHost::open(&path, vec![]);
		let cp = checkpoint(&host);
		host
			.store
			.begin_call(
				&cp.id,
				"action",
				"write",
				"{}",
				crate::storage::CallEffect::MayMutate,
			)
			.unwrap();
	}
	let host = TestHost::open(&path, vec![terminal()]);
	host.store.recover_interrupted().unwrap();
	host
		.store
		.stage_recovery("saved-turn", "preview:owner")
		.unwrap();
	recover_with_host(&host, "saved-turn", CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["inspectDiscordResources"]
	);
	assert!(
		host.requests.lock().unwrap()[0]
			.items
			.iter()
			.any(|item| item["call_id"] == "action"
				&& item["output"].as_str().unwrap_or("").contains("unknown"))
	);
	host
		.store
		.create_turn("new", "preview:owner", &json!({}))
		.unwrap();
	assert!(
		host
			.store
			.begin_call(
				"new",
				"new-id",
				"write",
				"{}",
				crate::storage::CallEffect::MayMutate
			)
			.is_err()
	);
}
#[tokio::test]
async fn canceled_or_legacy_ambiguous_requests_and_unknown_final_delivery_do_not_resume() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![]);
	let mut cp = checkpoint(&host);
	cp.recovery_eligible = false;
	host
		.store
		.checkpoint_turn(&cp.id, "interrupted", &serde_json::to_value(&cp).unwrap())
		.unwrap();
	assert!(host.store.recovery_candidates().unwrap().is_empty());
	assert!(!host.store.stage_recovery(&cp.id, "preview:owner").unwrap());
	cp.recovery_eligible = true;
	host
		.store
		.checkpoint_turn(&cp.id, "interrupted", &serde_json::to_value(&cp).unwrap())
		.unwrap();
	host.store.stage_recovery(&cp.id, "preview:owner").unwrap();
	let cancel = CancellationToken::new();
	cancel.cancel();
	recover_with_host(&host, &cp.id, cancel).await.unwrap();
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "cancelled");
	assert!(host.executed.lock().unwrap().is_empty());
	host
		.store
		.create_turn(
			"delivery",
			"preview:owner",
			&serde_json::to_value(&cp).unwrap(),
		)
		.unwrap();
	host
		.store
		.checkpoint_turn(
			"delivery",
			"delivering",
			&serde_json::to_value(&cp).unwrap(),
		)
		.unwrap();
	host.store.recover_interrupted().unwrap();
	assert_eq!(host.store.turn("delivery").unwrap().state, "unknown");
	assert!(
		!host
			.store
			.stage_recovery("delivery", "preview:owner")
			.unwrap()
	);
}
#[tokio::test]
async fn recovered_owner_request_cannot_downgrade_to_viewer() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![]);
	let cp = checkpoint(&host);
	host.store.recover_interrupted().unwrap();
	host.store.stage_recovery(&cp.id, "preview:owner").unwrap();
	host.owner.store(false, Ordering::SeqCst);
	assert!(
		recover_with_host(&host, &cp.id, CancellationToken::new())
			.await
			.unwrap_err()
			.to_string()
			.contains("Owner identity changed")
	);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "interrupted");
	assert!(host.executed.lock().unwrap().is_empty());
}
#[tokio::test]
async fn explicit_access_pause_has_no_ledger_effect_and_continues_saved_batch_after_setup() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
	let mut cp = checkpoint(&host);
	host.blocked.store(true, Ordering::SeqCst);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	let pending = host.store.pending_inputs().unwrap().remove(0);
	assert_eq!(pending.kind, "access");
	assert!(
		host
			.store
			.saved_call(&cp.id, "action", "write", "{}")
			.unwrap()
			.is_none()
	);
	host.blocked.store(false, Ordering::SeqCst);
	let answer = desktop_answer_message(&host.store, &pending.id, "continue").unwrap();
	assert!(
		resume(&host, &answer, CancellationToken::new())
			.await
			.unwrap()
	);
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["write", "inspectDiscordResources"]
	);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "completed");
}
#[tokio::test]
async fn malformed_final_delivers_one_safe_failure_without_success_history() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(
		&dir.path().join("state.db"),
		vec![
			json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"{bad-json"}]}]}),
		],
	);
	let mut cp = checkpoint(&host);
	cp.calls.clear();
	host
		.store
		.checkpoint_turn(&cp.id, "running", &serde_json::to_value(&cp).unwrap())
		.unwrap();
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "failed");
	assert_eq!(host.finals.lock().unwrap().len(), 1);
	assert_eq!(
		host.finals.lock().unwrap()[0].text,
		model::SAFE_FAILURE_REPLY
	);
	assert!(
		host
			.store
			.history(&durable::conversation_scope(&cp.source))
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn owner_questions_cannot_resume_as_viewer_even_without_shared_memories() {
	for choices in [vec![], vec!["blue"]] {
		let dir = tempfile::tempdir().unwrap();
		let host = TestHost::open(&dir.path().join("state.db"), vec![]);
		let mut cp = checkpoint(&host);
		cp.calls = vec![ToolCall {
			id: "question".into(),
			name: "requestUserInput".into(),
			arguments: json!({"question":"Which option?","choices":choices}).to_string(),
		}];
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.unwrap();
		assert!(!cp.owner_context);
		assert!(!cp.pending.as_ref().unwrap().owner_required);
		host.owner.store(false, Ordering::SeqCst);
		let mut answer = cp.source.clone();
		answer.text = "blue".into();
		assert!(
			resume(&host, &answer, CancellationToken::new())
				.await
				.unwrap_err()
				.to_string()
				.contains("Owner identity changed")
		);
		assert_eq!(host.store.turn(&cp.id).unwrap().state, "waiting");
		assert!(host.executed.lock().unwrap().is_empty());
		assert!(host.requests.lock().unwrap().is_empty());
	}
}
#[tokio::test]
async fn revoked_owner_and_denied_viewer_never_preflight_privileged_provider_setup() {
	for original_owner in [true, false] {
		let dir = tempfile::tempdir().unwrap();
		let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
		let mut cp = checkpoint(&host);
		cp.requester_was_owner = Some(original_owner);
		host.owner.store(false, Ordering::SeqCst);
		host.blocked.store(true, Ordering::SeqCst);
		let result = run_guarded(&host, &mut cp, CancellationToken::new()).await;
		assert_eq!(result.is_err(), original_owner);
		assert_eq!(host.preflights.load(Ordering::SeqCst), 0);
		assert!(host.store.pending_inputs().unwrap().is_empty());
		assert!(
			!host
				.executed
				.lock()
				.unwrap()
				.iter()
				.any(|name| name == "write")
		);
	}
}
#[tokio::test]
async fn partial_tool_delivery_keeps_receipts_and_blocks_equivalent_replay() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
	let mut cp = checkpoint(&host);
	host.partial_tool.store(true, Ordering::SeqCst);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	let (state, receipt) = host
		.store
		.saved_call(&cp.id, "action", "write", "{}")
		.unwrap()
		.unwrap();
	assert_eq!(state, "unknown");
	assert_eq!(
		serde_json::from_str::<Value>(&receipt.unwrap()).unwrap()["receipt"]["messageIds"],
		json!(["part-one"])
	);
	host
		.store
		.create_turn("later", "preview:owner", &json!({}))
		.unwrap();
	assert!(
		host
			.store
			.begin_call(
				"later",
				"new-id",
				"write",
				"{}",
				crate::storage::CallEffect::MayMutate
			)
			.is_err()
	);
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["write", "inspectDiscordResources"]
	);
}
#[tokio::test]
async fn partial_final_delivery_keeps_receipts_without_success_history_or_replay() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
	let mut cp = checkpoint(&host);
	cp.calls.clear();
	host.partial_final.store(true, Ordering::SeqCst);
	assert!(
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.is_err()
	);
	let record = host.store.turn(&cp.id).unwrap();
	assert_eq!(record.state, "unknown");
	assert_eq!(
		record.checkpoint["partialDelivery"]["messageIds"],
		json!(["final-part-one"])
	);
	assert!(
		host
			.store
			.history(&durable::conversation_scope(&cp.source))
			.unwrap()
			.is_empty()
	);
	host.store.recover_interrupted().unwrap();
	assert!(host.store.recovery_candidates().unwrap().is_empty());
	assert_eq!(host.finals.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn expired_source_authorization_pauses_before_dispatch_and_keeps_original_batch() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
	let mut cp = checkpoint(&host);
	host.expired.store(true, Ordering::SeqCst);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	let pending = host.store.pending_inputs().unwrap().remove(0);
	assert_eq!(pending.kind, "access");
	assert_eq!(host.preflights.load(Ordering::SeqCst), 0);
	assert!(host.executed.lock().unwrap().is_empty());
	let reply = desktop_answer_message(&host.store, &pending.id, "continue").unwrap();
	assert!(
		resume(&host, &reply, CancellationToken::new())
			.await
			.is_err()
	);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "waiting");
	host.expired.store(false, Ordering::SeqCst);
	resume(&host, &reply, CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["write", "inspectDiscordResources"]
	);
}
#[tokio::test]
async fn cancellation_after_local_commit_is_unknown_and_never_repeats() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![]);
	let mut cp = checkpoint(&host);
	cp.calls[0].name = "rememberMemory".into();
	assert!(
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.is_err()
	);
	assert_eq!(
		host.store.memories("preview:owner", false).unwrap().len(),
		1
	);
	assert_eq!(
		host
			.store
			.saved_call(&cp.id, "action", "rememberMemory", "{}")
			.unwrap()
			.unwrap()
			.0,
		"unknown"
	);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "cancelled");
	assert_eq!(*host.executed.lock().unwrap(), vec!["rememberMemory"]);
	host
		.store
		.create_turn("later", "preview:owner", &json!({}))
		.unwrap();
	assert!(
		host
			.store
			.begin_call(
				"later",
				"new-id",
				"rememberMemory",
				"{}",
				crate::storage::CallEffect::MayMutate
			)
			.is_err()
	);
}

#[tokio::test]
async fn declining_saved_confirmation_never_requests_revoked_access_or_loses_rejection() {
	let dir = tempfile::tempdir().unwrap();
	let host = TestHost::open(&dir.path().join("state.db"), vec![terminal()]);
	let mut cp = checkpoint(&host);
	cp.calls[0].name = "destructive".into();
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(cp.pending.as_ref().unwrap().kind, "confirmation");
	let checks = host.preflights.load(Ordering::SeqCst);
	host.blocked.store(true, Ordering::SeqCst);
	let mut answer = cp.source.clone();
	answer.text = "no".into();
	resume(&host, &answer, CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(host.preflights.load(Ordering::SeqCst), checks);
	assert!(host.store.pending_inputs().unwrap().is_empty());
	assert_eq!(
		*host.executed.lock().unwrap(),
		vec!["inspectDiscordResources"]
	);
	let (_, result) = host
		.store
		.saved_call(&cp.id, "action", "destructive", "{}")
		.unwrap()
		.unwrap();
	assert_eq!(
		serde_json::from_str::<Value>(&result.unwrap()).unwrap()["status"],
		"declined"
	);
}
