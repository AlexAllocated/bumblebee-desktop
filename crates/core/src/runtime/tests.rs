use super::*;
use crate::{agent_storage::PendingInput, providers::SecretStore};
use serde_json::{Value, json};

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
async fn session_stop_keeps_cancel_terminal_and_preserves_uncertain_effects_without_app_restart() {
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
			crate::storage::CallEffect::MayMutate,
		)
		.unwrap();
	engine.stop().await.unwrap();
	assert_eq!(engine.store.turn("aborted").unwrap().state, "cancelled");
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
				r#"{"broadcasterId":"1","userId":"2"}"#,
				crate::storage::CallEffect::MayMutate,
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
	assert!(
		matches!(&job.input, AgentInput::Message(message) if crate::agent::is_desktop_answer(message))
	);
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

async fn next_cue(events: &mut broadcast::Receiver<OverlayEvent>) -> OverlayEvent {
	tokio::time::timeout(Duration::from_secs(3), async {
		loop {
			let event = events.recv().await.unwrap();
			if matches!(event, OverlayEvent::Signal { .. }) {
				return event;
			}
		}
	})
	.await
	.unwrap()
}

#[tokio::test]
async fn voice_acknowledgements_preserve_original_gain_and_native_waveforms() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.patch_settings(&json!({"aiEnabled":true}))
		.unwrap();
	let capture = engine
		.begin_voice_capture("listener", "Listener".into(), true)
		.await
		.unwrap()
		.unwrap();
	let mut events = engine.events.subscribe();
	for (key, kind, expected_gain) in [
		(bumblebee_audio::SignalKey::WakeChirp, "wake", 0.20),
		(bumblebee_audio::SignalKey::HeardChirp, "heard", 0.32),
		(bumblebee_audio::SignalKey::TimeoutChirp, "timeout", 1.0),
	] {
		let plan = engine.voice_cue(key, &capture).unwrap();
		let task = tokio::spawn({
			let engine = engine.clone();
			async move {
				engine.play_voice_cue(plan).await.unwrap();
			}
		});
		let OverlayEvent::Signal {
			audio_path,
			gain,
			kind: actual,
			..
		} = next_cue(&mut events).await
		else {
			unreachable!()
		};
		assert_eq!(actual, kind);
		assert!((gain - expected_gain).abs() < 0.0001);
		let published = std::fs::read(engine.paths.data_dir.join("media").join(audio_path)).unwrap();
		assert_eq!(published, bumblebee_audio::signal_wav(key).unwrap());
		task.await.unwrap();
	}
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn owner_wake_interrupts_readout_but_preserves_unrelated_actions_and_rejects_duplicate_wake()
{
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.patch_settings(&json!({"aiEnabled":true,"ownerDiscordId":"123"}))
		.unwrap();
	let (readout, unrelated) = {
		let mut session = engine.session.lock().await;
		let s = session.as_mut().unwrap();
		(
			s.speech_cancel.child_token(),
			s.agent_scopes.lease("twitch:other"),
		)
	};
	let playback = engine.speech_lock.lock().await;
	let mut requests = HashMap::new();
	assert!(
		engine
			.accept_voice_wake("viewer", "Viewer".into(), true, &mut requests)
			.await
			.unwrap()
			.is_none()
	);
	let capture = engine
		.accept_voice_wake("123", "Owner".into(), true, &mut requests)
		.await
		.unwrap()
		.unwrap();
	assert!(readout.is_cancelled());
	assert!(!unrelated.cancel.is_cancelled());
	assert!(
		engine
			.accept_voice_wake("123", "Owner".into(), true, &mut requests)
			.await
			.unwrap()
			.is_none()
	);
	let mut events = engine.events.subscribe();
	let cue = engine
		.voice_cue(bumblebee_audio::SignalKey::WakeChirp, &capture)
		.unwrap();
	let task = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine.play_voice_cue(cue).await.unwrap();
		}
	});
	tokio::task::yield_now().await;
	assert!(events.try_recv().is_err());
	drop(playback);
	assert!(matches!(next_cue(&mut events).await,OverlayEvent::Signal{kind,..} if kind=="wake"));
	task.await.unwrap();
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn silence_deadline_does_not_expire_active_speech_or_emit_stale_timeout_cues() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.patch_settings(&json!({"aiEnabled":true}))
		.unwrap();
	let silent = engine
		.begin_voice_capture("silent", "Silent".into(), true)
		.await
		.unwrap()
		.unwrap();
	let mut speaking = engine
		.begin_voice_capture("speaking", "Speaking".into(), true)
		.await
		.unwrap()
		.unwrap();
	let started = silent.started_at;
	speaking.started_at = started;
	speaking.note_voice_activity(true, started + Duration::from_millis(20));
	let mut captures = HashMap::from([("silent".into(), silent), ("speaking".into(), speaking)]);
	assert!(voice::expired_captures(&mut captures, started + Duration::from_secs(15)).is_empty());
	let mut expired = voice::expired_captures(&mut captures, started + Duration::from_secs(16));
	assert_eq!(expired.len(), 1);
	let (user, capture, timed_out) = expired.remove(0);
	assert_eq!(user, "silent");
	assert!(timed_out);
	assert!(captures.contains_key("speaking"));
	let stale = engine
		.voice_cue(bumblebee_audio::SignalKey::TimeoutChirp, &capture)
		.unwrap();
	let mut events = engine.events.subscribe();
	// A new wake replaces/cancels this capture before the queued cue even polls.
	capture.cancel.cancel();
	engine.play_voice_cue(stale).await.unwrap();
	assert!(events.try_recv().is_err());
	// A continuous utterance is force-finalized20s after its FIRST voiced frame.
	// Later frames must not move that deadline or make it fire twice.
	captures
		.get_mut("speaking")
		.unwrap()
		.note_voice_activity(true, started + Duration::from_secs(19));
	assert!(
		voice::finalize_due_captures(&mut captures, started + Duration::from_secs(20)).is_empty()
	);
	assert_eq!(
		voice::finalize_due_captures(&mut captures, started + Duration::from_secs(21)),
		vec!["speaking"]
	);
	assert!(
		voice::finalize_due_captures(&mut captures, started + Duration::from_secs(22)).is_empty()
	);
	assert!(voice::expired_captures(&mut captures, started + Duration::from_secs(22)).is_empty());
	// A tiny VAD blip may be discarded by the native80ms threshold. Without a
	// native finalized event, close its capture rather than waiting indefinitely.
	let stalled = voice::expired_captures(&mut captures, started + Duration::from_secs(23));
	assert_eq!(stalled.len(), 1);
	assert!(stalled[0].2);
	let mut continuous = engine
		.begin_voice_capture("continuous", "Continuous".into(), true)
		.await
		.unwrap()
		.unwrap();
	continuous.started_at = started;
	continuous.note_voice_activity(true, started + Duration::from_secs(119));
	captures.insert("continuous".into(), continuous);
	let absolute = voice::expired_captures(&mut captures, started + Duration::from_secs(120));
	assert_eq!(absolute.len(), 1);
	assert!(absolute[0].2);
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn voice_cancel_acknowledges_after_cleanup_and_stale_cues_cannot_cross_cancellation() {
	let (_tmp, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.patch_settings(&json!({"aiEnabled":true}))
		.unwrap();
	let capture = engine
		.begin_voice_capture("listener", "Listener".into(), true)
		.await
		.unwrap()
		.unwrap();
	let old = engine
		.voice_cue(bumblebee_audio::SignalKey::HeardChirp, &capture)
		.unwrap();
	let mut events = engine.events.subscribe();
	engine.cancel().await;
	assert!(capture.cancel.is_cancelled());
	assert!(capture.actor_scope.cancel.is_cancelled());
	while events.try_recv().is_ok() {}
	engine.play_voice_cue(old).await.unwrap();
	assert!(events.try_recv().is_err());
	// Local cancellation feedback still works while AI is disabled.
	engine
		.store
		.patch_settings(&json!({"aiEnabled":false}))
		.unwrap();
	let (ack, token) = engine.cancel_voice_cue("listener").await.unwrap().unwrap();
	let task = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine.play_voice_cue(ack).await.unwrap();
		}
	});
	assert!(
		matches!(next_cue(&mut events).await,OverlayEvent::Signal{kind,gain,..} if kind=="cancel"&&gain==1.0)
	);
	token.cancel();
	task.await.unwrap();
	while events.try_recv().is_ok() {}
	let (stale, _) = engine.cancel_voice_cue("listener").await.unwrap().unwrap();
	let lock = engine.speech_lock.lock().await;
	let task = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine.play_voice_cue(stale).await.unwrap();
		}
	});
	tokio::task::yield_now().await;
	engine.cancel_signals();
	drop(lock);
	task.await.unwrap();
	assert!(events.try_recv().is_err());
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn thinking_progress_uses_stable_two_clip_batches_and_stops_before_delivery() {
	let (_tmp, engine) = fixture();
	let mut events = engine.events.subscribe();
	let variant = "one-durable-turn".to_string();
	let expected = bumblebee_audio::signal_wav_with_options(
		bumblebee_audio::SignalKey::ThinkingLoop,
		2,
		Some(&variant),
	)
	.unwrap();
	for _ in 0..2 {
		let (done, wait) = tokio::sync::oneshot::channel::<()>();
		let task = tokio::spawn({
			let engine = engine.clone();
			let variant = variant.clone();
			async move {
				engine
					.while_thinking(true, variant, CancellationToken::new(), async {
						wait.await.unwrap();
						Ok(())
					})
					.await
					.unwrap();
			}
		});
		let OverlayEvent::Signal {
			id,
			audio_path,
			kind,
			..
		} = next_cue(&mut events).await
		else {
			unreachable!()
		};
		assert_eq!(kind, "thinking");
		assert_eq!(
			std::fs::read(engine.paths.data_dir.join("media").join(audio_path)).unwrap(),
			expected
		);
		done.send(()).unwrap();
		task.await.unwrap();
		assert!(
			matches!(events.recv().await.unwrap(),OverlayEvent::StopSignal{id:stopped} if stopped==id)
		);
		assert!(engine.speech_lock.try_lock().is_ok());
	}
	engine
		.while_thinking(false, variant, CancellationToken::new(), async { Ok(()) })
		.await
		.unwrap();
	assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn voice_processing_feedback_ignores_text_progress_but_honors_silent_and_private_delivery() {
	use crate::agent::{Delivery, with_thinking_feedback};
	let (_tmp, engine) = fixture();
	let mut events = engine.events.subscribe();
	let spoken = Delivery {
		speech: true,
		public_progress: false,
		targets: vec!["source".into()],
		discord_dm_user_id: None,
		dm_channel: None,
	};
	let quiet = Delivery {
		speech: false,
		..spoken.clone()
	};
	let private = Delivery {
		targets: vec!["discord_dm".into()],
		..spoken.clone()
	};
	for (platform, delivery, expected) in [
		("discord_voice", None, true),
		("discord_voice", Some(spoken.clone()), true),
		("discord_voice", Some(quiet), false),
		("discord_voice", Some(private), false),
		("twitch", None, false),
		("discord", None, false),
	] {
		let (done, wait) = tokio::sync::oneshot::channel::<()>();
		let mut source = message("processing", "owner", true, "What's two plus two?");
		source.platform = platform.into();
		let task = tokio::spawn({
			let engine = engine.clone();
			async move {
				with_thinking_feedback(
					&engine,
					&source,
					delivery.as_ref(),
					"real-turn".into(),
					CancellationToken::new(),
					true,
					async {
						wait.await.unwrap();
						Ok(())
					},
				)
				.await
				.unwrap();
			}
		});
		if expected {
			let OverlayEvent::Signal {
				audio_path, kind, ..
			} = next_cue(&mut events).await
			else {
				unreachable!()
			};
			assert_eq!(kind, "thinking");
			assert_eq!(
				std::fs::read(engine.paths.data_dir.join("media").join(audio_path)).unwrap(),
				bumblebee_audio::signal_wav_with_options(
					bumblebee_audio::SignalKey::ThinkingLoop,
					2,
					Some("real-turn")
				)
				.unwrap()
			);
		} else {
			assert!(
				tokio::time::timeout(Duration::from_millis(40), events.recv())
					.await
					.is_err()
			);
		}
		done.send(()).unwrap();
		task.await.unwrap();
		if expected {
			assert!(matches!(
				events.recv().await.unwrap(),
				OverlayEvent::StopSignal { .. }
			));
		}
		assert!(events.try_recv().is_err());
	}
}

#[tokio::test]
async fn voice_provider_failure_has_nonverbal_feedback_but_cancelled_work_does_not() {
	let (_tmp, engine) = fixture();
	let mut events = engine.events.subscribe();
	let (done, wait) = tokio::sync::oneshot::channel::<()>();
	let task = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine
				.while_voice_processing("transcription".into(), CancellationToken::new(), async {
					wait.await.unwrap();
					Err::<(), _>(anyhow::anyhow!("Model access denied"))
				})
				.await
		}
	});
	assert!(matches!(next_cue(&mut events).await,OverlayEvent::Signal{kind,..} if kind=="thinking"));
	done.send(()).unwrap();
	assert!(matches!(
		events.recv().await.unwrap(),
		OverlayEvent::StopSignal { .. }
	));
	assert!(matches!(next_cue(&mut events).await,OverlayEvent::Signal{kind,..} if kind=="timeout"));
	assert!(
		task
			.await
			.unwrap()
			.unwrap_err()
			.to_string()
			.contains("Model access denied")
	);
	while events.try_recv().is_ok() {}
	let cancelled = CancellationToken::new();
	let token = cancelled.clone();
	let task = tokio::spawn({
		let engine = engine.clone();
		async move {
			engine
				.while_voice_processing("cancelled".into(), token.clone(), async {
					token.cancelled().await;
					Err::<(), _>(anyhow::anyhow!("cancelled"))
				})
				.await
		}
	});
	assert!(matches!(next_cue(&mut events).await,OverlayEvent::Signal{kind,..} if kind=="thinking"));
	cancelled.cancel();
	assert!(task.await.unwrap().is_err());
	assert!(matches!(
		events.recv().await.unwrap(),
		OverlayEvent::StopSignal { .. }
	));
	assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn voice_transcripts_never_create_chat_puppets_but_discord_text_still_does() {
	let (_dir, engine) = fixture();
	let mut settings = engine.store.settings().unwrap();
	settings.ai_enabled = false;
	settings.read_chat = false;
	settings.discord_text_channel_id = "123".into();
	engine.store.set("installation", &settings).unwrap();
	engine.start().await.unwrap();
	let mut events = engine.events.subscribe();
	let mut voice = message("voice-only", "speaker", false, "Ordinary voice transcript");
	voice.platform = "discord_voice".into();
	voice.channel_id = "123".into();
	engine.handle_chat(voice).await.unwrap();
	while let Ok(event) = events.try_recv() {
		assert!(!matches!(
			event,
			OverlayEvent::Chat { .. } | OverlayEvent::ChatterChanged { .. }
		));
	}
	let mut text = message("ordinary-text", "speaker", false, "Ordinary Discord text");
	text.platform = "discord".into();
	text.channel_id = "123".into();
	engine.handle_chat(text).await.unwrap();
	assert!(matches!(
		events.try_recv().unwrap(),
		OverlayEvent::Chat { .. }
	));
	engine.stop().await.unwrap();
}
fn saved_recovery(engine: &Engine, id: &str, eligible: bool) -> Value {
	let mut settings = engine.store.settings().unwrap();
	settings.ai_enabled = true;
	engine.store.set("installation", &settings).unwrap();
	let cp = json!({"id":id,"source":message("saved-message","owner",true,"Saved request"),"model":"unused-fixture","items":[],"rounds":2,"executed":0,"calls":[],"cursor":0,"delivery":{"speech":false,"publicProgress":false,"targets":[],"discordDmUserId":null},"artifacts":[],"pending":null,"reply_route":null,"answer":null,"approved_call":null,"voice_channel_id":null,"owner_context":false,"requester_was_owner":true,"access_bindings":{"identities":{}},"pending_final":{"text":"Saved local result","messages":null},"failure_final":false,"recovery_eligible":eligible});
	engine.store.create_turn(id, "preview:owner", &cp).unwrap();
	engine.store.recover_interrupted().unwrap();
	cp
}
#[tokio::test]
async fn starting_session_recovers_saved_local_final_through_queue_once_and_rejects_replay() {
	let (_dir, engine) = fixture();
	saved_recovery(&engine, "saved", true);
	saved_recovery(&engine, "legacy", false);
	assert_eq!(engine.store.turn("saved").unwrap().state, "interrupted");
	engine.start().await.unwrap();
	tokio::time::timeout(Duration::from_secs(2), async {
		while engine.store.turn("saved").unwrap().state != "completed" {
			tokio::time::sleep(Duration::from_millis(5)).await;
		}
	})
	.await
	.unwrap();
	assert_eq!(engine.store.turn("legacy").unwrap().state, "interrupted");
	assert!(engine.resume_interrupted("saved").await.is_err());
	assert!(engine.resume_interrupted("legacy").await.is_err());
	engine.stop().await.unwrap();
	engine.start().await.unwrap();
	tokio::time::sleep(Duration::from_millis(20)).await;
	assert_eq!(
		engine
			.store
			.history("preview:owner:preview:stream")
			.unwrap()
			.len(),
		1
	);
	engine.stop().await.unwrap();
}
#[tokio::test]
async fn explicit_retry_uses_queue_after_failed_startup_readiness_and_keeps_terminal_cancellation()
{
	let (_dir, engine) = fixture();
	saved_recovery(&engine, "saved", true);
	let mut settings = engine.store.settings().unwrap();
	settings.ai_enabled = false;
	engine.store.set("installation", &settings).unwrap();
	engine.start().await.unwrap();
	tokio::time::timeout(Duration::from_secs(2), async {
		while !engine.recovery_attempts.lock().unwrap().contains("saved") {
			tokio::task::yield_now().await;
		}
	})
	.await
	.unwrap();
	assert_eq!(engine.store.turn("saved").unwrap().state, "interrupted");
	assert!(engine.resume_interrupted("saved").await.is_err());
	settings.ai_enabled = true;
	engine.store.set("installation", &settings).unwrap();
	engine.resume_interrupted("saved").await.unwrap();
	tokio::time::timeout(Duration::from_secs(2), async {
		while engine.store.turn("saved").unwrap().state != "completed" {
			tokio::time::sleep(Duration::from_millis(5)).await;
		}
	})
	.await
	.unwrap();
	let cp = saved_recovery(&engine, "cancel-me", true);
	engine
		.store
		.checkpoint_turn("cancel-me", "queued_recovery", &cp)
		.unwrap();
	engine.cancel().await;
	assert_eq!(engine.store.turn("cancel-me").unwrap().state, "cancelled");
	assert!(engine.resume_interrupted("cancel-me").await.is_err());
	engine.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_request_epoch_stays_hidden_until_durable_cancellation_finishes() {
	let (_dir, engine) = fixture();
	let cancel = CancellationToken::new();
	let scopes = WorkScopes::new(&cancel);
	let previous = scopes.epoch();
	let (input_epoch, _) = watch::channel(previous.clone());
	*engine.session.lock().await = Some(Session {
		cancel: cancel.clone(),
		jobs: vec![],
		speech_tx: mpsc::channel(1).0,
		agent_tx: mpsc::channel(1).0,
		agent_scopes: scopes,
		input_epoch,
		speech_cancel: cancel.child_token(),
		voice_cancel: cancel.child_token(),
	});
	let database_guard = engine.store.db().unwrap();
	let cancel = tokio::spawn({
		let engine = engine.clone();
		async move { engine.cancel().await }
	});
	tokio::time::timeout(Duration::from_secs(2), previous.cancelled())
		.await
		.unwrap();
	assert!(
		tokio::time::timeout(Duration::from_millis(30), engine.session.lock())
			.await
			.is_err(),
		"fresh request scopes must not be exposed while old durable cancellation is blocked"
	);
	drop(database_guard);
	cancel.await.unwrap();
	let fresh = engine
		.session
		.lock()
		.await
		.as_mut()
		.unwrap()
		.agent_scopes
		.lease("preview:owner");
	assert!(!fresh.cancel.is_cancelled());
	engine.stop().await.unwrap();
}

#[tokio::test]
async fn stopping_always_cancels_and_joins_session_tasks_when_sqlite_cleanup_fails() {
	let (_dir, engine) = fixture();
	engine.start().await.unwrap();
	engine
		.store
		.create_turn("fail-on-stop", "preview:owner", &json!({}))
		.unwrap();
	let stopped = Arc::new(AtomicBool::new(false));
	let token = {
		let mut session = engine.session.lock().await;
		let session = session.as_mut().unwrap();
		let token = session.cancel.clone();
		let job_token = token.clone();
		let stopped = stopped.clone();
		session.jobs.push(tokio::spawn(async move {
			job_token.cancelled().await;
			stopped.store(true, Ordering::SeqCst);
		}));
		token
	};
	engine.store.db().unwrap().execute_batch("CREATE TRIGGER prevent_turn_write BEFORE UPDATE ON agent_turns BEGIN SELECT RAISE(ABORT,'test persistence failure'); END;").unwrap();
	let error = engine.stop().await.unwrap_err();
	assert!(error.to_string().contains("test persistence failure"));
	assert!(!engine.is_active());
	assert!(engine.session.lock().await.is_none());
	assert!(token.is_cancelled());
	assert!(stopped.load(Ordering::SeqCst));
	assert!(engine.preview_cancel.lock().await.is_cancelled());
}
