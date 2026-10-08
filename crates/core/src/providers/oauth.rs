use super::{Providers, check_response};
use crate::now_ms;
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpListener,
};
use tokio_util::sync::CancellationToken;

const TWITCH_SCOPES: &str = "user:read:chat user:write:chat";
const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";

// Intentionally not Debug or exposed as a command return type.
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
	pub access_token: String,
	pub refresh_token: String,
	pub expires_at: i64,
	pub scopes: Vec<String>,
	pub account_id: String,
	pub login: String,
	pub client_id: String,
	/// A rotated refresh token is persisted before network verification. It must
	/// not become usable authorization until identity and scopes are verified.
	#[serde(default)]
	pub validated: bool,
}
#[derive(Deserialize)]
struct TokenResponse {
	access_token: String,
	#[serde(default)]
	refresh_token: String,
	expires_in: u64,
	#[serde(default)]
	scope: serde_json::Value,
}
impl TokenResponse {
	fn into_tokens(self, client_id: String) -> Tokens {
		let scopes = match self.scope {
			serde_json::Value::Array(a) => a
				.into_iter()
				.filter_map(|v| v.as_str().map(str::to_owned))
				.collect(),
			serde_json::Value::String(s) => s.split_whitespace().map(str::to_owned).collect(),
			_ => Vec::new(),
		};
		Tokens {
			access_token: self.access_token,
			refresh_token: self.refresh_token,
			expires_at: now_ms() + (self.expires_in.min(i64::MAX as u64 / 1000) as i64) * 1000,
			scopes,
			account_id: String::new(),
			login: String::new(),
			client_id,
			validated: false,
		}
	}
}
#[derive(Deserialize)]
struct DeviceResponse {
	device_code: String,
	user_code: String,
	verification_uri: String,
	expires_in: u64,
	interval: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
	pub url: String,
	pub user_code: Option<String>,
	pub expires_in: u64,
}

