use std::{
	collections::{HashMap, HashSet, VecDeque},
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU64, Ordering},
	},
	time::Duration,
};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use songbird::{
	Call,
	events::{Event, EventContext, EventData, EventHandler as TrackEventHandler, TrackEvent},
	input::Input,
	tracks::{PlayMode, Track, TrackHandle},
};
use tokio::{
	sync::mpsc::{self, error::TrySendError},
	task::JoinHandle,
	time::timeout,
};

use crate::{
	audio::{
		FRAME_MS, INPUT_BITS_PER_SAMPLE, INPUT_SAMPLE_RATE, pcm_i16_to_le_bytes,
		wrap_discord_pcm_as_wav,
	},
	keyword::{KeywordKind, KeywordModelSettings, KeywordPipeline},
	native::vad::VoiceActivityDetector,
	protocol::{
		SongbirdEvent, SongbirdPlaybackClass, SongbirdPlaybackInputType, SongbirdPlaybackMode,
		SongbirdPresenceConfig, SongbirdSignalKey,
	},
	replay_buffer::{ReplayBuffer, ReplaySnapshot},
	signals::get_signal_audio,
};

const TURN_AUDIO_BUFFER_MS: usize = 2_000;
const TURN_AUDIO_BUFFER_BYTES: usize =
	(TURN_AUDIO_BUFFER_MS * INPUT_SAMPLE_RATE as usize * INPUT_BITS_PER_SAMPLE as usize / 8) / 1000;
const MIN_UTTERANCE_VOICED_MS: u64 = 80;
// Natural speech regularly contains pauses around a second between clauses. Give the
// speaker enough room to continue without splitting a request in the middle of a
// sentence; each new voiced frame still cancels and rearms this timer.
const UTTERANCE_FINALIZE_SILENCE_MS: u64 = 1_200;
const CHEAP_VOICE_ACTIVITY_RMS_THRESHOLD: f32 = 0.012;
pub const OUTBOUND_QUEUE_CAPACITY: usize = 1024;

fn keyword_settings_from_presence(
	presence: Option<&SongbirdPresenceConfig>,
) -> KeywordModelSettings {
	let Some(presence) = presence else {
		return KeywordModelSettings::default();
	};
	KeywordModelSettings {
		wake_word: presence.wake_word,
		wake_sensitivity: presence.wake_keyword_sensitivity,
		stop_sensitivity: presence.stop_keyword_sensitivity,
		cancel_sensitivity: presence.cancel_keyword_sensitivity,
	}
}

fn should_allocate_keyword_pipeline(voice_mentions_allowed: bool, is_owner: bool) -> bool {
	voice_mentions_allowed || is_owner
}

fn should_allocate_vad(
	voice_mentions_allowed: bool,
	is_owner: bool,
	capture_relevant: bool,
) -> bool {
	should_allocate_keyword_pipeline(voice_mentions_allowed, is_owner) || capture_relevant
}

// Return retired handles to the caller: SDK destructors must run after releasing
// Session::state, otherwise other Tokio workers can block acquiring that mutex.
fn take_stale_participant_resources(
	participant: &mut ParticipantState,
	keyword_settings: KeywordModelSettings,
	is_owner: bool,
	capture_relevant: bool,
) -> (Option<KeywordPipeline>, Option<VoiceActivityDetector>) {
	let mut keyword = None;
	let mut vad = None;
	let should_have_keyword =
		should_allocate_keyword_pipeline(participant.voice_mentions_allowed, is_owner);
	if !should_have_keyword
		|| participant
			.keyword_pipeline
			.as_ref()
			.map(|pipeline| pipeline.settings() != keyword_settings)
			.unwrap_or(false)
	{
		keyword = participant.keyword_pipeline.take();
	}

	if !should_allocate_vad(
		participant.voice_mentions_allowed,
		is_owner,
		capture_relevant,
	) {
		vad = participant.vad.take();
	}
	(keyword, vad)
}

fn keyword_pipeline_is_current(
	pipeline: &Option<KeywordPipeline>,
	keyword_settings: KeywordModelSettings,
) -> bool {
	pipeline
		.as_ref()
		.map(|pipeline| pipeline.settings() == keyword_settings)
		.unwrap_or(false)
}

fn ensure_keyword_pipeline(
	pipeline: Option<KeywordPipeline>,
	username: &str,
	user_id: &str,
	keyword_settings: KeywordModelSettings,
	should_allocate: bool,
) -> Result<Option<KeywordPipeline>> {
	if !should_allocate {
		return Ok(None);
	}
	if keyword_pipeline_is_current(&pipeline, keyword_settings) {
		return Ok(pipeline);
	}
	KeywordPipeline::new(username, keyword_settings)
		.with_context(|| format!("create keyword pipeline for {username} ({user_id})"))
		.map(Some)
}

fn ensure_vad(
	vad: Option<VoiceActivityDetector>,
	username: &str,
	should_allocate: bool,
) -> Result<Option<VoiceActivityDetector>> {
	if !should_allocate {
		return Ok(None);
	}
	if vad.is_some() {
		return Ok(vad);
	} else {
		VoiceActivityDetector::new()
			.with_context(|| format!("create TEN VAD for {username}"))
			.map(Some)
	}
}

fn cheap_rms_for_pcm_samples(pcm_samples: &[i16]) -> f32 {
	if pcm_samples.is_empty() {
		return 0.0;
	}

	let sum_squares = pcm_samples
		.iter()
		.map(|sample| {
			let sample = *sample as f64;
			sample * sample
		})
		.sum::<f64>();

	((sum_squares / pcm_samples.len() as f64).sqrt() / 32768.0) as f32
}

fn voice_resource_counts_from_state(voice_state: &VoiceSessionState) -> VoiceResourceCounts {
	let keyword_pipeline_count = voice_state
		.participants
		.values()
		.filter(|participant| participant.keyword_pipeline.is_some())
		.count();
	let keyword_recognizer_count = voice_state
		.participants
		.values()
		.filter_map(|participant| participant.keyword_pipeline.as_ref())
		.map(|pipeline| pipeline.recognizer_count())
		.sum();
	let vad_count = voice_state
		.participants
		.values()
		.filter(|participant| participant.vad.is_some())
		.count();

	VoiceResourceCounts {
		active_voice_session: true,
		participant_count: voice_state.participants.len(),
		keyword_pipeline_count,
		keyword_recognizer_count,
		vad_count,
		decoded_frame_count: voice_state.decoded_frame_count,
		voice_tick_count: voice_state.voice_tick_count,
	}
}

/// Events are transferred directly inside the process. PCM never passes through JSON.
#[derive(Debug, Clone)]
pub struct AudioEvent {
	/// Monotonic sequence includes dropped events; gaps invalidate in-flight capture.
	pub sequence: u64,
	pub event: SongbirdEvent,
	pub payload_type: Option<String>,
	pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VoiceBinding {
	pub guild_id: String,
	pub call: Arc<tokio::sync::Mutex<Call>>,
}

#[derive(Default)]
pub struct SessionState {
	presence_revision: u64,
	participant_revision: u64,
	pub presence: Option<SongbirdPresenceConfig>,
	pub streaming_users: HashSet<String>,
	pub pending_audio_by_user: HashMap<String, VecDeque<Vec<u8>>>,
	pub pending_audio_bytes_by_user: HashMap<String, usize>,
	pub pending_utterance_finalized_by_user: HashMap<String, PendingUtteranceFinalized>,
	pub usernames: HashMap<String, String>,
	pub capture_state_by_user: HashMap<String, CaptureState>,
	pub voice_state: Option<VoiceSessionState>,
	pub replay_buffer: ReplayBuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParticipantRevision {
	presence: u64,
	participants: u64,
}
impl ParticipantRevision {
	pub(crate) fn matches(self, state: &SessionState) -> bool {
		self.presence == state.presence_revision && self.participants == state.participant_revision
	}
}

pub struct PendingUtteranceFinalized {
	pub username: String,
	pub reason: String,
	pub duration_ms: u64,
	pub voiced_ms: u64,
}

pub struct Session {
	pub bot_id: String,
	pub state: Mutex<SessionState>,
	sender: mpsc::Sender<AudioEvent>,
	outbound_dropped_message_count: AtomicU64,
	event_sequence: Mutex<u64>,
	interruption_epoch: AtomicU64,
	playback_lock: tokio::sync::Mutex<()>,
	current_playback: Mutex<Option<ActivePlayback>>,
	playback_seq: AtomicU64,
}

pub struct VoiceSessionState {
	pub binding: Option<VoiceBinding>,
	pub ssrc_to_user: HashMap<u32, String>,
	pub current_speakers: HashSet<String>,
	pub participants: HashMap<String, ParticipantState>,
	pub has_seen_audio: bool,
	pub voice_tick_count: u64,
	pub speaking_state_update_count: u64,
	pub decoded_frame_count: u64,
	pub dropped_unmapped_ssrc_count: u64,
	pub warned_unmapped_ssrcs: HashSet<u32>,
	pub diagnostic: bool,
}

pub struct ParticipantState {
	pub username: String,
	pub voice_mentions_allowed: bool,
	pub keyword_pipeline: Option<KeywordPipeline>,
	pub vad: Option<VoiceActivityDetector>,
	pub open_listen_task: Option<JoinHandle<()>>,
	pub last_voice_activity_event_ms: u64,
}

struct VoiceProcessingJob {
	username: String,
	speaker_active: bool,
	capture_relevant: bool,
	is_owner: bool,
	keyword_settings: KeywordModelSettings,
	voice_mentions_allowed: bool,
	last_voice_activity_event_ms: u64,
	keyword_pipeline: Option<KeywordPipeline>,
	vad: Option<VoiceActivityDetector>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct VoiceResourceCounts {
	pub active_voice_session: bool,
	pub participant_count: usize,
	pub keyword_pipeline_count: usize,
	pub keyword_recognizer_count: usize,
	pub vad_count: usize,
	pub decoded_frame_count: u64,
	pub voice_tick_count: u64,
}

pub struct CaptureState {
	pub no_speech_timer: Option<JoinHandle<()>>,
	pub open_listen_armed: bool,
	pub segment_started_at_ms: Option<u64>,
	pub last_voiced_at_ms: Option<u64>,
	pub voiced_ms: u64,
	pub finalize_timer: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct ActivePlayback {
	playback_id: u64,
	playback_class: SongbirdPlaybackClass,
	handle: TrackHandle,
}

struct StopTrackOnDrop(TrackHandle);
impl Drop for StopTrackOnDrop {
	fn drop(&mut self) {
		let _ = self.0.stop();
	}
}

impl Session {
	pub fn new(bot_id: String, sender: mpsc::Sender<AudioEvent>) -> Arc<Self> {
		Arc::new(Self {
			bot_id,
			state: Mutex::new(SessionState::default()),
			sender,
			outbound_dropped_message_count: AtomicU64::new(0),
			event_sequence: Mutex::new(0),
			interruption_epoch: AtomicU64::new(0),
			playback_lock: tokio::sync::Mutex::new(()),
			current_playback: Mutex::new(None),
			playback_seq: AtomicU64::new(0),
		})
	}

