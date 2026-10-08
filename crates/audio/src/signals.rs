use std::{collections::HashMap, f32::consts::PI, sync::OnceLock};

use anyhow::Result;

use crate::{
	audio::{
		DISCORD_BITS_PER_SAMPLE, DISCORD_CHANNELS, DISCORD_SAMPLE_RATE, PLAYBACK_PADDING_MS,
		with_pcm_padding, wrap_pcm_as_wav,
	},
	protocol::SongbirdSignalKey,
};

const MAX_I16: f32 = 32767.0;
const MAX_REPEAT_COUNT: u32 = 12;
const MAX_GAIN: f32 = 4.0;

static SIGNAL_CACHE: OnceLock<std::sync::Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

pub fn get_signal_audio(
	signal_key: SongbirdSignalKey,
	gain: Option<f32>,
	repeat_count: Option<u32>,
	variant_key: Option<&str>,
) -> Result<Vec<u8>> {
	let gain = gain.unwrap_or(1.0);
	if !gain.is_finite() {
		anyhow::bail!("signal gain must be finite");
	}
	if !(0.0..=MAX_GAIN).contains(&gain) {
		anyhow::bail!("signal gain must be between 0 and {MAX_GAIN}");
	}
	let repeat_count = repeat_count.unwrap_or(1).max(1);
	if repeat_count > MAX_REPEAT_COUNT {
		anyhow::bail!("signal repeat_count must be at most {MAX_REPEAT_COUNT}");
	}

	// A turn key selects one of three sounds, not a unique cached waveform.
	// Keeping the raw turn ID here would retain a new WAV for every conversation.
	let variant = if signal_key == SongbirdSignalKey::ThinkingLoop {
		variant_key.map(hash_variant).unwrap_or(0) % 3
	} else {
		0
	};
	let cache_key = format!("{signal_key:?}:{gain:.3}:{repeat_count}:{variant}");
	let cache = SIGNAL_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
	if let Some(existing) = cache.lock().unwrap().get(&cache_key).cloned() {
		return Ok(existing);
	}

	let pcm = match signal_key {
		SongbirdSignalKey::WakeChirp => create_listening_chirp(),
		SongbirdSignalKey::HeardChirp => create_heard_chirp(),
		SongbirdSignalKey::TimeoutChirp => create_timeout_chirp(),
		SongbirdSignalKey::CancelChirp => create_cancel_chirp(),
		SongbirdSignalKey::CallWaiting => create_call_waiting_chime(),
		SongbirdSignalKey::ThinkingLoop => create_thinking_loop(variant),
	};

	let mut repeated = Vec::with_capacity(pcm.len() * repeat_count as usize);
	for _ in 0..repeat_count {
		repeated.extend_from_slice(&pcm);
	}
	apply_gain_i16le_in_place(&mut repeated, gain);
	let padded = with_pcm_padding(
		&repeated,
		DISCORD_SAMPLE_RATE,
		DISCORD_CHANNELS,
		DISCORD_BITS_PER_SAMPLE,
		PLAYBACK_PADDING_MS,
		PLAYBACK_PADDING_MS,
	);
	let wav = wrap_pcm_as_wav(
		&padded,
		DISCORD_SAMPLE_RATE,
		DISCORD_CHANNELS,
		DISCORD_BITS_PER_SAMPLE,
	);
	cache.lock().unwrap().insert(cache_key, wav.clone());
	Ok(wav)
}

fn render_pcm(
	duration_ms: u32,
	volume: f32,
	fade_in_ms: u32,
	fade_out_ms: u32,
	sample_at: impl Fn(f32) -> f32,
) -> Vec<u8> {
	let total_samples = ((duration_ms as u64 * DISCORD_SAMPLE_RATE as u64) / 1000).max(1) as usize;
	let fade_in_samples = ((fade_in_ms as u64 * DISCORD_SAMPLE_RATE as u64) / 1000) as usize;
	let fade_out_samples = ((fade_out_ms as u64 * DISCORD_SAMPLE_RATE as u64) / 1000) as usize;
	let mut out = Vec::with_capacity(total_samples * DISCORD_CHANNELS as usize * 2);

	for index in 0..total_samples {
		let t = index as f32 / DISCORD_SAMPLE_RATE as f32;
		let envelope = apply_fade(index, total_samples, fade_in_samples, fade_out_samples);
		let sample = (sample_at(t) * volume * envelope).clamp(-1.0, 1.0);
		let int_sample = (sample * MAX_I16).round() as i16;
		for _ in 0..DISCORD_CHANNELS {
			out.extend_from_slice(&int_sample.to_le_bytes());
		}
	}

	out
}

