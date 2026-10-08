//! Bumblebee's native Discord gateway and voice clock. There is no service process.
mod audio;
pub mod command_error;
mod discord;
mod keyword;
mod logging;
mod native;
pub mod protocol;
mod replay_buffer;
pub mod resources;
mod session;
mod signals;
pub mod speech;

use anyhow::{Result, bail};
use dashmap::DashMap;
pub use discord::DiscordHealth;
pub use protocol::{
	SongbirdEvent as VoiceEvent, SongbirdPlaybackClass as PlaybackClass,
	SongbirdPlaybackInputType as PlaybackInputType, SongbirdPlaybackMode as PlaybackMode,
	SongbirdPresenceConfig as PresenceConfig, SongbirdSignalKey as SignalKey,
	VoiceKeywordSensitivity, VoiceWakeWord,
};
pub use replay_buffer::ReplaySnapshot;
pub use resources::NativeResources;
pub use session::AudioEvent;
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct DiscordMessage {
	pub id: String,
	/// Empty for a direct message. The application must route private replies separately.
	pub guild_id: String,
	pub channel_id: String,
	pub user_id: String,
	pub display_name: String,
	pub text: String,
	pub role_ids: Vec<String>,
	pub moderator: bool,
}

#[derive(Clone, Debug)]
pub struct VoiceParticipant {
	pub user_id: String,
	pub username: String,
	pub role_ids: Vec<String>,
	pub is_owner: bool,
	pub voice_mentions_allowed: bool,
}
pub struct AudioRuntime {
	session: Arc<session::Session>,
	discord: Arc<discord::DiscordRuntime>,
	presence_change: Mutex<()>,
	stopped: AtomicBool,
}

impl AudioRuntime {
	/// The only Discord gateway owner. Create once per configured bot account;
	/// the same connection carries chat, member permissions and voice state.
	pub async fn connect(
		token: &str,
		resources: NativeResources,
	) -> Result<(
		Arc<Self>,
		mpsc::Receiver<AudioEvent>,
		mpsc::Receiver<DiscordMessage>,
	)> {
		anyhow::ensure!(!token.trim().is_empty(), "Discord bot token is missing");
		resources.initialize()?;
		let (events_tx, events_rx) = mpsc::channel(session::OUTBOUND_QUEUE_CAPACITY);
		let (chat_tx, chat_rx) = mpsc::channel(256);
		let session = session::Session::new("desktop".into(), events_tx);
		let sessions = Arc::new(DashMap::new());
		sessions.insert("desktop".into(), session.clone());
		let discord = discord::DiscordRuntime::new(token, sessions, chat_tx).await?;
		if let Err(error) = discord.wait_ready().await {
			discord.shutdown().await;
			return Err(error);
		}
		Ok((
			Arc::new(Self {
				session,
				discord,
				presence_change: Mutex::new(()),
				stopped: AtomicBool::new(false),
			}),
			events_rx,
			chat_rx,
		))
	}

	/// Identity verified by the shared Discord gateway during connection.
	pub fn bot_user_id(&self) -> String {
		self.discord.bot_user_id.to_string()
	}

	pub fn health(&self) -> DiscordHealth {
		self.discord.health()
	}

	pub fn has_voice_presence(&self) -> bool {
		self.session.get_voice_binding().is_some()
	}

