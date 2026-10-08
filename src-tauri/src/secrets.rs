use anyhow::{Context, Result};
use bumblebee_core::providers::SecretStore;

pub struct OsSecrets {
	service: String,
}
impl OsSecrets {
	pub fn new(service: String) -> Self {
		Self { service }
	}
	fn entry(&self, name: &str) -> Result<keyring::Entry> {
		anyhow::ensure!(
			[
				"azure_speech",
				"openai",
				"discord_bot",
				"google_client_secret",
				"twitch_tokens",
				"google_tokens"
			]
			.contains(&name),
			"Unknown credential type"
		);
		keyring::Entry::new(&self.service, name)
			.context("Cannot open the operating-system credential store")
	}
}
impl SecretStore for OsSecrets {
	fn get(&self, name: &str) -> Result<Option<String>> {
		match self.entry(name)?.get_password() {
			Ok(value) => Ok(Some(value)),
			Err(keyring::Error::NoEntry) => Ok(None),
			Err(_) => anyhow::bail!(
				"Cannot read the operating-system credential store. Unlock your keyring and try again."
			),
		}
	}
	fn set(&self, name: &str, value: &str) -> Result<()> {
		self.entry(name)?.set_password(value).map_err(|_| {
			anyhow::anyhow!(
				"Cannot save to the operating-system credential store. Unlock your keyring and try again."
			)
		})
	}
	fn delete(&self, name: &str) -> Result<()> {
		match self.entry(name)?.delete_credential() {
			Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
			Err(_) => anyhow::bail!(
				"Cannot remove the credential. Unlock your operating-system keyring and try again."
			),
		}
	}
}
