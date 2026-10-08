use std::{
	collections::HashSet,
	num::NonZeroU8,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::json;
use serenity::{
	all::{
		Cache, Channel, ChannelId, Client, Context as SerenityContext, CreateAllowedMentions,
		CreateMessage, EventHandler, GatewayIntents, GuildId, Http, Member, Message, Ready, RoleId,
		UserId, VoiceState,
	},
	gateway::{ConnectionStage, ShardStageUpdateEvent},
};
use songbird::{
	Config as VoiceConfig, SerenityInit, Songbird as VoiceManager,
	driver::{Channels, DecodeConfig, DecodeMode, SampleRate},
	events::{CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler},
};
use tokio::{
	sync::{Notify, mpsc},
	time::{Duration, sleep},
};
use tracing::{error, info, warn};

use crate::{
	logging,
	protocol::SongbirdPresenceConfig,
	session::{Session, VoiceBinding},
};

#[derive(Debug, serde::Serialize)]
pub struct DiscordHealth {
	pub ok: bool,
	pub ready: bool,
	pub connected: bool,
	pub client_exited: bool,
}

// Never detach the gateway owner: canceled connection/shutdown futures must
// drop an aborting task handle rather than silently leaving Client::start alive.
struct OwnedClientTask(tokio::task::JoinHandle<()>);
impl Drop for OwnedClientTask {
	fn drop(&mut self) {
		self.0.abort();
	}
}

pub struct DiscordRuntime {
	pub manager: Arc<VoiceManager>,
	client_task: parking_lot::Mutex<Option<OwnedClientTask>>,
	pub http: Arc<Http>,
	pub cache: Arc<Cache>,
	pub shard_manager: Arc<serenity::gateway::ShardManager>,
	pub bot_user_id: UserId,
	ready: Arc<AtomicBool>,
	connected: Arc<AtomicBool>,
	client_exited: Arc<AtomicBool>,
	ready_notify: Arc<Notify>,
}

impl DiscordRuntime {
	pub async fn new(
		token: &str,
		sessions: Arc<DashMap<String, Arc<Session>>>,
		chat: mpsc::Sender<crate::DiscordMessage>,
	) -> Result<Arc<Self>> {
		let intents = GatewayIntents::GUILDS
			| GatewayIntents::GUILD_MEMBERS
			| GatewayIntents::GUILD_VOICE_STATES
			| GatewayIntents::GUILD_MESSAGES
			| GatewayIntents::DIRECT_MESSAGES
			| GatewayIntents::MESSAGE_CONTENT;
		let mut voice_config = VoiceConfig::default();
		voice_config.decode_mode =
			DecodeMode::Decode(DecodeConfig::new(Channels::Mono, SampleRate::Hz16000));
		voice_config.playout_buffer_length = NonZeroU8::new(1).expect("non-zero");
		voice_config.preallocated_tracks = 1;
		let manager = VoiceManager::serenity_from_config(voice_config);
		let ready = Arc::new(AtomicBool::new(false));
		let connected = Arc::new(AtomicBool::new(false));
		let client_exited = Arc::new(AtomicBool::new(false));
		let ready_notify = Arc::new(Notify::new());
		let http = Arc::new(Http::new(&token));
		let bot_user_id = http
			.get_current_user()
			.await
			.context("fetch current discord user")?
			.id;
		let handler = DiscordClientHandler {
			sessions: sessions.clone(),
			chat,
			ready: ready.clone(),
			connected: connected.clone(),
			ready_notify: ready_notify.clone(),
			bot_user_id,
		};
		let mut client = Client::builder(&token, intents)
			.event_handler(handler)
			.register_songbird_with(manager.clone())
			.await
			.context("build serenity client for songbird")?;

		let http = client.http.clone();
		let cache = client.cache.clone();
		let shard_manager = client.shard_manager.clone();
		let task_ready = ready.clone();
		let task_connected = connected.clone();
		let task_client_exited = client_exited.clone();
		let task_ready_notify = ready_notify.clone();

		let client_task = tokio::spawn(async move {
			if let Err(error) = client.start().await {
				error!(?error, "songbird discord client exited");
			}
			task_ready.store(false, Ordering::Relaxed);
			task_connected.store(false, Ordering::Relaxed);
			task_client_exited.store(true, Ordering::Relaxed);
			task_ready_notify.notify_waiters();
		});

		Ok(Arc::new(Self {
			manager,
			client_task: parking_lot::Mutex::new(Some(OwnedClientTask(client_task))),
			http,
			cache,
			shard_manager,
			bot_user_id,
			ready,
			connected,
			client_exited,
			ready_notify,
		}))
	}

	pub async fn wait_ready(&self) -> Result<()> {
		tokio::time::timeout(Duration::from_secs(30), async {
			loop {
				let notified = self.ready_notify.notified();
				tokio::pin!(notified);
				notified.as_mut().enable();
				if self.client_exited.load(Ordering::Relaxed) {
					return Err(anyhow!("songbird discord client exited"));
				}
				if self.ready.load(Ordering::Relaxed) && self.connected.load(Ordering::Relaxed) {
					return Ok(());
				}
				notified.await;
			}
		})
		.await
		.context("Discord gateway readiness timed out")?
	}

	pub async fn send_chat(&self, channel_id: &str, text: &str) -> Result<String> {
		anyhow::ensure!(
			!text.is_empty() && text.chars().count() <= 2000,
			"Discord messages must contain 1 to 2000 characters"
		);
		let channel = ChannelId::new(
			channel_id
				.parse()
				.context("invalid Discord text channel ID")?,
		);
		let message = channel
			.send_message(
				&self.http,
				CreateMessage::new()
					.content(text)
					.allowed_mentions(CreateAllowedMentions::new()),
			)
			.await
			.context("send Discord message")?;
		Ok(message.id.to_string())
	}

	pub fn health(&self) -> DiscordHealth {
		let ready = self.ready.load(Ordering::Relaxed);
		let connected = self.connected.load(Ordering::Relaxed);
		let client_exited = self.client_exited.load(Ordering::Relaxed);
		DiscordHealth {
			ok: ready && connected && !client_exited,
			ready,
			connected,
			client_exited,
		}
	}

	pub async fn shutdown(&self) {
		self.shard_manager.shutdown_all().await;
		let task = self.client_task.lock().take();
		if let Some(mut task) = task {
			let _ = tokio::time::timeout(Duration::from_secs(1), &mut task.0).await;
			// The owned handle aborts if shutdown times out or is canceled.
		}
		self.connected.store(false, Ordering::Relaxed);
		self.ready.store(false, Ordering::Relaxed);
		self.client_exited.store(true, Ordering::Relaxed);
	}

	pub async fn validate_voice_destination(&self, config: &SongbirdPresenceConfig) -> Result<()> {
		self.wait_ready().await?;
		let guild_id = GuildId::new(config.guild_id.parse::<u64>().context("parse guild id")?);
		let channel_id = ChannelId::new(
			config
				.channel_id
				.parse::<u64>()
				.context("parse channel id")?,
		);
		let channel = channel_id
			.to_channel(&self.http)
			.await
			.with_context(|| format!("fetch discord voice channel {guild_id}/{channel_id}"))?;
		let Channel::Guild(guild_channel) = channel else {
			return Err(
				crate::command_error::VoiceConfigurationError(format!(
					"discord channel {guild_id}/{channel_id} is not a guild voice channel"
				))
				.into(),
			);
		};
		if guild_channel.guild_id != guild_id
			|| !matches!(
				guild_channel.kind,
				serenity::model::channel::ChannelType::Voice
					| serenity::model::channel::ChannelType::Stage
			) {
			return Err(
				crate::command_error::VoiceConfigurationError(
					"Selected destination must be a voice channel in the configured server".into(),
				)
				.into(),
			);
		}
		let bot_member = guild_id
			.member(&self.http, self.bot_user_id)
			.await
			.with_context(|| format!("fetch bot guild member {guild_id}/{}", self.bot_user_id))?;
		let bot_permissions = {
			let guild = self
				.cache
				.guild(guild_id)
				.ok_or_else(|| anyhow!("discord guild {guild_id} is not available in cache"))?;
			guild.user_permissions_in(&guild_channel, &bot_member)
		};
		if !has_required_voice_permissions(bot_permissions, config.require_speak) {
			return Err(
				crate::command_error::VoiceConfigurationError(format!(
					"bot is missing required voice permissions for discord channel {guild_id}/{channel_id}"
				))
				.into(),
			);
		}
		Ok(())
	}

	pub async fn join_voice_channel(
		self: &Arc<Self>,
		session: &Arc<Session>,
		config: &SongbirdPresenceConfig,
	) -> Result<VoiceBinding> {
		let (_, revision) = session.presence_snapshot();
		self
			.validate_voice_destination(config)
			.await
			.map_err(|error| crate::command_error::bind_access_check(error, revision))?;
		let guild_id = GuildId::new(config.guild_id.parse::<u64>().context("parse guild id")?);
		let channel_id = ChannelId::new(
			config
				.channel_id
				.parse::<u64>()
				.context("parse channel id")?,
		);
		logging::log_event(
			tracing::Level::INFO,
			"songbird.voice.join.started",
			"joining discord voice channel",
			json!({
					"botId": session.bot_id,
					"guildId": guild_id.get().to_string(),
					"channelId": channel_id.get().to_string(),
			}),
		);
		let call = self.manager.get_or_insert(guild_id);
		let binding = VoiceBinding {
			guild_id: guild_id.get().to_string(),
			call: Arc::clone(&call),
		};
		session.set_presence(config.clone(), binding.clone());
		let join = match async {
			let mut call_guard = call.lock().await;
			let voice_config = call_guard
				.config()
				.clone()
				.emit_output_voice_tick(config.replay_buffer_enabled);
			call_guard.set_config(voice_config);
			call_guard.remove_all_global_events();
			call_guard.add_global_event(
				Event::Core(CoreEvent::SpeakingStateUpdate),
				SpeakingStateHandler {
					session: Arc::clone(session),
					http: self.http.clone(),
					cache: self.cache.clone(),
					bot_user_id: self.bot_user_id,
				},
			);
			call_guard.add_global_event(
				Event::Core(CoreEvent::VoiceTick),
				VoiceTickHandler {
					session: Arc::clone(session),
				},
			);
			call_guard.add_global_event(
				Event::Core(CoreEvent::OutputVoiceTick),
				OutputVoiceTickHandler {
					session: Arc::clone(session),
				},
			);
			call_guard
				.deafen(false)
				.await
				.with_context(|| format!("undeafen discord voice {guild_id}/{channel_id}"))?;
			call_guard
				.mute(false)
				.await
				.with_context(|| format!("unmute discord voice {guild_id}/{channel_id}"))?;
			call_guard
				.join(channel_id)
				.await
				.with_context(|| format!("join discord voice {guild_id}/{channel_id}"))
		}
		.await
		{
			Ok(join) => join,
			Err(error) => {
				self
					.cleanup_failed_voice_join(session, guild_id, channel_id, &error)
					.await;
				return Err(error);
			}
		};
		if let Err(error) = join
			.await
			.map_err(anyhow::Error::from)
			.with_context(|| format!("join discord voice {guild_id}/{channel_id}"))
		{
			self
				.cleanup_failed_voice_join(session, guild_id, channel_id, &error)
				.await;
			return Err(error);
		}
		if let Err(error) = async {
			let mut call_guard = call.lock().await;
			call_guard
				.deafen(false)
				.await
				.with_context(|| format!("undeafen discord voice {guild_id}/{channel_id}"))?;
			call_guard
				.mute(false)
				.await
				.with_context(|| format!("unmute discord voice {guild_id}/{channel_id}"))
		}
		.await
		{
			self
				.cleanup_failed_voice_join(session, guild_id, channel_id, &error)
				.await;
			return Err(error);
		}
		self.schedule_bot_voice_state_check(guild_id, channel_id);
		logging::log_event(
			tracing::Level::INFO,
			"songbird.voice.join.completed",
			"joined discord voice channel",
			json!({
					"botId": session.bot_id,
					"guildId": guild_id.get().to_string(),
					"channelId": channel_id.get().to_string(),
			}),
		);
		Ok(binding)
	}

	async fn cleanup_failed_voice_join(
		&self,
		session: &Arc<Session>,
		guild_id: GuildId,
		channel_id: ChannelId,
		error: &anyhow::Error,
	) {
		session.clear_presence_state(Some("voice join failed"));
		let _ = self.manager.remove(guild_id).await;
		logging::log_event(
			tracing::Level::WARN,
			"songbird.voice.join.failed",
			"failed to join discord voice channel",
			json!({
					"botId": session.bot_id,
					"guildId": guild_id.get().to_string(),
					"channelId": channel_id.get().to_string(),
					"error": {
							 "message": format!("{error:#}")
					}
			}),
		);
	}

	pub async fn leave_voice_channel(&self, guild_id: &str) {
		let Ok(guild_id) = guild_id.parse::<u64>() else {
			return;
		};
		let _ = self.manager.remove(GuildId::new(guild_id)).await;
	}

	pub async fn revalidate_participant(
		&self,
		session: &Arc<Session>,
		user_id: &str,
	) -> Result<Option<Vec<String>>> {
		let (presence, revision) = session.participant_snapshot();
		let Some(config) = presence else {
			return Ok(None);
		};
		let member = GuildId::new(config.guild_id.parse()?)
			.member(&self.http, UserId::new(user_id.parse()?))
			.await
			.context("Recheck Discord participant permissions")?;
		let state = session.state.lock();
		if !revision.matches(&state)
			|| !state
				.voice_state
				.as_ref()
				.is_some_and(|v| v.participants.contains_key(user_id))
		{
			return Ok(None);
		}
		Ok(Some(member.roles.iter().map(ToString::to_string).collect()))
	}
	pub async fn revalidate_listener(&self, session: &Arc<Session>, user_id: &str) -> Result<bool> {
		let (presence, revision) = session.participant_snapshot();
		let Some(config) = presence else {
			return Ok(false);
		};
		let guild = GuildId::new(config.guild_id.parse()?);
		let user = UserId::new(user_id.parse()?);
		let member = guild
			.member(&self.http, user)
			.await
			.context("recheck Discord listener permissions")?;
		let exists = session
			.state
			.lock()
			.voice_state
			.as_ref()
			.is_some_and(|v| v.participants.contains_key(user_id));
		if !exists {
			return Ok(false);
		}
		let allowed = is_member_allowed(&config, &member);
		if session
			.upsert_voice_participant_if_current(user_id, member.display_name(), allowed, revision)?
			.is_none()
		{
			return Ok(false);
		}
		if !allowed {
			session.reset_listen(user_id, true);
		}
		Ok(allowed)
	}

	pub async fn has_audience(&self, guild_id: &str, channel_id: &str) -> Result<bool> {
		let guild_id = GuildId::new(guild_id.parse::<u64>().context("parse guild id")?);
		let channel_id = ChannelId::new(channel_id.parse::<u64>().context("parse channel id")?);
		let candidate_user_ids = {
			let Some(guild) = self.cache.guild(guild_id) else {
				warn!(
					  guild_id = %guild_id,
					  channel_id = %channel_id,
					  "discord voice audience cache unavailable; assuming audience present"
				);
				return Ok(true);
			};
			if guild.voice_states.is_empty() {
				warn!(
					  guild_id = %guild_id,
					  channel_id = %channel_id,
					  "discord voice state cache empty; assuming audience present"
				);
				return Ok(true);
			}
			guild
				.voice_states
				.iter()
				.filter_map(|(user_id, voice_state)| {
					if *user_id == self.bot_user_id || voice_state.channel_id != Some(channel_id) {
						return None;
					}
					Some(*user_id)
				})
				.collect::<Vec<_>>()
		};

		for user_id in candidate_user_ids {
			match guild_id.member(&self.http, user_id).await {
				Ok(member) if !member.user.bot => return Ok(true),
				Ok(_) => {}
				Err(error) => {
					warn!(
						  guild_id = %guild_id,
						  channel_id = %channel_id,
						  user_id = %user_id,
						  ?error,
						  "failed to fetch voice audience member; assuming audience present"
					);
					return Ok(true);
				}
			}
		}
		Ok(false)
	}

	pub async fn sync_session_participants(&self, session: &Arc<Session>) -> Result<()> {
		sync_session_participants_with_ctx(
			session,
			self.cache.clone(),
			self.http.clone(),
			self.bot_user_id,
		)
		.await
	}

	pub fn schedule_initial_participant_syncs(self: &Arc<Self>, session: &Arc<Session>) {
		let runtime = Arc::clone(self);
		let session = Arc::clone(session);
		tokio::spawn(async move {
			let mut delay_ms = 400_u64;
			for attempt in 1..=30 {
				sleep(Duration::from_millis(delay_ms)).await;
				if session.get_voice_binding().is_none() {
					return;
				}
				match runtime.sync_session_participants(&session).await {
					Ok(()) => {
						if attempt > 1 {
							info!(attempt, "deferred participant sync succeeded");
						}
						return;
					}
					Err(error) => {
						if attempt == 1 {
							session.send_session_error(format!("participant sync deferred: {error:#}"));
						}
						warn!(
							?error,
							attempt, delay_ms, "deferred participant sync failed"
						);
					}
				}
				delay_ms = match attempt {
					1 => 1_200,
					2 => 2_500,
					_ => 5_000,
				};
			}
			error!("deferred participant sync exhausted retries");
		});
	}

	pub fn schedule_startup_receive_health_check(self: &Arc<Self>, session: &Arc<Session>) {
		let runtime = Arc::clone(self);
		let session = Arc::clone(session);
		let (_, presence_revision) = session.presence_snapshot();
		tokio::spawn(async move {
			sleep(Duration::from_millis(8_000)).await;

			if !session.presence_is_current(presence_revision) || session.voice_receive_started() {
				return;
			}
			if session.voice_participant_count() == 0 {
				return;
			}

			let Some(config) = ({
				let state = session.state.lock();
				state.presence.clone()
			}) else {
				return;
			};

			let bot_voice_state = config
				.guild_id
				.parse::<u64>()
				.ok()
				.and_then(|guild_id| runtime.cache.guild(GuildId::new(guild_id)))
				.and_then(|guild| guild.voice_states.get(&runtime.bot_user_id).cloned())
				.map(|voice_state| {
					format!(
						"channel={:?},self_deaf={},self_mute={}",
						voice_state.channel_id.map(|id| id.get()),
						voice_state.self_deaf,
						voice_state.self_mute
					)
				})
				.unwrap_or_else(|| "missing".to_string());

			warn!(
					guild_id = %config.guild_id,
					channel_id = %config.channel_id,
					participant_count = session.voice_participant_count(),
					bot_voice_state = %bot_voice_state,
					voice_debug = %session.voice_debug_snapshot(),
					"voice receive loop did not start after joining Discord"
			);
		});
	}

	fn schedule_bot_voice_state_check(self: &Arc<Self>, guild_id: GuildId, channel_id: ChannelId) {
		let runtime = Arc::clone(self);
		tokio::spawn(async move {
			sleep(Duration::from_millis(2_000)).await;
			let Some(guild) = runtime.cache.guild(guild_id) else {
				warn!(
						guild_id = %guild_id,
						channel_id = %channel_id,
						"bot voice state missing from cache after join"
				);
				return;
			};
			let Some(voice_state) = guild.voice_states.get(&runtime.bot_user_id).cloned() else {
				warn!(
						guild_id = %guild_id,
						channel_id = %channel_id,
						"bot voice state unavailable after join"
				);
				return;
			};
			let joined_channel_matches = voice_state.channel_id == Some(channel_id);
			if !joined_channel_matches || voice_state.self_deaf || voice_state.self_mute {
				warn!(
						guild_id = %guild_id,
						expected_channel_id = %channel_id,
						actual_channel_id = ?voice_state.channel_id.map(|id| id.get()),
						self_deaf = voice_state.self_deaf,
						self_mute = voice_state.self_mute,
						"bot voice state not receive-ready after join"
				);
			}
		});
	}
}

impl Drop for DiscordRuntime {
	fn drop(&mut self) {
		if self.client_task.get_mut().take().is_some() {
			// Startup may be canceled before the first Ready event. Aborting the
			// owning client plus shutting down its manager covers both cases.
			let manager = self.shard_manager.clone();
			if let Ok(runtime) = tokio::runtime::Handle::try_current() {
				runtime.spawn(async move {
					manager.shutdown_all().await;
				});
			}
		}
	}
}

fn is_member_allowed(presence: &crate::protocol::SongbirdPresenceConfig, member: &Member) -> bool {
	is_listener_allowed(presence, &member.user.id.to_string(), &member.roles)
}
fn is_listener_allowed(
	presence: &crate::protocol::SongbirdPresenceConfig,
	user_id: &str,
	roles: &[RoleId],
) -> bool {
	if presence
		.listen_blocked_user_ids
		.iter()
		.any(|id| id == user_id)
	{
		return false;
	}
	if presence.owner_discord_id.as_deref() == Some(user_id)
		|| presence
			.listen_allowed_user_ids
			.iter()
			.any(|id| id == user_id)
		|| presence.listen_everyone
	{
		return true;
	}
	roles.iter().any(|role| {
		presence
			.listen_role_ids
			.iter()
			.any(|id| id == &role.to_string())
	})
}

fn has_required_voice_permissions(
	permissions: serenity::model::Permissions,
	require_speak: bool,
) -> bool {
	permissions.view_channel() && permissions.connect() && (!require_speak || permissions.speak())
}

fn bot_voice_disconnected(
	bot_user_id: UserId,
	voice_user_id: UserId,
	channel_id: Option<ChannelId>,
) -> bool {
	voice_user_id == bot_user_id && channel_id.is_none()
}

struct DiscordClientHandler {
	chat: mpsc::Sender<crate::DiscordMessage>,
	sessions: Arc<DashMap<String, Arc<Session>>>,
	ready: Arc<AtomicBool>,
	connected: Arc<AtomicBool>,
	ready_notify: Arc<Notify>,
	bot_user_id: UserId,
}

#[async_trait]
impl EventHandler for DiscordClientHandler {
	async fn message(&self, _ctx: SerenityContext, message: Message) {
		if message.author.bot || message.webhook_id.is_some() {
			return;
		}
		let role_ids = message
			.member
			.as_ref()
			.map(|m| m.roles.iter().map(ToString::to_string).collect())
			.unwrap_or_default();
		let moderator = message
			.member
			.as_ref()
			.and_then(|m| m.permissions)
			.is_some_and(|p| p.administrator() || p.manage_messages());
		let event = crate::DiscordMessage {
			id: message.id.get().to_string(),
			guild_id: message
				.guild_id
				.map(|id| id.get().to_string())
				.unwrap_or_default(),
			channel_id: message.channel_id.get().to_string(),
			user_id: message.author.id.get().to_string(),
			display_name: message
				.member
				.as_ref()
				.and_then(|m| m.nick.clone())
				.or(message.author.global_name)
				.unwrap_or(message.author.name),
			text: message.content,
			role_ids,
			moderator,
		};
		if self.chat.try_send(event).is_err() {
			tracing::warn!("Discord chat queue full; message dropped");
		}
	}
	async fn ready(&self, _ctx: SerenityContext, _ready: Ready) {
		self.connected.store(true, Ordering::Relaxed);
		if !self.ready.swap(true, Ordering::Relaxed) {
			info!("songbird discord client ready");
			self.ready_notify.notify_waiters();
		}
	}

	async fn shard_stage_update(&self, _ctx: SerenityContext, event: ShardStageUpdateEvent) {
		let connected = event.new == ConnectionStage::Connected;
		self.connected.store(connected, Ordering::Relaxed);
		if connected {
			self.ready.store(true, Ordering::Relaxed);
			self.ready_notify.notify_waiters();
			info!(shard_id = ?event.shard_id, old = ?event.old, new = ?event.new, "songbird discord shard connected");
		} else {
			warn!(shard_id = ?event.shard_id, old = ?event.old, new = ?event.new, "songbird discord shard not connected");
		}
	}

	async fn guild_member_update(
		&self,
		_ctx: SerenityContext,
		_old: Option<Member>,
		_new: Option<Member>,
		event: serenity::all::GuildMemberUpdateEvent,
	) {
		// The gateway payload has authoritative current roles even when the
		// member cache is empty; do not retain old permissions on an HTTP error.
		let sessions: Vec<_> = self.sessions.iter().map(|v| v.value().clone()).collect();
		let user_id = event.user.id.to_string();
		let name = event
			.nick
			.as_deref()
			.or(event.user.global_name.as_deref())
			.unwrap_or(&event.user.name);
		for session in sessions {
			let (presence, revision) = session.invalidate_participant_snapshot();
			let Some(presence) = presence else {
				continue;
			};
			if presence.guild_id != event.guild_id.to_string() {
				continue;
			}
			let present = session
				.state
				.lock()
				.voice_state
				.as_ref()
				.is_some_and(|v| v.participants.contains_key(&user_id));
			if !present {
				continue;
			}
			let allowed = is_listener_allowed(&presence, &user_id, &event.roles);
			if let Err(error) =
				session.upsert_voice_participant_if_current(&user_id, name, allowed, revision)
			{
				session.send_session_error(error.to_string())
			}
			if !allowed {
				session.reset_listen(&user_id, true);
			}
		}
	}

	async fn voice_state_update(
		&self,
		ctx: SerenityContext,
		old: Option<VoiceState>,
		new: VoiceState,
	) {
		let guild_id = match new
			.guild_id
			.or_else(|| old.as_ref().and_then(|state| state.guild_id))
		{
			Some(guild_id) => guild_id.get().to_string(),
			None => return,
		};
		for entry in self.sessions.iter() {
			let session = entry.value().clone();
			if session.get_presence_config().is_some_and(|presence| {
				Some(presence.guild_id) == new.guild_id.map(|id| id.to_string())
			}) {
				session.invalidate_participant_snapshot();
			}
			let (
				should_sync,
				target_channel_id,
				changed_relevant_member,
				bot_channel_changed,
				bot_disconnected,
			) = {
				let state = session.state.lock();
				match state.presence.as_ref() {
					None => (false, None, false, None, false),
					Some(presence) if !presence.guild_id.eq(&guild_id) => {
						(false, None, false, None, false)
					}
					Some(presence) => {
						let mut target_channel_id = Some(presence.channel_id.clone());
						let old_matches = old
							.as_ref()
							.and_then(|voice_state| voice_state.channel_id)
							.map(|channel_id| channel_id.get().to_string() == presence.channel_id)
							.unwrap_or(false);
						let mut new_matches = new
							.channel_id
							.map(|channel_id| channel_id.get().to_string() == presence.channel_id)
							.unwrap_or(false);
						let bot_disconnected =
							bot_voice_disconnected(self.bot_user_id, new.user_id, new.channel_id);
						let bot_channel_changed = if new.user_id == self.bot_user_id {
							new.channel_id
								.map(|channel_id| channel_id.get().to_string())
								.filter(|channel_id| channel_id != &presence.channel_id)
						} else {
							None
						};
						if let Some(channel_id) = bot_channel_changed.as_ref() {
							target_channel_id = Some(channel_id.clone());
							new_matches = true;
						}
						let changed_relevant_member = new.user_id != self.bot_user_id
							&& old.as_ref().and_then(|voice_state| voice_state.channel_id)
								!= new.channel_id
							&& (old_matches || new_matches);
						(
							old_matches
								|| new_matches || bot_channel_changed.is_some()
								|| bot_disconnected,
							target_channel_id,
							changed_relevant_member,
							bot_channel_changed,
							bot_disconnected,
						)
					}
				}
			};
			if bot_disconnected {
				info!(
						guild_id = %guild_id,
						old_channel_id = ?old.as_ref().and_then(|voice_state| voice_state.channel_id).map(|id| id.get()),
						"clearing session presence after bot disconnected from voice"
				);
				session.clear_presence_state(Some("bot voice disconnected"));
				continue;
			}
			if let Some(actual_channel_id) = bot_channel_changed.as_deref() {
				if session.update_presence_channel(&guild_id, actual_channel_id) {
					session.clear_ssrc_mappings();
					info!(
							guild_id = %guild_id,
							channel_id = %actual_channel_id,
							"updated session presence to actual bot voice channel"
					);
					session.send_event(crate::protocol::SongbirdEvent::SourceUp {
						guild_id: guild_id.clone(),
						channel_id: actual_channel_id.to_string(),
					});
				}
			}
			if !should_sync {
				continue;
			}
			let cache = ctx.cache.clone();
			let http = ctx.http.clone();
			let bot_user_id = self.bot_user_id;
			let sync_session = session.clone();
			tokio::spawn(async move {
				if let Err(error) =
					sync_session_participants_with_ctx(&sync_session, cache, http, bot_user_id).await
				{
					sync_session.send_session_error(format!("participant resync failed: {error:#}"));
				}
			});
			if changed_relevant_member {
				if let Some(target_channel_id) = target_channel_id.as_deref() {
					let removed_ssrc_mappings =
						session.clear_ssrc_mappings_for_user(&new.user_id.to_string());
					info!(
							guild_id = %guild_id,
							channel_id = %target_channel_id,
							user_id = %new.user_id,
							removed_ssrc_mappings,
							"cleared user SSRC mappings after relevant voice state change"
					);
				}
			}
			if new.user_id == self.bot_user_id {
				info!(
						guild_id = %guild_id,
						old_channel_id = ?old.as_ref().and_then(|voice_state| voice_state.channel_id).map(|id| id.get()),
						new_channel_id = ?new.channel_id.map(|id| id.get()),
						self_deaf = new.self_deaf,
						self_mute = new.self_mute,
						"bot voice state updated"
				);
			}
		}
	}
}

struct SpeakingStateHandler {
	session: Arc<Session>,
	http: Arc<Http>,
	cache: Arc<Cache>,
	bot_user_id: UserId,
}

#[async_trait]
impl VoiceEventHandler for SpeakingStateHandler {
	async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
		if let EventContext::SpeakingStateUpdate(speaking) = ctx {
			if let Some(user_id) = speaking.user_id {
				let user_id_string = user_id.0.to_string();
				let serenity_user_id = UserId::new(user_id.0);
				self.session.map_ssrc(speaking.ssrc, &user_id_string);
				if !self.session.has_voice_participant(&user_id_string) {
					let session = Arc::clone(&self.session);
					let http = self.http.clone();
					let cache = self.cache.clone();
					let bot_user_id = self.bot_user_id;
					tokio::spawn(async move {
						if let Err(error) = ensure_speaking_participant(
							session,
							cache,
							http,
							bot_user_id,
							serenity_user_id,
						)
						.await
						{
							warn!(
									?error,
									user_id = %user_id_string,
									"failed to ensure speaking participant"
							);
						}
					});
				}
			}
		}
		None
	}
}