impl Providers {
	pub async fn authorize_twitch(
		self: &Arc<Self>,
		cancel: CancellationToken,
	) -> Result<Authorization> {
		self.cancel_authorizations();
		let flow = self.oauth_cancellation.lock().unwrap().child_token();
		let guard = tokio::select! {_=cancel.cancelled()=>anyhow::bail!("Authorization canceled"),_=flow.cancelled()=>anyhow::bail!("Authorization superseded"),guard=self.oauth_lock.clone().lock_owned()=>guard};
		let settings = self.store.settings()?;
		let scopes =
			crate::agent::platform_tools::required_twitch_scopes(&settings.enabled_tool_groups)
				.join(" ");
		let client_id = settings.twitch_client_id;
		ensure!(
			!client_id.trim().is_empty(),
			"Enter your Twitch public application Client ID first"
		);
		let response = self
			.http
			.post("https://id.twitch.tv/oauth2/device")
			.form(&[
				("client_id", client_id.as_str()),
				("scopes", scopes.as_str()),
			])
			.send()
			.await?;
		check_response("twitch", &response)?;
		let device: DeviceResponse = response.json().await?;
		let parsed = url::Url::parse(&device.verification_uri)?;
		ensure!(
			parsed.scheme() == "https" && parsed.host_str() == Some("www.twitch.tv"),
			"Unexpected Twitch authorization address"
		);
		let authorization = Authorization {
			url: device.verification_uri.clone(),
			user_code: Some(device.user_code),
			expires_in: device.expires_in,
		};
		self.status(
			"twitch",
			"authorizing",
			"Finish authorization in your browser",
		);
		let providers = self.clone();
		tokio::spawn(async move {
			let _guard = guard;
			let result = tokio::select! {_=cancel.cancelled()=>Err(anyhow::anyhow!("Authorization canceled")),_=flow.cancelled()=>Err(anyhow::anyhow!("Authorization canceled because configuration changed")),result=providers.poll_twitch_device(&client_id,&device.device_code,&scopes,device.interval,device.expires_in,flow.clone())=>result};
			if let Err(error) = result {
				providers.status("twitch", "authorization_failed", error.to_string());
			}
		});
		Ok(authorization)
	}
	async fn poll_twitch_device(
		&self,
		client: &str,
		device: &str,
		scopes: &str,
		interval: u64,
		expires: u64,
		cancel: CancellationToken,
	) -> Result<()> {
		let deadline = tokio::time::Instant::now() + Duration::from_secs(expires.min(1800));
		let mut interval = interval.clamp(5, 60);
		loop {
			tokio::select! {_=cancel.cancelled()=>anyhow::bail!("Authorization cancelled"),_=tokio::time::sleep(Duration::from_secs(interval))=>{}}
			ensure!(
				tokio::time::Instant::now() < deadline,
				"Device authorization expired; connect again"
			);
			let response = twitch_device_token_request(&self.http, client, device, scopes)
				.send()
				.await?;
			if response.status().is_success() {
				let mut tokens = response
					.json::<TokenResponse>()
					.await?
					.into_tokens(client.to_owned());
				self.validate_twitch_tokens(&mut tokens).await?;
				self
					.commit_authorization("twitch", &tokens, &cancel)
					.await?;
				self.status(
					"twitch",
					"authorized",
					format!("Authorized as {}", tokens.login),
				);
				return Ok(());
			}
			let status = response.status();
			let body: serde_json::Value = response.json().await.unwrap_or_default();
			let code = body["message"]
				.as_str()
				.or(body["error"].as_str())
				.unwrap_or("");
			match code {
				"authorization_pending" => {}
				"slow_down" => interval = (interval + 5).min(60),
				"access_denied" => anyhow::bail!("Twitch authorization was declined"),
				"expired_token" | "invalid device code" => {
					anyhow::bail!("Device authorization expired; connect again")
				}
				_ => anyhow::bail!("Twitch authorization failed (HTTP {})", status.as_u16()),
			}
		}
	}
	pub async fn authorize_google(
		self: &Arc<Self>,
		cancel: CancellationToken,
	) -> Result<Authorization> {
		self.cancel_authorizations();
		let flow = self.oauth_cancellation.lock().unwrap().child_token();
		let guard = tokio::select! {_=cancel.cancelled()=>anyhow::bail!("Authorization canceled"),_=flow.cancelled()=>anyhow::bail!("Authorization superseded"),guard=self.oauth_lock.clone().lock_owned()=>guard};
		let client_id = self.store.settings()?.google_client_id;
		ensure!(
			!client_id.trim().is_empty(),
			"Enter your Google Desktop application Client ID first"
		);
		let client_secret = self.secret("google_client_secret")?;
		let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
		let redirect = format!(
			"http://127.0.0.1:{}/oauth/callback",
			listener.local_addr()?.port()
		);
		let state = format!(
			"{}{}",
			uuid::Uuid::new_v4().simple(),
			uuid::Uuid::new_v4().simple()
		);
		let verifier = format!(
			"{}{}",
			uuid::Uuid::new_v4().simple(),
			uuid::Uuid::new_v4().simple()
		);
		let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
			.encode(Sha256::digest(verifier.as_bytes()));
		let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
		url.query_pairs_mut().extend_pairs([
			("client_id", client_id.as_str()),
			("redirect_uri", redirect.as_str()),
			("response_type", "code"),
			("scope", GOOGLE_SCOPE),
			("state", state.as_str()),
			("code_challenge", challenge.as_str()),
			("code_challenge_method", "S256"),
			("access_type", "offline"),
			("prompt", "consent"),
		]);
		let authorization = Authorization {
			url: url.into(),
			user_code: None,
			expires_in: 300,
		};
		self.status(
			"youtube",
			"authorizing",
			"Finish authorization in your browser",
		);
		let providers = self.clone();
		tokio::spawn(async move {
			let _guard = guard;
			let operation = async {
				let code = tokio::select! {_=cancel.cancelled()=>anyhow::bail!("Authorization cancelled"),r=tokio::time::timeout(Duration::from_secs(300),google_callback(listener,&state))=>r.context("Google authorization timed out")??};
				let response = providers
					.http
					.post("https://oauth2.googleapis.com/token")
					.form(&[
						("client_id", client_id.as_str()),
						("client_secret", client_secret.as_str()),
						("code", code.as_str()),
						("code_verifier", verifier.as_str()),
						("redirect_uri", redirect.as_str()),
						("grant_type", "authorization_code"),
					])
					.send()
					.await?;
				check_response("youtube", &response)?;
				let mut tokens = response
					.json::<TokenResponse>()
					.await?
					.into_tokens(client_id);
				providers.validate_google_tokens(&mut tokens).await?;
				providers
					.commit_authorization("google", &tokens, &flow)
					.await?;
				providers.status(
					"youtube",
					"authorized",
					format!("Authorized as {}", tokens.login),
				);
				Ok::<_, anyhow::Error>(())
			};
			let result = tokio::select! {_=cancel.cancelled()=>Err(anyhow::anyhow!("Authorization canceled")),_=flow.cancelled()=>Err(anyhow::anyhow!("Authorization canceled because configuration changed")),result=operation=>result};
			if let Err(error) = result {
				providers.status("youtube", "authorization_failed", error.to_string());
			}
		});
		Ok(authorization)
	}
	async fn commit_authorization(
		&self,
		provider: &str,
		tokens: &Tokens,
		cancel: &CancellationToken,
	) -> Result<()> {
		let _guard = self.token_lock.lock().await;
		ensure!(
			!cancel.is_cancelled(),
			"Authorization canceled before saving credentials"
		);
		let settings = self.store.settings()?;
		let client = if provider == "twitch" {
			settings.twitch_client_id
		} else {
			settings.google_client_id
		};
		ensure!(
			client == tokens.client_id,
			"Provider Client ID changed; reconnect using the new configuration"
		);
		ensure!(tokens.validated, "Provider identity has not been verified");
		self.save_tokens(provider, tokens)
	}
	fn save_tokens(&self, provider: &str, tokens: &Tokens) -> Result<()> {
		self.secrets.set(
			&format!("{provider}_tokens"),
			&serde_json::to_string(tokens)?,
		)
	}
	pub async fn tokens(&self, provider: &str) -> Result<Tokens> {
		self.tokens_with(provider, &LiveTokenEndpoint(self)).await
	}
	async fn tokens_with(&self, provider: &str, endpoint: &impl TokenEndpoint) -> Result<Tokens> {
		ensure!(
			["twitch", "google"].contains(&provider),
			"Unsupported OAuth provider"
		);
		let _guard = self.token_lock.lock().await;
		let mut tokens: Tokens = serde_json::from_str(&self.secret(&format!("{provider}_tokens"))?)
			.context("Saved authorization is invalid; reconnect")?;
		let settings = self.store.settings()?;
		let configured = if provider == "twitch" {
			settings.twitch_client_id
		} else {
			settings.google_client_id
		};
		ensure!(
			tokens.client_id == configured,
			"Application Client ID changed; reconnect this account"
		);
		if tokens.expires_at > now_ms() + 60_000 {
			if !tokens.validated {
				let expected = tokens.account_id.clone();
				endpoint.validate(provider, &mut tokens).await?;
				ensure!(
					expected == tokens.account_id,
					"Authorization account changed; reconnect explicitly"
				);
				self.save_tokens(provider, &tokens)?;
			}
			return Ok(tokens);
		}
		ensure!(
			!tokens.refresh_token.is_empty(),
			"Authorization expired; reconnect this account"
		);
		let response = endpoint.refresh(provider, &tokens).await?;
		let mut renewed = response.into_tokens(tokens.client_id.clone());
		if renewed.refresh_token.is_empty() {
			renewed.refresh_token = tokens.refresh_token.clone();
		}
		if renewed.scopes.is_empty() {
			renewed.scopes = tokens.scopes.clone();
		}
		renewed.account_id = tokens.account_id.clone();
		renewed.login = tokens.login.clone();
		// Public-client Twitch refresh tokens rotate. Persist before any further network call.
		self.save_tokens(provider, &renewed)?;
		endpoint.validate(provider, &mut renewed).await?;
		ensure!(
			tokens.account_id == renewed.account_id,
			"Authorization account changed; reconnect explicitly"
		);
		self.save_tokens(provider, &renewed)?;
		tokens = renewed;
		Ok(tokens)
	}
	async fn validate_identity(&self, provider: &str, tokens: &mut Tokens) -> Result<()> {
		if provider == "twitch" {
			self.validate_twitch_tokens(tokens).await
		} else {
			self.validate_google_tokens(tokens).await
		}
	}
	pub async fn validate_twitch_tokens(&self, tokens: &mut Tokens) -> Result<()> {
		tokens.validated = false;
		let response = self
			.http
			.get("https://id.twitch.tv/oauth2/validate")
			.header("Authorization", format!("OAuth {}", tokens.access_token))
			.send()
			.await?;
		check_response("twitch", &response)?;
		let body: serde_json::Value = response.json().await?;
		ensure!(
			body["client_id"].as_str() == Some(tokens.client_id.as_str()),
			"Twitch token belongs to a different application"
		);
		tokens.account_id = body["user_id"]
			.as_str()
			.context("Twitch user authorization required")?
			.into();
		tokens.login = body["login"]
			.as_str()
			.context("Twitch login unavailable")?
			.into();
		tokens.scopes = body["scopes"]
			.as_array()
			.context("Twitch scopes unavailable")?
			.iter()
			.filter_map(|v| v.as_str().map(str::to_owned))
			.collect();
		ensure!(
			TWITCH_SCOPES
				.split_whitespace()
				.all(|scope| tokens.scopes.iter().any(|s| s == scope)),
			"Missing Twitch chat permissions; reconnect"
		);
		tokens.validated = true;
		Ok(())
	}
	pub async fn validate_google_tokens(&self, tokens: &mut Tokens) -> Result<()> {
		tokens.validated = false;
		let response = self
			.http
			.get("https://www.googleapis.com/youtube/v3/channels")
			.bearer_auth(&tokens.access_token)
			.query(&[("part", "snippet"), ("mine", "true")])
			.send()
			.await?;
		check_response("youtube", &response)?;
		let body: serde_json::Value = response.json().await?;
		let channel = body["items"]
			.as_array()
			.and_then(|a| a.first())
			.context("This Google account has no authorized YouTube channel")?;
		tokens.account_id = channel["id"]
			.as_str()
			.context("YouTube channel ID missing")?
			.into();
		tokens.login = channel["snippet"]["title"]
			.as_str()
			.unwrap_or("YouTube channel")
			.into();
		tokens.validated = true;
		Ok(())
	}
}

