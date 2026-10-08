//! Opt-in live Azure integration check. Never run automatically in CI.
//! Set BUMBLEBEE_SPEECH_PROBE_KEY in the process environment and pass the
//! prepared native resource directory. This makes one short paid synthesis.
use anyhow::{Context, Result, bail, ensure};
use bumblebee_core::{
	catalog,
	providers::{Providers, SecretStore},
	speech::Speech,
	storage::Store,
};
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

struct ProbeSecret {
	key: String,
	reads: AtomicUsize,
}
impl SecretStore for ProbeSecret {
	fn get(&self, name: &str) -> Result<Option<String>> {
		ensure!(
			name == "azure_speech",
			"Probe requested an unrelated credential"
		);
		self.reads.fetch_add(1, Ordering::SeqCst);
		Ok(Some(self.key.clone()))
	}
	fn set(&self, _: &str, _: &str) -> Result<()> {
		bail!("Probe cannot save credentials")
	}
	fn delete(&self, _: &str) -> Result<()> {
		bail!("Probe cannot delete credentials")
	}
}

#[tokio::main]
async fn main() -> Result<()> {
	let root = std::env::args()
		.nth(1)
		.context("Pass the prepared native resource directory")?;
	let key = std::env::var("BUMBLEBEE_SPEECH_PROBE_KEY")
		.context("Set the probe credential in the process environment")?;
	let resources = bumblebee_audio::NativeResources::from_bundle(root);
	resources.probe()?;
	let data = tempfile::tempdir()?;
	let store = Arc::new(Store::open(&data.path().join("state.sqlite"))?);
	if let Ok(region) = std::env::var("BUMBLEBEE_SPEECH_PROBE_REGION") {
		let mut settings = store.settings()?;
		settings.azure_region = region;
		settings.validate()?;
		store.set("settings", &settings)?;
	}
	let secret = Arc::new(ProbeSecret {
		key,
		reads: AtomicUsize::new(0),
	});
	let speech = Speech::new(
		Providers::new(store, secret.clone())?,
		data.path().to_owned(),
	)?;
	let voice = catalog::voices()
		.into_iter()
		.find(|v| v.id == "bumblebee-buddy")
		.context("Bumblebee's curated voice is missing")?;
	let text = "Hello! I'm Bumblebee. Let's check speech, captions, and a little character.";
	let first = speech
		.prepare(text, &voice, CancellationToken::new())
		.await?;
	ensure!(
		first.wav.len() > 44 && first.duration_ms > 1000,
		"No complete speech audio"
	);
	ensure!(first.words.len() >= 8, "Missing native word timing events");
	ensure!(
		first
			.words
			.windows(2)
			.all(|w| w[0].start_ms <= w[1].start_ms),
		"Word timings are not ordered"
	);
	ensure!(
		first.words.iter().all(|w| w.start_ms < first.duration_ms),
		"Word timing exceeds the audio clock"
	);
	let second = speech
		.prepare(text, &voice, CancellationToken::new())
		.await?;
	ensure!(
		first.wav == second.wav
			&& serde_json::to_vec(&first.words)? == serde_json::to_vec(&second.words)?,
		"Cache did not preserve exact audio and timing"
	);
	ensure!(
		secret.reads.load(Ordering::SeqCst) == 1,
		"Second request unexpectedly reached the provider"
	);
	let canceled = CancellationToken::new();
	canceled.cancel();
	ensure!(
		speech
			.prepare(
				"This canceled line must never reach Azure.",
				&voice,
				canceled
			)
			.await
			.is_err(),
		"Canceled synthesis unexpectedly succeeded"
	);
	ensure!(
		secret.reads.load(Ordering::SeqCst) == 1,
		"Canceled request reached the provider"
	);
	println!(
		"Live native speech passed: {} bytes, {} ms, {} word boundaries; exact cache hit; cancellation blocked provider dispatch.",
		first.wav.len(),
		first.duration_ms,
		first.words.len()
	);
	Ok(())
}
