use std::collections::VecDeque;

use anyhow::{Context, Result, bail};
use serde_json::json;
use tracing::info;

use crate::{
	logging,
	native::azure_speech::{KeywordPollStatus, KeywordRecognizer},
	protocol::{VoiceKeywordSensitivity, VoiceWakeWord},
};

pub const SAMPLE_RATE: usize = 16_000;
pub const BYTES_PER_SAMPLE: usize = 2;
pub const ROLLING_BUFFER_MS: usize = 3_000;
pub const OPEN_LISTEN_PREROLL_MS: usize = 300;
const CANCEL_KEYWORD_COOLDOWN_MS: u64 = 1_000;
const KEYWORD_RECOGNIZER_PRIME_MS: usize = 1_000;
const KEYWORD_RECOGNIZER_FIRST_AUDIO_REPRIME_MS: usize = 400;
const KEYWORD_RECOGNIZER_REPRIME_IDLE_MS: u64 = 5_000;
const KEYWORD_RECOGNIZER_REPRIME_MS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
	Wake,
	Cancel,
	Stop,
}

impl KeywordKind {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Wake => "wake",
			Self::Cancel => "cancel",
			Self::Stop => "stop",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeywordModelSettings {
	pub wake_word: VoiceWakeWord,
	pub wake_sensitivity: VoiceKeywordSensitivity,
	pub stop_sensitivity: VoiceKeywordSensitivity,
	pub cancel_sensitivity: VoiceKeywordSensitivity,
}

impl Default for KeywordModelSettings {
	fn default() -> Self {
		Self {
			wake_word: VoiceWakeWord::HeyBumblebee,
			wake_sensitivity: VoiceKeywordSensitivity::Balanced,
			stop_sensitivity: VoiceKeywordSensitivity::Balanced,
			cancel_sensitivity: VoiceKeywordSensitivity::Balanced,
		}
	}
}

impl KeywordModelSettings {
	pub fn model_file_for(self, kind: KeywordKind) -> String {
		let sensitivity = match kind {
			KeywordKind::Wake => self.wake_sensitivity,
			KeywordKind::Cancel => self.cancel_sensitivity,
			KeywordKind::Stop => self.stop_sensitivity,
		};
		let (dir, suffix) = sensitivity_model_parts(sensitivity);
		let keyword = match kind {
			KeywordKind::Wake => match self.wake_word {
				VoiceWakeWord::Bumblebee => "bumblebee",
				VoiceWakeWord::HeyBumblebee => "hey_bumblebee",
			},
			KeywordKind::Cancel => "cancel",
			KeywordKind::Stop => "stop",
		};
		format!("{dir}/{keyword}_{suffix}.table")
	}
}

#[derive(Debug, Clone)]
pub struct KeywordHit {
	pub keyword_text: String,
	pub keyword_kind: KeywordKind,
	pub model_name: String,
	pub verification_pcm: Vec<u8>,
}

struct ActiveRecognizer {
	kind: KeywordKind,
	model_name: String,
	recognizer: KeywordRecognizer,
}

pub struct KeywordPipeline {
	#[cfg(test)]
	drop_probe: Option<Box<dyn FnOnce() + Send>>,
	label: String,
	settings: KeywordModelSettings,
	active_recognizers: Vec<ActiveRecognizer>,
	rolling_buffers: VecDeque<Vec<u8>>,
	rolling_bytes: usize,
	wake_enabled: bool,
	cancel_enabled: bool,
	last_cancel_keyword_at_ms: u64,
	first_audio_at_ms: Option<u64>,
	last_audio_at_ms: Option<u64>,
	created_at_ms: u64,
}

impl KeywordPipeline {
	pub fn new(label: impl Into<String>, settings: KeywordModelSettings) -> Result<Self> {
		let label = label.into();
		let created_at_ms = now_ms();
		let keyword_models_dir = crate::resources::configured()?.keyword_models.clone();

		let mut active_recognizers = Vec::new();
		for kind in [KeywordKind::Wake, KeywordKind::Cancel, KeywordKind::Stop] {
			let model_name = settings.model_file_for(kind);
			let model_path = keyword_models_dir.join(&model_name);
			if !model_path.exists() {
				if kind == KeywordKind::Wake {
					bail!("wake keyword model missing: {}", model_path.display());
				}
				continue;
			}
			let recognizer = KeywordRecognizer::new(&model_path)
				.with_context(|| format!("create keyword recognizer {kind:?} for {}", label))?;
			active_recognizers.push(ActiveRecognizer {
				kind,
				model_name,
				recognizer,
			});
		}

		let mut pipeline = Self {
			#[cfg(test)]
			drop_probe: None,
			label,
			settings,
			active_recognizers,
			rolling_buffers: VecDeque::new(),
			rolling_bytes: 0,
			wake_enabled: true,
			cancel_enabled: false,
			last_cancel_keyword_at_ms: 0,
			first_audio_at_ms: None,
			last_audio_at_ms: None,
			created_at_ms,
		};

		let prime_bytes = pcm_bytes_for_ms(KEYWORD_RECOGNIZER_PRIME_MS);
		if prime_bytes > 0 {
			let silence = vec![0_u8; prime_bytes];
			pipeline.write_to_all_recognizers(&silence)?;
		}
		logging::log_event(
			tracing::Level::INFO,
			"songbird.keyword.recognizers.ready",
			"keyword recognizers ready",
			json!({
				  "data": {
						 "label": pipeline.label.clone(),
						 "recognizerKinds": pipeline
								 .active_recognizers
								 .iter()
								 .map(|it| it.kind.as_str())
								 .collect::<Vec<_>>(),
						 "primeMs": KEYWORD_RECOGNIZER_PRIME_MS,
						 "createdAtMs": pipeline.created_at_ms
				  }
			}),
		);
		Ok(pipeline)
	}