fn twitch_device_token_request(
	http: &reqwest::Client,
	client: &str,
	device: &str,
	scopes: &str,
) -> reqwest::RequestBuilder {
	http.post("https://id.twitch.tv/oauth2/token").form(&[
		("client_id", client),
		("device_code", device),
		// Twitch's DCF request contract is plural; token responses use `scope`.
		("scopes", scopes),
		("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
	])
}

#[async_trait::async_trait]
trait TokenEndpoint: Send + Sync {
	async fn refresh(&self, provider: &str, tokens: &Tokens) -> Result<TokenResponse>;
	async fn validate(&self, provider: &str, tokens: &mut Tokens) -> Result<()>;
}
struct LiveTokenEndpoint<'a>(&'a Providers);
#[async_trait::async_trait]
impl TokenEndpoint for LiveTokenEndpoint<'_> {
	async fn refresh(&self, provider: &str, tokens: &Tokens) -> Result<TokenResponse> {
		let mut form = vec![
			("client_id", tokens.client_id.clone()),
			("grant_type", "refresh_token".into()),
			("refresh_token", tokens.refresh_token.clone()),
		];
		let url = if provider == "twitch" {
			"https://id.twitch.tv/oauth2/token"
		} else {
			form.push(("client_secret", self.0.secret("google_client_secret")?));
			"https://oauth2.googleapis.com/token"
		};
		let response = self.0.http.post(url).form(&form).send().await?;
		check_response(provider, &response)?;
		let response: TokenResponse = response.json().await?;
		Ok(response)
	}
	async fn validate(&self, provider: &str, tokens: &mut Tokens) -> Result<()> {
		self.0.validate_identity(provider, tokens).await
	}
}

