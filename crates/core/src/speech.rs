use crate::{
	cache::{CachedSpeech, SpeechCache},
	model::{Voice, WordTiming},
	now_ms,
	providers::Providers,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub struct Speech {
	cache: Arc<SpeechCache>,
	media: PathBuf,
	media_publication: Arc<tokio::sync::Mutex<()>>,
	providers: Arc<Providers>,
}
pub struct PreparedSpeech {
	pub wav: Vec<u8>,
	pub words: Vec<WordTiming>,
	pub filename: String,
	pub duration_ms: u64,
}
impl Speech {
	pub fn new(providers: Arc<Providers>, data_dir: PathBuf) -> Result<Self> {
		let media = data_dir.join("media");
		std::fs::create_dir_all(&media)?;
		let cache = Arc::new(SpeechCache::open(
			&data_dir.join("speech-cache"),
			256 * 1024 * 1024,
		)?);
		Ok(Self {
			cache,
			media,
			media_publication: Arc::new(tokio::sync::Mutex::new(())),
			providers,
		})
	}
	pub async fn prepare(
		&self,
		text: &str,
		voice: &Voice,
		cancel: CancellationToken,
	) -> Result<PreparedSpeech> {
		ensure!(
			!text.trim().is_empty() && text.chars().count() <= 6000,
			"Speech must contain 1–6000 characters"
		);
		ensure!(
			voice.provider == "azure_speech",
			"This voice requires an unsupported provider"
		);
		let region = self.providers.store.settings()?.azure_region;
		let ssml = build_ssml(text, voice)?;
		let key = format!(
			"{:x}",
			Sha256::digest(serde_json::to_vec(&(
				1,
				"riff-48khz-16bit-mono-pcm",
				&region,
				&ssml
			))?)
		);
		let cache = self.cache.clone();
		let lookup = key.clone();
		let cached = tokio::task::spawn_blocking(move || cache.get(&lookup, now_ms())).await??;
		ensure!(!cancel.is_cancelled(), "Speech cancelled");
		let speech = match cached {
			Some(cached) => cached,
			None => {
				let generated = bumblebee_audio::speech::synthesize(
					self.providers.secret("azure_speech")?,
					region,
					ssml,
					cancel.clone(),
				)
				.await?;
				ensure!(!cancel.is_cancelled(), "Speech cancelled");
				let speech = CachedSpeech {
					audio: generated.wav,
					words: generated
						.words
						.into_iter()
						.map(|w| WordTiming {
							text: w.text,
							start_ms: w.start_ms,
							duration_ms: w.duration_ms,
						})
						.collect(),
					content_type: "audio/wav".into(),
				};
				let cache = self.cache.clone();
				let saving = key.clone();
				let data = speech.clone();
				tokio::task::spawn_blocking(move || cache.put(&saving, &data, now_ms())).await??;
				speech
			}
		};
		let duration_ms = wav_duration_ms(&speech.audio)?;
		let filename = format!("{key}.wav");
		// A second preparation must not prune a reused file between its existence
		// check and renewal for the overlay's next HTTP request.
		let publication = self.media_publication.clone().lock_owned().await;
		// HTTP media is disposable, independent of the bounded speech cache. Keep only
		// recent files so OBS can fetch a completed clip without indefinite disk growth.
		let media = self.media.clone();
		let current = filename.clone();
		let audio = speech.audio.clone();
		tokio::task::spawn_blocking(move || {
			// Keep ownership inside the task: cancelling prepare cannot release the
			// lock while its non-cancellable filesystem work is still running.
			let _publication = publication;
			publish_media(&media, &current, &audio)
		})
		.await??;
		Ok(PreparedSpeech {
			wav: speech.audio,
			words: speech.words,
			filename,
			duration_ms,
		})
	}
}
fn publish_media(dir: &std::path::Path, current: &str, audio: &[u8]) -> Result<()> {
	let path = dir.join(current);
	if !path.is_file() {
		let temporary = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
		std::fs::write(&temporary, audio)?;
		if let Err(error) = std::fs::rename(&temporary, &path) {
			let _ = std::fs::remove_file(&temporary);
			if !path.is_file() {
				return Err(error.into());
			}
		}
	}
	prune_media(dir, current)
}
fn prune_media(dir: &std::path::Path, current: &str) -> Result<()> {
	// Cache hits reuse the same immutable WAV. Renew its media lifetime as well
	// as its cache lifetime, or the next clip could remove it before OBS fetches it.
	std::fs::File::options()
		.write(true)
		.open(dir.join(current))?
		.set_modified(std::time::SystemTime::now())?;
	let mut total = 0u64;
	let mut entries = Vec::new();
	for entry in std::fs::read_dir(dir)? {
		let entry = entry?;
		let metadata = match entry.metadata() {
			Ok(metadata) => metadata,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
			Err(error) => return Err(error.into()),
		};
		if !metadata.is_file() {
			continue;
		}
		total += metadata.len();
		entries.push((entry.path(), metadata.modified()?, metadata.len()));
	}
	entries.sort_by_key(|e| e.1);
	for (path, modified, size) in entries {
		let old = modified.elapsed().unwrap_or_default() > std::time::Duration::from_secs(3600);
		if path.file_name().and_then(|s| s.to_str()) != Some(current)
			&& (old || total > 128 * 1024 * 1024)
		{
			match std::fs::remove_file(path) {
				Ok(()) => {}
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
				Err(error) => return Err(error.into()),
			}
			total = total.saturating_sub(size);
		}
	}
	Ok(())
}
fn escape(text: &str) -> String {
	text
		.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
		.replace('"', "&quot;")
		.replace('\'', "&apos;")
}
pub fn build_ssml(text: &str, voice: &Voice) -> Result<String> {
	ensure!(
		voice
			.voice_name
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || b == b'-'),
		"Invalid catalog voice name"
	);
	let rate: f32 = voice.rate.parse().context("Invalid preset speech rate")?;
	let pitch: f32 = voice.pitch.parse().context("Invalid preset speech pitch")?;
	ensure!(
		(0.5..=2.0).contains(&rate) && (0.5..=2.0).contains(&pitch),
		"Preset speech parameters outside allowed range"
	);
	let prosody = format!(
		"<prosody rate=\"{:+.2}%\" pitch=\"{:+.2}%\">{}</prosody>",
		(rate - 1.0) * 100.0,
		(pitch - 1.0) * 100.0,
		escape(text)
	);
	let styled = if voice.expression == "default" {
		prosody
	} else {
		format!(
			"<mstts:express-as style=\"{}\">{prosody}</mstts:express-as>",
			escape(&voice.expression)
		)
	};
	Ok(format!(
		"<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" xmlns:mstts=\"https://www.w3.org/2001/mstts\" xml:lang=\"en-US\"><voice name=\"{}\">{styled}</voice></speak>",
		voice.voice_name
	))
}
fn wav_duration_ms(bytes: &[u8]) -> Result<u64> {
	ensure!(
		bytes.len() >= 44 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
		"Invalid speech audio"
	);
	let mut offset = 12usize;
	let mut rate = None;
	let mut size = None;
	while offset + 8 <= bytes.len() {
		let n = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
		let begin = offset + 8;
		let end = begin.checked_add(n).context("Invalid WAV chunk")?;
		ensure!(end <= bytes.len(), "Truncated WAV chunk");
		if &bytes[offset..offset + 4] == b"fmt " && n >= 16 {
			rate = Some(u32::from_le_bytes(bytes[begin + 8..begin + 12].try_into()?));
		}
		if &bytes[offset..offset + 4] == b"data" {
			size = Some(n as u64)
		}
		offset = end + (n % 2);
	}
	let rate = rate.filter(|n| *n > 0).context("WAV sample rate missing")?;
	Ok(size.context("WAV samples missing")? * 1000 / rate as u64)
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn cached_wav_reuse_renews_the_overlay_file_before_the_next_clip_prunes() {
		let directory = tempfile::tempdir().unwrap();
		let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3601);
		for name in ["reused.wav", "expired.wav"] {
			let path = directory.path().join(name);
			std::fs::write(&path, b"immutable audio bytes").unwrap();
			std::fs::File::options()
				.write(true)
				.open(path)
				.unwrap()
				.set_modified(old)
				.unwrap();
		}
		publish_media(directory.path(), "reused.wav", b"immutable audio bytes").unwrap();
		publish_media(directory.path(), "next.wav", b"next clip").unwrap();
		assert_eq!(
			std::fs::read(directory.path().join("reused.wav")).unwrap(),
			b"immutable audio bytes"
		);
		assert!(!directory.path().join("expired.wav").exists());
	}
	#[test]
	fn ssml_escapes_untrusted_chat() {
		let voice = crate::catalog::voices()
			.into_iter()
			.find(|v| v.provider == "azure_speech")
			.unwrap();
		let ssml = build_ssml("<voice name='oops'> & hi", &voice).unwrap();
		assert!(ssml.contains("&lt;voice name=&apos;oops&apos;&gt; &amp; hi"));
		assert_eq!(ssml.matches("<voice name=").count(), 1);
	}
}