struct VoiceTickHandler {
	session: Arc<Session>,
}

struct OutputVoiceTickHandler {
	session: Arc<Session>,
}

async fn sync_session_participants_with_ctx(
	session: &Arc<Session>,
	cache: Arc<Cache>,
	http: Arc<Http>,
	bot_user_id: UserId,
) -> Result<()> {
	let (presence, revision) = session.participant_snapshot();
	let Some(presence) = presence else {
		return Ok(());
	};
	let guild_id = GuildId::new(presence.guild_id.parse::<u64>().context("parse guild id")?);
	let channel_id = ChannelId::new(
		presence
			.channel_id
			.parse::<u64>()
			.context("parse channel id")?,
	);
	let channel = channel_id
		.to_channel(&http)
		.await
		.with_context(|| format!("fetch channel {}", presence.channel_id))?;
	let Channel::Guild(guild_channel) = channel else {
		return Ok(());
	};
	let voice_state_cache_empty = cache
		.guild(guild_id)
		.map(|guild| guild.voice_states.is_empty())
		.unwrap_or(true);
	if voice_state_cache_empty {
		return Err(anyhow!(
			"discord voice state cache is empty for guild {}; participant sync will retry",
			presence.guild_id
		));
	}

	let mut desired = Vec::new();

	let mut candidate_user_ids = HashSet::new();
	if let Some(guild) = cache.guild(guild_id) {
		for (user_id, voice_state) in guild.voice_states.iter() {
			if voice_state.channel_id == Some(channel_id) && *user_id != bot_user_id {
				candidate_user_ids.insert(*user_id);
			}
		}
	}

	for member in guild_channel.members(&cache).unwrap_or_default() {
		if member.user.id != bot_user_id {
			candidate_user_ids.insert(member.user.id);
		}
	}

	for user_id in candidate_user_ids {
		let Ok(member) = guild_id.member(&http, user_id).await else {
			continue;
		};
		let in_channel = cache
			.guild(guild_id)
			.and_then(|guild| guild.voice_states.get(&user_id).cloned())
			.and_then(|voice_state| voice_state.channel_id)
			.map(|id| id == channel_id)
			.unwrap_or(false);
		if !in_channel || member.user.bot {
			continue;
		}
		let voice_mentions_allowed = is_member_allowed(&presence, &member);
		let user_id = member.user.id.get().to_string();
		desired.push((
			user_id,
			member.display_name().to_string(),
			voice_mentions_allowed,
		));
	}

	session.sync_participants_if_current(desired, revision)
}

