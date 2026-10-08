use crate::{
	catalog, images,
	model::{ChatMessage, Chatter},
	now_ms,
	storage::Store,
};
use anyhow::Result;
use std::path::Path;

pub struct CommandResult {
	pub reply: String,
	pub changed: Option<Chatter>,
}

pub async fn handle(
	store: &Store,
	message: &ChatMessage,
	images_dir: &Path,
) -> Result<Option<CommandResult>> {
	let (command, argument) = message
		.text
		.trim()
		.split_once(char::is_whitespace)
		.unwrap_or((message.text.trim(), ""));
	let argument = argument.trim();
	let command = command.to_ascii_lowercase();
	if !matches!(command.as_str(), "!puppet" | "!voice" | "!voices") {
		return Ok(None);
	}
	let chatter =
		store.ensure_chatter(&message.platform, &message.user_id, &message.display_name)?;
	let reply = |text: String| {
		Ok(Some(CommandResult {
			reply: text,
			changed: None,
		}))
	};
	if !store.claim_command(&message.platform, &message.user_id, now_ms())? {
		return reply("Please wait a few seconds before another customization command.".into());
	}
	if command == "!voices" {
		let (search, page) = voice_search(argument);
		let voices = catalog::eligible_voices(&store.catalog_voices()?);
		let matching: Vec<_> = voices
			.iter()
			.filter(|v| {
				format!(
					"{} {} {}",
					v.id,
					v.name.as_deref().unwrap_or(""),
					v.voice_name
				)
				.to_lowercase()
				.contains(&search.to_lowercase())
			})
			.collect();
		let total = matching.len();
		let items = matching
			.into_iter()
			.skip(page.saturating_sub(1).saturating_mul(5))
			.take(5)
			.map(|v| v.id.clone())
			.collect::<Vec<_>>();
		return reply(if items.is_empty() {
			"No matching voices on that page. Try !voices en-US or !voices dandy.".into()
		} else {
			format!(
				"{} (page {page}/{}{})",
				items.join(", "),
				total.div_ceil(5),
				if page < total.div_ceil(5) {
					format!(", use !voices {search} page:{}", page + 1)
				} else {
					String::new()
				}
			)
		});
	}
	if argument.is_empty() {
		return reply(if command == "!puppet" {
			format!(
				"Puppet: {}. Use !puppet random, !puppet <name>, or !puppet <https-image-url> (approval required).",
				if chatter.image_hash.is_some() {
					"custom image"
				} else {
					&chatter.puppet_id
				}
			)
		} else {
			format!(
				"Voice: {}. Use !voice random, !voice <name>, or !voices <search>.",
				chatter.voice_id
			)
		});
	}
	if chatter.customization_blocked {
		return reply("Customization is disabled for your profile by the streamer.".into());
	}
	if command == "!puppet" && argument.to_ascii_lowercase().starts_with("https://") {
		if !store.settings()?.custom_images_enabled {
			return reply("Custom image submissions are disabled on this stream.".into());
		}
		let request = store.begin_image_submission(&message.platform, &message.user_id)?;
		let hash = match images::download_image(argument, images_dir).await {
			Ok(hash) => hash,
			Err(error) => return reply(format!("Image not submitted: {error}")),
		};
		if store
			.complete_image_submission(&message.platform, &message.user_id, &request, &hash)?
			.is_none()
		{
			return reply("This image request was superseded or customization was disabled.".into());
		}
		return reply(
			"Image submitted for streamer approval. Your current puppet stays until it is approved."
				.into(),
		);
	}
	let changed = if command == "!puppet" {
		let puppets = catalog::puppets();
		if puppets.is_empty() {
			return reply("The built-in puppet catalog is unavailable.".into());
		}
		let candidates: Vec<_> = puppets
			.iter()
			.filter(|p| p.id != chatter.puppet_id || puppets.len() == 1)
			.collect();
		let selected = if argument.eq_ignore_ascii_case("random") {
			Some(candidates[catalog::random_index(candidates.len())])
		} else {
			puppets
				.iter()
				.find(|p| p.id.eq_ignore_ascii_case(argument) || p.name.eq_ignore_ascii_case(argument))
		};
		let Some(selected) = selected else {
			return reply(format!(
				"Unknown puppet. Available: {}",
				puppets
					.iter()
					.map(|p| p.id.as_str())
					.collect::<Vec<_>>()
					.join(", ")
			));
		};
		store.select_puppet(&message.platform, &message.user_id, &selected.id)?
	} else {
		let voices = catalog::eligible_voices(&store.catalog_voices()?);
		if voices.is_empty() {
			return reply("The voice catalog is unavailable.".into());
		}
		let candidates: Vec<_> = voices
			.iter()
			.filter(|v| v.id != chatter.voice_id || voices.len() == 1)
			.collect();
		let id = if argument.eq_ignore_ascii_case("random") {
			candidates[catalog::random_index(candidates.len())]
				.id
				.clone()
		} else {
			match catalog::resolve_voice(&voices, argument) {
				Ok(voice) => voice.id.clone(),
				Err(error) => return reply(error.to_string()),
			}
		};
		store.select_voice(&message.platform, &message.user_id, &id)?
	};
	Ok(Some(CommandResult {
		reply: format!(
			"Updated {} to {}.",
			if command == "!puppet" {
				"puppet"
			} else {
				"voice"
			},
			if command == "!puppet" {
				&changed.puppet_id
			} else {
				&changed.voice_id
			}
		),
		changed: Some(changed),
	}))
}

