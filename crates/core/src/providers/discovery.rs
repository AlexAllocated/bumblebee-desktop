//! Named provider resources for local Settings. Credentials never leave the core.
use super::{Providers, check_response};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordOption {
	pub id: String,
	pub name: String,
	pub kind: u64,
	pub parent_id: Option<String>,
}

#[derive(Serialize)]
pub struct DiscordOptions {
	pub channels: Vec<DiscordOption>,
	pub roles: Vec<DiscordOption>,
}

fn options(value: Value) -> Vec<DiscordOption> {
	let mut result: Vec<_> = value
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|item| {
			Some(DiscordOption {
				id: item["id"].as_str()?.into(),
				name: item["name"].as_str()?.into(),
				kind: item["type"].as_u64().unwrap_or(0),
				parent_id: item["parent_id"].as_str().map(str::to_owned),
			})
		})
		.collect();
	result.sort_by(|a, b| {
		a.name
			.to_lowercase()
			.cmp(&b.name.to_lowercase())
			.then(a.id.cmp(&b.id))
	});
	result
}

impl Providers {
	async fn discord_settings_get(&self, path: &str) -> Result<Value> {
		let token = self.secret("discord_bot")?;
		let response = self
			.http
			.get(format!("https://discord.com/api/v10/{path}"))
			.header("Authorization", format!("Bot {token}"))
			.send()
			.await?;
		check_response("discord", &response)?;
		Ok(response.json().await?)
	}

	pub async fn discord_guilds(&self) -> Result<Vec<DiscordOption>> {
		Ok(options(
			self
				.discord_settings_get("users/@me/guilds?limit=200")
				.await?,
		))
	}

	pub async fn discord_options(&self, guild_id: &str) -> Result<DiscordOptions> {
		ensure!(
			guild_id.parse::<u64>().is_ok_and(|id| id > 0),
			"Select a Discord server first"
		);
		let channels_path = format!("guilds/{guild_id}/channels");
		let roles_path = format!("guilds/{guild_id}/roles");
		let (channels, roles) = tokio::try_join!(
			self.discord_settings_get(&channels_path),
			self.discord_settings_get(&roles_path),
		)?;
		Ok(DiscordOptions {
			channels: options(channels),
			roles: options(roles),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn named_resources_keep_voice_channel_text_and_ignore_incomplete_rows() {
		let result = options(serde_json::json!([
			 {"id":"12","name":"Live","type":2,"parent_id":"10","permissions":"8"},
			 {"id":"11","name":"chat","type":0}, {"id":"13"}
		]));
		assert_eq!(result.len(), 2);
		assert_eq!(result[1].kind, 2);
		assert_eq!(result[1].parent_id.as_deref(), Some("10"));
		assert!(
			!serde_json::to_string(&result)
				.unwrap()
				.contains("permissions")
		);
	}
}