	fn send_outbound(&self, mut message: AudioEvent) {
		let mut sequence = self.event_sequence.lock();
		*sequence += 1;
		message.sequence = *sequence;
		// The audio clock may never wait for a UI or agent consumer. Overflow is
		// observable and bounded; the receiver must cancel its current capture
		// on a discontinuity instead of treating incomplete audio as a full turn.
		if let Err(error) = self.sender.try_send(message) {
			let dropped = self
				.outbound_dropped_message_count
				.fetch_add(1, Ordering::SeqCst)
				+ 1;
			if dropped == 1 || dropped.is_power_of_two() {
				tracing::warn!(
					dropped,
					closed = matches!(error, TrySendError::Closed(_)),
					"audio event consumer is unavailable"
				);
			}
		}
	}

	pub fn send_event(&self, event: SongbirdEvent) {
		self.send_outbound(AudioEvent {
			sequence: 0,
			event,
			payload_type: None,
			payload: Vec::new(),
		});
	}

	pub fn send_binary_event(&self, event: SongbirdEvent, payload_type: &str, payload: &[u8]) {
		self.send_outbound(AudioEvent {
			sequence: 0,
			event,
			payload_type: Some(payload_type.into()),
			payload: payload.to_vec(),
		});
	}

	pub fn send_session_error(&self, message: impl Into<String>) {
		self.send_event(SongbirdEvent::SessionError {
			message: message.into(),
		});
	}

	pub fn snapshot_debug_meta(&self) -> Value {
		let state = self.state.lock();
		json!({
				"botId": self.bot_id,
				"presence": state.presence,
				"streamingUsers": state.streaming_users,
				"pendingAudioBytesByUser": state.pending_audio_bytes_by_user,
				"outboundDroppedMessageCount": self.outbound_dropped_message_count.load(Ordering::SeqCst),
		})
	}

	pub fn set_presence(&self, config: SongbirdPresenceConfig, binding: VoiceBinding) {
		let _retired_voice;
		let mut state = self.state.lock();
		state
			.replay_buffer
			.configure(config.replay_buffer_enabled, config.replay_buffer_seconds);
		state.presence_revision = state.presence_revision.wrapping_add(1);
		state.presence = Some(config);
		_retired_voice = state.voice_state.replace(VoiceSessionState {
			binding: Some(binding),
			ssrc_to_user: HashMap::new(),
			current_speakers: HashSet::new(),
			participants: HashMap::new(),
			has_seen_audio: false,
			voice_tick_count: 0,
			speaking_state_update_count: 0,
			decoded_frame_count: 0,
			dropped_unmapped_ssrc_count: 0,
			warned_unmapped_ssrcs: HashSet::new(),
			diagnostic: false,
		});
	}

	#[cfg(test)]
	pub fn set_diagnostic_presence(
		self: &Arc<Self>,
		config: SongbirdPresenceConfig,
		user_id: &str,
		username: &str,
	) -> Result<()> {
		{
			let _retired_voice;
			let mut state = self.state.lock();
			state
				.replay_buffer
				.configure(config.replay_buffer_enabled, config.replay_buffer_seconds);
			state.presence_revision = state.presence_revision.wrapping_add(1);
			state.presence = Some(config);
			_retired_voice = state.voice_state.replace(VoiceSessionState {
				binding: None,
				ssrc_to_user: HashMap::new(),
				current_speakers: HashSet::new(),
				participants: HashMap::new(),
				has_seen_audio: false,
				voice_tick_count: 0,
				speaking_state_update_count: 0,
				decoded_frame_count: 0,
				dropped_unmapped_ssrc_count: 0,
				warned_unmapped_ssrcs: HashSet::new(),
				diagnostic: true,
			});
		}
		self.upsert_voice_participant(user_id, username, true)?;
		self.send_event(SongbirdEvent::SourceUp {
			guild_id: self
				.get_presence_config()
				.map(|presence| presence.guild_id)
				.unwrap_or_default(),
			channel_id: self
				.get_presence_config()
				.map(|presence| presence.channel_id)
				.unwrap_or_default(),
		});
		Ok(())
	}

	pub fn update_presence_config_if_current(
		&self,
		config: SongbirdPresenceConfig,
		revision: u64,
	) -> bool {
		let mut state = self.state.lock();
		if state.presence_revision != revision {
			return false;
		}
		state
			.replay_buffer
			.configure(config.replay_buffer_enabled, config.replay_buffer_seconds);
		state.presence_revision = state.presence_revision.wrapping_add(1);
		state.presence = Some(config);
		true
	}

	pub async fn set_replay_output_capture(&self, enabled: bool) -> Result<()> {
		let binding = self
			.get_voice_binding()
			.ok_or_else(|| anyhow!("no active voice presence"))?;
		let mut call = binding.call.lock().await;
		let voice_config = call.config().clone().emit_output_voice_tick(enabled);
		call.set_config(voice_config);
		Ok(())
	}

	pub fn clear_presence_state(&self, reason: Option<&str>) {
		self.clear_presence_at_revision(reason, None);
	}

	pub fn clear_presence_if_current(&self, reason: Option<&str>, revision: u64) -> bool {
		self.clear_presence_at_revision(reason, Some(revision))
	}

	fn clear_presence_at_revision(&self, reason: Option<&str>, revision: Option<u64>) -> bool {
		let _retired_voice;
		let mut state = self.state.lock();
		if revision.is_some_and(|expected| state.presence_revision != expected) {
			return false;
		}
		state.presence_revision = state.presence_revision.wrapping_add(1);
		for capture in state.capture_state_by_user.values_mut() {
			if let Some(task) = capture.no_speech_timer.take() {
				task.abort();
			}
			if let Some(task) = capture.finalize_timer.take() {
				task.abort();
			}
		}
		if let Some(voice_state) = state.voice_state.as_mut() {
			for participant in voice_state.participants.values_mut() {
				if let Some(task) = participant.open_listen_task.take() {
					task.abort();
				}
			}
		}
		_retired_voice = state.voice_state.take();
		state.presence = None;
		state.streaming_users.clear();
		state.pending_audio_by_user.clear();
		state.pending_audio_bytes_by_user.clear();
		state.pending_utterance_finalized_by_user.clear();
		state.capture_state_by_user.clear();
		state.replay_buffer.clear();
		self.current_playback.lock().take();
		self.send_event(SongbirdEvent::SourceDown {
			reason: reason.map(ToString::to_string),
		});
		true
	}

	pub fn update_presence_channel(&self, guild_id: &str, channel_id: &str) -> bool {
		let mut state = self.state.lock();
		let Some(presence) = state.presence.as_mut() else {
			return false;
		};
		if presence.guild_id != guild_id || presence.channel_id == channel_id {
			return false;
		}
		presence.channel_id = channel_id.to_string();
		state.presence_revision = state.presence_revision.wrapping_add(1);
		true
	}

	pub fn get_voice_binding(&self) -> Option<VoiceBinding> {
		self
			.state
			.lock()
			.voice_state
			.as_ref()
			.and_then(|voice_state| voice_state.binding.clone())
	}

	pub fn presence_snapshot(&self) -> (Option<SongbirdPresenceConfig>, u64) {
		let state = self.state.lock();
		(state.presence.clone(), state.presence_revision)
	}

	pub fn presence_is_current(&self, revision: u64) -> bool {
		self.state.lock().presence_revision == revision
	}
	pub fn participant_snapshot(&self) -> (Option<SongbirdPresenceConfig>, ParticipantRevision) {
		let state = self.state.lock();
		(
			state.presence.clone(),
			ParticipantRevision {
				presence: state.presence_revision,
				participants: state.participant_revision,
			},
		)
	}
	pub fn invalidate_participant_snapshot(
		&self,
	) -> (Option<SongbirdPresenceConfig>, ParticipantRevision) {
		let mut state = self.state.lock();
		state.participant_revision = state.participant_revision.wrapping_add(1);
		(
			state.presence.clone(),
			ParticipantRevision {
				presence: state.presence_revision,
				participants: state.participant_revision,
			},
		)
	}

	pub fn get_presence_config(&self) -> Option<SongbirdPresenceConfig> {
		self.state.lock().presence.clone()
	}

