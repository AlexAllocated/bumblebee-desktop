use super::*;
use std::{
	collections::VecDeque,
	sync::{
		Mutex,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
};

struct FixtureHost {
	store: Store,
	responses: Mutex<VecDeque<Value>>,
	effects: Mutex<Vec<String>>,
	owner: AtomicBool,
	delivered: AtomicUsize,
	cancel_during_effect: AtomicBool,
	cancel_during_prompt: AtomicBool,
	cancel_owner: Mutex<Option<(usize, CancellationToken)>>,
}
impl FixtureHost {
	fn new(path: &std::path::Path, responses: Vec<Value>) -> Self {
		Self {
			store: Store::open(path).unwrap(),
			responses: Mutex::new(responses.into()),
			effects: Mutex::new(vec![]),
			owner: AtomicBool::new(true),
			delivered: AtomicUsize::new(0),
			cancel_during_effect: AtomicBool::new(false),
			cancel_during_prompt: AtomicBool::new(false),
			cancel_owner: Mutex::new(None),
		}
	}
}
fn definition(name: &str, confirmation: bool, effect: bool) -> ToolDefinition {
	ToolDefinition {
		name: name.into(),
		description: name.into(),
		parameters: json!({"type":"object","properties":{},"additionalProperties":false,"required":[]}),
		owner_only: effect,
		requires_confirmation: confirmation,
		external_effect: effect,
	}
}
impl Host for FixtureHost {
	fn store(&self) -> &Store {
		&self.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		vec![
			definition("configureTurnDelivery", false, false),
			definition("ban", true, true),
			definition("after", false, true),
			definition("inspect", false, false),
		]
	}
	async fn owner(&self, _: &ChatMessage) -> Result<bool> {
		if let Some((remaining, cancel)) = self.cancel_owner.lock().unwrap().as_mut() {
			if *remaining == 0 {
				cancel.cancel();
			} else {
				*remaining -= 1;
			}
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
		if cp.rounds > 2 {
			assert!(
				cp.items
					.iter()
					.any(|v| v["type"] == "function_call_output" && v["call_id"] == "configure")
			);
		}
		self
			.responses
			.lock()
			.unwrap()
			.pop_front()
			.context("Unexpected additional model round")
	}
	async fn execute(
		&self,
		_: &mut Checkpoint,
		call: &ToolCall,
		_: &Value,
		cancel: CancellationToken,
	) -> Result<Value> {
		if call.name == "configureTurnDelivery" {
			return Ok(
				json!({"status":"configured","delivery":{"speech":false,"publicProgress":false,"targets":["source"],"discordDmUserId":null}}),
			);
		}
		self.effects.lock().unwrap().push(call.name.clone());
		if call.name == "ban" && self.cancel_during_effect.load(Ordering::SeqCst) {
			cancel.cancel();
			std::future::pending::<()>().await;
		}
		Ok(json!({"status":"verified","target":"exact-saved-user"}))
	}
	async fn prompt(
		&self,
		_: &Checkpoint,
		_: &PendingInput,
		cancel: CancellationToken,
	) -> Result<()> {
		if self.cancel_during_prompt.load(Ordering::SeqCst) {
			cancel.cancel();
		}
		Ok(())
	}
	async fn finish(&self, _: &Checkpoint, _: &FinalReply, _: CancellationToken) -> Result<Value> {
		self.delivered.fetch_add(1, Ordering::SeqCst);
		Ok(json!({"messageIds":["observed-final"]}))
	}
}
fn source(text: &str) -> ChatMessage {
	ChatMessage {
		platform: "discord".into(),
		user_id: "owner".into(),
		display_name: "Viewer-provided name".into(),
		message_id: uuid::Uuid::new_v4().to_string(),
		channel_id: "room".into(),
		text: text.into(),
		is_owner: true,
	}
}
fn calls(names: &[(&str, &str)]) -> Value {
	json!({"status":"completed","output":names.iter().map(|(id,name)|json!({"type":"function_call","call_id":id,"name":name,"arguments":"{}"})).collect::<Vec<_>>()})
}
fn terminal(text: &str) -> Value {
	json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":json!({"text":text,"messages":null}).to_string()}]}]})
}
fn checkpoint(host: &FixtureHost) -> Checkpoint {
	let cp = Checkpoint {
		id: uuid::Uuid::new_v4().to_string(),
		source: source("ban the saved user, then announce the result"),
		model: "fixture".into(),
		items: vec![json!({"role":"user","content":"a real request"})],
		rounds: 0,
		executed: 0,
		calls: vec![],
		cursor: 0,
		delivery: None,
		artifacts: vec![],
		pending: None,
		answer: None,
		approved_call: None,
		voice_channel_id: None,
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
async fn confirmation_preserves_later_calls_across_restart_and_rechecks_owner() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("state.sqlite");
	let id = {
		let host = FixtureHost::new(
			&path,
			vec![
				calls(&[("configure", "configureTurnDelivery")]),
				calls(&[("ban-id", "ban"), ("later-id", "after")]),
			],
		);
		let mut cp = checkpoint(&host);
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.unwrap();
		assert!(host.effects.lock().unwrap().is_empty());
		assert_eq!(host.store.turn(&cp.id).unwrap().state, "waiting");
		assert_eq!(cp.calls[1].id, "later-id");
		cp.id
	};
	let host = FixtureHost::new(&path, vec![terminal("Observed both completed actions.")]);
	host.store.recover_interrupted().unwrap();
	host.owner.store(false, Ordering::SeqCst);
	assert!(
		resume(&host, &source("yes"), CancellationToken::new())
			.await
			.is_err()
	);
	assert_eq!(host.store.pending_inputs().unwrap().len(), 1);
	assert!(host.effects.lock().unwrap().is_empty());
	host.owner.store(true, Ordering::SeqCst);
	assert!(
		resume(&host, &source("yes"), CancellationToken::new())
			.await
			.unwrap()
	);
	assert_eq!(*host.effects.lock().unwrap(), vec!["ban", "after"]);
	assert_eq!(host.delivered.load(Ordering::SeqCst), 1);
	assert_eq!(host.store.turn(&id).unwrap().state, "completed");
	assert!(
		!resume(&host, &source("yes"), CancellationToken::new())
			.await
			.unwrap()
	);
	assert_eq!(*host.effects.lock().unwrap(), vec!["ban", "after"]);
}
#[tokio::test]
async fn rejection_skips_only_the_declined_action_and_finishes_remaining_work() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("configure", "configureTurnDelivery")]),
			calls(&[("ban-id", "ban"), ("later-id", "after")]),
			terminal("Skipped the ban and completed the other action."),
		],
	);
	let mut cp = checkpoint(&host);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	resume(&host, &source("no"), CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(*host.effects.lock().unwrap(), vec!["after"]);
}
#[tokio::test]
async fn cancellation_after_dispatch_does_not_run_later_calls_or_reissue_unknown_action() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("configure", "configureTurnDelivery")]),
			calls(&[("ban-id", "ban"), ("later-id", "after")]),
		],
	);
	let mut cp = checkpoint(&host);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	host.cancel_during_effect.store(true, Ordering::SeqCst);
	assert!(
		resume(&host, &source("yes"), CancellationToken::new())
			.await
			.is_err()
	);
	assert_eq!(*host.effects.lock().unwrap(), vec!["ban"]);
	assert_eq!(host.delivered.load(Ordering::SeqCst), 0);
	let second = checkpoint(&host);
	assert!(
		host
			.store
			.begin_call(&second.id, "new-model-id", "ban", "{}")
			.is_err()
	);
}
#[test]
fn malformed_terminal_or_missing_call_identity_never_gets_a_compatibility_fallback() {
	for text in [
		"plain text",
		r#"{"text":"claimed success","messages":null,"extra":true}"#,
		r#"{"text":"one","messages":[{"text":"two","artifactIds":[]}]}"#,
	] {
		let response = json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}]});
		assert!(model::parse_response(response).is_err());
	}
	assert!(
		model::parse_response(
			json!({"status":"completed","output":[{"type":"function_call","name":"ban","arguments":"{}"}]})
		)
		.is_err()
	);
	assert!(model::parse_response(json!({"status":"incomplete","output":[]})).is_err());
	assert_eq!(
		model::parse_response(terminal("")).unwrap().2.unwrap().text,
		""
	);
}
#[test]
fn catalog_advertises_only_unique_closed_tool_schemas() {
	let mut names = std::collections::HashSet::new();
	for def in tools::definitions() {
		assert!(names.insert(def.name.clone()), "{}", def.name);
		assert_eq!(def.parameters["additionalProperties"], false);
		assert_eq!(
			def.parameters["required"].as_array().unwrap().len(),
			def.parameters["properties"].as_object().unwrap().len()
		);
	}
	for removed in [
		"setPromotionSettings",
		"startRaffle",
		"runMod",
		"setTwitchGoLiveNotificationText",
	] {
		assert!(!names.contains(removed));
	}
}
#[test]
fn tool_validation_rejects_extra_keys_and_inexact_provider_ids() {
	let definition = platform_tools::definitions()
		.into_iter()
		.find(|d| d.name == "banDiscordUser")
		.unwrap();
	assert!(model::validate(&definition.parameters, &json!({"anything":"goes"})).is_err());
	assert!(
		model::validate(
			&json!({"type":"string","pattern":"^[0-9]{1,20}$"}),
			&json!("../users")
		)
		.is_err()
	);
}

