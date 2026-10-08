//! Native provider clients. Tokens never cross the desktop command/event boundary.
pub mod discovery;
pub mod oauth;
pub mod streaming;

use crate::{model::Voice, storage::Store};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
	time::Duration,
};
use tokio::sync::{broadcast, watch};

#[derive(Clone)]
enum EchoOutcome {
	Pending,
	Sent(String),
	Rejected,
	Unknown,
}
struct EchoAttempt {
	platform: String,
	channel: String,
	text: String,
	expires: i64,
	outcome: watch::Sender<EchoOutcome>,
}
pub(crate) struct OutgoingEcho(Arc<EchoAttempt>);
impl OutgoingEcho {
	fn sent(&self, id: &str) {
		self.0.outcome.send_replace(EchoOutcome::Sent(id.into()));
	}
	pub(crate) fn rejected(&self) {
		self.0.outcome.send_replace(EchoOutcome::Rejected);
	}
}
impl Drop for OutgoingEcho {
	fn drop(&mut self) {
		// A dropped request may already have reached the provider. Do not invent
		// a successful receipt or retry it; retain only a bounded uncertain match.
		self.0.outcome.send_if_modified(|outcome| {
			if matches!(outcome, EchoOutcome::Pending) {
				*outcome = EchoOutcome::Unknown;
				true
			} else {
				false
			}
		});
	}
}

pub trait SecretStore: Send + Sync {
	fn get(&self, name: &str) -> Result<Option<String>>;
	fn set(&self, name: &str, value: &str) -> Result<()>;
	fn delete(&self, name: &str) -> Result<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
	pub provider: String,
	pub state: String,
	pub message: String,
}

pub struct Providers {
	pub store: Arc<Store>,
	pub secrets: Arc<dyn SecretStore>,
	pub http: reqwest::Client,
	pub status_events: broadcast::Sender<ProviderStatus>,
	statuses: Mutex<BTreeMap<String, ProviderStatus>>,
	stream_destinations: Mutex<BTreeMap<String, String>>,
	echoes: Mutex<Vec<Arc<EchoAttempt>>>,
	pub(crate) token_lock: Arc<tokio::sync::Mutex<()>>,
	pub(crate) oauth_lock: Arc<tokio::sync::Mutex<()>>,
	pub(crate) oauth_cancellation: Mutex<tokio_util::sync::CancellationToken>,
}

impl Providers {
	pub fn new(store: Arc<Store>, secrets: Arc<dyn SecretStore>) -> Result<Arc<Self>> {
		let (status_events, _) = broadcast::channel(64);
		Ok(Arc::new(Self {
			store,
			secrets,
			http: reqwest::Client::builder()
				.timeout(Duration::from_secs(30))
				.redirect(reqwest::redirect::Policy::none())
				.user_agent("BumblebeeDesktop/0.1")
				.build()?,
			status_events,
			statuses: Mutex::new(BTreeMap::new()),
			stream_destinations: Mutex::new(BTreeMap::new()),
			echoes: Mutex::new(Vec::new()),
			token_lock: Arc::new(tokio::sync::Mutex::new(())),
			oauth_lock: Arc::new(tokio::sync::Mutex::new(())),
			oauth_cancellation: Mutex::new(tokio_util::sync::CancellationToken::new()),
		}))
	}
	pub fn stream_chat_destination(&self, platform: &str) -> Option<String> {
		self
			.stream_destinations
			.lock()
			.unwrap()
			.get(platform)
			.cloned()
	}
	pub(crate) fn set_stream_chat_destination(&self, platform: &str, channel: Option<&str>) {
		let mut destinations = self.stream_destinations.lock().unwrap();
		if let Some(channel) = channel {
			destinations.insert(platform.into(), channel.into());
		} else {
			destinations.remove(platform);
		}
	}