	pub fn set_streaming(self: &Arc<Self>, user_id: &str, enabled: bool) {
		if enabled {
			{
				let mut state = self.state.lock();
				state.streaming_users.insert(user_id.to_string());
				state
					.capture_state_by_user
					.entry(user_id.to_string())
					.or_insert_with(CaptureState::default);
			}
			self.flush_pending_audio(user_id);
			self.flush_pending_utterance_finalized(user_id);
		} else {
			let mut state = self.state.lock();
			state.streaming_users.remove(user_id);
			state.pending_audio_by_user.remove(user_id);
			state.pending_audio_bytes_by_user.remove(user_id);
			state.pending_utterance_finalized_by_user.remove(user_id);
			state.usernames.remove(user_id);
			clear_capture_state_for_user(&mut state, user_id);
		}
	}

	#[cfg(test)]
	pub fn sync_participants(
		self: &Arc<Self>,
		participants: Vec<(String, String, bool)>,
	) -> Result<()> {
		self.sync_participants_at_revision(participants, None)
	}
	pub fn sync_participants_if_current(
		self: &Arc<Self>,
		participants: Vec<(String, String, bool)>,
		revision: ParticipantRevision,
	) -> Result<()> {
		self.sync_participants_at_revision(participants, Some(revision))
	}
	fn sync_participants_at_revision(
		self: &Arc<Self>,
		participants: Vec<(String, String, bool)>,
		revision: Option<ParticipantRevision>,
	) -> Result<()> {
		// Declared before the guard so retired native handles drop after unlock,
		// including early returns and unwinding.
		let mut retired_participants = Vec::new();
		let mut retired_resources = Vec::new();
		let mut state = self.state.lock();
		if revision.is_some_and(|expected| !expected.matches(&state)) {
			return Ok(());
		}
		if state.voice_state.is_none() {
			return Ok(());
		}
		state.participant_revision = state.participant_revision.wrapping_add(1);
		let owner_discord_id = state
			.presence
			.as_ref()
			.and_then(|presence| presence.owner_discord_id.clone());
		let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());

		let desired: HashMap<String, (String, bool)> = participants
			.into_iter()
			.map(|(user_id, username, voice_mentions_allowed)| {
				(user_id, (username, voice_mentions_allowed))
			})
			.collect();
		let existing_ids: Vec<String> = state
			.voice_state
			.as_ref()
			.map(|voice_state| voice_state.participants.keys().cloned().collect())
			.unwrap_or_default();

		for user_id in existing_ids {
			if desired.contains_key(&user_id) {
				continue;
			}
			let removed_participant = {
				let voice_state = state.voice_state.as_mut().expect("voice state");
				voice_state.current_speakers.remove(&user_id);
				voice_state
					.ssrc_to_user
					.retain(|_, mapped_user_id| mapped_user_id != &user_id);
				voice_state.participants.remove(&user_id)
			};
			let Some(mut participant) = removed_participant else {
				continue;
			};
			if let Some(task) = participant.open_listen_task.take() {
				task.abort();
			}
			state.usernames.remove(&user_id);
			state.streaming_users.remove(&user_id);
			state.pending_audio_by_user.remove(&user_id);
			state.pending_audio_bytes_by_user.remove(&user_id);
			state.pending_utterance_finalized_by_user.remove(&user_id);
			clear_capture_state_for_user(&mut state, &user_id);
			retired_participants.push(participant);
			self.send_event(SongbirdEvent::ParticipantLeft { user_id });
		}

		for (user_id, (username, voice_mentions_allowed)) in desired {
			state.usernames.insert(user_id.clone(), username.clone());
			let is_owner = owner_discord_id
				.as_deref()
				.map(|owner_discord_id| owner_discord_id == user_id)
				.unwrap_or(false);
			let capture_relevant = state.streaming_users.contains(&user_id)
				|| state.pending_audio_by_user.contains_key(&user_id)
				|| state.capture_state_by_user.contains_key(&user_id);
			let mut updated_existing = false;
			if let Some(existing) = state
				.voice_state
				.as_mut()
				.expect("voice state")
				.participants
				.get_mut(&user_id)
			{
				existing.username = username.clone();
				existing.voice_mentions_allowed = voice_mentions_allowed;
				retired_resources.push(take_stale_participant_resources(
					existing,
					keyword_settings,
					is_owner,
					capture_relevant,
				));
				updated_existing = true;
			}
			if updated_existing {
				continue;
			}
			let mut participant = ParticipantState {
				username: username.clone(),
				voice_mentions_allowed,
				keyword_pipeline: None,
				vad: None,
				open_listen_task: None,
				last_voice_activity_event_ms: 0,
			};
			retired_resources.push(take_stale_participant_resources(
				&mut participant,
				keyword_settings,
				is_owner,
				false,
			));
			state
				.voice_state
				.as_mut()
				.expect("voice state")
				.participants
				.insert(user_id.clone(), participant);
			self.send_event(SongbirdEvent::ParticipantJoined {
				user_id: user_id.clone(),
				username,
				owner: is_owner,
			});
		}

		Ok(())
	}

	pub fn map_ssrc(&self, ssrc: u32, user_id: &str) {
		let mut state = self.state.lock();
		if let Some(voice_state) = state.voice_state.as_mut() {
			voice_state.speaking_state_update_count += 1;
			voice_state.ssrc_to_user.insert(ssrc, user_id.to_string());
		}
	}

	pub fn clear_ssrc_mappings(&self) {
		let mut state = self.state.lock();
		if let Some(voice_state) = state.voice_state.as_mut() {
			voice_state.ssrc_to_user.clear();
			voice_state.current_speakers.clear();
			voice_state.warned_unmapped_ssrcs.clear();
		}
	}

	pub fn clear_ssrc_mappings_for_user(&self, user_id: &str) -> usize {
		let mut state = self.state.lock();
		if let Some(voice_state) = state.voice_state.as_mut() {
			let before = voice_state.ssrc_to_user.len();
			let mut removed_ssrcs = Vec::new();
			voice_state.ssrc_to_user.retain(|ssrc, mapped_user_id| {
				let keep = mapped_user_id != user_id;
				if !keep {
					removed_ssrcs.push(*ssrc);
				}
				keep
			});
			for ssrc in removed_ssrcs {
				voice_state.warned_unmapped_ssrcs.remove(&ssrc);
			}
			voice_state.current_speakers.remove(user_id);
			return before.saturating_sub(voice_state.ssrc_to_user.len());
		}
		0
	}

	pub fn voice_participant_count(&self) -> usize {
		self
			.state
			.lock()
			.voice_state
			.as_ref()
			.map(|voice_state| voice_state.participants.len())
			.unwrap_or(0)
	}

	pub fn has_voice_participant(&self, user_id: &str) -> bool {
		self
			.state
			.lock()
			.voice_state
			.as_ref()
			.map(|voice_state| voice_state.participants.contains_key(user_id))
			.unwrap_or(false)
	}

	#[cfg(test)]
	pub fn upsert_voice_participant(
		self: &Arc<Self>,
		user_id: &str,
		username: &str,
		voice_mentions_allowed: bool,
	) -> Result<bool> {
		Ok(self
			.upsert_participant_at_revision(user_id, username, voice_mentions_allowed, None)?
			.unwrap_or(false))
	}
	pub fn upsert_voice_participant_if_current(
		self: &Arc<Self>,
		user_id: &str,
		username: &str,
		voice_mentions_allowed: bool,
		revision: ParticipantRevision,
	) -> Result<Option<bool>> {
		self.upsert_participant_at_revision(user_id, username, voice_mentions_allowed, Some(revision))
	}
	fn upsert_participant_at_revision(
		self: &Arc<Self>,
		user_id: &str,
		username: &str,
		voice_mentions_allowed: bool,
		revision: Option<ParticipantRevision>,
	) -> Result<Option<bool>> {
		let mut retired_resources = Vec::new();
		let mut state = self.state.lock();
		if revision.is_some_and(|expected| !expected.matches(&state)) {
			return Ok(None);
		}
		if state.voice_state.is_none() {
			return Ok(None);
		}
		state.participant_revision = state.participant_revision.wrapping_add(1);
		let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());
		let is_owner = self.is_owner_user_from_state(&state, user_id);

		state
			.usernames
			.insert(user_id.to_string(), username.to_string());
		let capture_relevant = state.streaming_users.contains(user_id)
			|| state.pending_audio_by_user.contains_key(user_id)
			|| state.capture_state_by_user.contains_key(user_id);
		if let Some(existing) = state
			.voice_state
			.as_mut()
			.expect("voice state")
			.participants
			.get_mut(user_id)
		{
			existing.username = username.to_string();
			existing.voice_mentions_allowed = voice_mentions_allowed;
			retired_resources.push(take_stale_participant_resources(
				existing,
				keyword_settings,
				is_owner,
				capture_relevant,
			));
			return Ok(Some(false));
		}

		let mut participant = ParticipantState {
			username: username.to_string(),
			voice_mentions_allowed,
			keyword_pipeline: None,
			vad: None,
			open_listen_task: None,
			last_voice_activity_event_ms: 0,
		};
		retired_resources.push(take_stale_participant_resources(
			&mut participant,
			keyword_settings,
			is_owner,
			false,
		));
		state
			.voice_state
			.as_mut()
			.expect("voice state")
			.participants
			.insert(user_id.to_string(), participant);

		self.send_event(SongbirdEvent::ParticipantJoined {
			user_id: user_id.to_string(),
			username: username.to_string(),
			owner: is_owner,
		});
		Ok(Some(true))
	}

	/// Idle Discord receivers still emit voice ticks. Silence is not an audio failure.
	pub fn voice_receive_started(&self) -> bool {
		self
			.state
			.lock()
			.voice_state
			.as_ref()
			.map(|voice_state| voice_state.voice_tick_count > 0)
			.unwrap_or(false)
	}

