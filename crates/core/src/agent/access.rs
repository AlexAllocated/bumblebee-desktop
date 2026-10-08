//! Repairable access is inspected before a call enters the execution ledger.
//! Only an explicit requester continuation may try this preflight again. Target
//! errors and dispatched provider failures never become reconnect prompts.
use crate::{
	model::{ChatMessage, Settings},
	providers::{AuthorizationRequired, Providers, oauth::Tokens},
	runtime::Engine,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessBindings {
	// Nonsecret provider identities only. Never bind raw credentials or a token
	// fingerprint: routine refresh/rotation must not change the human identity.
	identities: BTreeMap<String, String>,
}
impl AccessBindings {
	pub(super) fn snapshot_source(settings: &Settings, source: &ChatMessage) -> Result<Self> {
		let mut value = Self::default();
		if matches!(source.platform.as_str(), "discord" | "discord_voice") {
			value.bind("discord_owner", &settings.owner_discord_id)?;
			if source.platform == "discord_voice"
				|| source.channel_id == settings.discord_text_channel_id
			{
				value.bind("discord_guild", &settings.discord_guild_id)?;
				if source.platform == "discord_voice" {
					value.bind("discord_voice_channel", &settings.discord_voice_channel_id)?;
					if !settings.discord_text_channel_id.is_empty() {
						value.bind("discord_text_channel", &settings.discord_text_channel_id)?;
					}
				} else {
					value.bind("discord_text_channel", &settings.discord_text_channel_id)?;
				}
			}
		}
		Ok(value)
	}
	pub(super) fn validate_source(&self, settings: &Settings, source: &ChatMessage) -> Result<()> {
		for (key, current) in [
			("discord_owner", &settings.owner_discord_id),
			("discord_guild", &settings.discord_guild_id),
			("discord_text_channel", &settings.discord_text_channel_id),
			("discord_voice_channel", &settings.discord_voice_channel_id),
		] {
			if let Some(saved) = self.identities.get(key) {
				ensure!(
					saved == current,
					"The {key} configuration changed; cancel this saved request and start a new one"
				);
			}
		}
		let source_key = match source.platform.as_str() {
			"discord" | "discord_voice" => Some("discord_bot"),
			"twitch" => Some("twitch"),
			"youtube" => Some("google"),
			_ => None,
		};
		if let Some(key) = source_key {
			ensure!(
				self.identities.contains_key(key),
				"This older request has no saved provider identity; cancel it and start a new request"
			);
		}
		Ok(())
	}
	fn bind(&mut self, provider: &str, identity: &str) -> Result<()> {
		ensure!(!identity.is_empty(), "Provider identity is unavailable");
		if let Some(expected) = self.identities.get(provider) {
			ensure!(
				expected == identity,
				"The {provider} identity changed; cancel this saved request and start a new one"
			);
		} else {
			self.identities.insert(provider.into(), identity.into());
		}
		Ok(())
	}
}
#[derive(Debug)]
pub struct AccessBlocker {
	pub prompt: String,
}
impl AccessBlocker {
	pub(super) fn repair(detail: impl AsRef<str>) -> Self {
		Self {
			prompt: format!(
				"{}. Update Settings or reconnect the same account, then choose continue to retry this exact step, or cancel to stop the request.",
				detail.as_ref()
			),
		}
	}
}
/// Derived from explicit, observed permission bits before dispatch; never from
/// a generic HTTP rejection that may concern a deleted or inaccessible target.
#[derive(Debug)]
pub(super) struct PermissionRequired(pub String);
impl std::fmt::Display for PermissionRequired {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}
impl std::error::Error for PermissionRequired {}

#[derive(Clone, Copy, Debug, Default)]
struct Requirement {
	provider: Option<&'static str>,
	group: Option<&'static str>,
	scope: Option<&'static str>,
	guild: bool,
}
fn requirement(name: &str, args: &Value, source: &ChatMessage) -> Result<Requirement> {
	let mut r = Requirement::default();
	let platform = |key: &str| -> Result<&'static str> {
		match args[key].as_str() {
			Some("twitch") => Ok("twitch"),
			Some("youtube") => Ok("google"),
			Some("discord") => Ok("discord"),
			_ => anyhow::bail!("Unsupported platform"),
		}
	};
	match name {
		"resolveUserTarget" => {
			r.provider = Some(platform("platform")?);
			r.guild = r.provider == Some("discord");
		}
		"createPoll" | "closePoll" => {
			r.provider = Some(platform("platform")?);
			match r.provider {
				Some("twitch") => {
					r.group = Some("twitch_polls");
					r.scope = Some("channel:manage:polls");
				}
				Some("google") => {
					r.group = Some("youtube_polls");
					r.scope = Some("youtube_write");
				}
				_ => anyhow::bail!("Unsupported poll platform"),
			}
		}
		"inspectTwitchResources" => {
			r.provider = Some("twitch");
			if args["kind"] == "polls" {
				r.scope = Some("channel:manage:polls");
			}
		}
		"banTwitchUser" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_moderation");
			r.scope = Some("moderator:manage:banned_users");
		}
		"updateTwitchChatSettings" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_moderation");
			r.scope = Some("moderator:manage:chat_settings");
		}
		"twitchShoutout" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_moderation");
			r.scope = Some("moderator:manage:shoutouts");
		}
		"setTwitchTitle" | "setTwitchCategory" | "setTwitchTags" | "createTwitchMarker" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_broadcast");
			r.scope = Some("channel:manage:broadcast");
		}
		"startTwitchRaid" | "cancelTwitchRaid" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_broadcast");
			r.scope = Some("channel:manage:raids");
		}
		"startTwitchCommercial" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_broadcast");
			r.scope = Some("channel:edit:commercial");
		}
		"createTwitchClip" => {
			r.provider = Some("twitch");
			r.group = Some("twitch_broadcast");
			r.scope = Some("clips:edit");
		}
		"inspectYoutubeResources" => r.provider = Some("google"),
		"banYoutubeUser" | "deleteYoutubeMessage" => {
			r.provider = Some("google");
			r.group = Some("youtube_moderation");
			r.scope = Some("youtube_write");
		}
		"inspectDiscordResources" => {
			r.provider = Some("discord");
			r.guild = true;
		}
		"createDiscordChannel"
		| "editDiscordChannel"
		| "deleteDiscordChannel"
		| "createDiscordThread"
		| "deleteDiscordMessage"
		| "pinDiscordMessage"
		| "unpinDiscordMessage" => {
			r.provider = Some("discord");
			r.group = Some("discord_resources");
			r.guild = true;
		}
		"moveDiscordVoiceUser"
		| "disconnectDiscordVoiceUser"
		| "timeoutDiscordUser"
		| "banDiscordUser"
		| "discordManageRole"
		| "setDiscordNickname"
		| "discordStageControlUser" => {
			r.provider = Some("discord");
			r.group = Some("discord_moderation");
			r.guild = true;
		}
		"configureTurnDelivery"
			if args["targets"]
				.as_array()
				.is_some_and(|a| a.iter().any(|t| t == "discord_dm")) =>
		{
			r.provider = Some("discord")
		}
		"deliverMessage" => match args["target"].as_str() {
			Some("discord_dm") => r.provider = Some("discord"),
			Some("discord_channel") => {
				r.provider = Some("discord");
				r.group = Some("discord_resources");
				r.guild = true;
			}
			Some("twitch") => r.provider = Some("twitch"),
			Some("youtube") => {
				r.provider = Some("google");
				r.scope = Some("youtube_write");
			}
			Some("source") => {
				r.provider = match source.platform.as_str() {
					"twitch" => Some("twitch"),
					"youtube" => {
						r.scope = Some("youtube_write");
						Some("google")
					}
					"discord" | "discord_voice" => Some("discord"),
					_ => None,
				}
			}
			_ => anyhow::bail!("Unknown delivery target"),
		},
		"researchWeb" | "analyzeWithCodeInterpreter" | "generateImage" | "editImage" => {
			r.provider = Some("openai")
		}
		_ => {}
	}
	Ok(r)
}

