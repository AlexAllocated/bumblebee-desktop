use super::*;
use crate::providers::oauth::Tokens;
use reqwest::Method;

pub(super) fn definitions() -> Vec<ToolDefinition> {
	let broadcaster = || ("broadcasterId", id());
	vec![
		definition(
			"inspectTwitchResources",
			"Inspect the authorized Twitch account, its stream, polls or chat modes, or discover categories/channels. No changes are made.",
			vec![
				(
					"kind",
					choice(&[
						"channel",
						"stream",
						"categories",
						"channels",
						"polls",
						"chat_settings",
					]),
				),
				("query", optional(string(100))),
				("cursor", optional(string(512))),
			],
			false,
			false,
		),
		definition(
			"banTwitchUser",
			"Ban or time out an exact user in the authorized broadcaster's chat.",
			vec![
				broadcaster(),
				("userId", id()),
				("durationSeconds", optional(integer(1, 1209600))),
				("reason", optional(string(500))),
			],
			true,
			true,
		),
		definition(
			"startTwitchRaid",
			"Start a raid from the authorized broadcaster to an exact discovered channel.",
			vec![broadcaster(), ("targetBroadcasterId", id())],
			true,
			true,
		),
		definition(
			"cancelTwitchRaid",
			"Cancel a pending raid from the exact authorized broadcaster.",
			vec![broadcaster()],
			true,
			true,
		),
		definition(
			"startTwitchCommercial",
			"Start a commercial during the authorized broadcaster's live stream. Twitch may serve a different ad duration.",
			vec![
				broadcaster(),
				(
					"lengthSeconds",
					json!({"type":"integer","enum":[30,60,90,120,150,180]}),
				),
			],
			true,
			true,
		),
		definition(
			"setTwitchTitle",
			"Change and read back the authorized broadcaster's stream title.",
			vec![broadcaster(), ("title", string(140))],
			true,
			true,
		),
		definition(
			"setTwitchCategory",
			"Set the exact game/category ID discovered from inspectTwitchResources; never guess an ambiguous category name.",
			vec![broadcaster(), ("categoryId", id())],
			true,
			true,
		),
		definition(
			"setTwitchTags",
			"Replace the authorized channel's tag list.",
			vec![
				broadcaster(),
				(
					"tags",
					json!({"type":"array","items":string(25),"maxItems":10}),
				),
			],
			true,
			true,
		),
		definition(
			"createTwitchMarker",
			"Create a marker at the current live-stream position.",
			vec![broadcaster(), ("description", optional(string(140)))],
			true,
			true,
		),
		definition(
			"createTwitchClip",
			"Request a clip of the current live stream. An accepted clip may still be processing.",
			vec![broadcaster()],
			true,
			true,
		),
		definition(
			"updateTwitchChatSettings",
			"Change explicit Twitch chat modes. Null leaves a setting unchanged.",
			vec![
				broadcaster(),
				("slowModeEnabled", optional(boolean())),
				("slowModeDelaySeconds", optional(integer(3, 120))),
				("followerOnlyModeEnabled", optional(boolean())),
				("followerOnlyModeDelayMinutes", optional(integer(0, 129600))),
				("subscriberOnlyModeEnabled", optional(boolean())),
				("emoteOnlyModeEnabled", optional(boolean())),
				("uniqueChatModeEnabled", optional(boolean())),
			],
			true,
			true,
		),
		definition(
			"twitchShoutout",
			"Send a Twitch shoutout to an exact discovered broadcaster.",
			vec![broadcaster(), ("targetBroadcasterId", id())],
			true,
			true,
		),
	]
}

