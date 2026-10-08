use std::collections::VecDeque;

use crate::audio::{INPUT_BITS_PER_SAMPLE, INPUT_CHANNELS, INPUT_SAMPLE_RATE, wrap_pcm_as_wav};

const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (INPUT_SAMPLE_RATE as usize * FRAME_MS as usize) / 1000;
const DEFAULT_WINDOW_SECONDS: u32 = 20;
const MAX_PENDING_OUTPUT_TICKS: usize = 10;
const LIMITER_CEILING: f64 = i16::MAX as f64 * 0.95;
const LIMITER_RELEASE_MS: f64 = 250.0;

pub struct ReplaySnapshot {
	pub wav: Vec<u8>,
	pub duration_ms: u64,
	pub source_hard_clipped_samples: u64,
	pub source_packet_loss_frames: u64,
	pub mix_overload_samples: u64,
	pub limited_frames: u64,
	pub peak_before_limit: i64,
}

struct ReplayFrame {
	samples: Vec<i32>,
	source_hard_clipped_samples: u32,
	source_packet_loss_frames: u32,
	mix_overload_samples: u32,
}

struct PendingOutputFrame {
	samples: Vec<i32>,
	source_hard_clipped_samples: u32,
}

#[derive(Default)]
pub struct ReplayBuffer {
	enabled: bool,
	window_seconds: u32,
	frames: VecDeque<ReplayFrame>,
	pending_output_frames: VecDeque<PendingOutputFrame>,
}

impl ReplayBuffer {
	pub fn configure(&mut self, enabled: bool, window_seconds: u32) {
		self.enabled = enabled;
		self.window_seconds = normalize_window_seconds(window_seconds);
		if !enabled {
			self.frames.clear();
			self.pending_output_frames.clear();
			return;
		}
		self.trim();
	}

	pub fn clear(&mut self) {
		self.frames.clear();
		self.pending_output_frames.clear();
	}

	pub fn push_tick<'a>(
		&mut self,
		participant_frames: impl IntoIterator<Item = (&'a [i16], bool)>,
	) {
		if !self.enabled {
			return;
		}

		// Keep the rolling mix wider than the final PCM format. Clamping each source as it is
		// added permanently destroys the waveform and can turn otherwise intelligible speech
		// into harsh static when voices or playback overlap.
		let mut mixed = vec![0_i32; FRAME_SAMPLES];
		let mut source_hard_clipped_samples = 0_u32;
		let mut source_packet_loss_frames = 0_u32;
		for (frame, packet_lost) in participant_frames {
			source_packet_loss_frames += u32::from(packet_lost);
			for (index, sample) in frame.iter().take(FRAME_SAMPLES).enumerate() {
				if *sample == i16::MIN || *sample == i16::MAX {
					source_hard_clipped_samples += 1;
				}
				mixed[index] = mixed[index].saturating_add(*sample as i32);
			}
		}
		if let Some(output_frame) = self.pending_output_frames.pop_front() {
			source_hard_clipped_samples += output_frame.source_hard_clipped_samples;
			mix_frame(&mut mixed, &output_frame.samples);
		}
		let mix_overload_samples = mixed
			.iter()
			.filter(|sample| **sample < i16::MIN as i32 || **sample > i16::MAX as i32)
			.count() as u32;
		self.frames.push_back(ReplayFrame {
			samples: mixed,
			source_hard_clipped_samples,
			source_packet_loss_frames,
			mix_overload_samples,
		});
		self.trim();
	}

	pub fn queue_output_tick(&mut self, samples: &[f32], sample_rate: u32, channels: u16) {
		if !self.enabled {
			return;
		}
		let Some(frame) = downmix_output_tick(samples, sample_rate, channels) else {
			return;
		};
		self.pending_output_frames.push_back(frame);
		while self.pending_output_frames.len() > MAX_PENDING_OUTPUT_TICKS {
			self.pending_output_frames.pop_front();
		}
	}