	pub fn voice_debug_snapshot(&self) -> Value {
		let state = self.state.lock();
		let Some(voice_state) = state.voice_state.as_ref() else {
			return json!({
					"hasVoiceState": false,
			});
		};
		let participants = voice_state
			.participants
			.iter()
			.map(|(user_id, participant)| {
				json!({
						"userId": user_id,
						"username": participant.username,
						"voiceMentionsAllowed": participant.voice_mentions_allowed,
						"keywordPipelineAllocated": participant.keyword_pipeline.is_some(),
						"keywordRecognizerCount": participant.keyword_pipeline
								 .as_ref()
								 .map(|pipeline| pipeline.recognizer_count())
								 .unwrap_or(0),
						"tenVadAllocated": participant.vad.is_some(),
				})
			})
			.collect::<Vec<_>>();
		let resource_counts = voice_resource_counts_from_state(voice_state);
		json!({
				"hasVoiceState": true,
				"guildId": voice_state
						 .binding
						 .as_ref()
						 .map(|binding| binding.guild_id.clone())
						 .or_else(|| state.presence.as_ref().map(|presence| presence.guild_id.clone())),
				"diagnostic": voice_state.diagnostic,
				"participantCount": voice_state.participants.len(),
				"participants": participants,
				"resourceCounts": {
						 "activeVoiceSession": resource_counts.active_voice_session,
						 "participantCount": resource_counts.participant_count,
						 "keywordPipelineCount": resource_counts.keyword_pipeline_count,
						 "keywordRecognizerCount": resource_counts.keyword_recognizer_count,
						 "tenVadCount": resource_counts.vad_count,
						 "decodedFrameCount": resource_counts.decoded_frame_count,
						 "voiceTickCount": resource_counts.voice_tick_count,
				},
				"mappedSsrcCount": voice_state.ssrc_to_user.len(),
				"currentSpeakerCount": voice_state.current_speakers.len(),
				"hasSeenAudio": voice_state.has_seen_audio,
				"voiceTickCount": voice_state.voice_tick_count,
				"speakingStateUpdateCount": voice_state.speaking_state_update_count,
				"decodedFrameCount": voice_state.decoded_frame_count,
				"droppedUnmappedSsrcCount": voice_state.dropped_unmapped_ssrc_count,
		})
	}

	pub fn voice_resource_counts(&self) -> VoiceResourceCounts {
		let state = self.state.lock();
		state
			.voice_state
			.as_ref()
			.map(voice_resource_counts_from_state)
			.unwrap_or_default()
	}

	pub fn handle_speaking_snapshot(&self, active_user_ids: HashSet<String>) {
		let mut started = Vec::new();
		let mut ended = Vec::new();
		{
			let mut state = self.state.lock();
			let Some(previous_speakers) = state
				.voice_state
				.as_ref()
				.map(|voice_state| voice_state.current_speakers.clone())
			else {
				return;
			};
			for user_id in active_user_ids
				.iter()
				.filter(|user_id| !previous_speakers.contains(*user_id))
			{
				let username = state
					.usernames
					.get(user_id)
					.cloned()
					.unwrap_or_else(|| user_id.clone());
				started.push((user_id.clone(), username));
			}
			for user_id in previous_speakers
				.iter()
				.filter(|user_id| !active_user_ids.contains(*user_id))
			{
				let username = state
					.usernames
					.get(user_id)
					.cloned()
					.unwrap_or_else(|| user_id.clone());
				ended.push((user_id.clone(), username));
			}
			if let Some(voice_state) = state.voice_state.as_mut() {
				voice_state.current_speakers = active_user_ids;
			}
		}

		for (user_id, username) in started {
			self.send_event(SongbirdEvent::SpeakingStarted { user_id, username });
		}
		for (user_id, username) in ended {
			self.send_event(SongbirdEvent::SpeakingEnded { user_id, username });
		}
	}

	pub fn handle_replay_audio_tick<'a>(
		&self,
		participant_frames: impl IntoIterator<Item = (&'a [i16], bool)>,
	) {
		self
			.state
			.lock()
			.replay_buffer
			.push_tick(participant_frames);
	}

	pub fn handle_replay_output_tick(&self, samples: &[f32], sample_rate: u32, channels: u16) {
		self
			.state
			.lock()
			.replay_buffer
			.queue_output_tick(samples, sample_rate, channels);
	}

	pub fn replay_snapshot(&self) -> Result<ReplaySnapshot> {
		self.state.lock().replay_buffer.snapshot()
	}

	pub fn handle_user_audio(self: &Arc<Self>, user_id: &str, pcm_samples: Vec<i16>) -> Result<()> {
		let mut retired_resources = Vec::new();
		let cheap_rms = cheap_rms_for_pcm_samples(&pcm_samples);
		let pcm16le = pcm_i16_to_le_bytes(&pcm_samples);
		if pcm16le.is_empty() {
			return Ok(());
		}

		let Some(job) = ({
			let mut state = self.state.lock();
			let Some(voice_state_ref) = state.voice_state.as_ref() else {
				return Ok(());
			};
			let speaker_active = voice_state_ref.current_speakers.contains(user_id);
			let should_send_frame = state.streaming_users.contains(user_id);
			let capture_relevant =
				should_send_frame || state.pending_audio_by_user.contains_key(user_id);
			let is_owner = self.is_owner_user_from_state(&state, user_id);
			let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());

			let Some(voice_state) = state.voice_state.as_mut() else {
				return Ok(());
			};
			voice_state.has_seen_audio = true;
			voice_state.decoded_frame_count += 1;
			let Some(participant) = voice_state.participants.get_mut(user_id) else {
				return Ok(());
			};
			let username = participant.username.clone();
			let voice_mentions_allowed = participant.voice_mentions_allowed;
			Some(VoiceProcessingJob {
				username,
				speaker_active,
				capture_relevant,
				is_owner,
				keyword_settings,
				voice_mentions_allowed,
				last_voice_activity_event_ms: participant.last_voice_activity_event_ms,
				keyword_pipeline: participant.keyword_pipeline.take(),
				vad: participant.vad.take(),
			})
		}) else {
			return Ok(());
		};

		let should_allocate_keyword =
			should_allocate_keyword_pipeline(job.voice_mentions_allowed, job.is_owner);
		let should_allocate_ten = should_allocate_vad(
			job.voice_mentions_allowed,
			job.is_owner,
			job.capture_relevant,
		);
		let mut keyword_pipeline = ensure_keyword_pipeline(
			job.keyword_pipeline,
			&job.username,
			user_id,
			job.keyword_settings,
			should_allocate_keyword,
		)?;
		let mut vad = ensure_vad(job.vad, &job.username, should_allocate_ten)?;

		let (vad_active, rms) = if let Some(vad) = vad.as_mut() {
			let vad_result = vad
				.process_frame(&pcm16le)
				.with_context(|| format!("TEN VAD process for {}", job.username))?;
			(vad_result.vad_active, vad_result.rms)
		} else {
			(
				job.speaker_active && cheap_rms >= CHEAP_VOICE_ACTIVITY_RMS_THRESHOLD,
				cheap_rms,
			)
		};

		let keyword_hits = if let Some(keyword_pipeline) = keyword_pipeline.as_mut() {
			keyword_pipeline.set_wake_enabled(job.voice_mentions_allowed);
			keyword_pipeline.set_cancel_enabled(true);
			keyword_pipeline
				.push_audio(&pcm16le)
				.with_context(|| format!("push keyword audio for {}", job.username))?
		} else {
			Vec::new()
		};

		let activity_now = now_ms();
		let should_send_voice_activity =
			job.speaker_active && activity_now.saturating_sub(job.last_voice_activity_event_ms) >= 240;

		let (username, should_send_frame, should_send_voice_activity) = {
			let mut state = self.state.lock();
			let is_owner = self.is_owner_user_from_state(&state, user_id);
			let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());
			let capture_relevant = state.streaming_users.contains(user_id)
				|| state.pending_audio_by_user.contains_key(user_id);
			let should_send_frame = state.streaming_users.contains(user_id);
			let should_buffer_frame =
				!should_send_frame && state.pending_audio_by_user.contains_key(user_id);
			let Some(voice_state) = state.voice_state.as_mut() else {
				return Ok(());
			};
			let Some(participant) = voice_state.participants.get_mut(user_id) else {
				return Ok(());
			};
			let username = participant.username.clone();
			let should_keep_keyword =
				should_allocate_keyword_pipeline(participant.voice_mentions_allowed, is_owner)
					&& keyword_pipeline_is_current(&keyword_pipeline, keyword_settings);
			if should_keep_keyword && participant.keyword_pipeline.is_none() {
				participant.keyword_pipeline = keyword_pipeline.take();
			}
			let should_keep_vad = should_allocate_vad(
				participant.voice_mentions_allowed,
				is_owner,
				capture_relevant,
			);
			if should_keep_vad && participant.vad.is_none() {
				participant.vad = vad.take();
			}
			if !should_keep_keyword {
				retired_resources.push((participant.keyword_pipeline.take(), None));
			}
			if !should_keep_vad {
				retired_resources.push((None, participant.vad.take()));
			}
			if should_send_voice_activity {
				participant.last_voice_activity_event_ms = activity_now;
			}

			if capture_relevant {
				track_voice_frame_locked(
					&mut state,
					user_id,
					&username,
					vad_active,
					FRAME_MS as u64,
					self,
				);
			}
			if should_buffer_frame {
				push_pending_audio_locked(&mut state, user_id, pcm16le.clone());
			}
			(username, should_send_frame, should_send_voice_activity)
		};