struct Twitch<'a> {
	engine: &'a Engine,
	source: &'a ChatMessage,
	tokens: Tokens,
	cancel: CancellationToken,
}
impl<'a> Twitch<'a> {
	async fn new(
		engine: &'a Engine,
		source: &'a ChatMessage,
		cancel: CancellationToken,
	) -> Result<Self> {
		let mut tokens = engine.providers.tokens("twitch").await?;
		let previous = tokens.account_id.clone();
		engine.providers.validate_twitch_tokens(&mut tokens).await?;
		ensure!(
			tokens.account_id == previous,
			"Twitch account changed; reconnect before using tools"
		);
		Ok(Self {
			engine,
			source,
			tokens,
			cancel,
		})
	}
	fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
		self
			.engine
			.providers
			.http
			.request(method, format!("https://api.twitch.tv/helix/{path}"))
			.bearer_auth(&self.tokens.access_token)
			.header("Client-Id", &self.tokens.client_id)
	}
	fn own(&self, id: &str) -> Result<()> {
		ensure!(
			id == self.tokens.account_id,
			"The confirmed broadcaster does not match the currently authorized Twitch account"
		);
		Ok(())
	}
	fn scope(&self, scope: &str) -> Result<()> {
		ensure!(
			self.tokens.scopes.iter().any(|s| s == scope),
			"Twitch permission {scope} is missing; enable its tool group and reconnect Twitch in Settings"
		);
		Ok(())
	}
	async fn read(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
		request(
			self.request(Method::GET, path).query(query),
			"Twitch",
			false,
			&self.cancel,
		)
		.await
	}
	async fn mutate(
		&self,
		group: &str,
		scope: &str,
		request_builder: reqwest::RequestBuilder,
	) -> Result<Value> {
		super::super::require_owner(self.engine, self.source).await?;
		grant(self.engine, group)?;
		self.scope(scope)?;
		request(request_builder, "Twitch", true, &self.cancel).await
	}
}

pub(super) async fn resolve(
	engine: &Engine,
	source: &ChatMessage,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	let client = Twitch::new(engine, source, cancel).await?;
	let query = text(args, "query", 100)?.trim_start_matches('@');
	ensure!(
		query.len() <= 25
			&& query
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || b == b'_'),
		"Provide an exact Twitch login, without a URL"
	);
	Ok(receipt(
		"verified",
		client.read("users", &[("login", query)]).await?,
	))
}

