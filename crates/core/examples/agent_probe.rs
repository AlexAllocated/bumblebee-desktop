//! Opt-in live OpenAI check; never run automatically in CI.
//! Provide BUMBLEBEE_AGENT_PROBE_KEY securely and pass a model ID explicitly.
//! No audio, platform connections, messages, or real tool executors are started.
use anyhow::{Context, Result, bail, ensure};
use bumblebee_core::{
	agent::probe,
	providers::{Providers, SecretStore},
	storage::Store,
};
use std::sync::Arc;
struct OpenAiOnly(String);
impl SecretStore for OpenAiOnly {
	fn get(&self, name: &str) -> Result<Option<String>> {
		ensure!(name == "openai", "Probe requested an unrelated credential");
		Ok(Some(self.0.clone()))
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
	let model = std::env::args()
		.nth(1)
		.context("Pass the explicit OpenAI model ID to probe")?;
	ensure!(
		!model.is_empty()
			&& model.len() <= 100
			&& model
				.bytes()
				.all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
		"Invalid model ID"
	);
	let key = std::env::var("BUMBLEBEE_AGENT_PROBE_KEY")
		.context("Set the OpenAI probe credential in the process environment")?;
	let data = tempfile::tempdir()?;
	std::fs::create_dir(data.path().join("artifacts"))?;
	let path = data.path().join("state.sqlite");
	let store = Arc::new(Store::open(&path)?);
	let providers = Providers::new(store.clone(), Arc::new(OpenAiOnly(key)))?;
	let model_check = providers
		.http
		.get(format!("https://api.openai.com/v1/models/{model}"))
		.bearer_auth(providers.secret("openai")?)
		.send()
		.await?;
	ensure!(
		model_check.status().is_success(),
		"Requested probe model is unavailable to this credential (HTTP {})",
		model_check.status().as_u16()
	);
	let report = probe::run(providers, &model).await?;
	drop(store);
	let reopened = Store::open(&path)?;
	ensure!(
		reopened.get::<String>("agent_probe_reply")?.as_deref() == Some(report.reply.as_str()),
		"Probe result did not survive reopening SQLite"
	);
	ensure!(
		std::fs::read_dir(data.path().join("artifacts"))?
			.next()
			.is_none(),
		"Probe unexpectedly created artifacts"
	);
	println!("{}", serde_json::to_string_pretty(&report)?);
	Ok(())
}