	pub fn set_wake_enabled(&mut self, enabled: bool) {
		self.wake_enabled = enabled;
	}

	pub fn settings(&self) -> KeywordModelSettings {
		self.settings
	}

	pub fn recognizer_count(&self) -> usize {
		self.active_recognizers.len()
	}

	pub fn set_cancel_enabled(&mut self, enabled: bool) {
		self.cancel_enabled = enabled;
	}

	pub fn clear_pre_roll(&mut self) {
		self.rolling_buffers.clear();
		self.rolling_bytes = 0;
	}

	pub fn build_rolling_tail(&self, max_bytes: usize) -> Vec<u8> {
		if max_bytes == 0 || self.rolling_bytes == 0 {
			return Vec::new();
		}
		if self.rolling_bytes <= max_bytes {
			let mut out = Vec::with_capacity(self.rolling_bytes);
			for chunk in &self.rolling_buffers {
				out.extend_from_slice(chunk);
			}
			return out;
		}

		let mut selected: Vec<&[u8]> = Vec::new();
		let mut selected_bytes = 0_usize;
		for chunk in self.rolling_buffers.iter().rev() {
			if chunk.is_empty() {
				continue;
			}
			let remaining = max_bytes.saturating_sub(selected_bytes);
			if remaining == 0 {
				break;
			}
			if chunk.len() <= remaining {
				selected.push(chunk);
				selected_bytes += chunk.len();
			} else {
				selected.push(&chunk[chunk.len() - remaining..]);
				selected_bytes += remaining;
				break;
			}
		}

		let mut out = Vec::with_capacity(selected_bytes);
		for chunk in selected.iter().rev() {
			out.extend_from_slice(chunk);
		}
		out
	}

	pub fn open_listen_preroll(&self) -> Vec<u8> {
		self.build_rolling_tail(pcm_bytes_for_ms(OPEN_LISTEN_PREROLL_MS))
	}