async fn google_callback(listener: TcpListener, expected_state: &str) -> Result<String> {
	loop {
		let (mut stream, address) = listener.accept().await?;
		if !address.ip().is_loopback() {
			continue;
		}
		let mut bytes = Vec::with_capacity(1024);
		let read = tokio::time::timeout(Duration::from_secs(5), async {
			loop {
				let mut chunk = [0u8; 1024];
				let n = stream.read(&mut chunk).await?;
				if n == 0 {
					return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
				}
				bytes.extend_from_slice(&chunk[..n]);
				if bytes.len() > 8192 {
					return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
				}
				if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
					return Ok(());
				}
			}
		})
		.await;
		if !matches!(read, Ok(Ok(()))) {
			continue;
		}
		let request = String::from_utf8_lossy(&bytes);
		let line = request.lines().next().unwrap_or("");
		let parts: Vec<_> = line.split_whitespace().collect();
		let parsed = if parts.len() == 3 && parts[0] == "GET" {
			url::Url::parse(&format!("http://localhost{}", parts[1])).ok()
		} else {
			None
		};
		let outcome = parsed.and_then(|u| {
			if u.path() != "/oauth/callback" {
				return None;
			}
			let query: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
			if query.get("state").map(String::as_str) != Some(expected_state) {
				return None;
			}
			Some(
				match query
					.get("code")
					.filter(|c| !c.is_empty() && c.len() < 4096)
				{
					Some(code) => Ok(code.clone()),
					None => Err(anyhow::anyhow!("Google authorization declined")),
				},
			)
		});
		let (status, body) = if outcome.is_some() {
			(
				"200 OK",
				"Authorization received. You can return to Bumblebee.",
			)
		} else {
			("400 Bad Request", "Invalid authorization callback.")
		};
		let response = format!(
			"HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
			body.len()
		);
		let _ = stream.write_all(response.as_bytes()).await;
		if let Some(result) = outcome {
			return result;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn twitch_device_poll_serializes_the_required_public_client_form() {
		for groups in [
			vec![],
			vec!["twitch_broadcast".into(), "twitch_moderation".into()],
		] {
			let scopes = crate::agent::platform_tools::required_twitch_scopes(&groups).join(" ");
			let request = twitch_device_token_request(
				&reqwest::Client::new(),
				"public-client",
				"device+/=code",
				&scopes,
			)
			.build()
			.unwrap();
			assert_eq!(request.method(), reqwest::Method::POST);
			assert_eq!(request.url().as_str(), "https://id.twitch.tv/oauth2/token");
			assert_eq!(
				request.headers()[reqwest::header::CONTENT_TYPE],
				"application/x-www-form-urlencoded"
			);
			let body = request.body().unwrap().as_bytes().unwrap();
			let fields: std::collections::BTreeMap<_, _> =
				url::form_urlencoded::parse(body).into_owned().collect();
			assert_eq!(fields.len(), 4);
			assert_eq!(fields["client_id"], "public-client");
			assert_eq!(fields["device_code"], "device+/=code");
			assert_eq!(
				fields["grant_type"],
				"urn:ietf:params:oauth:grant-type:device_code"
			);
			assert_eq!(fields["scopes"], scopes);
			assert!(!fields.contains_key("scope"));
			assert!(!fields.contains_key("client_secret"));
		}
	}
	#[tokio::test]
	async fn forged_callback_does_not_consume_valid_authorization() {
		let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
			.await
			.unwrap();
		let address = listener.local_addr().unwrap();
		let callback =
			tokio::spawn(async move { google_callback(listener, "unpredictable-state").await });
		let client = reqwest::Client::new();
		let invalid = client
			.get(format!(
				"http://{address}/oauth/callback?state=wrong&code=attacker"
			))
			.send()
			.await
			.unwrap();
		assert_eq!(invalid.status(), 400);
		assert!(!callback.is_finished());
		let valid = client
			.get(format!(
				"http://{address}/oauth/callback?state=unpredictable-state&code=authorized-code"
			))
			.send()
			.await
			.unwrap();
		assert_eq!(valid.status(), 200);
		assert_eq!(callback.await.unwrap().unwrap(), "authorized-code");
	}
	#[tokio::test]
	async fn denied_authorization_is_an_error_not_a_saved_token() {
		let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
			.await
			.unwrap();
		let address = listener.local_addr().unwrap();
		let callback = tokio::spawn(async move { google_callback(listener, "state").await });
		let response = reqwest::get(format!(
			"http://{address}/oauth/callback?state=state&error=access_denied"
		))
		.await
		.unwrap();
		assert_eq!(response.status(), 200);
		assert!(callback.await.unwrap().is_err());
	}
	#[derive(Default)]
	struct TestSecrets(std::sync::Mutex<std::collections::BTreeMap<String, String>>);
	impl crate::providers::SecretStore for TestSecrets {
		fn get(&self, name: &str) -> Result<Option<String>> {
			Ok(self.0.lock().unwrap().get(name).cloned())
		}
		fn set(&self, name: &str, value: &str) -> Result<()> {
			self.0.lock().unwrap().insert(name.into(), value.into());
			Ok(())
		}
		fn delete(&self, name: &str) -> Result<()> {
			self.0.lock().unwrap().remove(name);
			Ok(())
		}
	}
	struct FakeEndpoint {
		renewals: std::sync::atomic::AtomicUsize,
		validations: std::sync::atomic::AtomicUsize,
		fail_once: std::sync::atomic::AtomicBool,
		wrong_account: bool,
		pause: bool,
		entered: tokio::sync::Notify,
		release: tokio::sync::Notify,
	}
	impl FakeEndpoint {
		fn new() -> Self {
			Self {
				renewals: 0.into(),
				validations: 0.into(),
				fail_once: false.into(),
				wrong_account: false,
				pause: false,
				entered: tokio::sync::Notify::new(),
				release: tokio::sync::Notify::new(),
			}
		}
	}
	#[async_trait::async_trait]
	impl TokenEndpoint for FakeEndpoint {
		async fn refresh(&self, _provider: &str, _tokens: &Tokens) -> Result<TokenResponse> {
			self
				.renewals
				.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			self.entered.notify_one();
			if self.pause {
				self.release.notified().await
			}
			Ok(TokenResponse {
				access_token: "rotated-access".into(),
				refresh_token: "rotated-refresh".into(),
				expires_in: 3600,
				scope: serde_json::json!(["user:read:chat", "user:write:chat"]),
			})
		}
		async fn validate(&self, _provider: &str, tokens: &mut Tokens) -> Result<()> {
			self
				.validations
				.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			tokens.validated = false;
			ensure!(
				!self
					.fail_once
					.swap(false, std::sync::atomic::Ordering::SeqCst),
				"Temporary identity validation outage"
			);
			if self.wrong_account {
				tokens.account_id = "different-account".into()
			}
			tokens.validated = true;
			Ok(())
		}
	}
	fn fixture() -> (tempfile::TempDir, Arc<Providers>) {
		let dir = tempfile::tempdir().unwrap();
		let store = Arc::new(crate::storage::Store::open(&dir.path().join("test.sqlite")).unwrap());
		let mut settings = store.settings().unwrap();
		settings.twitch_client_id = "client".into();
		store.set("installation", &settings).unwrap();
		let providers = Providers::new(store, Arc::new(TestSecrets::default())).unwrap();
		providers
			.save_tokens(
				"twitch",
				&Tokens {
					access_token: "old-access".into(),
					refresh_token: "old-refresh".into(),
					expires_at: 0,
					scopes: vec![],
					account_id: "original-account".into(),
					login: "owner".into(),
					client_id: "client".into(),
					validated: true,
				},
			)
			.unwrap();
		(dir, providers)
	}
	fn saved(providers: &Providers) -> Tokens {
		serde_json::from_str(&providers.secret("twitch_tokens").unwrap()).unwrap()
	}
	#[tokio::test]
	async fn rotated_token_survives_validation_outage_but_is_not_trusted() {
		let (_dir, providers) = fixture();
		let endpoint = FakeEndpoint::new();
		endpoint
			.fail_once
			.store(true, std::sync::atomic::Ordering::SeqCst);
		assert!(providers.tokens_with("twitch", &endpoint).await.is_err());
		let pending = saved(&providers);
		assert_eq!(pending.refresh_token, "rotated-refresh");
		assert!(!pending.validated);
		let verified = providers.tokens_with("twitch", &endpoint).await.unwrap();
		assert!(verified.validated);
		assert_eq!(verified.account_id, "original-account");
		assert_eq!(
			endpoint.renewals.load(std::sync::atomic::Ordering::SeqCst),
			1
		);
		assert_eq!(
			endpoint
				.validations
				.load(std::sync::atomic::Ordering::SeqCst),
			2
		);
	}
	#[tokio::test]
	async fn rejected_identity_cannot_become_a_fresh_authorization_on_next_read() {
		let (_dir, providers) = fixture();
		let mut endpoint = FakeEndpoint::new();
		endpoint.wrong_account = true;
		for _ in 0..2 {
			assert!(providers.tokens_with("twitch", &endpoint).await.is_err());
			assert!(!saved(&providers).validated)
		}
		assert_eq!(
			endpoint.renewals.load(std::sync::atomic::Ordering::SeqCst),
			1
		);
		assert_eq!(
			endpoint
				.validations
				.load(std::sync::atomic::Ordering::SeqCst),
			2
		);
	}
	#[tokio::test]
	async fn slow_refresh_cannot_overwrite_new_browser_authorization() {
		let (_dir, providers) = fixture();
		let mut endpoint = FakeEndpoint::new();
		endpoint.pause = true;
		let endpoint = Arc::new(endpoint);
		let worker = providers.clone();
		let service = endpoint.clone();
		let refresh =
			tokio::spawn(async move { worker.tokens_with("twitch", service.as_ref()).await });
		endpoint.entered.notified().await;
		let mut replacement = saved(&providers);
		replacement.account_id = "newly-authorized-account".into();
		replacement.access_token = "newly-authorized-access".into();
		replacement.expires_at = now_ms() + 3600000;
		let writer = providers.clone();
		let authorization = tokio::spawn(async move {
			writer
				.commit_authorization("twitch", &replacement, &CancellationToken::new())
				.await
		});
		tokio::task::yield_now().await;
		assert!(!authorization.is_finished());
		endpoint.release.notify_one();
		refresh.await.unwrap().unwrap();
		authorization.await.unwrap().unwrap();
		assert_eq!(saved(&providers).account_id, "newly-authorized-account");
	}
	#[tokio::test]
	async fn deleting_credentials_invalidates_a_pending_browser_callback() {
		let (_dir, providers) = fixture();
		let tokens = saved(&providers);
		let flow = providers.oauth_cancellation.lock().unwrap().child_token();
		providers.delete_credential("twitch_tokens").await.unwrap();
		assert!(flow.is_cancelled());
		assert!(
			providers
				.commit_authorization("twitch", &tokens, &flow)
				.await
				.is_err()
		);
		assert!(providers.secrets.get("twitch_tokens").unwrap().is_none());
	}
}
