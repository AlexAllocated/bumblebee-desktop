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
		access: Default::default(),
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
async fn disabled_agent_blocks_new_captures_and_late_voice_provider_dispatch() {
	let (_temporary, engine) = fixture();
	engine.start().await.unwrap();
	assert!(
		engine
			.begin_voice_capture("listener", "Listener".into(), true)
			.await
			.unwrap()
			.is_none()
	);
	let mut settings = engine.store.settings().unwrap();
	settings.ai_enabled = true;
	engine.store.set("installation", &settings).unwrap();
	assert!(
		engine
			.begin_voice_capture("listener", "Listener".into(), false)
			.await
			.unwrap()
			.is_none()
	);
	let mut capture = engine
		.begin_voice_capture("listener", "Listener".into(), true)
		.await
		.unwrap()
		.unwrap();
	capture.pcm = vec![0; 6400];
	let voice_cancel = capture.cancel.clone();
	let scope = capture.actor_scope.clone();
	let session_guard = engine.session.lock().await;
	let mut queued_capture = Box::pin(engine.begin_voice_capture("queued", "Queued".into(), true));
	assert!(futures_util::poll!(queued_capture.as_mut()).is_pending());
	settings.ai_enabled = false;
	engine.store.set("installation", &settings).unwrap();
	drop(session_guard);
	assert!(queued_capture.await.unwrap().is_none());
	// The real provider-dispatch entry point returns before requiring a gateway or credential.
	assert!(
		engine
			.transcribe_voice("listener".into(), capture)
			.await
			.unwrap()
			.is_none()
	);
	let mut events = engine.events.subscribe();
	let mut source = message(
		"late-disabled-transcript",
		"listener",
		false,
		"Bumblebee answer me",
	);
	source.platform = "discord_voice".into();
	engine
		.dispatch_transcription(source, voice_cancel, scope.clone())
		.await
		.unwrap();
	assert!(engine.store.chatter("discord", "listener").is_err());
	assert!(events.try_recv().is_err());
	// Local interruption remains available even though new AI work is disabled.
	let mut cancel = message("disabled-stop", "owner", true, "bumblebee stop");
	cancel.platform = "discord_voice".into();
	engine
		.dispatch_transcription(cancel, CancellationToken::new(), scope.clone())
		.await
		.unwrap();
	assert!(scope.cancel.is_cancelled());
	settings.ai_enabled = true;
	engine.store.set("installation", &settings).unwrap();
	let mut capture = engine
		.begin_voice_capture("listener", "Listener".into(), true)
		.await
		.unwrap()
		.unwrap();
	capture.pcm = vec![0; 6400];
	assert!(
		engine
			.transcribe_voice("listener".into(), capture)
			.await
			.unwrap_err()
			.to_string()
			.contains("Discord voice is disconnected")
	);
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

#[tokio::test]
async fn desktop_pending_answer_is_private_and_uses_original_actor_cancellation() {
	let (_temporary, engine) = fixture();
	let mut settings = engine.store.settings().unwrap();
	settings.ai_enabled = true;
	engine.store.set("installation", &settings).unwrap();
	engine.start().await.unwrap();
	let (tx, mut rx) = mpsc::channel(4);
	engine.session.lock().await.as_mut().unwrap().agent_tx = tx;
	let mut events = engine.events.subscribe();
	let pending = PendingInput {
		id: "question".into(),
		turn_id: "turn".into(),
		actor: "twitch:123".into(),
		channel: "twitch:456".into(),
		kind: "question".into(),
		prompt: "Private question".into(),
		choices: vec![],
		owner_required: false,
		expires_at: crate::now_ms() + 10000,
	};
	engine
		.store
		.create_turn("turn", "twitch:123", &json!({}))
		.unwrap();
	engine
		.store
		.suspend_turn(&pending, &json!({"reply_route":{"kind":"dashboard"}}))
		.unwrap();
	let message =
		crate::agent::desktop_answer_message(&engine.store, "question", "private answer").unwrap();
	engine.handle_chat(message).await.unwrap();
	let job = rx.try_recv().unwrap();
	assert!(crate::agent::is_desktop_answer(&job.message));
	assert!(engine.store.chatters("").unwrap().is_empty());
	while let Ok(event) = events.try_recv() {
		assert!(!matches!(
			event,
			OverlayEvent::Chat { .. } | OverlayEvent::Speech { .. }
		));
	}
	assert!(engine.cancel_actor("twitch:123").await);
	assert!(job.scope.cancel.is_cancelled());
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn volume_changes_keep_sessions_and_pending_work_alive_but_revocation_cancels() {
	let (_directory, engine) = fixture();
	engine.start().await.unwrap();
	let before = engine.store.settings().unwrap();
	let (agent, speech) = {
		let mut session = engine.session.lock().await;
		let s = session.as_mut().unwrap();
		(
			s.agent_scopes.lease("preview:owner"),
			s.speech_cancel.clone(),
		)
	};
	engine
		.store
		.patch_settings(&json!({"masterVolume":0.2,"bumblebeeTtsVolume":0.4}))
		.unwrap();
	engine.settings_changed(&before).await.unwrap();
	assert!(!agent.cancel.is_cancelled());
	assert!(!speech.is_cancelled());
	assert!(engine.is_active());
	let before = engine.store.settings().unwrap();
	engine
		.store
		.patch_settings(&json!({"chatPlatforms":{"twitch":{"monitor":false}}}))
		.unwrap();
	engine.settings_changed(&before).await.unwrap();
	assert!(agent.cancel.is_cancelled());
	assert!(speech.is_cancelled());
	assert!(engine.is_active());
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn expired_and_blocked_readouts_never_reach_a_provider() {
	let (_directory, engine) = fixture();
	let source = ChatMessage {
		platform: "twitch".into(),
		..message("m", "viewer", false, "hello")
	};
	engine
		.store
		.ensure_chatter("twitch", "viewer", "Viewer")
		.unwrap();
	engine
		.run_speech_job(SpeechJob {
			source: source.clone(),
			queued_at: std::time::Instant::now() - Duration::from_secs(61),
			cancel: CancellationToken::new(),
		})
		.await
		.unwrap();
	engine
		.store
		.patch_settings(&json!({"chatTtsBlockedWords":["hello"]}))
		.unwrap();
	engine
		.run_speech_job(SpeechJob {
			source,
			queued_at: std::time::Instant::now(),
			cancel: CancellationToken::new(),
		})
		.await
		.unwrap();
}

#[tokio::test]
async fn local_setting_effect_does_not_cancel_its_own_success_before_receipt() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	let previous = engine
		.store
		.patch_settings(&serde_json::json!({"aiEnabled":true}))
		.unwrap();
	let scope = engine
		.session
		.lock()
		.await
		.as_mut()
		.unwrap()
		.agent_scopes
		.lease("preview:owner");
	engine
		.store
		.patch_settings(&serde_json::json!({"aiEnabled":false}))
		.unwrap();
	engine.settings_changed_from_agent(&previous).await.unwrap();
	assert!(!scope.cancel.is_cancelled());
	assert!(!engine.store.settings().unwrap().ai_enabled);
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn quiet_output_cues_share_native_wav_and_cancel_without_spoken_events() {
	let (_tmp, engine) = fixture();
	let mut events = engine.events.subscribe();
	let cue = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine
				.play_cue(
					bumblebee_audio::SignalKey::ThinkingLoop,
					CancellationToken::new(),
				)
				.await
		}
	});
	let event = tokio::time::timeout(Duration::from_secs(3), events.recv())
		.await
		.unwrap()
		.unwrap();
	let OverlayEvent::Signal {
		id,
		audio_path,
		kind,
		audible,
		..
	} = event
	else {
		panic!("Expected renderer cue")
	};
	assert!(audible);
	assert_eq!(kind, "thinking");
	let bytes = std::fs::read(engine.paths.data_dir.join("media").join(audio_path)).unwrap();
	assert_eq!(&bytes[..4], b"RIFF");
	engine.cancel_signals();
	cue.await.unwrap().unwrap();
	assert!(
		matches!(events.recv().await.unwrap(),OverlayEvent::StopSignal{id:stopped} if stopped==id)
	);
	let token = CancellationToken::new();
	token.cancel();
	engine
		.play_cue(bumblebee_audio::SignalKey::WakeChirp, token)
		.await
		.unwrap();
	assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn caption_off_on_invalidates_old_work_without_canceling_conversation_or_speech() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	let caption = engine.caption_cancel.lock().unwrap().child_token();
	let mut enabled = crate::model::OverlaySettings::default();
	enabled.streamer_voice_bubble.enabled = true;
	let mut disabled = enabled.clone();
	disabled.streamer_voice_bubble.enabled = false;
	let (agent, speech) = {
		let mut session = engine.session.lock().await;
		let s = session.as_mut().unwrap();
		(s.agent_scopes.lease("discord:123"), s.speech_cancel.clone())
	};
	// Store already sees reenabled settings when the delayed off notification arrives.
	engine.store.set("overlay_settings", &enabled).unwrap();
	engine.overlay_settings_changed(&disabled).unwrap();
	engine.overlay_settings_changed(&enabled).unwrap();
	assert!(caption.is_cancelled());
	assert!(!agent.cancel.is_cancelled());
	assert!(!speech.is_cancelled());
	let fresh = engine.caption_cancel.lock().unwrap().child_token();
	enabled.streamer_voice_bubble.position.horizontal_percent = 45.;
	engine.overlay_settings_changed(&enabled).unwrap();
	assert!(!fresh.is_cancelled());
	engine.stop().await.unwrap();
	assert!(fresh.is_cancelled());
}
#[tokio::test]
async fn profile_ai_block_prevents_wake_audio_capture_before_transcription() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.patch_settings(&json!({"aiEnabled":true}))
		.unwrap();
	engine
		.store
		.ensure_chatter("discord", "123", "Viewer")
		.unwrap();
	engine
		.store
		.set_chatter_overrides(
			"discord",
			"123",
			&crate::model::ChatterOverrides {
				ai_access: crate::model::Override::Block,
				..Default::default()
			},
		)
		.unwrap();
	assert!(
		engine
			.begin_voice_capture("123", "Viewer".into(), true)
			.await
			.unwrap()
			.is_none()
	);
	engine.stop().await.unwrap();
}