async fn ensure_speaking_participant(
	session: Arc<Session>,
	cache: Arc<Cache>,
	http: Arc<Http>,
	bot_user_id: UserId,
	user_id: UserId,
) -> Result<()> {
	if user_id == bot_user_id {
		return Ok(());
	}
	let (presence, revision) = session.participant_snapshot();
	let Some(presence) = presence else {
		return Ok(());
	};
	let guild_id = GuildId::new(presence.guild_id.parse::<u64>().context("parse guild id")?);
	let channel_id = ChannelId::new(
		presence
			.channel_id
			.parse::<u64>()
			.context("parse channel id")?,
	);
	let cached_voice_channel = cache
		.guild(guild_id)
		.and_then(|guild| guild.voice_states.get(&user_id).cloned())
		.and_then(|voice_state| voice_state.channel_id);
	if cached_voice_channel
		.map(|cached_channel_id| cached_channel_id != channel_id)
		.unwrap_or(false)
	{
		return Ok(());
	}

	let member = guild_id
		.member(&http, user_id)
		.await
		.with_context(|| format!("fetch speaking member {user_id}"))?;
	if member.user.bot {
		return Ok(());
	}
	let voice_mentions_allowed = is_member_allowed(&presence, &member);

	let inserted = session.upsert_voice_participant_if_current(
		&user_id.get().to_string(),
		&member.display_name().to_string(),
		voice_mentions_allowed,
		revision,
	)?;
	if inserted == Some(true) {
		info!(
				guild_id = %presence.guild_id,
				channel_id = %presence.channel_id,
				user_id = %user_id,
				"registered speaking participant from speaking state update"
		);
	}
	Ok(())
}

