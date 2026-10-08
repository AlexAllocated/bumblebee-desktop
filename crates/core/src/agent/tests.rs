use super::*;
use std::{
	collections::VecDeque,
	sync::{
		Mutex,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
};

struct FixtureHost {
	reply_route: Mutex<Option<ReplyRoute>>,
	prompts: Mutex<Vec<ReplyRoute>>,
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
			reply_route: Mutex::new(None),
			prompts: Mutex::new(vec![]),
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
	async fn reply_route(&self, cp: &Checkpoint, _: bool) -> Result<ReplyRoute> {
		Ok(self.reply_route.lock().unwrap().clone().unwrap_or_else(|| {
			if cp.delivery.as_ref().is_some_and(|d| d.targets.is_empty()) {
				ReplyRoute::Dashboard
			} else {
				ReplyRoute::Source
			}
		}))
	}
	fn store(&self) -> &Store {
		&self.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		vec![
			definition("configureTurnDelivery", false, false),
			tools::definitions()
				.into_iter()
				.find(|d| d.name == "requestUserInput")
				.unwrap(),
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
		cp: &Checkpoint,
		_: &PendingInput,
		cancel: CancellationToken,
	) -> Result<()> {
		self
			.prompts
			.lock()
			.unwrap()
			.push(cp.reply_route.clone().unwrap());
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
	checkpoint_source(host, source("ban the saved user, then announce the result"))
}
fn checkpoint_source(host: &FixtureHost, input: ChatMessage) -> Checkpoint {
	let mut settings = host.store.settings().unwrap();
	settings.ai_enabled = true;
	host.store.set("installation", &settings).unwrap();
	let cp = Checkpoint {
		id: uuid::Uuid::new_v4().to_string(),
		source: input,
		model: "fixture".into(),
		items: vec![json!({"role":"user","content":"a real request"})],
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

#[tokio::test]
async fn cross_platform_private_answers_survive_restart_without_delegating_authority() {
	for platform in ["twitch", "youtube"] {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("state.sqlite");
		let original = ChatMessage {
			platform: platform.into(),
			user_id: "streamer-id".into(),
			channel_id: "broadcast".into(),
			..source("perform a private action")
		};
		let (turn, pending) = {
			let host = FixtureHost::new(&path, vec![calls(&[("ban-id", "ban")])]);
			let mut settings = host.store.settings().unwrap();
			settings.owner_discord_id = "999".into();
			host.store.set("installation", &settings).unwrap();
			*host.reply_route.lock().unwrap() = Some(ReplyRoute::DiscordDm {
				user_id: "999".into(),
				channel_id: "55".into(),
				owner_link: true,
			});
			let mut cp = checkpoint_source(&host, original.clone());
			cp.delivery = Some(Delivery {
				speech: false,
				public_progress: false,
				targets: vec!["discord_dm".into()],
				discord_dm_user_id: Some("777".into()),
				dm_channel: Some("friend-dm".into()),
			});
			run_guarded(&host, &mut cp, CancellationToken::new())
				.await
				.unwrap();
			let pending = host.store.pending_inputs().unwrap().remove(0);
			assert_eq!(pending.actor, format!("{platform}:streamer-id"));
			assert_eq!(pending.channel, format!("{platform}:broadcast"));
			assert_eq!(
				pending_to_cancel(
					&host.store,
					&ChatMessage {
						text: "!cancel".into(),
						..original.clone()
					}
				)
				.unwrap()
				.len(),
				1
			);
			(cp.id, pending)
		};
		let host = FixtureHost::new(&path, vec![terminal("Observed the approved action.")]);
		host.store.recover_agent_state().unwrap();
		let reply = ChatMessage {
			platform: "discord".into(),
			user_id: "999".into(),
			channel_id: "55".into(),
			..source("yes")
		};
		assert!(pending_for_message(&host.store, &reply).unwrap());
		assert_eq!(
			pending_actor_for_message(&host.store, &reply).unwrap(),
			Some(format!("{platform}:streamer-id"))
		);
		for bad in [
			ChatMessage {
				user_id: "777".into(),
				..reply.clone()
			},
			ChatMessage {
				channel_id: "other-room".into(),
				..reply.clone()
			},
			ChatMessage {
				text: "yes".into(),
				..original.clone()
			},
		] {
			assert!(!resume(&host, &bad, CancellationToken::new()).await.unwrap());
		}
		let desktop = desktop_answer_message(&host.store, &pending.id, "yes").unwrap();
		assert!(is_desktop_answer(&desktop));
		assert_eq!(
			pending_actor_for_message(&host.store, &desktop).unwrap(),
			Some(format!("{platform}:streamer-id"))
		);
		let mut settings = host.store.settings().unwrap();
		settings.owner_discord_id = "1000".into();
		host.store.set("installation", &settings).unwrap();
		assert!(!pending_for_message(&host.store, &reply).unwrap());
		assert!(
			resume(&host, &desktop, CancellationToken::new())
				.await
				.is_err()
		);
		settings.owner_discord_id = "999".into();
		host.store.set("installation", &settings).unwrap();
		host.owner.store(false, Ordering::SeqCst);
		assert!(
			resume(&host, &reply, CancellationToken::new())
				.await
				.is_err()
		);
		assert_eq!(host.store.pending_inputs().unwrap().len(), 1);
		assert!(host.effects.lock().unwrap().is_empty());
		host.owner.store(true, Ordering::SeqCst);
		let accepted = if platform == "twitch" {
			&reply
		} else {
			&desktop
		};
		assert!(
			resume(&host, accepted, CancellationToken::new())
				.await
				.unwrap()
		);
		assert_eq!(*host.effects.lock().unwrap(), vec!["ban"]);
		let record = host.store.turn(&turn).unwrap();
		assert_eq!(record.state, "completed");
		assert_eq!(record.checkpoint["source"]["platform"], platform);
		assert!(
			!resume(&host, accepted, CancellationToken::new())
				.await
				.unwrap()
		);
	}
}

#[tokio::test]
async fn silent_pending_questions_are_dashboard_only_and_do_not_create_chatter_messages() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![calls(&[("ban-id", "ban")]), terminal("Done.")],
	);
	let mut cp = checkpoint(&host);
	cp.delivery = Some(Delivery {
		speech: false,
		public_progress: false,
		targets: vec![],
		discord_dm_user_id: None,
		dm_channel: None,
	});
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	assert_eq!(*host.prompts.lock().unwrap(), vec![ReplyRoute::Dashboard]);
	assert!(!pending_for_message(&host.store, &source("yes")).unwrap());
	let p = host.store.pending_inputs().unwrap().remove(0);
	let answer = desktop_answer_message(&host.store, &p.id, "yes").unwrap();
	assert!(is_desktop_answer(&answer));
	assert_eq!(answer.channel_id, p.id);
	assert!(
		resume(&host, &answer, CancellationToken::new())
			.await
			.unwrap()
	);
	assert!(host.store.chatters("").unwrap().is_empty());
}

#[tokio::test]
async fn spoken_confirmations_accept_the_wake_address_but_never_infer_compound_approval() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![calls(&[("ban-id", "ban")]), terminal("Done.")],
	);
	let original = ChatMessage {
		platform: "discord_voice".into(),
		..source("ban the exact approved target")
	};
	let mut cp = checkpoint_source(&host, original.clone());
	cp.delivery = Some(Delivery {
		speech: true,
		public_progress: false,
		targets: vec!["source".into()],
		discord_dm_user_id: None,
		dm_channel: None,
	});
	run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap();
	for text in [
		"Yes?",
		"Hey Bumblebee, yes, but use another target.",
		"Hey Bumblebee, don't approve.",
		"Hey Bumblebees yes.",
	] {
		let reply = ChatMessage {
			text: text.into(),
			..original.clone()
		};
		assert!(
			resume(&host, &reply, CancellationToken::new())
				.await
				.is_err(),
			"{text}"
		);
		assert!(host.effects.lock().unwrap().is_empty());
	}
	let text_reply = ChatMessage {
		platform: "discord".into(),
		text: "Hey Bumblebee, yes.".into(),
		..original.clone()
	};
	assert!(
		resume(&host, &text_reply, CancellationToken::new())
			.await
			.is_err()
	);
	let reply = ChatMessage {
		text: "Hey Bumblebee, yes.".into(),
		..original
	};
	assert!(
		resume(&host, &reply, CancellationToken::new())
			.await
			.unwrap()
	);
	assert_eq!(*host.effects.lock().unwrap(), vec!["ban"]);
	assert_eq!(spoken_confirmation("Bumblebee, NO!", "bumblebee"), "NO");
	assert_eq!(spoken_confirmation("Yes.", "hey_bumblebee"), "Yes");
}

#[tokio::test]
async fn spoken_fixed_choices_preserve_literal_labels_before_stripping_sentence_punctuation() {
	for (answer, expected) in [
		("Hey Bumblebee, blue.", "Blue"),
		("Hey Bumblebee, Blue!", "Blue!"),
		("Blue!", "Blue!"),
	] {
		let question = json!({"status":"completed","output":[{"type":"function_call","call_id":"choose-color","name":"requestUserInput","arguments":json!({"question":"Which color?","choices":["Blue","Blue!","Red"]}).to_string()}]});
		let host = FixtureHost::new(
			std::path::Path::new(":memory:"),
			vec![question, terminal("Chosen.")],
		);
		let original = ChatMessage {
			platform: "discord_voice".into(),
			..source("choose a color")
		};
		let mut cp = checkpoint_source(&host, original.clone());
		cp.delivery = Some(Delivery {
			speech: true,
			public_progress: false,
			targets: vec!["source".into()],
			discord_dm_user_id: None,
			dm_channel: None,
		});
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.unwrap();
		for (platform, text) in [
			("discord", "Hey Bumblebee, blue."),
			("discord_voice", "Hey Bumblebee, blue or red."),
			("discord_voice", "Blue?"),
		] {
			let invalid = ChatMessage {
				platform: platform.into(),
				text: text.into(),
				..original.clone()
			};
			assert!(
				resume(&host, &invalid, CancellationToken::new())
					.await
					.is_err()
			);
			assert_eq!(host.store.pending_inputs().unwrap().len(), 1);
		}
		let reply = ChatMessage {
			text: answer.into(),
			..original
		};
		assert!(
			resume(&host, &reply, CancellationToken::new())
				.await
				.unwrap()
		);
		let saved = host.store.turn(&cp.id).unwrap();
		let output = saved.checkpoint["items"]
			.as_array()
			.unwrap()
			.iter()
			.find(|v| v["type"] == "function_call_output" && v["call_id"] == "choose-color")
			.unwrap();
		let result: Value = serde_json::from_str(output["output"].as_str().unwrap()).unwrap();
		assert_eq!(result["untrustedToolResult"]["answer"], expected);
		assert_eq!(saved.state, "completed");
	}
}

#[tokio::test]
async fn disabling_ai_pauses_all_pending_answers_but_preserves_cancel_and_later_resume() {
	let host = FixtureHost::new(
		std::path::Path::new(":memory:"),
		vec![
			calls(&[("first-ban", "ban")]),
			terminal("Done."),
			calls(&[("second-ban", "ban")]),
		],
	);
	for cancel_only in [false, true] {
		let mut cp = checkpoint(&host);
		cp.delivery = Some(Delivery {
			speech: false,
			public_progress: false,
			targets: vec!["source".into()],
			discord_dm_user_id: None,
			dm_channel: None,
		});
		run_guarded(&host, &mut cp, CancellationToken::new())
			.await
			.unwrap();
		let pending = host.store.pending_inputs().unwrap().remove(0);
		let queued_desktop = desktop_answer_message(&host.store, &pending.id, "yes").unwrap();
		let mut settings = host.store.settings().unwrap();
		settings.ai_enabled = false;
		host.store.set("installation", &settings).unwrap();
		assert!(!pending_for_message(&host.store, &source("unrelated ordinary chat")).unwrap());
		assert!(!pending_for_message(&host.store, &source("yes")).unwrap());
		assert!(pending_for_message(&host.store, &source("!cancel")).unwrap());
		assert!(pending_for_message(&host.store, &source("cancel")).unwrap());
		assert!(
			desktop_answer_message(&host.store, &pending.id, "yes")
				.unwrap_err()
				.to_string()
				.contains("Enable the agent")
		);
		let explicit = source(&format!("!answer {} yes", pending.id));
		assert!(pending_for_message(&host.store, &explicit).unwrap());
		let unexpected_permission_check = CancellationToken::new();
		*host.cancel_owner.lock().unwrap() = Some((0, unexpected_permission_check.clone()));
		let effects_before = host.effects.lock().unwrap().len();
		for reply in [source("yes"), explicit.clone(), queued_desktop] {
			assert!(
				resume(&host, &reply, CancellationToken::new())
					.await
					.unwrap_err()
					.to_string()
					.contains("Enable the agent")
			);
		}
		assert!(!unexpected_permission_check.is_cancelled());
		assert_eq!(host.store.turn(&cp.id).unwrap().state, "waiting");
		assert_eq!(host.effects.lock().unwrap().len(), effects_before);
		*host.cancel_owner.lock().unwrap() = None;
		if cancel_only {
			let cancel = desktop_answer_message(&host.store, &pending.id, "cancel").unwrap();
			assert!(
				resume(&host, &cancel, CancellationToken::new())
					.await
					.unwrap()
			);
			assert_eq!(host.store.turn(&cp.id).unwrap().state, "cancelled");
		} else {
			settings.ai_enabled = true;
			host.store.set("installation", &settings).unwrap();
			assert!(
				resume(&host, &explicit, CancellationToken::new())
					.await
					.unwrap()
			);
			assert_eq!(host.store.turn(&cp.id).unwrap().state, "completed");
			assert_eq!(host.effects.lock().unwrap().len(), effects_before + 1);
		}
	}
}

#[tokio::test]
async fn self_disable_keeps_its_success_receipt_and_stops_later_runtime_boundaries() {
	struct ForbiddenSecrets;
	impl crate::providers::SecretStore for ForbiddenSecrets {
		fn get(&self, _: &str) -> Result<Option<String>> {
			panic!("Disabled runtime must not access provider credentials")
		}
		fn set(&self, _: &str, _: &str) -> Result<()> {
			panic!("No test credentials")
		}
		fn delete(&self, _: &str) -> Result<()> {
			panic!("No test credentials")
		}
	}
	let data = tempfile::tempdir().unwrap();
	let store = Arc::new(Store::open(&data.path().join("state.sqlite")).unwrap());
	let mut settings = store.settings().unwrap();
	settings.ai_enabled = true;
	store.set("installation", &settings).unwrap();
	let providers =
		crate::providers::Providers::new(store.clone(), Arc::new(ForbiddenSecrets)).unwrap();
	let (events, mut received) = tokio::sync::broadcast::channel(32);
	let engine = Engine::new(
		providers,
		crate::runtime::EnginePaths {
			data_dir: data.path().into(),
			native_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
				.join("../../src-tauri/resources"),
		},
		events,
	)
	.unwrap();
	let host = RuntimeHost { engine };
	let disable_args = json!({"model":null,"enabled":false});
	let disable = ToolCall {
		id: "self-disable".into(),
		name: "setAiSettings".into(),
		arguments: disable_args.to_string(),
	};
	let later_args = json!({"enabled":false});
	let later = ToolCall {
		id: "later-change".into(),
		name: "setChatTtsSettings".into(),
		arguments: later_args.to_string(),
	};
	let mut cp = Checkpoint {
		id: "turn".into(),
		source: ChatMessage {
			platform: "preview".into(),
			..source("disable the agent")
		},
		model: "unused".into(),
		items: vec![],
		rounds: 0,
		executed: 0,
		calls: vec![disable.clone(), later.clone()],
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
	};
	store
		.create_turn(
			&cp.id,
			&durable::actor(&cp.source),
			&serde_json::to_value(&cp).unwrap(),
		)
		.unwrap();
	let error = run_guarded(&host, &mut cp, CancellationToken::new())
		.await
		.unwrap_err();
	assert!(error.to_string().contains("agent is disabled"));
	let receipt = store
		.begin_call(&cp.id, &disable.id, &disable.name, &disable.arguments)
		.unwrap()
		.unwrap();
	assert_eq!(
		serde_json::from_str::<Value>(&receipt).unwrap()["status"],
		"applied"
	);
	assert!(!store.settings().unwrap().ai_enabled);
	assert!(store.settings().unwrap().read_chat);
	assert_eq!(store.turn(&cp.id).unwrap().state, "failed");
	assert!(
		host
			.request(&cp, &[], true, CancellationToken::new())
			.await
			.unwrap_err()
			.to_string()
			.contains("agent is disabled")
	);
	assert!(
		host
			.execute(&mut cp, &later, &later_args, CancellationToken::new())
			.await
			.unwrap_err()
			.to_string()
			.contains("agent is disabled")
	);
	assert!(
		host
			.finish(
				&cp,
				&FinalReply {
					text: "Must stay local".into(),
					messages: None
				},
				CancellationToken::new()
			)
			.await
			.unwrap_err()
			.to_string()
			.contains("agent is disabled")
	);
	assert!(received.try_recv().is_err());
}
