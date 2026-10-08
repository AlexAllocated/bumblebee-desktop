use super::*;
use crate::{agent_storage::PendingInput, providers::SecretStore};
use serde_json::json;

struct EmptySecrets;
impl SecretStore for EmptySecrets {
	fn get(&self, _: &str) -> Result<Option<String>> {
		Ok(None)
	}
	fn set(&self, _: &str, _: &str) -> Result<()> {
		anyhow::bail!("Test must not store a credential")
	}
	fn delete(&self, _: &str) -> Result<()> {
		anyhow::bail!("Test must not delete a credential")
	}
}
fn fixture() -> (tempfile::TempDir, Arc<Engine>) {
	let temporary = tempfile::tempdir().unwrap();
	let store = Arc::new(Store::open(&temporary.path().join("test.sqlite")).unwrap());
	let providers = Providers::new(store, Arc::new(EmptySecrets)).unwrap();
	let (events, _) = broadcast::channel(32);
	let engine = Engine::new(
		providers,
		EnginePaths {
			data_dir: temporary.path().to_path_buf(),
			native_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources"),
		},
		events,
	)
	.unwrap();
	(temporary, engine)
}
fn message(id: &str, user: &str, owner: bool, text: &str) -> ChatMessage {
	ChatMessage {
		platform: "preview".into(),
		user_id: user.into(),
		display_name: user.into(),
		message_id: id.into(),
		channel_id: "stream".into(),
		text: text.into(),
		is_owner: owner,
	}
}

#[tokio::test]
async fn owner_cancel_invalidates_other_actors_and_speech_already_queued() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap(); // Empty credentials: no provider connection or outbound message.
	let (own, other, speech, ingress) = {
		let mut session = engine.session.lock().await;
		let session = session.as_mut().unwrap();
		(
			session.agent_scopes.lease("preview:owner"),
			session.agent_scopes.lease("preview:viewer"),
			session.speech_cancel.child_token(),
			session.agent_scopes.epoch(),
		)
	};
	engine
		.handle_chat(message("cancel", "owner", true, "!cancel"))
		.await
		.unwrap();
	assert!(own.cancel.is_cancelled());
	assert!(other.cancel.is_cancelled());
	assert!(speech.is_cancelled());
	assert!(ingress.is_cancelled());
	assert!(engine.is_active());
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn actor_cancel_cancels_only_their_pending_questions_in_the_same_channel() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	for (id, actor, channel) in [
		("own", "preview:alice", "preview:stream"),
		("other", "preview:bob", "preview:stream"),
		("other-channel", "preview:alice", "preview:private"),
	] {
		engine.store.create_turn(id, actor, &json!({})).unwrap();
		engine
			.store
			.suspend_turn(
				&PendingInput {
					id: id.into(),
					turn_id: id.into(),
					actor: actor.into(),
					channel: channel.into(),
					kind: "confirmation".into(),
					prompt: "Approve?".into(),
					choices: vec!["yes".into(), "no".into()],
					owner_required: false,
					expires_at: crate::now_ms() + 10000,
				},
				&json!({}),
			)
			.unwrap();
	}
	engine
		.handle_chat(message("cancel", "alice", false, "!cancel"))
		.await
		.unwrap();
	assert_eq!(engine.store.turn("own").unwrap().state, "cancelled");
	assert_eq!(engine.store.turn("other").unwrap().state, "waiting");
	assert_eq!(engine.store.turn("other-channel").unwrap().state, "waiting");
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn stale_session_input_cannot_enter_a_restarted_session() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	let old_epoch = engine
		.session
		.lock()
		.await
		.as_ref()
		.unwrap()
		.agent_scopes
		.epoch();
	engine.stop().await.unwrap();
	engine.start().await.unwrap();
	engine
		.handle_chat_scoped(
			message("stale", "alice", false, "!bee execute old request"),
			old_epoch,
		)
		.await
		.unwrap();
	assert!(engine.store.chatter("preview", "alice").is_err());
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn stop_cancels_preview_even_without_an_active_session() {
	let (_temporary, engine) = fixture();
	let preview = engine.preview_cancel.lock().await.clone();
	engine.stop().await.unwrap();
	assert!(preview.is_cancelled());
}

