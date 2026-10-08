//! Application-owned sessions with bounded input and speech queues.
mod queue;
use crate::{
	commands,
	model::{ChatMessage, Chatter, OverlayEvent},
	providers::Providers,
	speech::Speech,
	storage::Store,
};
use anyhow::{Context, Result, ensure};
use bumblebee_audio::{AudioRuntime, NativeResources, VoiceEvent};
use queue::{AgentJob, WorkScopes, run_agent_queue};
use std::{
	collections::HashMap,
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Duration,
};
use tokio::{
	sync::{Mutex, broadcast, mpsc, watch},
	task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub struct EnginePaths {
	pub data_dir: PathBuf,
	pub native_dir: PathBuf,
}
pub struct Engine {
	pub providers: Arc<Providers>,
	pub store: Arc<Store>,
	pub paths: EnginePaths,
	events: broadcast::Sender<OverlayEvent>,
	speech: Speech,
	active: AtomicBool,
	lifecycle: Mutex<()>,
	session: Mutex<Option<Session>>,
	audio_runtime: Mutex<Option<Arc<AudioRuntime>>>,
	speech_lock: Mutex<()>,
	preview_cancel: Mutex<CancellationToken>,
}
struct Session {
	cancel: CancellationToken,
	jobs: Vec<tokio::task::JoinHandle<()>>,
	speech_tx: mpsc::Sender<SpeechJob>,
	agent_tx: mpsc::Sender<AgentJob>,
	agent_scopes: WorkScopes,
	input_epoch: watch::Sender<CancellationToken>,
	speech_cancel: CancellationToken,
	voice_cancel: CancellationToken,
}
struct SpeechJob {
	text: String,
	chatter: Option<Chatter>,
	cancel: CancellationToken,
}
struct Capture {
	username: String,
	pcm: Vec<u8>,
	cancel: CancellationToken,
	actor_scope: Arc<queue::ActorScope>,
}
impl Engine {
	pub fn new(
		providers: Arc<Providers>,
		paths: EnginePaths,
		events: broadcast::Sender<OverlayEvent>,
	) -> Result<Arc<Self>> {
		std::fs::create_dir_all(paths.data_dir.join("images"))?;
		NativeResources::from_bundle(&paths.native_dir).initialize()?;
		providers.store.recover_interrupted()?;
		providers.store.recover_agent_state()?;
		let speech = Speech::new(providers.clone(), paths.data_dir.clone())?;
		Ok(Arc::new(Self {
			store: providers.store.clone(),
			providers,
			paths,
			events,
			speech,
			active: AtomicBool::new(false),
			lifecycle: Mutex::new(()),
			session: Mutex::new(None),
			audio_runtime: Mutex::new(None),
			speech_lock: Mutex::new(()),
			preview_cancel: Mutex::new(CancellationToken::new()),
		}))
	}
	pub fn is_active(&self) -> bool {
		self.active.load(Ordering::SeqCst)
	}
	pub fn emit(&self, event: OverlayEvent) {
		let _ = self.events.send(event);
	}
	pub async fn audio(&self) -> Option<Arc<AudioRuntime>> {
		self.audio_runtime.lock().await.clone()
	}
	pub async fn start(self: &Arc<Self>) -> Result<()> {
		let _lifecycle = self.lifecycle.lock().await;
		ensure!(!self.is_active(), "A session is already running");
		let settings = self.store.settings()?;
		settings.validate()?;
		let twitch_configured = self.providers.secrets.get("twitch_tokens")?.is_some();
		let youtube_configured = self.providers.secrets.get("google_tokens")?.is_some();
		let discord_token = self
			.providers
			.secrets
			.get("discord_bot")?
			.filter(|s| !s.is_empty());
		let speech_configured = self.providers.secrets.get("azure_speech")?.is_some();
		let cancel = CancellationToken::new();
		let (messages_tx, mut messages_rx) = mpsc::channel::<ChatMessage>(256);
		let (speech_tx, mut speech_rx) = mpsc::channel::<SpeechJob>(32);
		let (agent_tx, agent_rx) = mpsc::channel::<AgentJob>(16);
		let agent_scopes = WorkScopes::new(&cancel);
		let (input_epoch, input_epoch_rx) = watch::channel(agent_scopes.epoch());
		let mut jobs = Vec::new();
		let engine = self.clone();
		let token = cancel.clone();
		jobs.push(tokio::spawn(async move {
            let mut epoch=input_epoch_rx.borrow().clone();
            loop {
                tokio::select! {biased;
                    _=token.cancelled()=>break,
                    _=epoch.cancelled()=>{
                        // Discard the bounded inbox present at interruption, then accept fresh input.
                        for _ in 0..256 {if messages_rx.try_recv().is_err(){break;}}
                        epoch=input_epoch_rx.borrow().clone();
                    },
                    message=messages_rx.recv()=>match message {
                        Some(message)=>if let Err(error)=engine.handle_chat_scoped(message,epoch.child_token()).await {
                            if !epoch.is_cancelled() && !token.is_cancelled(){engine.emit(OverlayEvent::Status{message:error.to_string()});}
                        },
                        None=>break,
                    }
                }
            }
        }));
		let engine = self.clone();
		let token = cancel.clone();
		jobs.push(tokio::spawn(async move{loop{tokio::select!{_=token.cancelled()=>break,job=speech_rx.recv()=>match job{Some(job)=>if !job.cancel.is_cancelled(){if let Err(error)=engine.speak(&job.text,job.chatter,job.cancel).await{if !token.is_cancelled(){engine.emit(OverlayEvent::Status{message:error.to_string()});}}},None=>break}}}}));
		let engine = self.clone();
		let token = cancel.clone();
		jobs.push(tokio::spawn(run_agent_queue(
			agent_rx,
			token.clone(),
			move |message, turn| {
				let engine = engine.clone();
				let token = token.clone();
				async move {
					if let Err(error) = crate::agent::run(engine.clone(), message, turn.clone()).await {
						if !token.is_cancelled() && !turn.is_cancelled() {
							engine.emit(OverlayEvent::Status {
								message: error.to_string(),
							});
						}
					}
				}
			},
		)));
		*self.session.lock().await = Some(Session {
			cancel: cancel.clone(),
			jobs: Vec::new(),
			speech_tx,
			agent_tx,
			agent_scopes,
			input_epoch,
			speech_cancel: cancel.child_token(),
			voice_cancel: cancel.child_token(),
		});
		self.active.store(true, Ordering::SeqCst);
		if twitch_configured {
			let providers = self.providers.clone();
			let tx = messages_tx.clone();
			let token = cancel.child_token();
			jobs.push(tokio::spawn(async move {
				let _ = providers.twitch_chat(tx, token).await;
			}));
		} else {
			self.providers.status(
				"twitch",
				"missing_credentials",
				"Connect your Twitch account in Settings",
			);
		}
		if youtube_configured {
			let providers = self.providers.clone();
			let tx = messages_tx.clone();
			let token = cancel.child_token();
			jobs.push(tokio::spawn(async move {
				let _ = providers.youtube_chat(tx, token).await;
			}));
		} else {
			self.providers.status(
				"youtube",
				"missing_credentials",
				"Connect your YouTube channel in Settings",
			);
		}
		if let Some(discord_token) = discord_token {
			let engine = self.clone();
			let token = cancel.child_token();
			jobs.push(tokio::spawn(async move {
				let result = engine.run_discord(discord_token, messages_tx, token).await;
				if let Err(error) = result {
					engine
						.providers
						.status("discord", "connection_failed", error.to_string());
				}
			}));
		} else {
			self.providers.status(
				"discord",
				"missing_credentials",
				"Add your Discord bot token in Settings",
			);
		}
		if !settings.azure_region.is_empty() && speech_configured {
			let providers = self.providers.clone();
			jobs.push(tokio::spawn(async move {
				if let Err(error) = providers.refresh_voices().await {
					providers.status("azure_speech", "connection_failed", error.to_string());
				}
			}));
		}
		let engine = self.clone();
		let token = cancel.child_token();
		jobs.push(tokio::spawn(async move {
			crate::agent::reminder_loop(engine, token).await;
		}));
		if let Some(session) = self.session.lock().await.as_mut() {
			session.jobs = jobs;
		}
		self.emit(OverlayEvent::Status {
			message: "Session started".into(),
		});
		Ok(())
	}
	pub async fn stop(&self) -> Result<()> {
		let _lifecycle = self.lifecycle.lock().await;
		let session = self.session.lock().await.take();
		self.active.store(false, Ordering::SeqCst);
		if let Some(session) = session.as_ref() {
			session.cancel.cancel();
		}
		self.cancel().await;
		if let Some(session) = session {
			if let Some(audio) = self.audio_runtime.lock().await.take() {
				audio.shutdown().await;
			}
			for mut job in session.jobs {
				if tokio::time::timeout(Duration::from_secs(5), &mut job)
					.await
					.is_err()
				{
					job.abort();
					let _ = job.await;
				}
			}
		}
		// Every session task has finished or been aborted. Surface uncertain receipts immediately,
		// including jobs that could not finish their own cancellation checkpoint before timeout.
		self.store.recover_interrupted()?;
		self.store.recover_agent_state()?;
		self.emit(OverlayEvent::StopSpeech);
		self.emit(OverlayEvent::Status {
			message: "Session stopped".into(),
		});
		Ok(())
	}
	pub async fn cancel(&self) {
		if let Some(session) = self.session.lock().await.as_mut() {
			let epoch = session.agent_scopes.cancel_all(&session.cancel);
			session.input_epoch.send_replace(epoch);
			session.speech_cancel.cancel();
			session.speech_cancel = session.cancel.child_token();
			session.voice_cancel.cancel();
			session.voice_cancel = session.cancel.child_token();
		}
		self.preview_cancel.lock().await.cancel();
		if let Some(audio) = self.audio().await {
			audio.interrupt().await;
		}
		self.emit(OverlayEvent::StopSpeech);
	}
	async fn cancel_actor(&self, actor: &str) -> bool {
		self
			.session
			.lock()
			.await
			.as_mut()
			.is_some_and(|session| session.agent_scopes.cancel_actor(actor))
	}
	pub async fn say_preview(self: &Arc<Self>, text: String) -> Result<()> {
		let token = {
			let mut token = self.preview_cancel.lock().await;
			token.cancel();
			*token = CancellationToken::new();
			token.clone()
		};
		self.speak(&text, None, token).await
	}
	pub async fn speak(
		&self,
		text: &str,
		chatter: Option<Chatter>,
		cancel: CancellationToken,
	) -> Result<()> {
		let _lock =
			tokio::select! {_=cancel.cancelled()=>return Ok(()),lock=self.speech_lock.lock()=>lock};
		if cancel.is_cancelled() {
			return Ok(());
		}
		let voices = self.store.catalog_voices()?;
		let voice_id = chatter
			.as_ref()
			.map(|c| c.voice_id.clone())
			.unwrap_or(self.store.settings()?.bumblebee_voice);
		let voice = crate::catalog::resolve_voice(&voices, &voice_id)?;
		let speech = self.speech.prepare(text, voice, cancel.clone()).await?;
		if cancel.is_cancelled() {
			return Ok(());
		}
		self.emit(OverlayEvent::Speech {
			id: uuid::Uuid::new_v4().to_string(),
			chatter,
			text: text.into(),
			audio_path: speech.filename,
			words: speech.words,
		});
		if let Some(audio) = self.audio().await {
			if audio.has_voice_presence() {
				let result = audio
					.play_audio(
						speech.wav,
						bumblebee_audio::PlaybackInputType::Encoded,
						1.0,
						cancel.clone(),
					)
					.await;
				if !result? {
					self.emit(OverlayEvent::StopSpeech);
				}
				return Ok(());
			}
		}
		tokio::select! {_=cancel.cancelled()=>self.emit(OverlayEvent::StopSpeech),_=tokio::time::sleep(Duration::from_millis(speech.duration_ms))=>{}}
		Ok(())
	}
	pub async fn handle_chat(self: &Arc<Self>, message: ChatMessage) -> Result<()> {
		let epoch = {
			let session = self.session.lock().await;
			let Some(session) = session.as_ref() else {
				return Ok(());
			};
			session.agent_scopes.epoch()
		};
		self.handle_chat_scoped(message, epoch).await
	}
	async fn handle_chat_scoped(
		self: &Arc<Self>,
		message: ChatMessage,
		cancel: CancellationToken,
	) -> Result<()> {
		if !self.is_active() || cancel.is_cancelled() {
			return Ok(());
		}
		if !self
			.store
			.claim_message(&message.platform, &message.message_id)?
		{
			return Ok(());
		}
		let mut command_message = message.clone();
		if command_message.platform == "discord_voice" {
			command_message.platform = "discord".into();
		}
		let text = message.text.trim().to_lowercase();
		let cancellation = is_cancel_request(&message);
		if cancellation {
			let actor = crate::agent_storage::actor(&message);
			// Sender/channel matching is the same as answering a durable confirmation.
			for pending in self.store.pending_inputs()?.into_iter().filter(|pending| {
				pending.actor == actor && pending.channel == crate::agent_storage::channel(&message)
			}) {
				self.store.cancel_agent_turn(&pending.turn_id)?;
			}
			if message.is_owner {
				self.cancel().await;
			} else {
				self.cancel_actor(&actor).await;
			}
			return Ok(());
		}
		let images = self.paths.data_dir.join("images");
		let command = tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),result=commands::handle(
		 &self.store,
		 &command_message,
		 &images,
		)=>result?};
		if cancel.is_cancelled() {
			return Ok(());
		}
		if let Some(command) = command {
			if let Some(chatter) = command.changed {
				self.emit(OverlayEvent::ChatterChanged { chatter });
			}
			tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),result=self.send_message(&message,&command.reply)=>{result?;}}
			return Ok(());
		}
		let profile_platform = if message.platform == "discord_voice" {
			"discord"
		} else {
			message.platform.as_str()
		};
		let chatter =
			self
				.store
				.ensure_chatter(profile_platform, &message.user_id, &message.display_name)?;
		let settings = self.store.settings()?;
		let private_dm =
			message.platform == "discord" && message.channel_id != settings.discord_text_channel_id;
		if !private_dm {
			self.emit(OverlayEvent::Chat {
				chatter: chatter.clone(),
				text: message.text.clone(),
			});
		}

		let mut session = self.session.lock().await;
		let Some(session) = session.as_mut() else {
			return Ok(());
		};
		if cancel.is_cancelled() || session.cancel.is_cancelled() {
			return Ok(());
		}
		if crate::agent::pending_for_message(&self.store, &message)?
			|| (settings.ai_enabled
				&& (text.starts_with("!bee ")
					|| text.contains("bumblebee")
					|| message.platform == "discord_voice"
					|| private_dm))
		{
			let scope = session
				.agent_scopes
				.lease(&crate::agent_storage::actor(&message));
			session
				.agent_tx
				.try_send(AgentJob { message, scope })
				.context("Bumblebee's conversation queue is full; try again shortly")?;
		} else if settings.read_chat && !private_dm && message.platform != "discord_voice" {
			session
				.speech_tx
				.try_send(SpeechJob {
					text: message.text.chars().take(6000).collect(),
					chatter: Some(chatter),
					cancel: session.speech_cancel.child_token(),
				})
				.context("The speech queue is full")?;
		}
		Ok(())
	}
	pub async fn send_message(&self, source: &ChatMessage, text: &str) -> Result<Vec<String>> {
		if source.platform == "preview" {
			self.emit(OverlayEvent::Status {
				message: text.into(),
			});
			return Ok(vec![]);
		}
		let platform = if source.platform == "discord_voice" {
			"discord"
		} else {
			source.platform.as_str()
		};
		let limit = match platform {
			"twitch" => 500,
			"youtube" => 200,
			_ => 1900,
		};
		let mut receipts = Vec::new();
		for part in split_message(text, limit) {
			let receipt = if platform == "discord" {
				self
					.audio()
					.await
					.context("Discord is disconnected")?
					.send_chat(&source.channel_id, &part)
					.await?
			} else {
				self
					.providers
					.send_stream_chat(platform, &source.channel_id, &part)
					.await?
			};
			receipts.push(receipt);
		}
		Ok(receipts)
	}
	pub async fn refresh_voice_settings(&self) -> Result<()> {
		let Some(audio) = self.audio().await else {
			return Ok(());
		};
		let settings = self.store.settings()?;
		settings.validate()?;
		if let Some(session) = self.session.lock().await.as_mut() {
			session.voice_cancel.cancel();
			session.voice_cancel = session.cancel.child_token();
		}
		let presence =
			if settings.discord_guild_id.is_empty() || settings.discord_voice_channel_id.is_empty() {
				None
			} else {
				Some(bumblebee_audio::PresenceConfig {
					guild_id: settings.discord_guild_id,
					channel_id: settings.discord_voice_channel_id,
					require_speak: true,
					listen_everyone: settings.discord_listen_everyone,
					listen_role_ids: settings.discord_listen_role_ids,
					listen_allowed_user_ids: settings.discord_listen_allowed_user_ids,
					listen_blocked_user_ids: settings.discord_listen_blocked_user_ids,
					owner_discord_id: (!settings.owner_discord_id.is_empty())
						.then_some(settings.owner_discord_id),
					replay_buffer_enabled: settings.replay_enabled,
					replay_buffer_seconds: settings.replay_seconds,
					wake_word: if settings.wake_word == "bumblebee" {
						bumblebee_audio::VoiceWakeWord::Bumblebee
					} else {
						bumblebee_audio::VoiceWakeWord::HeyBumblebee
					},
					wake_keyword_sensitivity: bumblebee_audio::VoiceKeywordSensitivity::Balanced,
					stop_keyword_sensitivity: bumblebee_audio::VoiceKeywordSensitivity::Balanced,
					cancel_keyword_sensitivity: bumblebee_audio::VoiceKeywordSensitivity::Balanced,
				})
			};
		audio.configure_presence(presence).await
	}
	async fn run_discord(
		self: &Arc<Self>,
		token: String,
		messages: mpsc::Sender<ChatMessage>,
		cancel: CancellationToken,
	) -> Result<()> {
		self
			.providers
			.status("discord", "connecting", "Connecting Discord bot");
		let (audio, mut events, mut chat) = tokio::select! {_=cancel.cancelled()=>return Ok(()),r=AudioRuntime::connect(&token,NativeResources::from_bundle(&self.paths.native_dir))=>r?};
		*self.audio_runtime.lock().await = Some(audio.clone());
		if let Err(error) = self.refresh_voice_settings().await {
			self
				.providers
				.status("discord_voice", "connection_failed", error.to_string());
		}
		self
			.providers
			.status("discord", "connected", "Discord gateway connected");
		let mut captures = HashMap::<String, Capture>::new();
		let mut sequence = None;
		let mut transcriptions = JoinSet::<Result<()>>::new();
		let result:Result<()>=async {loop {tokio::select!{
            _=cancel.cancelled()=>break,
            finished=transcriptions.join_next(),if !transcriptions.is_empty()=>{
                if let Some(Ok(Err(error)))=finished {self.emit(OverlayEvent::Status{message:error.to_string()});}
            },
            incoming=chat.recv()=>{let Some(incoming)=incoming else{break};let current=self.store.settings()?;
                let private_dm=incoming.guild_id.is_empty();
                if !private_dm&&(incoming.channel_id!=current.discord_text_channel_id||incoming.guild_id!=current.discord_guild_id){continue}
                let message=ChatMessage{platform:"discord".into(),is_owner:incoming.user_id==current.owner_discord_id,user_id:incoming.user_id,display_name:incoming.display_name,message_id:incoming.id,channel_id:incoming.channel_id,text:incoming.text};
                if private_dm&&!message.is_owner&&!crate::agent::pending_for_message(&self.store,&message)?{continue}
                tokio::select!{_=cancel.cancelled()=>break,r=messages.send(message)=>r?};
            },
            incoming=events.recv()=>{let Some(incoming)=incoming else{break};
                if sequence.is_some_and(|previous|incoming.sequence!=previous+1){for (user,capture) in &captures{capture.cancel.cancel();audio.stream_participant(user,false);}captures.clear();}
                sequence=Some(incoming.sequence);
                match incoming.event {
                    VoiceEvent::KeywordDetected{user_id,username,keyword_kind,..}=>{
                        if keyword_kind=="wake"&&audio.is_listen_allowed(&user_id){
                            if captures.len()>=4&&!captures.contains_key(&user_id){continue}
                            let (capture_cancel,actor_scope)=match self.session.lock().await.as_mut(){Some(session)=>(session.voice_cancel.child_token(),session.agent_scopes.lease(&format!("discord:{user_id}"))),None=>continue};
                            audio.stream_participant(&user_id,true);
                            if let Some(previous)=captures.insert(user_id,Capture{username,pcm:Vec::new(),cancel:capture_cancel,actor_scope}){previous.cancel.cancel();}
                        }
                        else if keyword_kind=="cancel"||keyword_kind=="stop"{
                            // Listener grants do not grant control over another person's turn.
                            if !audio.is_listen_allowed(&user_id){continue}
                            let current=self.store.settings()?;
                            if user_id==current.owner_discord_id{
                                for (id,capture) in captures.drain(){capture.cancel.cancel();audio.stream_participant(&id,false);}
                                self.cancel().await;
                            } else {
                                self.cancel_actor(&format!("discord:{user_id}")).await;
                                if let Some(capture)=captures.remove(&user_id){capture.cancel.cancel();}
                                audio.stream_participant(&user_id,false);
                            }
                        }
                    },
                    VoiceEvent::AudioFrame{user_id,sample_rate_hz,channels,..}=>{
                        if let Some(capture)=captures.get_mut(&user_id){if !capture.cancel.is_cancelled()&&!capture.actor_scope.cancel.is_cancelled()&&audio.is_listen_allowed(&user_id)&&sample_rate_hz==16000&&channels==1&&capture.pcm.len()+incoming.payload.len()<=16000*2*120{capture.pcm.extend(incoming.payload);}else{captures.remove(&user_id);audio.stream_participant(&user_id,false);}}
                    },
                    VoiceEvent::UtteranceFinalized{user_id,..}=>{audio.stream_participant(&user_id,false);if let Some(capture)=captures.remove(&user_id){
                        if capture.cancel.is_cancelled()||capture.actor_scope.cancel.is_cancelled(){continue}
                        if transcriptions.len()>=4{self.emit(OverlayEvent::Status{message:"Voice transcription is busy; please try again".into()});continue}
                        let engine=self.clone();let audio=audio.clone();
                        // Keep the actor lease through capture, transcription and queue submission.
                        transcriptions.spawn(async move{
                            let scope=capture.actor_scope.clone();
                            let voice_cancel=capture.cancel.clone();
                            let message=tokio::select!{biased;_=scope.cancel.cancelled()=>return Ok(()),result=engine.transcribe_voice(user_id,capture,audio)=>result?};
                            if let Some(message)=message {engine.dispatch_transcription(message,voice_cancel,scope).await} else {Ok(())}
                        });
                    }},
                    VoiceEvent::ParticipantLeft{user_id}|VoiceEvent::NoSpeech{user_id,..}=>{captures.remove(&user_id);audio.stream_participant(&user_id,false);},
                    VoiceEvent::SourceDown{..}=>{captures.clear();self.cancel().await;},
                    _=>{}
                }
            }
        }}Ok(())}.await;
		transcriptions.abort_all();
		while transcriptions.join_next().await.is_some() {}
		audio.shutdown().await;
		*self.audio_runtime.lock().await = None;
		self
			.providers
			.status("discord", "disconnected", "Discord disconnected");
		result
	}
	async fn transcribe_voice(
		self: &Arc<Self>,
		user_id: String,
		capture: Capture,
		audio: Arc<AudioRuntime>,
	) -> Result<Option<ChatMessage>> {
		let cancel = capture.cancel;
		let actor_scope = capture.actor_scope;
		if cancel.is_cancelled() || actor_scope.cancel.is_cancelled() || capture.pcm.len() < 3200 {
			return Ok(None);
		}
		let settings = self.store.settings()?;
		let allowed = tokio::select! {_=cancel.cancelled()=>return Ok(None),allowed=audio.revalidate_listener(&user_id)=>allowed?};
		ensure!(allowed, "Voice requester is no longer permitted");
		let wav = pcm_wav(&capture.pcm);
		let part = reqwest::multipart::Part::bytes(wav)
			.file_name("voice.wav")
			.mime_str("audio/wav")?;
		let form = reqwest::multipart::Form::new()
			.part("file", part)
			.text("model", "whisper-1")
			.text("language", "en");
		let response = tokio::select! {_=cancel.cancelled()=>return Ok(None),r=self.providers.http.post("https://api.openai.com/v1/audio/transcriptions").bearer_auth(self.providers.secret("openai")?).multipart(form).send()=>r?};
		crate::providers::check_response("openai", &response)?;
		let body: serde_json::Value =
			tokio::select! {_=cancel.cancelled()=>return Ok(None),body=response.json()=>body?};
		let text = body["text"]
			.as_str()
			.context("Transcription returned no text")?;
		if text.trim().is_empty() || cancel.is_cancelled() {
			return Ok(None);
		}
		let allowed = tokio::select! {_=cancel.cancelled()=>return Ok(None),allowed=audio.revalidate_listener(&user_id)=>allowed?};
		ensure!(allowed, "Voice permission changed during transcription");
		let current = self.store.settings()?;
		ensure!(
			current.discord_guild_id == settings.discord_guild_id
				&& current.discord_voice_channel_id == settings.discord_voice_channel_id,
			"Voice session changed during transcription"
		);
		let message = ChatMessage {
			platform: "discord_voice".into(),
			is_owner: user_id == current.owner_discord_id,
			user_id,
			display_name: capture.username,
			message_id: uuid::Uuid::new_v4().to_string(),
			channel_id: current.discord_text_channel_id,
			text: text.into(),
		};
		Ok(Some(message))
	}
	async fn dispatch_transcription(
		self: &Arc<Self>,
		message: ChatMessage,
		voice_cancel: CancellationToken,
		scope: Arc<queue::ActorScope>,
	) -> Result<()> {
		if voice_cancel.is_cancelled() || scope.cancel.is_cancelled() {
			return Ok(());
		}
		// A cancellation command must finish canceling its own scope, not abort itself mid-cleanup.
		if is_cancel_request(&message) {
			return self
				.handle_chat_scoped(message, scope.cancel.child_token())
				.await;
		}
		// Do not turn a canceled utterance into fresh work by putting it back through raw chat ingress.
		tokio::select! {biased;
			 _=voice_cancel.cancelled()=>Ok(()),
			 _=scope.cancel.cancelled()=>Ok(()),
			 result=self.handle_chat_scoped(message,scope.cancel.child_token())=>result,
		}
	}
}
fn is_cancel_request(message: &ChatMessage) -> bool {
	let text = message.text.trim().to_ascii_lowercase();
	text == "!cancel"
		|| (message.is_owner && matches!(text.as_str(), "bumblebee cancel" | "bumblebee stop"))
}
fn pcm_wav(pcm: &[u8]) -> Vec<u8> {
	let n = pcm.len() as u32;
	let mut b = Vec::with_capacity(pcm.len() + 44);
	b.extend(b"RIFF");
	b.extend((n + 36).to_le_bytes());
	b.extend(b"WAVEfmt ");
	b.extend(16u32.to_le_bytes());
	b.extend(1u16.to_le_bytes());
	b.extend(1u16.to_le_bytes());
	b.extend(16000u32.to_le_bytes());
	b.extend(32000u32.to_le_bytes());
	b.extend(2u16.to_le_bytes());
	b.extend(16u16.to_le_bytes());
	b.extend(b"data");
	b.extend(n.to_le_bytes());
	b.extend(pcm);
	b
}
pub fn split_message(text: &str, limit: usize) -> Vec<String> {
	let mut parts = Vec::new();
	let mut part = String::new();
	let mut count = 0;
	for c in text.chars() {
		if count == limit {
			parts.push(std::mem::take(&mut part));
			count = 0
		}
		part.push(c);
		count += 1
	}
	if !part.is_empty() {
		parts.push(part)
	}
	parts
}

#[cfg(test)]
mod tests;