	pub fn snapshot(&self) -> anyhow::Result<ReplaySnapshot> {
		if !self.enabled {
			anyhow::bail!("voice replay buffer is disabled");
		}
		if self.frames.is_empty() {
			anyhow::bail!("voice replay buffer does not contain audio yet");
		}

		let (rendered_frames, limited_frames, peak_before_limit) =
			render_limited_frames(&self.frames);
		let mut pcm = Vec::with_capacity(rendered_frames.len() * FRAME_SAMPLES * 2);
		for frame in rendered_frames {
			for sample in frame {
				pcm.extend_from_slice(&sample.to_le_bytes());
			}
		}
		let source_hard_clipped_samples = self
			.frames
			.iter()
			.map(|frame| frame.source_hard_clipped_samples as u64)
			.sum();
		let mix_overload_samples = self
			.frames
			.iter()
			.map(|frame| frame.mix_overload_samples as u64)
			.sum();
		let source_packet_loss_frames = self
			.frames
			.iter()
			.map(|frame| frame.source_packet_loss_frames as u64)
			.sum();
		Ok(ReplaySnapshot {
			wav: wrap_pcm_as_wav(
				&pcm,
				INPUT_SAMPLE_RATE,
				INPUT_CHANNELS,
				INPUT_BITS_PER_SAMPLE,
			),
			duration_ms: self.frames.len() as u64 * FRAME_MS as u64,
			source_hard_clipped_samples,
			source_packet_loss_frames,
			mix_overload_samples,
			limited_frames,
			peak_before_limit,
		})
	}

	fn trim(&mut self) {
		let max_frames = (self.window_seconds * 1000 / FRAME_MS) as usize;
		while self.frames.len() > max_frames {
			self.frames.pop_front();
		}
	}
}

fn mix_frame(target: &mut [i32], source: &[i32]) {
	for (target_sample, source_sample) in target.iter_mut().zip(source) {
		*target_sample = target_sample.saturating_add(*source_sample);
	}
}

fn render_limited_frames(frames: &VecDeque<ReplayFrame>) -> (Vec<Vec<i16>>, u64, i64) {
	let peaks = frames
		.iter()
		.map(|frame| {
			frame
				.samples
				.iter()
				.map(|sample| (*sample as i64).abs())
				.max()
				.unwrap_or(0)
		})
		.collect::<Vec<_>>();
	let peak_before_limit = peaks.iter().copied().max().unwrap_or(0);
	let required_gains = peaks
		.iter()
		.map(|peak| {
			if *peak == 0 {
				1.0
			} else {
				(LIMITER_CEILING / *peak as f64).min(1.0)
			}
		})
		.collect::<Vec<_>>();

	// A frame of lookahead starts attenuation before an overloaded frame. Release is gradual so
	// the mix does not pump as speakers and Bumblebee alternate.
	let release = 1.0 - (-(FRAME_MS as f64) / LIMITER_RELEASE_MS).exp();
	let mut gain = 1.0_f64;
	let mut limited_frames = 0_u64;
	let rendered = frames
		.iter()
		.enumerate()
		.map(|(index, frame)| {
			let target =
				required_gains[index].min(required_gains.get(index + 1).copied().unwrap_or(1.0));
			if target < gain {
				gain = target;
			} else {
				gain += (target - gain) * release;
			}
			if gain < 0.999_999 {
				limited_frames += 1;
			}
			frame
				.samples
				.iter()
				.map(|sample| {
					(*sample as f64 * gain)
						.round()
						.clamp(i16::MIN as f64, i16::MAX as f64) as i16
				})
				.collect()
		})
		.collect();
	(rendered, limited_frames, peak_before_limit)
}

fn downmix_output_tick(
	samples: &[f32],
	sample_rate: u32,
	channels: u16,
) -> Option<PendingOutputFrame> {
	if samples.is_empty() || sample_rate == 0 || channels == 0 {
		return None;
	}
	let channels = channels as usize;
	let input_frames = samples.len() / channels;
	if input_frames == 0 {
		return None;
	}

	let mut output = vec![0_i32; FRAME_SAMPLES];
	let mut source_hard_clipped_samples = 0_u32;
	for (output_index, output_sample) in output.iter_mut().enumerate() {
		let start = output_index * sample_rate as usize / INPUT_SAMPLE_RATE as usize;
		let end = ((output_index + 1) * sample_rate as usize / INPUT_SAMPLE_RATE as usize)
			.max(start + 1)
			.min(input_frames);
		if start >= input_frames || start >= end {
			break;
		}

		let mut sum = 0.0_f64;
		let mut count = 0_usize;
		for input_frame in start..end {
			for channel in 0..channels {
				sum += samples[input_frame * channels + channel] as f64;
				count += 1;
			}
		}
		let normalized = sum / count as f64;
		if !(-1.0..=1.0).contains(&normalized) {
			source_hard_clipped_samples += 1;
		}
		*output_sample = (normalized * i16::MAX as f64)
			.round()
			.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
	}
	Some(PendingOutputFrame {
		samples: output,
		source_hard_clipped_samples,
	})
}