		for hit in keyword_hits {
			if hit.keyword_kind == KeywordKind::Wake {
				self.prime_capture_window_for_user(user_id, &username, None);
			}
			self.send_binary_event(
				SongbirdEvent::KeywordDetected {
					user_id: user_id.to_string(),
					username: username.clone(),
					owner: self.is_owner_user(user_id),
					keyword_text: hit.keyword_text,
					keyword_kind: hit.keyword_kind.as_str().to_string(),
					model_name: hit.model_name,
				},
				"verificationPcm",
				&hit.verification_pcm,
			);
		}

		if should_send_frame {
			self.send_binary_event(
				SongbirdEvent::AudioFrame {
					user_id: user_id.to_string(),
					username: username.clone(),
					sample_rate_hz: INPUT_SAMPLE_RATE,
					channels: 1,
					frame_ms: FRAME_MS,
					vad_active: Some(vad_active),
					rms: Some(rms),
				},
				"audioFramePcm16le",
				&pcm16le,
			);
		}
		if should_send_voice_activity {
			self.send_event(SongbirdEvent::VoiceActivity {
				user_id: user_id.to_string(),
				username,
				frame_ms: FRAME_MS,
				vad_active,
				rms,
			});
		}

		Ok(())
	}

	pub fn prime_capture_window_for_user(
		self: &Arc<Self>,
		user_id: &str,
		username: &str,
		no_speech_duration_ms: Option<u64>,
	) {
		{
			let mut state = self.state.lock();
			state
				.usernames
				.insert(user_id.to_string(), username.to_string());
			state
				.pending_audio_by_user
				.insert(user_id.to_string(), VecDeque::new());
			state
				.pending_audio_bytes_by_user
				.insert(user_id.to_string(), 0);
			let capture = state
				.capture_state_by_user
				.entry(user_id.to_string())
				.or_insert_with(CaptureState::default);
			reset_capture_state(capture, false);
			capture.open_listen_armed = false;
		}

		if let Some(duration_ms) = no_speech_duration_ms {
			self.arm_no_speech_timer(user_id, username, duration_ms);
		}
	}

	pub fn open_listen(
		self: &Arc<Self>,
		user_id: &str,
		duration_ms: u64,
		delay_ms: u64,
		reason: Option<String>,
	) -> Result<bool> {
		let listen_reason = reason.unwrap_or_else(|| "open_listen".to_string());
		let Some((username, keyword_settings, keyword_pipeline)) = ({
			let mut state = self.state.lock();
			let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());
			let is_owner = self.is_owner_user_from_state(&state, user_id);
			let Some(voice_state) = state.voice_state.as_mut() else {
				return Ok(false);
			};
			let Some(participant) = voice_state.participants.get_mut(user_id) else {
				return Ok(false);
			};
			if !participant.voice_mentions_allowed {
				return Ok(false);
			}
			if participant.open_listen_task.is_some() {
				return Ok(false);
			}
			let username = participant.username.clone();
			let keyword_pipeline =
				if should_allocate_keyword_pipeline(participant.voice_mentions_allowed, is_owner) {
					participant.keyword_pipeline.take()
				} else {
					None
				};
			Some((username, keyword_settings, keyword_pipeline))
		}) else {
			return Ok(false);
		};

		let mut keyword_pipeline =
			ensure_keyword_pipeline(keyword_pipeline, &username, user_id, keyword_settings, true)?;
		let verification_pcm = keyword_pipeline
			.as_ref()
			.map(|pipeline| pipeline.open_listen_preroll())
			.unwrap_or_default();