pub(super) async fn execute(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	let client = Twitch::new(engine, source, cancel).await?;
	let broadcaster = client.tokens.account_id.as_str();
	if name == "inspectTwitchResources" {
		let query = optional_text(args, "query", 100)?.unwrap_or("");
		let cursor = optional_text(args, "cursor", 512)?;
		let mut params = vec![];
		let endpoint = match text(args, "kind", 30)? {
			"channel" => {
				params.push(("broadcaster_id", broadcaster));
				"channels"
			}
			"stream" => {
				params.push(("user_id", broadcaster));
				"streams"
			}
			"categories" => {
				ensure!(!query.is_empty(), "Supply a category search query");
				params.extend([("query", query), ("first", "20")]);
				"search/categories"
			}
			"channels" => {
				ensure!(!query.is_empty(), "Supply a channel search query");
				params.extend([("query", query), ("first", "20")]);
				"search/channels"
			}
			"polls" => {
				client.scope("channel:manage:polls")?;
				params.extend([("broadcaster_id", broadcaster), ("first", "20")]);
				"polls"
			}
			"chat_settings" => {
				params.push(("broadcaster_id", broadcaster));
				"chat/settings"
			}
			_ => anyhow::bail!("Unsupported Twitch resource kind"),
		};
		if let Some(cursor) = cursor {
			if matches!(endpoint, "search/categories" | "search/channels" | "polls") {
				params.push(("after", cursor))
			}
		}
		return Ok(receipt(
			"verified",
			json!({"accountId":broadcaster,"login":client.tokens.login,"result":client.read(endpoint,&params).await?}),
		));
	}
	let expected = if matches!(name, "createPoll" | "closePoll") {
		snowflake(args, "channelId")?
	} else {
		snowflake(args, "broadcasterId")?
	};
	client.own(expected)?;
	let bparams = [("broadcaster_id", broadcaster)];
	let body = match name {
		"banTwitchUser" => {
			let user = snowflake(args, "userId")?;
			ensure!(user != broadcaster, "Cannot ban the broadcaster");
			let mut data = json!({"user_id":user});
			if !args["durationSeconds"].is_null() {
				data["duration"] = number(args, "durationSeconds", 1, 1209600)?.into();
			}
			if let Some(reason) = optional_text(args, "reason", 500)? {
				data["reason"] = reason.into();
			}
			client
				.mutate(
					"twitch_moderation",
					"moderator:manage:banned_users",
					client
						.request(Method::POST, "moderation/bans")
						.query(&[
							("broadcaster_id", broadcaster),
							("moderator_id", broadcaster),
						])
						.json(&json!({"data":data})),
				)
				.await?
		}
		"startTwitchRaid" => {
			let target = snowflake(args, "targetBroadcasterId")?;
			ensure!(target != broadcaster, "Cannot raid your own channel");
			let streams = client.read("streams", &[("user_id", target)]).await?;
			ensure!(
				streams["data"].as_array().is_some_and(|a| !a.is_empty()),
				"The selected raid target is not live"
			);
			client
				.mutate(
					"twitch_broadcast",
					"channel:manage:raids",
					client.request(Method::POST, "raids").query(&[
						("from_broadcaster_id", broadcaster),
						("to_broadcaster_id", target),
					]),
				)
				.await?
		}
		"cancelTwitchRaid" => {
			client
				.mutate(
					"twitch_broadcast",
					"channel:manage:raids",
					client.request(Method::DELETE, "raids").query(&bparams),
				)
				.await?
		}
		"startTwitchCommercial" => {
			let length = number(args, "lengthSeconds", 30, 180)?;
			ensure!(
				length % 30 == 0,
				"Commercial duration must be a multiple of 30 seconds"
			);
			client
				.mutate(
					"twitch_broadcast",
					"channel:edit:commercial",
					client
						.request(Method::POST, "channels/commercial")
						.json(&json!({"broadcaster_id":broadcaster,"length":length})),
				)
				.await?
		}
		"setTwitchTitle" | "setTwitchCategory" | "setTwitchTags" => {
			let patch = match name {
				"setTwitchTitle" => json!({"title":text(args,"title",140)?}),
				"setTwitchCategory" => {
					let game = snowflake(args, "categoryId")?;
					let result = client.read("games", &[("id", game)]).await?;
					ensure!(
						result["data"][0]["id"].as_str() == Some(game),
						"The selected category no longer exists"
					);
					json!({"game_id":game})
				}
				_ => {
					let tags = args["tags"].as_array().context("tags must be an array")?;
					ensure!(tags.len() <= 10, "At most ten Twitch tags are supported");
					let mut unique = std::collections::HashSet::new();
					for tag in tags {
						let tag = tag.as_str().context("Each tag must be text")?;
						ensure!(
							!tag.is_empty()
								&& tag.chars().count() <= 25
								&& tag.chars().all(char::is_alphanumeric),
							"Tags must contain 1 to 25 letters or numbers"
						);
						ensure!(
							unique.insert(tag.to_lowercase()),
							"Duplicate tags are not allowed"
						);
					}
					json!({"tags":tags})
				}
			};
			client
				.mutate(
					"twitch_broadcast",
					"channel:manage:broadcast",
					client
						.request(Method::PATCH, "channels")
						.query(&bparams)
						.json(&patch),
				)
				.await?;
			let observed = client.read("channels", &bparams).await.map_err(|_| {
				UncertainOutcome(
					"Twitch accepted the channel change, but read-back verification failed".into(),
				)
			})?;
			for (key, value) in patch.as_object().unwrap() {
				if observed["data"][0][key] != *value {
					return Err(
						UncertainOutcome(format!(
							"Twitch channel {key} does not match the acknowledged change; inspect before retrying"
						))
						.into(),
					);
				}
			}
			return Ok(receipt("verified", observed));
		}
		"createTwitchMarker" => {
			let mut body = json!({"user_id":broadcaster});
			if let Some(description) = optional_text(args, "description", 140)? {
				body["description"] = description.into();
			}
			client
				.mutate(
					"twitch_broadcast",
					"channel:manage:broadcast",
					client.request(Method::POST, "streams/markers").json(&body),
				)
				.await?
		}
		"createTwitchClip" => {
			client
				.mutate(
					"twitch_broadcast",
					"clips:edit",
					client.request(Method::POST, "clips").query(&bparams),
				)
				.await?
		}
		"updateTwitchChatSettings" => {
			let patch = chat_settings_patch(args)?;
			client
				.mutate(
					"twitch_moderation",
					"moderator:manage:chat_settings",
					client
						.request(Method::PATCH, "chat/settings")
						.query(&[
							("broadcaster_id", broadcaster),
							("moderator_id", broadcaster),
						])
						.json(&patch),
				)
				.await?
		}
		"twitchShoutout" => {
			let target = snowflake(args, "targetBroadcasterId")?;
			ensure!(target != broadcaster, "Cannot shout out your own channel");
			client
				.mutate(
					"twitch_moderation",
					"moderator:manage:shoutouts",
					client.request(Method::POST, "chat/shoutouts").query(&[
						("from_broadcaster_id", broadcaster),
						("to_broadcaster_id", target),
						("moderator_id", broadcaster),
					]),
				)
				.await?
		}
		"createPoll" => {
			let question = text(args, "question", 60)?;
			let options = args["options"].as_array().context("Poll options missing")?;
			ensure!(
				(2..=4).contains(&options.len()),
				"Polls need two to four options"
			);
			let mut choices = vec![];
			for option in options {
				let s = option.as_str().context("Poll option must be text")?;
				ensure!(
					!s.is_empty() && s.chars().count() <= 25,
					"Twitch poll options must contain 1 to 25 characters"
				);
				choices.push(json!({"title":s}));
			}
			let duration = if args["durationSeconds"].is_null() {
				120
			} else {
				number(args, "durationSeconds", 15, 1800)?
			};
			let points = if args["channelPointsPerVote"].is_null() {
				0
			} else {
				number(args, "channelPointsPerVote", 0, 1000000)?
			};
			client.mutate("twitch_polls","channel:manage:polls",client.request(Method::POST,"polls").json(&json!({"broadcaster_id":broadcaster,"title":question,"choices":choices,"duration":duration,"channel_points_voting_enabled":points>0,"channel_points_per_vote":points}))).await?
		}
		"closePoll" => {
			let poll = text(args, "pollId", 256)?;
			let observed = client
				.read("polls", &[("broadcaster_id", broadcaster), ("id", poll)])
				.await?;
			ensure!(
				observed["data"][0]["id"].as_str() == Some(poll)
					&& observed["data"][0]["status"] == "ACTIVE",
				"The confirmed poll is not active"
			);
			let show = args["showResults"]
				.as_bool()
				.context("showResults must be boolean")?;
			client.mutate("twitch_polls","channel:manage:polls",client.request(Method::PATCH,"polls").json(&json!({"broadcaster_id":broadcaster,"id":poll,"status":if show{"TERMINATED"}else{"ARCHIVED"}}))).await?
		}
		_ => anyhow::bail!("Unknown Twitch tool"),
	};
	Ok(receipt("accepted", body))
}
fn chat_settings_patch(args: &Value) -> Result<Value> {
	let mut patch = serde_json::Map::new();
	for (input, output) in [
		("slowModeEnabled", "slow_mode"),
		("followerOnlyModeEnabled", "follower_mode"),
		("subscriberOnlyModeEnabled", "subscriber_mode"),
		("emoteOnlyModeEnabled", "emote_mode"),
		("uniqueChatModeEnabled", "unique_chat_mode"),
	] {
		if !args[input].is_null() {
			patch.insert(
				output.into(),
				Value::Bool(
					args[input]
						.as_bool()
						.with_context(|| format!("{input} must be boolean"))?,
				),
			);
		}
	}
	for (input, output, mode, min, max) in [
		(
			"slowModeDelaySeconds",
			"slow_mode_wait_time",
			"slow_mode",
			3,
			120,
		),
		(
			"followerOnlyModeDelayMinutes",
			"follower_mode_duration",
			"follower_mode",
			0,
			129600,
		),
	] {
		if !args[input].is_null() {
			ensure!(
				patch.get(mode) == Some(&Value::Bool(true)),
				"Set the corresponding mode to true when specifying a delay"
			);
			patch.insert(output.into(), number(args, input, min, max)?.into());
		}
	}
	ensure!(!patch.is_empty(), "No chat setting changes requested");
	Ok(Value::Object(patch))
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn delays_require_the_mode_and_preserve_false() {
		assert!(chat_settings_patch(&json!({"slowModeDelaySeconds":20})).is_err());
		assert_eq!(
			chat_settings_patch(&json!({"slowModeEnabled":false})).unwrap(),
			json!({"slow_mode":false})
		);
		assert_eq!(
			chat_settings_patch(&json!({"slowModeEnabled":true,"slowModeDelaySeconds":20})).unwrap(),
			json!({"slow_mode":true,"slow_mode_wait_time":20})
		);
	}
}