pub(super) async fn bind_source(
	engine: &Engine,
	source: &ChatMessage,
	bindings: &mut AccessBindings,
	cancel: &CancellationToken,
) -> Result<()> {
	let snapshot = AccessBindings::snapshot_source(&engine.store.settings()?, source)?;
	for (key, value) in snapshot.identities {
		bindings.bind(&key, &value)?;
	}
	match source.platform.as_str() {
		"discord" | "discord_voice" => {
			let id = current_discord_identity(engine, cancel).await?;
			bindings.bind("discord_bot", &id)?;
		}
		"twitch" | "youtube" => {
			let provider = if source.platform == "twitch" {
				"twitch"
			} else {
				"google"
			};
			let tokens = tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),tokens=engine.providers.tokens(provider)=>tokens?};
			bindings.bind(provider, &tokens.account_id)?;
		}
		_ => {}
	}
	Ok(())
}
/// Revalidate previously captured identities without adding/rebinding them.
/// This also protects saved private-delivery targets when the source differs.
pub(super) async fn verify_bound_source(
	engine: &Engine,
	source: &ChatMessage,
	bindings: &AccessBindings,
	cancel: &CancellationToken,
) -> Result<()> {
	bindings.validate_source(&engine.store.settings()?, source)?;
	for provider in ["twitch", "google"] {
		if let Some(expected) = bindings.identities.get(provider) {
			let tokens = tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),tokens=engine.providers.tokens(provider)=>tokens?};
			ensure!(
				expected == &tokens.account_id,
				"The {provider} identity changed; cancel this saved request and start a new one"
			);
		}
	}
	if let Some(expected) = bindings.identities.get("discord_bot") {
		ensure!(
			expected == &current_discord_identity(engine, cancel).await?,
			"The Discord bot identity changed; cancel this saved request and start a new one"
		);
	}
	ensure!(!cancel.is_cancelled(), "Access check cancelled");
	Ok(())
}
async fn current_discord_identity(engine: &Engine, cancel: &CancellationToken) -> Result<String> {
	if let Some(audio) = engine
		.audio()
		.await
		.filter(|audio| audio.health().connected)
	{
		return Ok(audio.bot_user_id());
	}
	tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),identity=discord_identity(&engine.providers)=>identity}
}