	pub fn push_audio(&mut self, pcm: &[u8]) -> Result<Vec<KeywordHit>> {
		if pcm.is_empty() {
			return Ok(Vec::new());
		}

		if self.first_audio_at_ms.is_none() {
			let first_audio_at_ms = now_ms();
			self.first_audio_at_ms = Some(first_audio_at_ms);
			let reprime_bytes = pcm_bytes_for_ms(KEYWORD_RECOGNIZER_FIRST_AUDIO_REPRIME_MS);
			if reprime_bytes > 0 {
				info!(
					  label = %self.label,
					  ms_since_recognizer_ready = first_audio_at_ms.saturating_sub(self.created_at_ms),
					  reprime_ms = KEYWORD_RECOGNIZER_FIRST_AUDIO_REPRIME_MS,
					  frame_bytes = pcm.len(),
					  "re-priming keyword recognizers before first live audio frame"
				);
				self.write_to_all_recognizers(&vec![0_u8; reprime_bytes])?;
			}
			info!(
				  label = %self.label,
				  ms_since_recognizer_ready = first_audio_at_ms.saturating_sub(self.created_at_ms),
				  frame_bytes = pcm.len(),
				  "first keyword audio frame received"
			);
		}

		let now = now_ms();
		if let Some(last_audio_at_ms) = self.last_audio_at_ms {
			let idle_ms = now.saturating_sub(last_audio_at_ms);
			if idle_ms >= KEYWORD_RECOGNIZER_REPRIME_IDLE_MS {
				let reprime_bytes = pcm_bytes_for_ms(KEYWORD_RECOGNIZER_REPRIME_MS);
				if reprime_bytes > 0 {
					info!(
						  label = %self.label,
						  idle_ms,
						  reprime_ms = KEYWORD_RECOGNIZER_REPRIME_MS,
						  "re-priming keyword recognizers after idle gap"
					);
					self.write_to_all_recognizers(&vec![0_u8; reprime_bytes])?;
				}
			}
		}
		self.last_audio_at_ms = Some(now);

		self.append_rolling_audio(pcm);
		self.write_to_recognizers(pcm)?;

		let mut hits = Vec::new();
		for recognizer in &self.active_recognizers {
			if recognizer.kind == KeywordKind::Wake && !self.wake_enabled {
				continue;
			}
			if recognizer.kind != KeywordKind::Wake && !self.cancel_enabled {
				continue;
			}
			let poll = recognizer.recognizer.poll(0).with_context(|| {
				format!(
					"poll keyword recognizer {} kind={}",
					self.label,
					recognizer.kind.as_str()
				)
			})?;
			if poll.status != KeywordPollStatus::Ready {
				continue;
			}
			let keyword_text = poll.text.trim().to_string();
			if keyword_text.is_empty() {
				continue;
			}
			if recognizer.kind != KeywordKind::Wake {
				let now = now_ms();
				if now.saturating_sub(self.last_cancel_keyword_at_ms) < CANCEL_KEYWORD_COOLDOWN_MS {
					continue;
				}
				self.last_cancel_keyword_at_ms = now;
			}
			hits.push(KeywordHit {
				keyword_text,
				keyword_kind: recognizer.kind,
				model_name: recognizer.model_name.clone(),
				verification_pcm: self.build_rolling_tail(pcm_bytes_for_ms(ROLLING_BUFFER_MS)),
			});
		}
		Ok(hits)
	}

	fn append_rolling_audio(&mut self, pcm: &[u8]) {
		let rolling_max_bytes = pcm_bytes_for_ms(ROLLING_BUFFER_MS);
		self.rolling_buffers.push_back(pcm.to_vec());
		self.rolling_bytes += pcm.len();
		while self.rolling_bytes > rolling_max_bytes {
			let Some(removed) = self.rolling_buffers.pop_front() else {
				break;
			};
			self.rolling_bytes = self.rolling_bytes.saturating_sub(removed.len());
		}
	}

	fn write_to_recognizers(&mut self, pcm: &[u8]) -> Result<()> {
		for recognizer in &self.active_recognizers {
			if recognizer.kind == KeywordKind::Wake && !self.wake_enabled {
				continue;
			}
			if recognizer.kind != KeywordKind::Wake && !self.cancel_enabled {
				continue;
			}
			recognizer.recognizer.write(pcm).with_context(|| {
				format!(
					"write keyword stream {} kind={}",
					self.label,
					recognizer.kind.as_str()
				)
			})?;
		}
		Ok(())
	}

	fn write_to_all_recognizers(&mut self, pcm: &[u8]) -> Result<()> {
		for recognizer in &self.active_recognizers {
			recognizer.recognizer.write(pcm).with_context(|| {
				format!(
					"write keyword stream {} kind={}",
					self.label,
					recognizer.kind.as_str()
				)
			})?;
		}
		Ok(())
	}
}

#[cfg(test)]
impl KeywordPipeline {
	pub(crate) fn with_drop_probe(probe: impl FnOnce() + Send + 'static) -> Self {
		Self {
			drop_probe: Some(Box::new(probe)),
			label: "drop-regression".to_string(),
			settings: KeywordModelSettings::default(),
			active_recognizers: Vec::new(),
			rolling_buffers: VecDeque::new(),
			rolling_bytes: 0,
			wake_enabled: true,
			cancel_enabled: false,
			last_cancel_keyword_at_ms: 0,
			first_audio_at_ms: None,
			last_audio_at_ms: None,
			created_at_ms: 0,
		}
	}
}

#[cfg(test)]
impl Drop for KeywordPipeline {
	fn drop(&mut self) {
		if let Some(probe) = self.drop_probe.take() {
			probe();
		}
	}
}

fn sensitivity_model_parts(sensitivity: VoiceKeywordSensitivity) -> (&'static str, &'static str) {
	match sensitivity {
		VoiceKeywordSensitivity::Strict => ("low-false-accepts", "lowfa"),
		VoiceKeywordSensitivity::Balanced => ("default", "default"),
		VoiceKeywordSensitivity::Loose => ("high-false-accepts", "highfa"),
	}
}

fn pcm_bytes_for_ms(duration_ms: usize) -> usize {
	((duration_ms as u64 * SAMPLE_RATE as u64 * BYTES_PER_SAMPLE as u64) / 1000) as usize
}

fn now_ms() -> u64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as u64
}
