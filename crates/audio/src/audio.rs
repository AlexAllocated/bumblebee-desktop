pub const DISCORD_SAMPLE_RATE: u32 = 48_000;
pub const DISCORD_CHANNELS: u16 = 2;
pub const DISCORD_BITS_PER_SAMPLE: u16 = 16;
pub const INPUT_SAMPLE_RATE: u32 = 16_000;
pub const INPUT_CHANNELS: u16 = 1;
pub const INPUT_BITS_PER_SAMPLE: u16 = 16;
pub const FRAME_MS: u32 = 20;
pub const PLAYBACK_PADDING_MS: u32 = 80;

pub fn wrap_pcm_as_wav(pcm: &[u8], sample_rate: u32, channels: u16, bit_depth: u16) -> Vec<u8> {
	let bytes_per_sample = bit_depth / 8;
	let byte_rate = sample_rate * channels as u32 * bytes_per_sample as u32;
	let block_align = channels * bytes_per_sample;
	let data_size = pcm.len() as u32;
	let total_size = 44 + pcm.len();

	let mut out = vec![0_u8; total_size];
	out[0..4].copy_from_slice(b"RIFF");
	out[4..8].copy_from_slice(&(total_size as u32 - 8).to_le_bytes());
	out[8..12].copy_from_slice(b"WAVE");
	out[12..16].copy_from_slice(b"fmt ");
	out[16..20].copy_from_slice(&16_u32.to_le_bytes());
	out[20..22].copy_from_slice(&1_u16.to_le_bytes());
	out[22..24].copy_from_slice(&channels.to_le_bytes());
	out[24..28].copy_from_slice(&sample_rate.to_le_bytes());
	out[28..32].copy_from_slice(&byte_rate.to_le_bytes());
	out[32..34].copy_from_slice(&block_align.to_le_bytes());
	out[34..36].copy_from_slice(&bit_depth.to_le_bytes());
	out[36..40].copy_from_slice(b"data");
	out[40..44].copy_from_slice(&data_size.to_le_bytes());
	out[44..].copy_from_slice(pcm);
	out
}

pub fn wrap_discord_pcm_as_wav(pcm: &[u8]) -> Vec<u8> {
	wrap_pcm_as_wav(
		pcm,
		DISCORD_SAMPLE_RATE,
		DISCORD_CHANNELS,
		DISCORD_BITS_PER_SAMPLE,
	)
}

pub fn with_pcm_padding(
	pcm: &[u8],
	sample_rate: u32,
	channels: u16,
	bit_depth: u16,
	leading_ms: u32,
	trailing_ms: u32,
) -> Vec<u8> {
	let bytes_per_sample = (bit_depth / 8) as usize;
	let channels = channels as usize;
	let leading = silence_pcm_bytes(sample_rate, channels as u16, bit_depth, leading_ms);
	let trailing = silence_pcm_bytes(sample_rate, channels as u16, bit_depth, trailing_ms);
	let mut out = Vec::with_capacity(leading.len() + pcm.len() + trailing.len());
	out.extend_from_slice(&leading);
	out.extend_from_slice(pcm);
	out.extend_from_slice(&trailing);
	if bytes_per_sample == 0 || channels == 0 {
		return pcm.to_vec();
	}
	out
}

pub fn silence_pcm_bytes(
	sample_rate: u32,
	channels: u16,
	bit_depth: u16,
	duration_ms: u32,
) -> Vec<u8> {
	let bytes_per_sample = (bit_depth / 8) as usize;
	let sample_count = ((duration_ms as u64 * sample_rate as u64) / 1000) as usize;
	vec![0_u8; sample_count * channels as usize * bytes_per_sample]
}

pub fn pcm_i16_to_le_bytes(samples: &[i16]) -> Vec<u8> {
	let mut out = Vec::with_capacity(samples.len() * 2);
	for sample in samples {
		out.extend_from_slice(&sample.to_le_bytes());
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn wrap_pcm_as_wav_writes_expected_header_fields() {
		let pcm = [0x01_u8, 0x02, 0x03, 0x04];
		let wav = wrap_pcm_as_wav(&pcm, 16_000, 1, 16);

		assert_eq!(&wav[0..4], b"RIFF");
		assert_eq!(&wav[8..12], b"WAVE");
		assert_eq!(&wav[12..16], b"fmt ");
		assert_eq!(&wav[36..40], b"data");
		assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
		assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
		assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
		assert_eq!(
			u32::from_le_bytes(wav[40..44].try_into().unwrap()),
			pcm.len() as u32
		);
		assert_eq!(&wav[44..], &pcm);
	}

	#[test]
	fn wrap_discord_pcm_as_wav_uses_playback_audio_format() {
		let pcm = [0_u8; 192];
		let wav = wrap_discord_pcm_as_wav(&pcm);

		assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 48_000);
		assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
		assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
		assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 192_000);
		assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 4);
	}

	#[test]
	fn with_pcm_padding_adds_requested_leading_and_trailing_silence() {
		let pcm = [0x11_u8, 0x22, 0x33, 0x44];
		let padded = with_pcm_padding(&pcm, 1_000, 1, 16, 2, 3);

		assert_eq!(padded.len(), 4 + pcm.len() + 6);
		assert_eq!(&padded[0..4], &[0, 0, 0, 0]);
		assert_eq!(&padded[4..8], &pcm);
		assert_eq!(&padded[8..], &[0, 0, 0, 0, 0, 0]);
	}

	#[test]
	fn pcm_i16_to_le_bytes_preserves_sample_order() {
		let bytes = pcm_i16_to_le_bytes(&[0x1234, -2]);
		assert_eq!(bytes, vec![0x34, 0x12, 0xfe, 0xff]);
	}
}