fn silence_pcm(duration_ms: u32) -> Vec<u8> {
	let total_samples = ((duration_ms as u64 * DISCORD_SAMPLE_RATE as u64) / 1000).max(1) as usize;
	vec![0_u8; total_samples * DISCORD_CHANNELS as usize * 2]
}

fn apply_fade(index: usize, total: usize, fade_in: usize, fade_out: usize) -> f32 {
	if fade_in > 0 && index < fade_in {
		return index as f32 / fade_in as f32;
	}
	if fade_out > 0 && index > total.saturating_sub(fade_out) {
		return (total.saturating_sub(index)) as f32 / fade_out as f32;
	}
	1.0
}

fn create_chirp(duration_ms: u32, start_hz: f32, end_hz: f32, volume: f32) -> Vec<u8> {
	let duration_s = duration_ms as f32 / 1000.0;
	let k = (end_hz - start_hz) / duration_s;
	render_pcm(duration_ms, volume, 4, 16, move |t| {
		let phase = 2.0 * PI * (start_hz * t + 0.5 * k * t * t);
		phase.sin()
	})
}

fn create_tone(duration_ms: u32, hz: f32, volume: f32) -> Vec<u8> {
	render_pcm(duration_ms, volume, 6, 18, move |t| {
		(2.0 * PI * hz * t).sin()
	})
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
	let total = parts.iter().map(Vec::len).sum();
	let mut out = Vec::with_capacity(total);
	for part in parts {
		out.extend_from_slice(part);
	}
	out
}

fn create_listening_chirp() -> Vec<u8> {
	concat(&[
		create_chirp(70, 620.0, 980.0, 0.26),
		silence_pcm(28),
		create_chirp(85, 700.0, 1160.0, 0.26),
	])
}

fn create_heard_chirp() -> Vec<u8> {
	concat(&[
		create_chirp(55, 1120.0, 760.0, 0.22),
		silence_pcm(24),
		create_chirp(72, 960.0, 620.0, 0.22),
	])
}

fn create_timeout_chirp() -> Vec<u8> {
	concat(&[
		create_chirp(90, 980.0, 620.0, 0.40),
		silence_pcm(40),
		create_chirp(110, 820.0, 520.0, 0.40),
	])
}

fn create_cancel_chirp() -> Vec<u8> {
	concat(&[
		create_chirp(65, 740.0, 480.0, 0.34),
		silence_pcm(22),
		create_chirp(90, 620.0, 360.0, 0.28),
	])
}

fn create_call_waiting_chime() -> Vec<u8> {
	concat(&[
		create_tone(70, 523.0, 0.26),
		silence_pcm(45),
		create_tone(85, 659.0, 0.26),
	])
}