		{
			let mut state = self.state.lock();
			let keyword_settings = keyword_settings_from_presence(state.presence.as_ref());
			let is_owner = self.is_owner_user_from_state(&state, user_id);
			let Some(voice_state) = state.voice_state.as_mut() else {
				return Ok(false);
			};
			let Some(participant) = voice_state.participants.get_mut(user_id) else {
				return Ok(false);
			};
			if !participant.voice_mentions_allowed {
				return Ok(false);
			}
			if participant.open_listen_task.is_some() {
				return Ok(false);
			}
			if should_allocate_keyword_pipeline(participant.voice_mentions_allowed, is_owner)
				&& keyword_pipeline_is_current(&keyword_pipeline, keyword_settings)
				&& participant.keyword_pipeline.is_none()
			{
				participant.keyword_pipeline = keyword_pipeline.take();
			}
			let session = Arc::clone(self);
			let user_id = user_id.to_string();
			let username_for_task = username.clone();
			let listen_reason_for_task = listen_reason.clone();
			participant.open_listen_task = Some(tokio::spawn(async move {
				if delay_ms > 0 {
					tokio::time::sleep(Duration::from_millis(delay_ms)).await;
				}
				{
					let mut state = session.state.lock();
					let Some(voice_state) = state.voice_state.as_mut() else {
						return;
					};
					let Some(participant) = voice_state.participants.get_mut(&user_id) else {
						return;
					};
					participant.open_listen_task = None;
				}
				session.prime_capture_window_for_user(&user_id, &username_for_task, Some(duration_ms));
				{
					let mut state = session.state.lock();
					state.streaming_users.insert(user_id.clone());
				}
				let owner = session.is_owner_user(&user_id);
				session.send_binary_event(
					SongbirdEvent::ListenStarted {
						user_id: user_id.clone(),
						username: username_for_task,
						owner,
						duration_ms,
						reason: listen_reason_for_task,
					},
					"verificationPcm",
					&verification_pcm,
				);
			}));
		}
		Ok(true)
	}

	pub fn reset_listen(&self, user_id: &str, clear_pre_roll: bool) -> bool {
		let mut state = self.state.lock();
		{
			let Some(voice_state) = state.voice_state.as_mut() else {
				return false;
			};
			let Some(participant) = voice_state.participants.get_mut(user_id) else {
				return false;
			};
			if let Some(task) = participant.open_listen_task.take() {
				task.abort();
			}
			if clear_pre_roll {
				if let Some(keyword_pipeline) = participant.keyword_pipeline.as_mut() {
					keyword_pipeline.clear_pre_roll();
				}
			}
		}
		clear_capture_state_for_user(&mut state, user_id);
		state.pending_audio_by_user.remove(user_id);
		state.pending_audio_bytes_by_user.remove(user_id);
		state.pending_utterance_finalized_by_user.remove(user_id);
		state.streaming_users.remove(user_id);
		true
	}

	pub fn set_speech_gain(&self, gain: f32) -> Result<()> {
		anyhow::ensure!(
			gain.is_finite() && (0.0..=2.0).contains(&gain),
			"Invalid speech gain"
		);
		if let Some(playback) = self.current_playback.lock().as_ref() {
			if playback.playback_class == SongbirdPlaybackClass::Speech {
				let _ = playback.handle.set_volume(gain);
			}
		}
		Ok(())
	}
	pub fn set_cue_gain(&self, gain: f32) -> Result<()> {
		anyhow::ensure!(
			gain.is_finite() && (0.0..=2.0).contains(&gain),
			"Invalid cue gain"
		);
		if let Some(playback) = self.current_playback.lock().as_ref() {
			if matches!(
				playback.playback_class,
				SongbirdPlaybackClass::Cue | SongbirdPlaybackClass::Thinking
			) {
				let _ = playback.handle.set_volume(gain);
			}
		}
		Ok(())
	}
	pub async fn interrupt_playback(&self) {
		self.interruption_epoch.fetch_add(1, Ordering::SeqCst);
		let current = self.current_playback.lock().clone();
		if let Some(current) = current {
			let _ = current.handle.stop();
		}
	}

	pub async fn play_audio(
		self: &Arc<Self>,
		audio: Vec<u8>,
		input_type: SongbirdPlaybackInputType,
		mode: Option<SongbirdPlaybackMode>,
		playback_class: SongbirdPlaybackClass,
		timeout_ms: Option<u64>,
		gain: Option<f32>,
		debug_meta: Option<Value>,
	) -> Result<bool> {
		const MAX_PLAYBACK_GAIN: f32 = 2.0;
		let interruption_epoch = self.interruption_epoch.load(Ordering::SeqCst);

		let gain = gain.unwrap_or(1.0);
		if !gain.is_finite() {
			anyhow::bail!("playback gain must be finite");
		}
		if !(0.0..=MAX_PLAYBACK_GAIN).contains(&gain) {
			anyhow::bail!("playback gain must be between 0 and {MAX_PLAYBACK_GAIN}");
		}

		let binding = self
			.get_voice_binding()
			.ok_or_else(|| anyhow!("no active voice presence"))?;
		let next_playback_id = self.playback_seq.fetch_add(1, Ordering::Relaxed) + 1;
		let debug_meta_value = debug_meta.unwrap_or_else(|| json!({}));

		if matches!(mode, Some(SongbirdPlaybackMode::Urgent)) {
			let maybe_stop = {
				let current = self.current_playback.lock();
				current.as_ref().and_then(|current| {
					if should_preempt(current.playback_class, playback_class) {
						Some(current.handle.clone())
					} else {
						None
					}
				})
			};
			if let Some(handle) = maybe_stop {
				let _ = handle.stop();
			}
		}

		let _playback_guard = self.playback_lock.lock().await;
		if interruption_epoch != self.interruption_epoch.load(Ordering::SeqCst) {
			return Ok(false);
		}
		let input_bytes = match input_type {
			SongbirdPlaybackInputType::Raw => wrap_discord_pcm_as_wav(&audio),
			SongbirdPlaybackInputType::OggOpus | SongbirdPlaybackInputType::Encoded => audio,
		};
		if !debug_meta_value.is_null() {
			tracing::info!(
					playback_id = next_playback_id,
					playback_class = ?playback_class,
					input_type = ?input_type,
					input_bytes = input_bytes.len(),
					timeout_ms = timeout_ms.unwrap_or(30_000),
					debug_meta = ?debug_meta_value,
					"starting songbird playback"
			);
		}
		let input: Input = input_bytes.into();
		let mut track = Track::from(input).volume(gain);
		let (tx, rx) = tokio::sync::oneshot::channel::<Result<bool>>();
		// Install observers before handing the track to the mixer: short or invalid
		// audio can finish before an asynchronous handle.add_event reaches it.
		track.events.add_event(
			EventData::new(
				Event::Track(TrackEvent::End),
				TrackEndNotifier {
					tx: Mutex::new(Some(tx)),
				},
			),
			Duration::ZERO,
		);
		track.events.add_event(
			EventData::new(
				Event::Track(TrackEvent::Playable),
				TrackPlaybackStartedNotifier {
					session: Arc::clone(self),
					playback_id: next_playback_id,
					playback_class: playback_class.clone(),
					debug_meta: if debug_meta_value.is_null() {
						None
					} else {
						Some(debug_meta_value.clone())
					},
					fired: Arc::new(AtomicBool::new(false)),
				},
			),
			Duration::ZERO,
		);
		let handle = {
			let mut call = binding.call.lock().await;
			call.play_only(track)
		};
		let _stop_on_drop = StopTrackOnDrop(handle.clone());
		{
			let mut current = self.current_playback.lock();
			if interruption_epoch != self.interruption_epoch.load(Ordering::SeqCst) {
				return Ok(false);
			}
			*current = Some(ActivePlayback {
				playback_id: next_playback_id,
				playback_class: playback_class.clone(),
				handle: handle.clone(),
			});
		}
		// The drop guard stops this track if its orchestration future is cancelled.
		let timeout_ms = timeout_ms.unwrap_or(30_000);
		let completion = timeout(Duration::from_millis(timeout_ms), rx).await;
		let timed_out = completion.is_err();
		let outcome = match completion {
			Ok(Ok(result)) => result,
			Ok(Err(_)) => Err(anyhow!("playback completion observer was dropped")),
			Err(_) => Ok(false),
		};
		let finished = matches!(&outcome, Ok(true));
		if !finished {
			let _ = handle.stop();
		}
		if !debug_meta_value.is_null() {
			tracing::info!(
					playback_id = next_playback_id,
					playback_class = ?playback_class,
					finished,
					timed_out,
					debug_meta = ?debug_meta_value,
					"finished songbird playback"
			);
		}
		let mut current = self.current_playback.lock();
		if current
			.as_ref()
			.map(|active| active.playback_id == next_playback_id)
			.unwrap_or(false)
		{
			*current = None;
		}
		outcome
	}

	pub async fn play_signal(
		self: &Arc<Self>,
		signal_key: SongbirdSignalKey,
		gain: Option<f32>,
		repeat_count: Option<u32>,
		variant_key: Option<String>,
		mode: Option<SongbirdPlaybackMode>,
		playback_class: Option<SongbirdPlaybackClass>,
		timeout_ms: Option<u64>,
		debug_meta: Option<Value>,
	) -> Result<bool> {
		let signal_audio = get_signal_audio(signal_key, gain, repeat_count, variant_key.as_deref())?;
		self
			.play_audio(
				signal_audio,
				SongbirdPlaybackInputType::Encoded,
				mode,
				playback_class.unwrap_or(match signal_key {
					SongbirdSignalKey::ThinkingLoop => SongbirdPlaybackClass::Thinking,
					_ => SongbirdPlaybackClass::Cue,
				}),
				timeout_ms,
				None,
				debug_meta,
			)
			.await
	}

	fn flush_pending_audio(&self, user_id: &str) {
		let (username, buffers) = {
			let mut state = self.state.lock();
			let buffers = state
				.pending_audio_by_user
				.remove(user_id)
				.unwrap_or_default()
				.into_iter()
				.collect::<Vec<_>>();
			state.pending_audio_bytes_by_user.remove(user_id);
			let username = state
				.usernames
				.get(user_id)
				.cloned()
				.unwrap_or_else(|| user_id.to_string());
			if let Some(capture) = state.capture_state_by_user.get_mut(user_id) {
				if let Some(task) = capture.no_speech_timer.take() {
					task.abort();
				}
			}
			(username, buffers)
		};

		for buffer in buffers {
			self.send_binary_event(
				SongbirdEvent::AudioFrame {
					user_id: user_id.to_string(),
					username: username.clone(),
					sample_rate_hz: INPUT_SAMPLE_RATE,
					channels: 1,
					frame_ms: FRAME_MS,
					vad_active: None,
					rms: None,
				},
				"audioFramePcm16le",
				&buffer,
			);
		}
	}

	fn flush_pending_utterance_finalized(&self, user_id: &str) {
		let pending = {
			let mut state = self.state.lock();
			state.pending_utterance_finalized_by_user.remove(user_id)
		};
		if let Some(pending) = pending {
			self.send_utterance_finalized_event(
				user_id,
				pending.username,
				pending.reason,
				pending.duration_ms,
				pending.voiced_ms,
			);
		}
	}

	fn send_utterance_finalized_event(
		&self,
		user_id: &str,
		username: String,
		reason: String,
		duration_ms: u64,
		voiced_ms: u64,
	) {
		self.send_event(SongbirdEvent::UtteranceFinalized {
			user_id: user_id.to_string(),
			username,
			reason,
			duration_ms,
			voiced_ms,
		});
	}

	fn arm_no_speech_timer(self: &Arc<Self>, user_id: &str, username: &str, duration_ms: u64) {
		let user_id = user_id.to_string();
		let username = username.to_string();
		let mut state = self.state.lock();
		let capture = state
			.capture_state_by_user
			.entry(user_id.clone())
			.or_insert_with(CaptureState::default);
		if let Some(task) = capture.no_speech_timer.take() {
			task.abort();
		}
		capture.open_listen_armed = true;
		let session = Arc::clone(self);
		capture.no_speech_timer = Some(tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(duration_ms)).await;
			let mut state = session.state.lock();
			let Some(capture) = state.capture_state_by_user.get_mut(&user_id) else {
				return;
			};
			capture.no_speech_timer = None;
			if !capture.open_listen_armed
				|| capture.segment_started_at_ms.is_some()
				|| capture.voiced_ms > 0
			{
				return;
			}
			reset_capture_state(capture, false);
			capture.open_listen_armed = false;
			drop(state);
			session.send_event(SongbirdEvent::NoSpeech { user_id, username });
		}));
	}

	pub fn finalize_utterance_for_user(self: &Arc<Self>, user_id: &str, reason: &str) {
		let maybe_event = {
			let mut state = self.state.lock();
			let username = state
				.usernames
				.get(user_id)
				.cloned()
				.unwrap_or_else(|| user_id.to_string());
			let Some(capture) = state.capture_state_by_user.get_mut(user_id) else {
				return;
			};
			if capture.segment_started_at_ms.is_none() || capture.voiced_ms < MIN_UTTERANCE_VOICED_MS {
				reset_capture_state(capture, false);
				capture.open_listen_armed = false;
				return;
			}
			let segment_started_at_ms = capture.segment_started_at_ms.unwrap_or_default();
			let segment_ended_at_ms = capture.last_voiced_at_ms.unwrap_or(segment_started_at_ms);
			let duration_ms = segment_ended_at_ms
				.saturating_sub(segment_started_at_ms)
				.saturating_add(FRAME_MS as u64);
			let voiced_ms = capture.voiced_ms;
			reset_capture_state(capture, false);
			capture.open_listen_armed = false;
			let duration_ms = duration_ms.max(FRAME_MS as u64);
			if state.pending_audio_by_user.contains_key(user_id)
				&& !state.streaming_users.contains(user_id)
			{
				state.pending_utterance_finalized_by_user.insert(
					user_id.to_string(),
					PendingUtteranceFinalized {
						username,
						reason: reason.to_string(),
						duration_ms,
						voiced_ms,
					},
				);
				None
			} else {
				state.pending_utterance_finalized_by_user.remove(user_id);
				Some((username, reason.to_string(), duration_ms, voiced_ms))
			}
		};

		if let Some((username, reason, duration_ms, voiced_ms)) = maybe_event {
			self.send_utterance_finalized_event(user_id, username, reason, duration_ms, voiced_ms);
		}
	}

	fn is_owner_user(&self, user_id: &str) -> bool {
		let state = self.state.lock();
		self.is_owner_user_from_state(&state, user_id)
	}

	fn is_owner_user_from_state(&self, state: &SessionState, user_id: &str) -> bool {
		state
			.presence
			.as_ref()
			.and_then(|it| it.owner_discord_id.as_ref())
			.map(|it| it == user_id)
			.unwrap_or(false)
	}
}

#[derive(Default, Debug)]
pub struct TrackEndNotifier {
	tx: Mutex<Option<tokio::sync::oneshot::Sender<Result<bool>>>>,
}

pub struct TrackPlaybackStartedNotifier {
	session: Arc<Session>,
	playback_id: u64,
	playback_class: SongbirdPlaybackClass,
	debug_meta: Option<Value>,
	fired: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl TrackEventHandler for TrackPlaybackStartedNotifier {
	async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
		if self.fired.swap(true, Ordering::SeqCst) {
			return Some(Event::Cancel);
		}
		self.session.send_event(SongbirdEvent::PlaybackStarted {
			playback_id: self.playback_id,
			playback_class: self.playback_class.clone(),
			debug_meta: self.debug_meta.clone(),
		});
		Some(Event::Cancel)
	}
}