fn voice_search(argument: &str) -> (&str, usize) {
	let (search, last) = argument
		.rsplit_once(char::is_whitespace)
		.unwrap_or(("", argument));
	match last
		.strip_prefix("page:")
		.and_then(|page| page.parse::<usize>().ok())
	{
		Some(page) => (search.trim(), page.max(1)),
		None => (argument, 1),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn message(text: &str) -> ChatMessage {
		ChatMessage {
			platform: "twitch".into(),
			user_id: "123".into(),
			display_name: "Viewer".into(),
			message_id: uuid::Uuid::new_v4().to_string(),
			channel_id: "channel".into(),
			text: text.into(),
			is_owner: false,
		}
	}
	#[tokio::test]
	async fn recognized_commands_do_not_change_unrelated_preferences() {
		let store = Store::open(Path::new(":memory:")).unwrap();
		let before = store.ensure_chatter("twitch", "123", "Viewer").unwrap();
		let result = handle(&store, &message("!voice random"), Path::new("unused"))
			.await
			.unwrap()
			.unwrap();
		let changed = result.changed.unwrap();
		assert_eq!(changed.puppet_id, before.puppet_id);
		assert_ne!(changed.voice_id, before.voice_id);
		assert!(
			catalog::eligible_voices(&store.catalog_voices().unwrap())
				.iter()
				.any(|voice| voice.id == changed.voice_id)
		);
		let cooldown = handle(&store, &message("!puppet random"), Path::new("unused"))
			.await
			.unwrap()
			.unwrap();
		assert!(cooldown.changed.is_none());
		assert_eq!(
			store.chatter("twitch", "123").unwrap().puppet_id,
			before.puppet_id
		);
		assert!(
			handle(&store, &message("ordinary chat"), Path::new("unused"))
				.await
				.unwrap()
				.is_none()
		);
	}
	#[tokio::test]
	async fn unknown_and_bumblebee_voice_names_leave_the_profile_unchanged() {
		for voice in ["ana", "not-a-voice"] {
			let store = Store::open(Path::new(":memory:")).unwrap();
			let before = store.ensure_chatter("twitch", "123", "Viewer").unwrap();
			let result = handle(
				&store,
				&message(&format!("!voice {voice}")),
				Path::new("unused"),
			)
			.await
			.unwrap()
			.unwrap();
			assert!(result.changed.is_none());
			assert_eq!(
				store.chatter("twitch", "123").unwrap().voice_id,
				before.voice_id
			);
		}
	}
	#[test]
	fn pagination_handles_empty_and_multiword_searches() {
		assert_eq!(voice_search("page:2"), ("", 2));
		assert_eq!(
			voice_search("Queen Beeatrice page:2"),
			("Queen Beeatrice", 2)
		);
		assert_eq!(voice_search("en-US"), ("en-US", 1));
		assert_eq!(voice_search("page:0"), ("", 1));
	}
}