fn create_thinking_loop(variant: u32) -> Vec<u8> {
	let variants = [
		[
			(0.05, 480.0, 0.07, 0.55, 30.0, 0.05),
			(0.18, 520.0, 0.06, 0.45, 34.0, 0.04),
			(0.32, 440.0, 0.08, 0.50, 28.0, 0.05),
			(0.46, 550.0, 0.06, 0.42, 36.0, 0.04),
			(0.62, 470.0, 0.07, 0.50, 30.0, 0.05),
			(0.78, 580.0, 0.06, 0.40, 36.0, 0.04),
		],
		[
			(0.07, 500.0, 0.07, 0.50, 32.0, 0.05),
			(0.20, 550.0, 0.06, 0.45, 36.0, 0.04),
			(0.34, 450.0, 0.08, 0.52, 28.0, 0.06),
			(0.50, 530.0, 0.06, 0.42, 36.0, 0.04),
			(0.66, 490.0, 0.07, 0.48, 30.0, 0.05),
			(0.82, 580.0, 0.06, 0.40, 36.0, 0.04),
		],
		[
			(0.04, 520.0, 0.07, 0.50, 32.0, 0.05),
			(0.16, 580.0, 0.06, 0.44, 36.0, 0.04),
			(0.30, 470.0, 0.08, 0.55, 28.0, 0.06),
			(0.44, 560.0, 0.06, 0.42, 36.0, 0.04),
			(0.60, 500.0, 0.07, 0.50, 30.0, 0.05),
			(0.76, 600.0, 0.06, 0.40, 36.0, 0.04),
		],
	];
	let selected = variants[(variant as usize) % variants.len()];
	render_pcm(1000, 0.10, 20, 20, move |t| {
		let mut value = 0.0_f32;
		for &(start, freq, duration, amp, decay, glide) in &selected {
			if t < start || t > start + duration {
				continue;
			}
			let local = t - start;
			let env = (-decay * local).exp();
			let current_freq = freq * (1.0 + glide * (local / duration));
			value += amp * env * (2.0 * PI * current_freq * local).sin();
		}
		value
	})
}

fn apply_gain_i16le_in_place(bytes: &mut [u8], gain: f32) {
	if (gain - 1.0).abs() < f32::EPSILON {
		return;
	}
	for chunk in bytes.chunks_exact_mut(2) {
		let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32;
		let scaled = (sample * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
		chunk.copy_from_slice(&scaled.to_le_bytes());
	}
}

fn hash_variant(value: &str) -> u32 {
	use std::hash::{Hash, Hasher};
	let mut hasher = std::collections::hash_map::DefaultHasher::new();
	value.hash(&mut hasher);
	hasher.finish() as u32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rejects_non_finite_gain() {
		let result = get_signal_audio(SongbirdSignalKey::WakeChirp, Some(f32::NAN), None, None);
		assert!(result.is_err());
	}

	#[test]
	fn rejects_excessive_repeat_count() {
		let result = get_signal_audio(
			SongbirdSignalKey::ThinkingLoop,
			Some(1.0),
			Some(MAX_REPEAT_COUNT + 1),
			None,
		);
		assert!(result.is_err());
	}

	#[test]
	fn allows_capped_repeat_count() {
		let result = get_signal_audio(
			SongbirdSignalKey::CallWaiting,
			Some(1.0),
			Some(MAX_REPEAT_COUNT),
			None,
		);
		assert!(result.is_ok());
	}

	#[test]
	fn thinking_loop_is_soft_but_audible() {
		let pcm = create_thinking_loop(2);
		let peak = pcm
			.chunks_exact(2)
			.map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs())
			.max()
			.unwrap_or_default();

		assert!(peak > 100);
		assert!(peak < 2_000);
	}

	#[test]
	fn many_turn_keys_reuse_only_three_identical_thinking_waveforms() {
		let mut variants = HashMap::new();
		for index in 0..100 {
			let key = format!("conversation-turn-{index}");
			let variant = hash_variant(&key) % 3;
			let wav = get_signal_audio(
				SongbirdSignalKey::ThinkingLoop,
				Some(1.0),
				Some(2),
				Some(&key),
			)
			.unwrap();
			if let Some(previous) = variants.insert(variant, wav.clone()) {
				assert_eq!(previous, wav);
			}
		}
		assert_eq!(variants.len(), 3);
		assert_ne!(variants[&0], variants[&1]);
		assert_ne!(variants[&1], variants[&2]);
		assert_ne!(variants[&0], variants[&2]);
		let cache = SIGNAL_CACHE.get().unwrap().lock().unwrap();
		assert_eq!(
			cache
				.keys()
				.filter(|key| key.starts_with("ThinkingLoop:1.000:2:"))
				.count(),
			3
		);
	}
}