#[async_trait]
impl VoiceEventHandler for VoiceTickHandler {
	async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
		let EventContext::VoiceTick(tick) = ctx else {
			return None;
		};

		let mut active_user_ids = HashSet::new();
		let mut frames = Vec::new();
		let mut unmapped_ssrcs = Vec::new();
		{
			let mut state = self.session.state.lock();
			let Some(voice_state) = state.voice_state.as_mut() else {
				return None;
			};
			voice_state.voice_tick_count += 1;
			for (ssrc, voice_data) in &tick.speaking {
				let user_id = if let Some(user_id) = voice_state.ssrc_to_user.get(ssrc).cloned() {
					user_id
				} else if voice_state.participants.len() == 1 {
					let fallback_user_id = voice_state
						.participants
						.keys()
						.next()
						.cloned()
						.expect("single participant");
					voice_state
						.ssrc_to_user
						.insert(*ssrc, fallback_user_id.clone());
					info!(
							ssrc = *ssrc,
							user_id = %fallback_user_id,
							"mapped unmapped voice SSRC via single-participant fallback"
					);
					fallback_user_id
				} else if let Some(fallback_user_id) = {
					let mapped_user_ids: HashSet<_> =
						voice_state.ssrc_to_user.values().cloned().collect();
					let unmapped_participants = voice_state
						.participants
						.keys()
						.filter(|user_id| !mapped_user_ids.contains(*user_id))
						.cloned()
						.collect::<Vec<_>>();
					if unmapped_participants.len() == 1 {
						unmapped_participants.into_iter().next()
					} else {
						None
					}
				} {
					voice_state
						.ssrc_to_user
						.insert(*ssrc, fallback_user_id.clone());
					info!(
							ssrc = *ssrc,
							user_id = %fallback_user_id,
							participant_count = voice_state.participants.len(),
							mapped_ssrc_count = voice_state.ssrc_to_user.len(),
							"mapped unmapped voice SSRC via single-unmapped-participant fallback"
					);
					fallback_user_id
				} else {
					voice_state.dropped_unmapped_ssrc_count += 1;
					if voice_state.warned_unmapped_ssrcs.insert(*ssrc) {
						unmapped_ssrcs.push(*ssrc);
					}
					continue;
				};
				if !voice_state.participants.contains_key(&user_id) {
					continue;
				}
				active_user_ids.insert(user_id.clone());
				if let Some(decoded_voice) = voice_data.decoded_voice.clone() {
					frames.push((user_id, decoded_voice, voice_data.packet.is_none()));
				}
			}
			if !unmapped_ssrcs.is_empty() {
				warn!(
					?unmapped_ssrcs,
					participant_count = voice_state.participants.len(),
					mapped_ssrc_count = voice_state.ssrc_to_user.len(),
					speaking_state_update_count = voice_state.speaking_state_update_count,
					voice_tick_count = voice_state.voice_tick_count,
					"voice tick had unmapped SSRCs with multiple participants"
				);
			}
		}