	pub fn participants(&self) -> Vec<VoiceParticipant> {
		let state = self.session.state.lock();
		let Some(presence) = state.presence.as_ref() else {
			return Vec::new();
		};
		let guild = presence.guild_id.parse::<u64>().ok().and_then(|id| {
			self
				.discord
				.cache
				.guild(serenity::model::id::GuildId::new(id))
		});
		state
			.voice_state
			.as_ref()
			.into_iter()
			.flat_map(|voice| voice.participants.iter())
			.map(|(id, p)| VoiceParticipant {
				user_id: id.clone(),
				username: p.username.clone(),
				role_ids: id
					.parse::<u64>()
					.ok()
					.and_then(|id| {
						guild
							.as_ref()
							.and_then(|g| g.members.get(&serenity::model::id::UserId::new(id)))
					})
					.map(|m| m.roles.iter().map(ToString::to_string).collect())
					.unwrap_or_default(),
				is_owner: presence.owner_discord_id.as_deref() == Some(id.as_str()),
				voice_mentions_allowed: p.voice_mentions_allowed,
			})
			.collect()
	}
	pub async fn revalidate_participant(&self, user_id: &str) -> Result<Option<Vec<String>>> {
		self.ensure_running()?;
		self
			.discord
			.revalidate_participant(&self.session, user_id)
			.await
	}
	pub fn is_listen_allowed(&self, user_id: &str) -> bool {
		self
			.session
			.state
			.lock()
			.voice_state
			.as_ref()
			.and_then(|v| v.participants.get(user_id))
			.is_some_and(|p| p.voice_mentions_allowed)
	}
	pub async fn revalidate_listener(&self, user_id: &str) -> Result<bool> {
		self.ensure_running()?;
		self
			.discord
			.revalidate_listener(&self.session, user_id)
			.await
	}

	pub async fn send_chat(&self, channel_id: &str, text: &str) -> Result<String> {
		self.ensure_running()?;
		self.discord.send_chat(channel_id, text).await
	}

	pub async fn configure_presence(&self, config: Option<PresenceConfig>) -> Result<()> {
		let _change = self.presence_change.lock().await;
		self.ensure_running()?;
		if let Some(config) = config {
			resources::configured()?.validate()?;
			let (current, revision) = self.session.presence_snapshot();
			let same = current
				.as_ref()
				.is_some_and(|p| p.guild_id == config.guild_id && p.channel_id == config.channel_id)
				&& self.session.get_voice_binding().is_some();
			if same {
				let validation = self.discord.validate_voice_destination(&config).await;
				if !self.session.presence_is_current(revision) {
					return Ok(());
				}
				if let Err(error) = validation {
					if command_error::is_destination_access_error(&error) {
						let binding = self.session.get_voice_binding();
						if self
							.session
							.clear_presence_if_current(Some("destination_access_lost"), revision)
						{
							self.session.interrupt_playback().await;
							if let Some(binding) = binding {
								self.discord.leave_voice_channel(&binding.guild_id).await;
							}
						}
					}
					return Err(error);
				}
				self
					.session
					.set_replay_output_capture(config.replay_buffer_enabled)
					.await?;
				self
					.session
					.update_presence_config_if_current(config, revision);
			} else {
				self.disconnect_voice().await;
				self
					.discord
					.join_voice_channel(&self.session, &config)
					.await?;
			}
			self
				.discord
				.sync_session_participants(&self.session)
				.await?;
			self
				.discord
				.schedule_initial_participant_syncs(&self.session);
			self
				.discord
				.schedule_startup_receive_health_check(&self.session);
		} else {
			self.disconnect_voice().await;
		}
		Ok(())
	}

	/// Called after reconnect or a permission update before continuing sensitive work.
	pub async fn revalidate_presence(&self) -> Result<()> {
		self.ensure_running()?;
		let config = self.session.get_presence_config();
		if config.is_none() {
			bail!("no active voice channel");
		}
		self.configure_presence(config).await
	}

	pub async fn has_audience(&self, guild_id: &str, channel_id: &str) -> Result<bool> {
		self.ensure_running()?;
		self.discord.has_audience(guild_id, channel_id).await
	}