#[tokio::test]
async fn canceled_voice_capture_cannot_reenter_as_a_fresh_transcribed_turn() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	let (scope, voice_cancel) = {
		let mut session = engine.session.lock().await;
		let session = session.as_mut().unwrap();
		(
			session.agent_scopes.lease("discord:listener"),
			session.voice_cancel.child_token(),
		)
	};
	assert!(engine.cancel_actor("discord:listener").await);
	let mut source = message(
		"late-transcript",
		"listener",
		false,
		"Bumblebee perform the old request",
	);
	source.platform = "discord_voice".into();
	engine
		.dispatch_transcription(source, voice_cancel, scope)
		.await
		.unwrap();
	assert!(engine.store.chatter("discord", "listener").is_err());
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn revoked_voice_capture_cannot_dispatch_even_when_actor_requests_remain_allowed() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	let (scope, voice_cancel) = {
		let mut session = engine.session.lock().await;
		let session = session.as_mut().unwrap();
		(
			session.agent_scopes.lease("discord:listener"),
			session.voice_cancel.child_token(),
		)
	};
	voice_cancel.cancel();
	assert!(!scope.cancel.is_cancelled());
	let mut source = message(
		"revoked-transcript",
		"listener",
		false,
		"Bumblebee perform the old request",
	);
	source.platform = "discord_voice".into();
	engine
		.dispatch_transcription(source, voice_cancel, scope)
		.await
		.unwrap();
	assert!(engine.store.chatter("discord", "listener").is_err());
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn transcribed_cancel_finishes_cleanup_without_cancelling_its_own_future() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	let (scope, voice_cancel) = {
		let mut session = engine.session.lock().await;
		let session = session.as_mut().unwrap();
		(
			session.agent_scopes.lease("discord:owner"),
			session.voice_cancel.child_token(),
		)
	};
	let preview_guard = engine.preview_cancel.lock().await;
	let preview = preview_guard.clone();
	let mut source = message("spoken-stop", "owner", true, "bumblebee stop");
	source.platform = "discord_voice".into();
	let mut task = tokio::spawn({
		let engine = engine.clone();
		let scope = scope.clone();
		async move {
			engine
				.dispatch_transcription(source, voice_cancel, scope)
				.await
		}
	});
	tokio::time::timeout(Duration::from_secs(2), scope.cancel.cancelled())
		.await
		.unwrap();
	assert!(
		tokio::time::timeout(Duration::from_millis(20), &mut task)
			.await
			.is_err(),
		"cancel must wait for its cleanup rather than dropping its own future"
	);
	drop(preview_guard);
	task.await.unwrap().unwrap();
	assert!(preview.is_cancelled());
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn session_stop_surfaces_aborted_work_and_preserves_uncertain_effects_without_app_restart() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.create_turn("aborted", "preview:owner", &json!({}))
		.unwrap();
	engine
		.store
		.begin_call(
			"aborted",
			"write",
			"banTwitchUser",
			r#"{"broadcasterId":"1","userId":"2"}"#,
		)
		.unwrap();
	engine.stop().await.unwrap();
	assert!(
		engine
			.store
			.interrupted_turns()
			.unwrap()
			.iter()
			.any(|turn| turn.id == "aborted")
	);
	engine.start().await.unwrap();
	engine
		.store
		.create_turn("fresh", "preview:owner", &json!({}))
		.unwrap();
	assert!(
		engine
			.store
			.begin_call(
				"fresh",
				"new-call-id",
				"banTwitchUser",
				r#"{"broadcasterId":"1","userId":"2"}"#
			)
			.is_err()
	);
	engine.stop().await.unwrap();
}