		self.session.handle_speaking_snapshot(active_user_ids);
		self.session.handle_replay_audio_tick(
			frames
				.iter()
				.map(|(_, decoded_voice, packet_lost)| (decoded_voice.as_slice(), *packet_lost)),
		);
		for (user_id, decoded_voice, _) in frames {
			if let Err(error) = self.session.handle_user_audio(&user_id, decoded_voice) {
				self
					.session
					.send_session_error(format!("voice tick processing failed: {error:#}"));
			}
		}
		None
	}
}

#[async_trait]
impl VoiceEventHandler for OutputVoiceTickHandler {
	async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
		let EventContext::OutputVoiceTick(tick) = ctx else {
			return None;
		};
		self
			.session
			.handle_replay_output_tick(&tick.samples, tick.sample_rate, tick.channels);
		None
	}
}

#[cfg(test)]
mod tests {
	#[tokio::test]
	async fn canceled_gateway_owner_does_not_leave_a_detached_task() {
		struct Guard(std::sync::Arc<std::sync::atomic::AtomicBool>);
		impl Drop for Guard {
			fn drop(&mut self) {
				self.0.store(true, std::sync::atomic::Ordering::SeqCst)
			}
		}
		let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
		let copy = dropped.clone();
		let (started_tx, started_rx) = tokio::sync::oneshot::channel();
		let task = super::OwnedClientTask(tokio::spawn(async move {
			let _guard = Guard(copy);
			let _ = started_tx.send(());
			std::future::pending::<()>().await;
		}));
		started_rx.await.unwrap();
		drop(task);
		tokio::time::timeout(std::time::Duration::from_secs(1), async {
			while !dropped.load(std::sync::atomic::Ordering::SeqCst) {
				tokio::task::yield_now().await
			}
		})
		.await
		.unwrap();
	}