	/// True means natural completion, false means interruption or timeout.
	/// Dropping/cancelling the future also stops its own active track.
	pub async fn play_audio(
		&self,
		bytes: Vec<u8>,
		format: PlaybackInputType,
		gain: f32,
		cancellation: CancellationToken,
	) -> Result<bool> {
		self.ensure_running()?;
		anyhow::ensure!(
			bytes.len() <= 32 * 1024 * 1024,
			"audio exceeds the 32 MiB playback limit"
		);
		tokio::select! {
			  biased;
			  _ = cancellation.cancelled() => Ok(false),
			  result = self.session.play_audio(bytes, format, None, PlaybackClass::Speech, Some(180_000), Some(gain), None) => result,
		}
	}

	pub fn set_speech_gain(&self, gain: f32) -> Result<()> {
		self.ensure_running()?;
		self.session.set_speech_gain(gain)
	}
	pub fn set_cue_gain(&self, gain: f32) -> Result<()> {
		self.ensure_running()?;
		self.session.set_cue_gain(gain)
	}
	pub async fn play_signal_gain(
		&self,
		signal: SignalKey,
		gain: f32,
		cancellation: CancellationToken,
	) -> Result<bool> {
		self
			.play_signal_gain_with_options(signal, gain, 1, None, cancellation)
			.await
	}
	/// Repeat a cue in one padded playback batch, with a stable thinking variant.
	/// Cancelling or dropping this future stops its own active track.
	pub async fn play_signal_gain_with_options(
		&self,
		signal: SignalKey,
		gain: f32,
		repeat_count: u32,
		variant_key: Option<&str>,
		cancellation: CancellationToken,
	) -> Result<bool> {
		self.ensure_running()?;
		anyhow::ensure!(
			gain.is_finite() && (0.0..=2.0).contains(&gain),
			"Invalid signal gain"
		);
		let bytes = signal_wav_with_options(signal, repeat_count, variant_key)?;
		// Generated cues are PCM WAVs with a 44-byte header. Allow the longest
		// supported repeat batch to finish without shortening the usual deadline.
		let bytes_per_second = u64::from(audio::DISCORD_SAMPLE_RATE)
			* u64::from(audio::DISCORD_CHANNELS)
			* u64::from(audio::DISCORD_BITS_PER_SAMPLE / 8);
		let duration_ms = bytes.len().saturating_sub(44) as u64 * 1000 / bytes_per_second;
		let timeout_ms = 10_000.max(duration_ms + 1000);
		let class = if signal == SignalKey::ThinkingLoop {
			PlaybackClass::Thinking
		} else {
			PlaybackClass::Cue
		};
		tokio::select! {
			biased;
			_ = cancellation.cancelled() => Ok(false),
			result = self.session.play_audio(bytes, PlaybackInputType::Encoded, Some(PlaybackMode::Urgent), class, Some(timeout_ms), Some(gain), None) => result,
		}
	}
	pub async fn play_signal(
		&self,
		signal: SignalKey,
		cancellation: CancellationToken,
	) -> Result<bool> {
		self.ensure_running()?;
		tokio::select! {
			  biased;
			  _ = cancellation.cancelled() => Ok(false),
			  result = self.session.play_signal(signal, Some(1.0), None, None, Some(PlaybackMode::Urgent), Some(PlaybackClass::Cue), Some(10_000), None) => result,
		}
	}
	pub async fn interrupt(&self) {
		self.session.interrupt_playback().await;
	}
	pub fn open_listen(&self, user_id: &str, duration_ms: u64) -> Result<bool> {
		self.ensure_running()?;
		anyhow::ensure!(
			(1..=120_000).contains(&duration_ms),
			"listen duration must be at most 120 seconds"
		);
		self
			.session
			.open_listen(user_id, duration_ms, 0, Some("desktop_turn".into()))
	}
	pub fn reset_listen(&self, user_id: &str) {
		self.session.reset_listen(user_id, true);
	}
	/// Finalize the current native capture once, preserving its pending audio/event order.
	/// Repeated calls do not emit duplicate utterances after the native state is cleared.
	pub fn finalize_utterance(&self, user_id: &str) -> Result<()> {
		self.ensure_running()?;
		self
			.session
			.finalize_utterance_for_user(user_id, "desktop_post_speech_deadline");
		Ok(())
	}
	pub fn stream_participant(&self, user_id: &str, enabled: bool) {
		self.session.set_streaming(user_id, enabled);
	}
	pub fn replay_snapshot(&self) -> Result<ReplaySnapshot> {
		self.session.replay_snapshot()
	}
	pub fn diagnostics(&self) -> serde_json::Value {
		serde_json::json!({ "session": self.session.snapshot_debug_meta(), "resources": self.session.voice_resource_counts(), "native": native::blocking::snapshot() })
	}