#[async_trait::async_trait]
impl TrackEventHandler for TrackEndNotifier {
	async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
		if let Some(tx) = self.tx.lock().take() {
			let result = match ctx {
				EventContext::Track(tracks) => tracks
					.first()
					.map(|(state, _)| playback_outcome(&state.playing))
					.unwrap_or_else(|| Err(anyhow!("playback ended without track state"))),
				_ => Err(anyhow!("playback ended without track context")),
			};
			let _ = tx.send(result);
		}
		Some(Event::Cancel)
	}
}

fn playback_outcome(mode: &PlayMode) -> Result<bool> {
	match mode {
		PlayMode::End => Ok(true),
		PlayMode::Errored(error) => Err(anyhow!("audio playback failed: {error}")),
		_ => Ok(false),
	}
}

impl Default for CaptureState {
	fn default() -> Self {
		Self {
			no_speech_timer: None,
			open_listen_armed: false,
			segment_started_at_ms: None,
			last_voiced_at_ms: None,
			voiced_ms: 0,
			finalize_timer: None,
		}
	}
}

fn clear_capture_state_for_user(state: &mut SessionState, user_id: &str) {
	let Some(mut capture) = state.capture_state_by_user.remove(user_id) else {
		return;
	};
	if let Some(task) = capture.no_speech_timer.take() {
		task.abort();
	}
	if let Some(task) = capture.finalize_timer.take() {
		task.abort();
	}
}

fn reset_capture_state(capture: &mut CaptureState, keep_no_speech_timer: bool) {
	if !keep_no_speech_timer {
		if let Some(task) = capture.no_speech_timer.take() {
			task.abort();
		}
	}
	if let Some(task) = capture.finalize_timer.take() {
		task.abort();
	}
	capture.segment_started_at_ms = None;
	capture.last_voiced_at_ms = None;
	capture.voiced_ms = 0;
}

fn track_voice_frame_locked(
	state: &mut SessionState,
	user_id: &str,
	username: &str,
	vad_active: bool,
	frame_ms: u64,
	session: &Arc<Session>,
) {
	let capture_relevant =
		state.streaming_users.contains(user_id) || state.pending_audio_by_user.contains_key(user_id);
	if !capture_relevant || !vad_active {
		return;
	}

	let capture = state
		.capture_state_by_user
		.entry(user_id.to_string())
		.or_insert_with(CaptureState::default);
	if let Some(task) = capture.no_speech_timer.take() {
		task.abort();
	}
	capture.open_listen_armed = false;
	let now = now_ms();
	if capture.segment_started_at_ms.is_none() {
		capture.segment_started_at_ms = Some(now);
		capture.voiced_ms = 0;
	}
	capture.last_voiced_at_ms = Some(now);
	capture.voiced_ms += frame_ms;
	if let Some(task) = capture.finalize_timer.take() {
		task.abort();
	}
	let session_clone = Arc::clone(session);
	let user_id = user_id.to_string();
	let _username = username.to_string();
	capture.finalize_timer = Some(tokio::spawn(async move {
		tokio::time::sleep(Duration::from_millis(UTTERANCE_FINALIZE_SILENCE_MS)).await;
		session_clone.finalize_utterance_for_user(&user_id, "vad_silence");
	}));
}

fn push_pending_audio_locked(state: &mut SessionState, user_id: &str, chunk: Vec<u8>) {
	let buffers = state
		.pending_audio_by_user
		.entry(user_id.to_string())
		.or_insert_with(VecDeque::new);
	let current_bytes = state
		.pending_audio_bytes_by_user
		.get(user_id)
		.copied()
		.unwrap_or(0);
	buffers.push_back(chunk);
	let mut next_bytes = current_bytes
		+ buffers
			.back()
			.map(|buffer| buffer.len())
			.unwrap_or_default();
	while next_bytes > TURN_AUDIO_BUFFER_BYTES {
		let Some(removed) = buffers.pop_front() else {
			break;
		};
		next_bytes = next_bytes.saturating_sub(removed.len());
	}
	state
		.pending_audio_bytes_by_user
		.insert(user_id.to_string(), next_bytes);
}

fn should_preempt(current: SongbirdPlaybackClass, next: SongbirdPlaybackClass) -> bool {
	matches!(
		(current, next),
		(SongbirdPlaybackClass::Thinking, SongbirdPlaybackClass::Cue)
			| (
				SongbirdPlaybackClass::Thinking,
				SongbirdPlaybackClass::Speech
			) | (SongbirdPlaybackClass::Cue, SongbirdPlaybackClass::Speech)
	)
}

fn now_ms() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as u64
}

#[cfg(test)]
mod tests {
	use super::*;

	fn diagnostic_session() -> Arc<Session> {
		let (tx, _rx) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
		let session = Session::new("retirement-test".to_string(), tx);
		let presence = SongbirdPresenceConfig {
			guild_id: "guild".into(),
			channel_id: "channel".into(),
			require_speak: false,
			listen_everyone: true,
			listen_role_ids: Vec::new(),
			listen_allowed_user_ids: Vec::new(),
			listen_blocked_user_ids: Vec::new(),
			owner_discord_id: None,
			replay_buffer_enabled: false,
			replay_buffer_seconds: 20,
			wake_word: crate::protocol::VoiceWakeWord::HeyBumblebee,
			wake_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
			stop_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
			cancel_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
		};
		session
			.set_diagnostic_presence(presence, "speaker", "fixture")
			.unwrap();
		session
	}

	#[test]
	fn idle_voice_ticks_prove_receive_startup_without_requiring_speech() {
		let session = diagnostic_session();
		assert!(!session.voice_receive_started());
		{
			let mut state = session.state.lock();
			let voice = state.voice_state.as_mut().unwrap();
			assert!(!voice.has_seen_audio);
			assert_eq!(voice.decoded_frame_count, 0);
			voice.voice_tick_count = 1;
		}
		assert!(session.voice_receive_started());
		session.clear_presence_state(None);
		assert!(!session.voice_receive_started());
	}

	fn native_retirement_session() -> (Arc<Session>, Arc<AtomicBool>) {
		let session = diagnostic_session();
		let dropped = Arc::new(AtomicBool::new(false));
		let weak_session = Arc::downgrade(&session);
		let observed = dropped.clone();
		let pipeline = KeywordPipeline::with_drop_probe(move || {
			let session = weak_session.upgrade().unwrap();
			assert!(
				session.state.try_lock().is_some(),
				"native destructor ran under session lock"
			);
			observed.store(true, Ordering::SeqCst);
		});
		session
			.state
			.lock()
			.voice_state
			.as_mut()
			.unwrap()
			.participants
			.get_mut("speaker")
			.unwrap()
			.keyword_pipeline = Some(pipeline);
		(session, dropped)
	}