	#[test]
	fn fresh_member_roles_revoke_listening_and_block_overrides_owner() {
		let mut policy = crate::PresenceConfig::owner_only("1".into(), "2".into(), "3".into());
		policy.listen_role_ids = vec!["8".into()];
		assert!(super::is_listener_allowed(
			&policy,
			"4",
			&[serenity::all::RoleId::new(8)]
		));
		assert!(!super::is_listener_allowed(&policy, "4", &[]));
		assert!(super::is_listener_allowed(&policy, "3", &[]));
		policy.listen_blocked_user_ids.push("3".into());
		assert!(!super::is_listener_allowed(&policy, "3", &[]));
	}

	#[test]
	fn listening_only_does_not_require_speak_but_voice_output_does() {
		let listen =
			serenity::model::Permissions::VIEW_CHANNEL | serenity::model::Permissions::CONNECT;
		assert!(super::has_required_voice_permissions(listen, false));
		assert!(!super::has_required_voice_permissions(listen, true));
		assert!(super::has_required_voice_permissions(
			listen | serenity::model::Permissions::SPEAK,
			true
		));
		assert!(!super::has_required_voice_permissions(
			serenity::model::Permissions::CONNECT,
			false
		));
	}
	use super::*;

	#[test]
	fn bot_voice_disconnect_is_detected_only_for_bot_channel_none() {
		let bot_user_id = UserId::new(10);

		assert!(bot_voice_disconnected(bot_user_id, bot_user_id, None));
		assert!(!bot_voice_disconnected(
			bot_user_id,
			bot_user_id,
			Some(ChannelId::new(20))
		));
		assert!(!bot_voice_disconnected(bot_user_id, UserId::new(11), None));
	}
}