	/// Invalidate a pending browser flow before changing its configuration.
	pub fn cancel_authorizations(&self) {
		let mut token = self.oauth_cancellation.lock().unwrap();
		token.cancel();
		*token = tokio_util::sync::CancellationToken::new();
	}
	pub async fn set_credential(&self, name: &str, value: &str) -> Result<()> {
		ensure!(
			[
				"azure_speech",
				"openai",
				"discord_bot",
				"google_client_secret"
			]
			.contains(&name),
			"Use browser authorization for OAuth credentials"
		);
		ensure!(
			!value.trim().is_empty() && value.len() <= 16384,
			"Invalid credential"
		);
		self.cancel_authorizations();
		let guard = self.token_lock.clone().lock_owned().await;
		let secrets = self.secrets.clone();
		let name = name.to_owned();
		let value = value.trim().to_owned();
		tokio::task::spawn_blocking(move || {
			let _guard = guard;
			secrets.set(&name, &value)
		})
		.await?
	}
	pub async fn delete_credential(&self, name: &str) -> Result<()> {
		ensure!(
			[
				"azure_speech",
				"openai",
				"discord_bot",
				"google_client_secret",
				"twitch_tokens",
				"google_tokens"
			]
			.contains(&name),
			"Unknown credential"
		);
		self.cancel_authorizations();
		let guard = self.token_lock.clone().lock_owned().await;
		let secrets = self.secrets.clone();
		let name = name.to_owned();
		tokio::task::spawn_blocking(move || {
			let _guard = guard;
			secrets.delete(&name)
		})
		.await?
	}