	pub async fn shutdown(&self) {
		if self.stopped.swap(true, Ordering::SeqCst) {
			return;
		}
		let _change = self.presence_change.lock().await;
		self.disconnect_voice().await;
		self.discord.shutdown().await;
	}
	async fn disconnect_voice(&self) {
		self.session.interrupt_playback().await;
		let binding = self.session.get_voice_binding();
		self.session.clear_presence_state(Some("disconnected"));
		if let Some(binding) = binding {
			self.discord.leave_voice_channel(&binding.guild_id).await;
		}
	}
	fn ensure_running(&self) -> Result<()> {
		anyhow::ensure!(
			!self.stopped.load(Ordering::SeqCst),
			"audio runtime has shut down"
		);
		Ok(())
	}
}

/// Shared cue bytes for the OBS renderer and native Discord playback.
pub fn signal_wav(signal: SignalKey) -> Result<Vec<u8>> {
	signal_wav_with_options(signal, 1, None)
}

/// Shared cue bytes with one padding pair around the whole repeat batch.
/// Thinking variants are selected deterministically from the caller's turn key.
/// Zero repeats retains the native default of one; counts above twelve fail.
pub fn signal_wav_with_options(
	signal: SignalKey,
	repeat_count: u32,
	variant_key: Option<&str>,
) -> Result<Vec<u8>> {
	signals::get_signal_audio(signal, Some(1.0), Some(repeat_count), variant_key)
}

#[cfg(test)]
mod cue_tests {
	use super::*;

	#[test]
	fn thinking_batch_repeats_the_same_clip_with_only_outer_padding() {
		let single = signal_wav_with_options(SignalKey::ThinkingLoop, 1, Some("turn-42")).unwrap();
		let batch = signal_wav_with_options(SignalKey::ThinkingLoop, 2, Some("turn-42")).unwrap();
		let bytes_per_ms = 48_000 * 2 * 2 / 1000;
		let padding = audio::PLAYBACK_PADDING_MS as usize * bytes_per_ms;
		let clip = &single[44 + padding..single.len() - padding];
		assert_eq!(single.len(), 44 + 1160 * bytes_per_ms);
		assert_eq!(batch.len(), 44 + 2160 * bytes_per_ms);
		assert_eq!(&batch[44 + padding..44 + padding + clip.len()], clip);
		assert_eq!(
			&batch[44 + padding + clip.len()..batch.len() - padding],
			clip
		);
		assert!(batch[44..44 + padding].iter().all(|byte| *byte == 0));
		assert!(batch[batch.len() - padding..].iter().all(|byte| *byte == 0));
		assert_eq!(
			u32::from_le_bytes(batch[40..44].try_into().unwrap()) as usize,
			batch.len() - 44
		);
	}

	#[test]
	fn exported_cue_options_preserve_defaults_and_native_repeat_bound() {
		assert_eq!(
			signal_wav(SignalKey::WakeChirp).unwrap(),
			signal_wav_with_options(SignalKey::WakeChirp, 0, Some("ignored")).unwrap()
		);
		assert!(signal_wav_with_options(SignalKey::ThinkingLoop, 13, None).is_err());
		let longest = signal_wav_with_options(SignalKey::ThinkingLoop, 12, None).unwrap();
		assert_eq!(longest.len(), 44 + 12_160 * (48_000 * 2 * 2 / 1000));
	}
}