pub(super) async fn inspect(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	bindings: &mut AccessBindings,
	cancel: &CancellationToken,
) -> Result<Option<AccessBlocker>> {
	let settings = engine.store.settings()?;
	let blocker = inspect_with(
		&engine.providers,
		&settings,
		source,
		name,
		args,
		bindings,
		cancel,
	)
	.await?;
	let required = requirement(name, args, source)?;
	// Existing Discord credentials are identified even while a local grant is
	// missing, binding the pending action to this same bot after continuation.
	if required.provider == Some("discord")
		&& engine
			.providers
			.secrets
			.get("discord_bot")?
			.is_some_and(|s| !s.trim().is_empty())
	{
		let identity = tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),result=discord_identity(&engine.providers)=>match result {
			Ok(identity)=>identity,
			Err(error) if error.downcast_ref::<AuthorizationRequired>().is_some()=>return Ok(Some(AccessBlocker::repair("Reconnect the Discord bot"))),
			Err(error)=>return Err(error),
		}};
		bindings.bind("discord_bot", &identity)?;
	}
	if blocker.is_some() {
		return Ok(blocker);
	}
	if let Some(provider @ ("twitch" | "google")) = required.provider {
		// Cached credentials alone cannot detect a revoked grant. This is a
		// read-only identity check; actual dispatch still rechecks permissions.
		let validated = async {
			let mut tokens = engine.providers.tokens(provider).await?;
			if provider == "twitch" {
				engine.providers.validate_twitch_tokens(&mut tokens).await?;
			} else {
				engine.providers.validate_google_tokens(&mut tokens).await?;
			}
			Ok::<_, anyhow::Error>(tokens)
		};
		let tokens = tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),result=validated=>match result {
			Ok(tokens)=>tokens,
			Err(error) if error.downcast_ref::<AuthorizationRequired>().is_some()=>return Ok(Some(AccessBlocker::repair(format!("Reconnect the same {provider} account")))),
			Err(error)=>return Err(error),
		}};
		bindings.bind(provider, &tokens.account_id)?;
		check_target(provider, name, args, &tokens)?;
		if scope_missing(required, &tokens) {
			return Ok(Some(AccessBlocker::repair(format!(
				"The {provider} account lacks the permission required by {name}"
			))));
		}
	}

	if required.guild {
		// A viewer may not use an access prompt to inspect owner-only resources.
		super::require_owner(engine, source).await?;
		return super::platform_tools::preflight_discord(engine, source, name, args, cancel).await;
	}
	Ok(None)
}
async fn discord_identity(providers: &Providers) -> Result<String> {
	let response = providers
		.http
		.get("https://discord.com/api/v10/users/@me")
		.header(
			"Authorization",
			format!("Bot {}", providers.secret("discord_bot")?),
		)
		.send()
		.await?;
	crate::providers::check_response("discord", &response)?;
	let body: Value = response.json().await?;
	ensure!(body["bot"] == true, "Discord credential is not a bot token");
	Ok(body["id"]
		.as_str()
		.context("Discord bot identity unavailable")?
		.into())
}
async fn inspect_with(
	providers: &Providers,
	settings: &Settings,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	bindings: &mut AccessBindings,
	cancel: &CancellationToken,
) -> Result<Option<AccessBlocker>> {
	ensure!(!cancel.is_cancelled(), "Access check cancelled");
	let r = requirement(name, args, source)?;
	// Capture existing account/configuration before a disabled-group prompt.
	// Otherwise reconnecting while that question waits could retarget the call.
	if let Some(provider @ ("twitch" | "google")) = r.provider {
		if let Some(saved) = providers
			.secrets
			.get(&format!("{provider}_tokens"))?
			.filter(|s| !s.trim().is_empty())
		{
			let tokens: Tokens =
				serde_json::from_str(&saved).context("Saved authorization is invalid; reconnect")?;
			bindings.bind(provider, &tokens.account_id)?;
			check_target(provider, name, args, &tokens)?;
		}
	}
	if r.provider == Some("discord") {
		if !settings.owner_discord_id.is_empty() {
			bindings.bind("discord_owner", &settings.owner_discord_id)?;
		}
		if r.guild && !settings.discord_guild_id.is_empty() {
			bindings.bind("discord_guild", &settings.discord_guild_id)?;
		}
		if r.guild {
			if let Some(id) = args["guildId"].as_str() {
				ensure!(
					settings.discord_guild_id.is_empty() || id == settings.discord_guild_id,
					"Approved guild differs from current Settings; request a new action"
				);
			}
			if settings.owner_discord_id.is_empty() || settings.discord_guild_id.is_empty() {
				return Ok(Some(AccessBlocker::repair(
					"Configure the Discord owner and guild",
				)));
			}
		}
	}
	if !crate::settings::tool_enabled(settings, name) {
		return Ok(Some(AccessBlocker::repair(format!(
			"The {name} capability is disabled in Settings"
		))));
	}
	if let Some(group) = r.group {
		if !settings.enabled_tool_groups.iter().any(|g| g == group) {
			return Ok(Some(AccessBlocker::repair(format!(
				"The {group} tool group is disabled"
			))));
		}
	}
	if let Some(provider) = r.provider {
		if matches!(provider, "twitch" | "google") {
			let tokens = tokio::select! {biased;_=cancel.cancelled()=>anyhow::bail!("Access check cancelled"),result=providers.tokens(provider)=>match result {
				Ok(tokens)=>tokens,
				Err(error) if error.downcast_ref::<AuthorizationRequired>().is_some()=>return Ok(Some(AccessBlocker::repair(format!("Authorize {provider} in Settings")))),
				Err(error)=>return Err(error),
			}};
			bindings.bind(provider, &tokens.account_id)?;
			check_target(provider, name, args, &tokens)?;

			if scope_missing(r, &tokens) {
				return Ok(Some(AccessBlocker::repair(format!(
					"The {provider} account lacks the permission required by {name}; reconnect after enabling its tool group"
				))));
			}
		} else {
			let key = if provider == "discord" {
				"discord_bot"
			} else {
				provider
			};
			match providers.secret(key) {
				Ok(_) => {}
				Err(error) if error.downcast_ref::<AuthorizationRequired>().is_some() => {
					return Ok(Some(AccessBlocker::repair(format!(
						"Configure the {provider} credentials"
					))));
				}
				Err(error) => return Err(error),
			}
		}
	}
	ensure!(!cancel.is_cancelled(), "Access check cancelled");
	Ok(None)
}
fn scope_missing(r: Requirement, tokens: &Tokens) -> bool {
	let missing = r.scope.is_some_and(|scope| {
		if scope == "youtube_write" {
			!tokens.scopes.iter().any(|s| {
				matches!(
					s.as_str(),
					"https://www.googleapis.com/auth/youtube.force-ssl"
						| "https://www.googleapis.com/auth/youtube"
				)
			})
		} else {
			!tokens.scopes.iter().any(|s| s == scope)
		}
	});
	missing
		|| r.provider == Some("twitch")
			&& ["user:read:chat", "user:write:chat"]
				.iter()
				.any(|s| !tokens.scopes.iter().any(|scope| scope == s))
}
fn check_target(provider: &str, name: &str, args: &Value, tokens: &Tokens) -> Result<()> {
	if provider == "twitch" && name != "inspectTwitchResources" {
		let target = if matches!(name, "createPoll" | "closePoll") {
			args["channelId"].as_str()
		} else if name == "deliverMessage" && args["target"] == "twitch" {
			args["destinationId"].as_str()
		} else {
			args["broadcasterId"].as_str()
		};
		if let Some(target) = target {
			ensure!(
				target == tokens.account_id,
				"Saved Twitch target differs from the authorized account; request a new action"
			);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{providers::SecretStore, storage::Store};
	use serde_json::json;
	use std::sync::{Arc, Mutex};
	#[derive(Default)]
	struct MemorySecrets(Mutex<BTreeMap<String, String>>);
	impl SecretStore for MemorySecrets {
		fn get(&self, key: &str) -> Result<Option<String>> {
			Ok(self.0.lock().unwrap().get(key).cloned())
		}
		fn set(&self, key: &str, value: &str) -> Result<()> {
			self.0.lock().unwrap().insert(key.into(), value.into());
			Ok(())
		}
		fn delete(&self, key: &str) -> Result<()> {
			self.0.lock().unwrap().remove(key);
			Ok(())
		}
	}
	fn fixture() -> (tempfile::TempDir, Arc<Providers>, ChatMessage) {
		let dir = tempfile::tempdir().unwrap();
		let store = Arc::new(Store::open(&dir.path().join("test.sqlite")).unwrap());
		let providers = Providers::new(store, Arc::new(MemorySecrets::default())).unwrap();
		let mut settings = providers.store.settings().unwrap();
		settings.twitch_client_id = "test-client".into();
		settings.google_client_id = "test-google".into();
		providers.store.set("installation", &settings).unwrap();
		let source = ChatMessage {
			platform: "preview".into(),
			user_id: "owner".into(),
			display_name: "Owner".into(),
			message_id: "message".into(),
			channel_id: "dashboard".into(),
			text: "Change my title".into(),
			is_owner: true,
			access: Default::default(),
		};
		(dir, providers, source)
	}
	fn save(providers: &Providers, account: &str, scope: bool, expired: bool) {
		let mut scopes = vec!["user:read:chat".into(), "user:write:chat".into()];
		if scope {
			scopes.push("channel:manage:broadcast".into());
		}
		providers
			.secrets
			.set(
				"twitch_tokens",
				&serde_json::to_string(&Tokens {
					access_token: "fixture-token-never-sent".into(),
					refresh_token: String::new(),
					expires_at: if expired {
						0
					} else {
						crate::now_ms() + 3600000
					},
					scopes,
					account_id: account.into(),
					login: "fixture".into(),
					client_id: "test-client".into(),
					validated: true,
				})
				.unwrap(),
			)
			.unwrap();
	}
	async fn check(
		providers: &Providers,
		source: &ChatMessage,
		name: &str,
		args: Value,
		bindings: &mut AccessBindings,
	) -> Result<Option<AccessBlocker>> {
		inspect_with(
			providers,
			&providers.store.settings()?,
			source,
			name,
			&args,
			bindings,
			&CancellationToken::new(),
		)
		.await
	}
	#[tokio::test]
	async fn repeated_missing_grant_scope_and_expiry_stay_blocked_until_explicit_repair() {
		let (_dir, p, source) = fixture();
		save(&p, "123", false, false);
		let args = json!({"broadcasterId":"123","title":"Exact saved title"});
		let mut binding = AccessBindings::default();
		for _ in 0..2 {
			assert!(
				check(&p, &source, "setTwitchTitle", args.clone(), &mut binding)
					.await
					.unwrap()
					.unwrap()
					.prompt
					.contains("twitch_broadcast")
			);
		}
		// A prompt cannot enable its own grant, alter the request, or write a receipt.
		assert!(p.store.settings().unwrap().enabled_tool_groups.is_empty());
		let mut settings = p.store.settings().unwrap();
		settings.enabled_tool_groups.push("twitch_broadcast".into());
		p.store.set("installation", &settings).unwrap();
		assert!(
			check(&p, &source, "setTwitchTitle", args.clone(), &mut binding)
				.await
				.unwrap()
				.unwrap()
				.prompt
				.contains("permission")
		);
		save(&p, "123", true, true);
		assert!(
			check(&p, &source, "setTwitchTitle", args.clone(), &mut binding)
				.await
				.unwrap()
				.unwrap()
				.prompt
				.contains("Authorize")
		);
		save(&p, "123", true, false);
		assert!(
			check(&p, &source, "setTwitchTitle", args, &mut binding)
				.await
				.unwrap()
				.is_none()
		);
		assert_eq!(
			binding.identities.get("twitch").map(String::as_str),
			Some("123")
		);
	}
	#[tokio::test]
	async fn changed_account_after_wait_or_restart_is_terminal_even_when_grant_is_still_missing() {
		let (_dir, p, source) = fixture();
		save(&p, "123", false, false);
		let mut binding = AccessBindings::default();
		assert!(
			check(
				&p,
				&source,
				"setTwitchTitle",
				json!({"broadcasterId":"123"}),
				&mut binding
			)
			.await
			.unwrap()
			.is_some()
		);
		let mut recovered: AccessBindings =
			serde_json::from_value(serde_json::to_value(&binding).unwrap()).unwrap();
		save(&p, "456", true, false);
		let error = check(
			&p,
			&source,
			"setTwitchTitle",
			json!({"broadcasterId":"123"}),
			&mut recovered,
		)
		.await
		.unwrap_err();
		assert!(error.to_string().contains("identity changed"));
		assert!(error.downcast_ref::<AuthorizationRequired>().is_none());
	}
	#[tokio::test]
	async fn missing_tokens_preserve_existing_binding_and_wrong_targets_are_not_access_prompts() {
		let (_dir, p, source) = fixture();
		save(&p, "123", true, false);
		let mut binding = AccessBindings::default();
		assert!(
			check(
				&p,
				&source,
				"setTwitchTitle",
				json!({"broadcasterId":"999"}),
				&mut binding
			)
			.await
			.is_err()
		);
		p.secrets.delete("twitch_tokens").unwrap();
		let mut settings = p.store.settings().unwrap();
		settings.enabled_tool_groups.push("twitch_broadcast".into());
		p.store.set("installation", &settings).unwrap();
		assert!(
			check(
				&p,
				&source,
				"setTwitchTitle",
				json!({"broadcasterId":"123"}),
				&mut binding
			)
			.await
			.unwrap()
			.is_some()
		);
		save(&p, "456", true, false);
		assert!(
			check(
				&p,
				&source,
				"setTwitchTitle",
				json!({"broadcasterId":"123"}),
				&mut binding
			)
			.await
			.is_err()
		);
	}
	#[tokio::test]
	async fn discord_owner_and_guild_are_bound_before_missing_credential_prompt() {
		let (_dir, p, source) = fixture();
		let mut settings = p.store.settings().unwrap();
		settings.owner_discord_id = "111".into();
		settings.discord_guild_id = "222".into();
		p.store.set("installation", &settings).unwrap();
		let mut binding = AccessBindings::default();
		assert!(
			check(
				&p,
				&source,
				"inspectDiscordResources",
				json!({"kind":"channels"}),
				&mut binding
			)
			.await
			.unwrap()
			.is_some()
		);
		settings.owner_discord_id = "333".into();
		p.store.set("installation", &settings).unwrap();
		assert!(
			check(
				&p,
				&source,
				"inspectDiscordResources",
				json!({"kind":"channels"}),
				&mut binding
			)
			.await
			.unwrap_err()
			.to_string()
			.contains("identity changed")
		);
		settings.owner_discord_id = "111".into();
		settings.discord_guild_id = "444".into();
		p.store.set("installation", &settings).unwrap();
		assert!(
			check(
				&p,
				&source,
				"inspectDiscordResources",
				json!({"kind":"channels"}),
				&mut binding
			)
			.await
			.is_err()
		);
	}
	#[test]
	fn saved_public_source_requires_same_guild_channel_and_bot_but_dm_ignores_public_channel() {
		let (_dir, p, mut source) = fixture();
		let mut settings = p.store.settings().unwrap();
		settings.owner_discord_id = "111".into();
		settings.discord_guild_id = "222".into();
		settings.discord_text_channel_id = "333".into();
		source.platform = "discord".into();
		source.channel_id = "333".into();
		let mut bindings = AccessBindings::snapshot_source(&settings, &source).unwrap();
		bindings.bind("discord_bot", "444").unwrap();
		bindings.validate_source(&settings, &source).unwrap();
		settings.discord_text_channel_id = "555".into();
		assert!(bindings.validate_source(&settings, &source).is_err());
		assert!(bindings.bind("discord_bot", "666").is_err());
		source.channel_id = "private-dm".into();
		let mut private = AccessBindings::snapshot_source(&settings, &source).unwrap();
		private.bind("discord_bot", "444").unwrap();
		settings.discord_text_channel_id = "777".into();
		settings.discord_guild_id = "888".into();
		private.validate_source(&settings, &source).unwrap();
		assert!(
			AccessBindings::default()
				.validate_source(&settings, &source)
				.is_err()
		);
	}
	#[tokio::test]
	async fn cancelled_preflight_never_opens_a_new_access_prompt() {
		let (_dir, p, source) = fixture();
		let cancel = CancellationToken::new();
		cancel.cancel();
		assert!(
			inspect_with(
				&p,
				&p.store.settings().unwrap(),
				&source,
				"setTwitchTitle",
				&json!({}),
				&mut AccessBindings::default(),
				&cancel
			)
			.await
			.is_err()
		);
	}
	#[test]
	fn platform_registry_is_covered_and_one_poll_never_requests_another_platforms_grants() {
		let (_dir, _p, source) = fixture();
		for definition in super::super::platform_tools::definitions() {
			let r = requirement(&definition.name, &json!({"platform":"twitch"}), &source).unwrap();
			assert!(
				r.provider.is_some(),
				"unclassified platform tool {}",
				definition.name
			);
		}
		let twitch = requirement("createPoll", &json!({"platform":"twitch"}), &source).unwrap();
		let youtube = requirement("createPoll", &json!({"platform":"youtube"}), &source).unwrap();
		assert_eq!(twitch.group, Some("twitch_polls"));
		assert_eq!(youtube.group, Some("youtube_polls"));
		assert_eq!(twitch.provider, Some("twitch"));
		assert_eq!(youtube.provider, Some("google"));
	}
}