	#[test]
	fn clearing_voice_releases_lock_before_native_destruction() {
		let (session, dropped) = native_retirement_session();
		session.clear_presence_state(None);
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn removing_participant_releases_lock_before_native_destruction() {
		let (session, dropped) = native_retirement_session();
		session.sync_participants(Vec::new()).unwrap();
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn permission_updates_release_lock_before_native_destruction() {
		for update_by_sync in [false, true] {
			let (session, dropped) = native_retirement_session();
			if update_by_sync {
				session
					.sync_participants(vec![("speaker".into(), "fixture".into(), false)])
					.unwrap();
			} else {
				session
					.upsert_voice_participant("speaker", "fixture", false)
					.unwrap();
			}
			assert!(dropped.load(Ordering::SeqCst));
		}
	}

	#[tokio::test]
	async fn delayed_member_fetches_cannot_restore_a_revoked_listener() {
		// All three HTTP paths (whole-channel sync, speaking discovery and
		// pre-transcription revalidation) commit through these guarded methods.
		for bulk_sync in [false, true] {
			let session = diagnostic_session();
			let (_, revision) = session.participant_snapshot();
			let (complete, fetched) = tokio::sync::oneshot::channel();
			let pending = session.clone();
			let request = tokio::spawn(async move {
				fetched.await.unwrap();
				if bulk_sync {
					pending
						.sync_participants_if_current(
							vec![("speaker".into(), "stale HTTP".into(), true)],
							revision,
						)
						.unwrap();
				} else {
					assert_eq!(
						pending
							.upsert_voice_participant_if_current("speaker", "stale HTTP", true, revision)
							.unwrap(),
						None
					);
				}
			});
			// A gateway member update arrives while the HTTP response is pending.
			let (_, current) = session.invalidate_participant_snapshot();
			session
				.upsert_voice_participant_if_current("speaker", "revoked", false, current)
				.unwrap();
			complete.send(()).unwrap();
			request.await.unwrap();
			let state = session.state.lock();
			let speaker = &state.voice_state.as_ref().unwrap().participants["speaker"];
			assert!(!speaker.voice_mentions_allowed);
			assert_eq!(speaker.username, "revoked");
		}
	}
	#[tokio::test]
	async fn delayed_participant_list_cannot_cross_a_channel_or_settings_change() {
		let session = diagnostic_session();
		let (_, revision) = session.participant_snapshot();
		let (complete, fetched) = tokio::sync::oneshot::channel();
		let pending = session.clone();
		let request = tokio::spawn(async move {
			fetched.await.unwrap();
			pending
				.sync_participants_if_current(
					vec![("old-channel-user".into(), "old".into(), true)],
					revision,
				)
				.unwrap();
		});
		let mut config = session.get_presence_config().unwrap();
		config.channel_id = "new-channel".into();
		config.listen_everyone = false;
		config.listen_blocked_user_ids.push("speaker".into());
		session
			.set_diagnostic_presence(config, "speaker", "current")
			.unwrap();
		complete.send(()).unwrap();
		request.await.unwrap();
		assert!(!session.has_voice_participant("old-channel-user"));
		assert!(session.has_voice_participant("speaker"));
	}

	#[test]
	fn replacing_diagnostic_presence_releases_lock_before_native_destruction() {
		let (session, dropped) = native_retirement_session();
		let presence = session.get_presence_config().unwrap();
		session
			.set_diagnostic_presence(presence, "speaker", "fixture")
			.unwrap();
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn audio_permission_race_releases_lock_before_native_destruction() {
		let (session, dropped) = native_retirement_session();
		session
			.state
			.lock()
			.voice_state
			.as_mut()
			.unwrap()
			.participants
			.get_mut("speaker")
			.unwrap()
			.voice_mentions_allowed = false;
		session.handle_user_audio("speaker", vec![0; 160]).unwrap();
		assert!(dropped.load(Ordering::SeqCst));
	}

	#[test]
	fn deferred_utterance_finalization_flushes_after_pending_audio() {
		let (tx, mut rx) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
		let session = Session::new("bot-1".to_string(), tx);
		let user_id = "user-1";
		let username = "Alex";
		let audio = vec![1_u8, 2, 3, 4];

		{
			let mut state = session.state.lock();
			state
				.usernames
				.insert(user_id.to_string(), username.to_string());
			state
				.pending_audio_by_user
				.insert(user_id.to_string(), VecDeque::from([audio.clone()]));
			state
				.pending_audio_bytes_by_user
				.insert(user_id.to_string(), audio.len());
			state.capture_state_by_user.insert(
				user_id.to_string(),
				CaptureState {
					segment_started_at_ms: Some(1_000),
					last_voiced_at_ms: Some(1_080),
					voiced_ms: 100,
					..CaptureState::default()
				},
			);
		}

		session.finalize_utterance_for_user(user_id, "vad_silence");
		assert!(rx.try_recv().is_err());

		session.set_streaming(user_id, true);

		let first = rx.try_recv().expect("pending audio frame");
		let decoded = first;
		assert_eq!(decoded.payload.as_slice(), audio.as_slice());
		assert!(matches!(
			  decoded.event,
			  SongbirdEvent::AudioFrame { ref user_id, .. } if user_id == "user-1"
		));

		let second = rx.try_recv().expect("utterance finalized event");
		let envelope = second;
		assert!(matches!(
			  envelope.event,
			  SongbirdEvent::UtteranceFinalized {
					 ref user_id,
					 ref reason,
					 duration_ms,
					 voiced_ms,
					 ..
			  } if user_id == "user-1"
					 && reason == "vad_silence"
					 && duration_ms == 100
					 && voiced_ms == 100
		));
		assert!(rx.try_recv().is_err());
	}

	#[tokio::test]
	async fn interruption_discards_an_already_queued_track() {
		let (tx, _rx) = mpsc::channel(8);
		let session = Session::new("test".into(), tx);
		let call = songbird::Call::standalone(
			std::num::NonZeroU64::new(1).unwrap(),
			std::num::NonZeroU64::new(2).unwrap(),
		);
		session.set_presence(
			SongbirdPresenceConfig::owner_only("1".into(), "3".into(), "4".into()),
			VoiceBinding {
				guild_id: "1".into(),
				call: Arc::new(tokio::sync::Mutex::new(call)),
			},
		);
		let playback_lock = session.playback_lock.lock().await;
		let pending = session.play_audio(
			vec![0; 640],
			SongbirdPlaybackInputType::Raw,
			None,
			SongbirdPlaybackClass::Speech,
			Some(50),
			Some(1.0),
			None,
		);
		tokio::pin!(pending);
		tokio::select! {
			 biased;
			 result = &mut pending => panic!("queued playback resolved too early: {result:?}"),
			 _ = tokio::task::yield_now() => {},
		}
		session.interrupt_playback().await;
		drop(playback_lock);
		assert!(!pending.await.unwrap());
		assert!(session.current_playback.lock().is_none());
	}

	#[test]
	fn overflow_leaves_an_observable_sequence_gap() {
		let (tx, mut rx) = mpsc::channel(1);
		let session = Session::new("test".into(), tx);
		session.send_session_error("accepted");
		session.send_session_error("dropped");
		assert_eq!(rx.try_recv().unwrap().sequence, 1);
		session.send_session_error("resumed");
		assert_eq!(rx.try_recv().unwrap().sequence, 3);
	}

	#[test]
	fn playback_preemption_policy_stays_narrow() {
		assert!(should_preempt(
			SongbirdPlaybackClass::Thinking,
			SongbirdPlaybackClass::Cue
		));
		assert!(should_preempt(
			SongbirdPlaybackClass::Thinking,
			SongbirdPlaybackClass::Speech
		));
		assert!(should_preempt(
			SongbirdPlaybackClass::Cue,
			SongbirdPlaybackClass::Speech
		));

		assert!(!should_preempt(
			SongbirdPlaybackClass::Speech,
			SongbirdPlaybackClass::Cue
		));
		assert!(!should_preempt(
			SongbirdPlaybackClass::Speech,
			SongbirdPlaybackClass::Thinking
		));
		assert!(!should_preempt(
			SongbirdPlaybackClass::Cue,
			SongbirdPlaybackClass::Cue
		));
	}

	#[test]
	fn outbound_queue_is_bounded_when_consumer_stalls() {
		let (tx, mut rx) = mpsc::channel(1);
		let session = Session::new("bot-1".to_string(), tx);

		session.send_session_error("first");
		session.send_session_error("second");

		assert_eq!(
			session
				.outbound_dropped_message_count
				.load(Ordering::SeqCst),
			1
		);
		let meta = session.snapshot_debug_meta();
		assert_eq!(meta["outboundDroppedMessageCount"], json!(1));

		assert!(matches!(rx.try_recv().expect("first event").event,
            SongbirdEvent::SessionError { message } if message == "first"));
		assert!(rx.try_recv().is_err());
	}

	#[test]
	fn handle_user_audio_uses_cheap_activity_path_without_native_resources() {
		let (tx, mut rx) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
		let session = Session::new("bot-1".to_string(), tx);
		let user_id = "user-1";
		let username = "Alex";

		{
			let mut state = session.state.lock();
			state.presence = Some(SongbirdPresenceConfig {
				guild_id: "guild-1".to_string(),
				channel_id: "channel-1".to_string(),
				require_speak: false,
				listen_everyone: false,
				listen_role_ids: Vec::new(),
				listen_allowed_user_ids: Vec::new(),
				listen_blocked_user_ids: Vec::new(),
				owner_discord_id: None,
				replay_buffer_enabled: false,
				replay_buffer_seconds: 20,
				wake_word: crate::protocol::VoiceWakeWord::HeyBumblebee,
				wake_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
				stop_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
				cancel_keyword_sensitivity: crate::protocol::VoiceKeywordSensitivity::Balanced,
			});
			state.voice_state = Some(VoiceSessionState {
				binding: None,
				ssrc_to_user: HashMap::new(),
				current_speakers: HashSet::from([user_id.to_string()]),
				participants: HashMap::from([(
					user_id.to_string(),
					ParticipantState {
						username: username.to_string(),
						voice_mentions_allowed: false,
						keyword_pipeline: None,
						vad: None,
						open_listen_task: None,
						last_voice_activity_event_ms: 0,
					},
				)]),
				has_seen_audio: false,
				voice_tick_count: 0,
				speaking_state_update_count: 0,
				decoded_frame_count: 0,
				dropped_unmapped_ssrc_count: 0,
				warned_unmapped_ssrcs: HashSet::new(),
				diagnostic: true,
			});
		}

		session
			.handle_user_audio(user_id, vec![1_500_i16; 160])
			.expect("handle audio");

		let message = rx.try_recv().expect("voice activity event");
		let envelope = message;
		assert!(matches!(
			  envelope.event,
			  SongbirdEvent::VoiceActivity {
					 ref user_id,
					 vad_active: true,
					 ..
			  } if user_id == "user-1"
		));
		assert!(rx.try_recv().is_err());

		let state = session.state.lock();
		let voice_state = state.voice_state.as_ref().expect("voice state");
		let participant = voice_state.participants.get(user_id).expect("participant");
		assert!(participant.keyword_pipeline.is_none());
		assert!(participant.vad.is_none());
		assert!(voice_state.has_seen_audio);
		assert_eq!(voice_state.decoded_frame_count, 1);
	}
}

#[cfg(test)]
mod playback_contract_tests {
	use super::*;
	use songbird::input::{
		AudioStreamError,
		codecs::{get_codec_registry, get_probe},
	};
	use songbird::tracks::PlayError;

	#[tokio::test]
	async fn production_decoder_registry_accepts_tts_and_signal_formats() {
		let samples = [
			include_bytes!("../tests/fixtures/tone.ogg").to_vec(),
			include_bytes!("../tests/fixtures/tone.mp3").to_vec(),
			wrap_discord_pcm_as_wav(&vec![0; 48_000 / 10 * 2 * 2]),
		];
		for audio in samples {
			let input: Input = audio.into();
			let parsed = input
				.make_playable_async(get_codec_registry(), get_probe())
				.await
				.expect("the playback driver's registry must decode all formats we send");
			assert!(parsed.is_playable());
		}
	}

	#[tokio::test]
	async fn invalid_audio_is_rejected_by_the_playback_decoder() {
		let input: Input = b"not an audio container".to_vec().into();
		assert!(
			input
				.make_playable_async(get_codec_registry(), get_probe())
				.await
				.is_err()
		);
	}

	#[test]
	fn only_natural_track_completion_reports_success() {
		assert!(playback_outcome(&PlayMode::End).unwrap());
		assert!(!playback_outcome(&PlayMode::Stop).unwrap());
		let failed = PlayMode::Errored(PlayError::Create(Arc::new(AudioStreamError::Unsupported)));
		assert!(playback_outcome(&failed).is_err());
	}
}