	pub fn status(&self, provider: &str, state: &str, message: impl Into<String>) {
		let status = ProviderStatus {
			provider: provider.into(),
			state: state.into(),
			message: message.into(),
		};
		self
			.statuses
			.lock()
			.unwrap()
			.insert(provider.into(), status.clone());
		let _ = self.status_events.send(status);
	}
	pub(crate) fn expect_echo(&self, platform: &str, channel: &str, text: &str) -> OutgoingEcho {
		let mut echoes = self.echoes.lock().unwrap();
		let now = crate::now_ms();
		echoes.retain(|e| e.expires > now);
		if echoes.len() >= 256 {
			echoes.remove(0);
		}
		let (outcome, _) = watch::channel(EchoOutcome::Pending);
		let attempt = Arc::new(EchoAttempt {
			platform: platform.into(),
			channel: channel.into(),
			text: text.into(),
			expires: now + 60_000,
			outcome,
		});
		echoes.push(attempt.clone());
		OutgoingEcho(attempt)
	}
	pub(crate) fn record_sent_echo(&self, echo: &OutgoingEcho, id: &str) -> Result<()> {
		// Persist even when the matching gateway event has not arrived yet. This
		// also suppresses duplicate delivery after a reconnect or app restart.
		echo.sent(id);
		self.store.claim_message(&echo.0.platform, id)?;
		Ok(())
	}
	pub(crate) async fn is_echo(
		&self,
		platform: &str,
		channel: &str,
		id: &str,
		text: &str,
		cancel: &tokio_util::sync::CancellationToken,
	) -> Result<bool> {
		let attempts = {
			let mut echoes = self.echoes.lock().unwrap();
			echoes.retain(|e| e.expires > crate::now_ms());
			echoes
				.iter()
				.filter(|e| e.platform == platform && e.channel == channel)
				.cloned()
				.collect::<Vec<_>>()
		};
		for attempt in attempts {
			let mut outcome = attempt.outcome.subscribe();
			loop {
				let current = outcome.borrow_and_update().clone();
				match current {
					EchoOutcome::Pending => {
						// The event can beat the HTTP receipt. Wait for its exact ID;
						// text alone could be the streamer's own identical message.
						tokio::select! { _=cancel.cancelled()=>return Ok(false), changed=outcome.changed()=>{changed?;} }
					}
					EchoOutcome::Sent(receipt) if receipt == id => {
						self.store.claim_message(platform, id)?;
						return Ok(true);
					}
					EchoOutcome::Unknown if attempt.text == text => {
						// An interrupted write has no receipt. Conservatively match
						// one event, then remember its ID rather than consuming text.
						attempt.outcome.send_replace(EchoOutcome::Sent(id.into()));
						self.store.claim_message(platform, id)?;
						return Ok(true);
					}
					_ => break,
				}
			}
		}
		Ok(false)
	}
	pub fn statuses(&self) -> Vec<ProviderStatus> {
		self.statuses.lock().unwrap().values().cloned().collect()
	}
	pub fn secret(&self, id: &str) -> Result<String> {
		self
			.secrets
			.get(id)?
			.filter(|v| !v.trim().is_empty())
			.context("Missing credentials; configure this provider in Settings")
	}
	pub async fn refresh_voices(&self) -> Result<Vec<Voice>> {
		let settings = self.store.settings()?;
		ensure!(
			!settings.azure_region.is_empty()
				&& settings.azure_region.len() < 40
				&& settings
					.azure_region
					.bytes()
					.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
			"Invalid Azure Speech region"
		);
		let key = self.secret("azure_speech")?;
		let response = self
			.http
			.get(format!(
				"https://{}.tts.speech.microsoft.com/cognitiveservices/voices/list",
				settings.azure_region
			))
			.header("Ocp-Apim-Subscription-Key", key)
			.send()
			.await
			.context("Cannot reach Azure Speech")?;
		check_response("azure_speech", &response)?;
		let rows: Vec<AzureVoice> = response
			.json()
			.await
			.context("Invalid Azure voice catalog")?;
		let voices: Vec<_> = rows
			.into_iter()
			.filter(|v| v.locale.starts_with("en-") && v.status.as_deref().unwrap_or("GA") == "GA")
			.map(|v| Voice {
				id: format!("azure:{}", v.short_name),
				voice_name: v.short_name,
				provider: "azure_speech".into(),
				name: Some(format!("{} ({})", v.display_name, v.locale)),
				rate: "1.0".into(),
				pitch: "1.0".into(),
				expression: "default".into(),
				role: "puppet".into(),
			})
			.collect();
		ensure!(
			!voices.is_empty(),
			"Azure returned no eligible English voices; retaining the previous catalog"
		);
		self.store.set("azure_voices", &voices)?;
		self.status(
			"azure_speech",
			"connected",
			format!("{} English voices available", voices.len()),
		);
		self.store.catalog_voices()
	}
	pub async fn openai_models(&self) -> Result<Vec<String>> {
		let response = self
			.http
			.get("https://api.openai.com/v1/models")
			.bearer_auth(self.secret("openai")?)
			.send()
			.await
			.context("Cannot reach OpenAI")?;
		check_response("openai", &response)?;
		let body: serde_json::Value = response.json().await?;
		let mut models: Vec<String> = body["data"]
			.as_array()
			.context("Invalid OpenAI model list")?
			.iter()
			.filter_map(|v| v["id"].as_str().map(str::to_owned))
			.collect();
		models.sort();
		self.status("openai", "connected", "API key validated");
		Ok(models)
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AzureVoice {
	short_name: String,
	locale: String,
	display_name: String,
	status: Option<String>,
}

/// Deliberately exclude provider response bodies: they may echo tokens or private requests.
pub fn check_response(provider: &str, response: &reqwest::Response) -> Result<()> {
	let code = response.status();
	if code.is_success() {
		return Ok(());
	}
	let kind = match code.as_u16() {
		401 => "Authorization expired or credentials invalid",
		403 => "Missing provider permissions",
		429 => "Provider rate limit reached",
		_ => "Provider request failed",
	};
	anyhow::bail!("{provider}: {kind} (HTTP {})", code.as_u16())
}

#[cfg(test)]
mod echo_tests {
	use super::*;
	use tokio_util::sync::CancellationToken;
	struct NoSecrets;
	impl SecretStore for NoSecrets {
		fn get(&self, _: &str) -> Result<Option<String>> {
			Ok(None)
		}
		fn set(&self, _: &str, _: &str) -> Result<()> {
			anyhow::bail!("unexpected credential write")
		}
		fn delete(&self, _: &str) -> Result<()> {
			anyhow::bail!("unexpected credential removal")
		}
	}
	fn fixture() -> (tempfile::TempDir, Arc<Providers>) {
		let temp = tempfile::tempdir().unwrap();
		let store = Arc::new(Store::open(&temp.path().join("state.sqlite")).unwrap());
		let providers = Providers::new(store, Arc::new(NoSecrets)).unwrap();
		(temp, providers)
	}
	#[tokio::test]
	async fn event_before_http_receipt_waits_and_duplicates_remain_suppressed() {
		let (_temp, providers) = fixture();
		let echo = providers.expect_echo("twitch", "room", "Bumblebee output");
		let matcher = providers.clone();
		let task = tokio::spawn(async move {
			matcher
				.is_echo(
					"twitch",
					"room",
					"receipt",
					"normalized output",
					&CancellationToken::new(),
				)
				.await
				.unwrap()
		});
		tokio::task::yield_now().await;
		assert!(!task.is_finished());
		providers.record_sent_echo(&echo, "receipt").unwrap();
		assert!(task.await.unwrap());
		for _ in 0..2 {
			assert!(
				providers
					.is_echo(
						"twitch",
						"room",
						"receipt",
						"normalized output",
						&CancellationToken::new()
					)
					.await
					.unwrap()
			);
		}
		assert!(!providers.store.claim_message("twitch", "receipt").unwrap());
		// The durable engine deduplicator also remembers the receipt after restart.
		let reopened = Providers::new(providers.store.clone(), Arc::new(NoSecrets)).unwrap();
		assert!(!reopened.store.claim_message("twitch", "receipt").unwrap());
	}
	#[tokio::test]
	async fn rejected_send_cannot_swallow_identical_streamer_chat() {
		let (_temp, providers) = fixture();
		let echo = providers.expect_echo("youtube", "room", "same text");
		let matcher = providers.clone();
		let task = tokio::spawn(async move {
			matcher
				.is_echo(
					"youtube",
					"room",
					"manual",
					"same text",
					&CancellationToken::new(),
				)
				.await
				.unwrap()
		});
		tokio::task::yield_now().await;
		assert!(!task.is_finished());
		echo.rejected();
		assert!(!task.await.unwrap());
		assert!(providers.store.claim_message("youtube", "manual").unwrap());
	}
	#[tokio::test]
	async fn identical_owner_text_with_different_receipt_is_not_an_echo() {
		let (_temp, providers) = fixture();
		let echo = providers.expect_echo("twitch", "room", "same text");
		providers.record_sent_echo(&echo, "app").unwrap();
		assert!(
			!providers
				.is_echo(
					"twitch",
					"room",
					"manual",
					"same text",
					&CancellationToken::new()
				)
				.await
				.unwrap()
		);
		assert!(
			!providers
				.is_echo(
					"twitch",
					"other-room",
					"app",
					"same text",
					&CancellationToken::new()
				)
				.await
				.unwrap()
		);
		assert!(
			!providers
				.is_echo(
					"youtube",
					"room",
					"app",
					"same text",
					&CancellationToken::new()
				)
				.await
				.unwrap()
		);
	}
	#[tokio::test]
	async fn interrupted_write_matches_one_uncertain_event_then_uses_its_id() {
		let (_temp, providers) = fixture();
		drop(providers.expect_echo("twitch", "room", "uncertain output"));
		for _ in 0..2 {
			assert!(
				providers
					.is_echo(
						"twitch",
						"room",
						"observed",
						"uncertain output",
						&CancellationToken::new()
					)
					.await
					.unwrap()
			);
		}
		assert!(
			!providers
				.is_echo(
					"twitch",
					"room",
					"manual",
					"uncertain output",
					&CancellationToken::new()
				)
				.await
				.unwrap()
		);
		assert!(!providers.store.claim_message("twitch", "observed").unwrap());
	}
	#[tokio::test]
	async fn stopping_the_session_does_not_wait_for_send_acknowledgement() {
		let (_temp, providers) = fixture();
		let _echo = providers.expect_echo("twitch", "room", "output");
		let cancel = CancellationToken::new();
		cancel.cancel();
		assert!(
			!tokio::time::timeout(
				Duration::from_secs(1),
				providers.is_echo("twitch", "room", "event", "output", &cancel)
			)
			.await
			.unwrap()
			.unwrap()
		);
	}
}