fn normalize_window_seconds(value: u32) -> u32 {
	match value {
		20 | 30 | 45 | 60 => value,
		_ => DEFAULT_WINDOW_SECONDS,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn keeps_only_the_configured_rolling_window() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		for value in 0..1005 {
			let frame = vec![value as i16; FRAME_SAMPLES];
			buffer.push_tick([(frame.as_slice(), false)]);
		}

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.duration_ms, 20_000);
		assert_eq!(snapshot.wav.len(), 44 + 1000 * FRAME_SAMPLES * 2);
		assert_eq!(i16::from_le_bytes([snapshot.wav[44], snapshot.wav[45]]), 5);
	}

	#[test]
	fn mixes_participants_and_inserts_silence_for_empty_ticks() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let first = vec![20_000_i16; FRAME_SAMPLES];
		let second = vec![20_000_i16; FRAME_SAMPLES];
		buffer.push_tick([(first.as_slice(), false), (second.as_slice(), false)]);
		buffer.push_tick(std::iter::empty());

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.duration_ms, 40);
		assert_eq!(
			i16::from_le_bytes([snapshot.wav[44], snapshot.wav[45]]),
			LIMITER_CEILING.round() as i16
		);
		assert_eq!(snapshot.mix_overload_samples, FRAME_SAMPLES as u64);
		assert_eq!(snapshot.source_hard_clipped_samples, 0);
		assert_eq!(snapshot.limited_frames, 2);
		let silence_offset = 44 + FRAME_SAMPLES * 2;
		assert_eq!(snapshot.wav[silence_offset..silence_offset + 2], [0, 0]);
	}

	#[test]
	fn preserves_a_clean_single_participant_without_gain_changes() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let frame = vec![30_000_i16; FRAME_SAMPLES];
		buffer.push_tick([(frame.as_slice(), false)]);

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(
			i16::from_le_bytes([snapshot.wav[44], snapshot.wav[45]]),
			30_000
		);
		assert_eq!(snapshot.mix_overload_samples, 0);
		assert_eq!(snapshot.limited_frames, 0);
	}

	#[test]
	fn does_not_destroy_audio_that_cancels_after_intermediate_sources() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let positive = vec![30_000_i16; FRAME_SAMPLES];
		let negative = vec![-30_000_i16; FRAME_SAMPLES];
		buffer.push_tick([
			(positive.as_slice(), false),
			(positive.as_slice(), false),
			(negative.as_slice(), false),
		]);

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(
			i16::from_le_bytes([snapshot.wav[44], snapshot.wav[45]]),
			30_000
		);
		assert_eq!(snapshot.mix_overload_samples, 0);
		assert_eq!(snapshot.limited_frames, 0);
	}

	#[test]
	fn reports_source_audio_that_arrived_already_hard_clipped() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let mut frame = vec![0_i16; FRAME_SAMPLES];
		frame[0] = i16::MIN;
		frame[1] = i16::MAX;
		buffer.push_tick([(frame.as_slice(), false)]);

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.source_hard_clipped_samples, 2);
		assert_eq!(snapshot.mix_overload_samples, 0);
	}

	#[test]
	fn reports_packet_loss_concealment_frames_in_the_saved_window() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let frame = vec![100_i16; FRAME_SAMPLES];
		buffer.push_tick([(frame.as_slice(), true)]);

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.source_packet_loss_frames, 1);
	}

	#[test]
	fn mixes_outgoing_playback_into_the_next_channel_tick() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 20);
		let output = vec![0.5_f32; 48_000 / 50 * 2];
		buffer.queue_output_tick(&output, 48_000, 2);
		let participant = vec![1_000_i16; FRAME_SAMPLES];
		buffer.push_tick([(participant.as_slice(), false)]);

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.duration_ms, 20);
		assert_eq!(
			i16::from_le_bytes([snapshot.wav[44], snapshot.wav[45]]),
			17_384
		);
	}

	#[test]
	fn outgoing_playback_never_enters_a_disabled_buffer() {
		let mut buffer = ReplayBuffer::default();
		let output = vec![0.5_f32; 48_000 / 50 * 2];
		buffer.queue_output_tick(&output, 48_000, 2);
		buffer.configure(true, 20);
		buffer.push_tick(std::iter::empty());

		let snapshot = buffer.snapshot().expect("snapshot replay");
		assert_eq!(snapshot.wav[44..46], [0, 0]);
	}

	#[test]
	fn disabling_clears_captured_audio() {
		let mut buffer = ReplayBuffer::default();
		buffer.configure(true, 60);
		let frame = vec![100_i16; FRAME_SAMPLES];
		buffer.push_tick([(frame.as_slice(), false)]);
		buffer.configure(false, 60);
		assert!(buffer.snapshot().is_err());
	}
}