#[tokio::test]
async fn cancelled_permission_check_does_not_consume_a_saved_confirmation() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("configure", "configureTurnDelivery")]),
			calls(&[("ban-id", "ban")]),
		],
	);
	let mut cp = checkpoint(&host);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	let cancel = CancellationToken::new();
	*host.cancel_owner.lock().unwrap() = Some((0, cancel.clone()));
	assert!(resume(&host, &source("yes"), cancel).await.is_err());
	assert_eq!(host.store.pending_inputs().unwrap().len(), 1);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "waiting");
	assert!(host.effects.lock().unwrap().is_empty());
}
#[tokio::test]
async fn cancelled_permission_recheck_does_not_dispatch_or_mark_an_unstarted_action_unknown() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("configure", "configureTurnDelivery")]),
			calls(&[("ban-id", "ban")]),
		],
	);
	let mut cp = checkpoint(&host);
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	let cancel = CancellationToken::new();
	*host.cancel_owner.lock().unwrap() = Some((1, cancel.clone()));
	assert!(resume(&host, &source("yes"), cancel).await.is_err());
	assert!(host.effects.lock().unwrap().is_empty());
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "interrupted");
	let count: i64 = host
		.store
		.db()
		.unwrap()
		.query_row(
			"SELECT COUNT(*) FROM tool_calls WHERE name='ban'",
			[],
			|row| row.get(0),
		)
		.unwrap();
	assert_eq!(
		count, 0,
		"an action canceled before dispatch must have no uncertain write receipt"
	);
}
#[tokio::test]
async fn interruption_while_asking_a_question_cancels_its_durable_continuation() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("configure", "configureTurnDelivery")]),
			calls(&[("ban-id", "ban")]),
		],
	);
	host.cancel_during_prompt.store(true, Ordering::SeqCst);
	let mut cp = checkpoint(&host);
	assert!(
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.is_err()
	);
	assert_eq!(host.store.turn(&cp.id).unwrap().state, "cancelled");
	assert!(host.store.pending_inputs().unwrap().is_empty());
	assert!(host.effects.lock().unwrap().is_empty());
}
