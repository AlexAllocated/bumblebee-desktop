//! Discord uses REST against the same bot identity as the native gateway. Every
//! action checks the configured human owner and bot against current guild roles,
//! channel overwrites and hierarchy; cached chat flags are never authorization.
use super::*;
use reqwest::Method;
use std::collections::HashSet;

const ADMIN: u64 = 1 << 3;
const VIEW: u64 = 1 << 10;
const SEND: u64 = 1 << 11;
const HISTORY: u64 = 1 << 16;
const MANAGE_CHANNELS: u64 = 1 << 4;
const MANAGE_MESSAGES: u64 = 1 << 13;
const MANAGE_THREADS: u64 = 1 << 34;
const CONNECT: u64 = 1 << 20;
const MUTE: u64 = 1 << 22;
const MOVE: u64 = 1 << 24;
const MANAGE_NICKNAMES: u64 = 1 << 27;
const MANAGE_ROLES: u64 = 1 << 28;
const MODERATE: u64 = 1 << 40;
const BAN: u64 = 1 << 2;
const PUBLIC_THREADS: u64 = 1 << 35;
const SEND_THREADS: u64 = 1 << 38;
const PIN: u64 = 1 << 51;
const API: &str = "https://discord.com/api/v10";

fn require_permission(who: &str, scope: &str, observed: u64, required: u64) -> Result<()> {
	if observed & required != required {
		return Err(
			super::super::access::PermissionRequired(format!(
				"Discord {who} lacks the required {scope} permission"
			))
			.into(),
		);
	}
	Ok(())
}

fn mutation(
	name: &str,
	description: &str,
	mut fields: Vec<(&str, Value)>,
	confirmation: bool,
) -> ToolDefinition {
	fields.insert(0, ("guildId", id()));
	fields.push(("reason", optional(string(300))));
	definition(name, description, fields, confirmation, true)
}
pub(super) fn definitions() -> Vec<ToolDefinition> {
	vec![
		definition(
			"inspectDiscordResources",
			"Discover current accessible Discord channels, roles, members or messages. Use exact IDs from these results in later actions. Members/messages support provider pagination.",
			vec![
				(
					"kind",
					choice(&["channels", "roles", "members", "messages"]),
				),
				("channelId", optional(id())),
				("query", optional(string(100))),
				("cursor", optional(id())),
				("limit", optional(integer(1, 100))),
			],
			false,
			false,
		),
		mutation(
			"createDiscordChannel",
			"Create a channel in the configured Discord guild. The parent, if given, must be an accessible category.",
			vec![
				("name", string(100)),
				(
					"type",
					choice(&["text", "voice", "stage", "category", "forum"]),
				),
				("parentId", optional(id())),
				("topic", optional(string(1024))),
			],
			false,
		),
		mutation(
			"editDiscordChannel",
			"Edit an exact Discord channel. Null fields leave values unchanged; removeParent explicitly moves it outside its category.",
			vec![
				("channelId", id()),
				("name", optional(string(100))),
				("topic", optional(string(1024))),
				("parentId", optional(id())),
				("removeParent", boolean()),
				("position", optional(integer(0, 1000))),
				("slowmodeSeconds", optional(integer(0, 21600))),
				("nsfw", optional(boolean())),
				("userLimit", optional(integer(0, 99))),
			],
			false,
		),
		mutation(
			"deleteDiscordChannel",
			"Delete the exact discovered Discord channel, including its message history. Requires explicit confirmation.",
			vec![("channelId", id())],
			true,
		),
		mutation(
			"moveDiscordVoiceUser",
			"Move the exact current member to an accessible voice/stage channel in this guild.",
			vec![("userId", id()), ("channelId", id())],
			true,
		),
		mutation(
			"disconnectDiscordVoiceUser",
			"Disconnect the exact member from their current Discord voice channel.",
			vec![("userId", id())],
			true,
		),
		mutation(
			"timeoutDiscordUser",
			"Timeout a guild member, or clear their timeout with durationSeconds=0. Refuses administrators, the guild owner and protected role hierarchy.",
			vec![("userId", id()), ("durationSeconds", integer(0, 2419200))],
			true,
		),
		mutation(
			"banDiscordUser",
			"Ban an exact guild member. Optionally delete at most seven days of their messages.",
			vec![
				("userId", id()),
				("deleteMessageSeconds", optional(integer(0, 604800))),
			],
			true,
		),
		mutation(
			"discordManageRole",
			"Add or remove an existing role on an exact member. Refuses managed roles and roles above either the bot or human owner.",
			vec![
				("userId", id()),
				("roleId", id()),
				("action", choice(&["add", "remove"])),
			],
			true,
		),
		mutation(
			"deleteDiscordMessage",
			"Delete an exact message in an accessible channel after checking both participants' Manage Messages permission.",
			vec![("channelId", id()), ("messageId", id())],
			true,
		),
		mutation(
			"pinDiscordMessage",
			"Pin an exact message after checking the current Pin Messages permission.",
			vec![("channelId", id()), ("messageId", id())],
			true,
		),
		mutation(
			"unpinDiscordMessage",
			"Unpin an exact message after checking the current Pin Messages permission.",
			vec![("channelId", id()), ("messageId", id())],
			true,
		),
		mutation(
			"setDiscordNickname",
			"Set or clear a member nickname, respecting both the bot and human owner's role hierarchy.",
			vec![("userId", id()), ("nickname", optional(string(32)))],
			true,
		),
		mutation(
			"createDiscordThread",
			"Create a public thread in an accessible text/news channel, optionally attached to an exact message.",
			vec![
				("channelId", id()),
				("messageId", optional(id())),
				("name", string(100)),
				(
					"autoArchiveMinutes",
					json!({"type":"integer","enum":[60,1440,4320,10080]}),
				),
			],
			true,
		),
		mutation(
			"discordStageControlUser",
			"Move a current Stage participant to speaker/audience, or server mute/unmute them. Requires both bot and owner permission in that Stage channel.",
			vec![
				("userId", id()),
				("channelId", id()),
				("action", choice(&["speaker", "audience", "mute", "unmute"])),
			],
			true,
		),
	]
}

struct Discord<'a> {
	engine: &'a Engine,
	source: &'a ChatMessage,
	token: String,
	guild_id: String,
	owner_id: String,
	bot_id: String,
	guild: Value,
	owner: Value,
	bot: Value,
	cancel: CancellationToken,
}
impl<'a> Discord<'a> {
	async fn new(
		engine: &'a Engine,
		source: &'a ChatMessage,
		cancel: CancellationToken,
	) -> Result<Self> {
		let settings = engine.store.settings()?;
		let guild_id = settings.discord_guild_id;
		let owner_id = settings.owner_discord_id;
		ensure_id(&guild_id)?;
		ensure_id(&owner_id)?;
		let token = engine.providers.secret("discord_bot")?;
		let get = |path: String| {
			engine
				.providers
				.http
				.get(format!("{API}{path}"))
				.header("Authorization", format!("Bot {token}"))
		};
		let (me, guild) = tokio::try_join!(
			request(get("/users/@me".into()), "Discord", false, &cancel),
			request(
				get(format!("/guilds/{guild_id}")),
				"Discord",
				false,
				&cancel
			)
		)?;
		let bot_id = text(&me, "id", 20)?.to_string();
		ensure_id(&bot_id)?;
		ensure!(me["bot"] == true, "Discord credential is not a bot token");
		let (owner, bot) = tokio::try_join!(
			request(
				get(format!("/guilds/{guild_id}/members/{owner_id}")),
				"Discord",
				false,
				&cancel
			),
			request(
				get(format!("/guilds/{guild_id}/members/{bot_id}")),
				"Discord",
				false,
				&cancel
			)
		)?;
		Ok(Self {
			engine,
			source,
			token,
			guild_id,
			owner_id,
			bot_id,
			guild,
			owner,
			bot,
			cancel,
		})
	}
	fn req(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
		self
			.engine
			.providers
			.http
			.request(method, format!("{API}{path}"))
			.header("Authorization", format!("Bot {}", self.token))
	}
	async fn read(&self, path: &str) -> Result<Value> {
		request(self.req(Method::GET, path), "Discord", false, &self.cancel).await
	}
	async fn write(
		&self,
		method: Method,
		path: &str,
		body: Option<Value>,
		group: &str,
		reason: Option<&str>,
	) -> Result<Value> {
		super::super::require_owner(self.engine, self.source).await?;
		grant(self.engine, group)?;
		// Changing a configured guild/owner during confirmation or preflight may
		// never redirect the already-approved action to the new configuration.
		let settings = self.engine.store.settings()?;
		ensure!(
			settings.discord_guild_id == self.guild_id && settings.owner_discord_id == self.owner_id,
			"Discord identity changed; rediscover and request a new action"
		);
		let mut builder = self.req(method, path);
		if let Some(body) = body {
			builder = builder.json(&body)
		}
		if let Some(reason) = reason {
			let encoded: String = url::form_urlencoded::byte_serialize(reason.as_bytes()).collect();
			builder = builder.header("X-Audit-Log-Reason", encoded)
		}
		request(builder, "Discord", true, &self.cancel).await
	}
	fn roles(&self) -> &[Value] {
		self.guild["roles"]
			.as_array()
			.map(Vec::as_slice)
			.unwrap_or(&[])
	}
	fn guild_permission(&self, bit: u64) -> Result<()> {
		for (who, id, member) in [
			("human owner", self.owner_id.as_str(), &self.owner),
			("bot", self.bot_id.as_str(), &self.bot),
		] {
			require_permission(
				who,
				"guild",
				effective_permissions(&self.guild, id, member, None),
				bit,
			)?;
		}
		Ok(())
	}
	async fn channel(&self, id: &str) -> Result<Value> {
		ensure_id(id)?;
		let channel = self.read(&format!("/channels/{id}")).await?;
		ensure!(
			channel["guild_id"].as_str() == Some(self.guild_id.as_str()),
			"Channel is outside the configured Discord guild"
		);
		Ok(channel)
	}
	async fn channel_permission(&self, channel: &Value, bits: u64) -> Result<()> {
		let channel_type = channel["type"]
			.as_u64()
			.context("Invalid Discord channel type")?;
		let parent;
		let permission_channel = if matches!(channel_type, 10..=12) {
			parent = self.channel(text(channel, "parent_id", 20)?).await?;
			&parent
		} else {
			channel
		};
		for (who, id, member) in [
			("human owner", self.owner_id.as_str(), &self.owner),
			("bot", self.bot_id.as_str(), &self.bot),
		] {
			let permissions = effective_permissions(&self.guild, id, member, Some(permission_channel));
			require_permission(who, "channel", permissions, bits | VIEW)?;
			if channel_type == 12 && permissions & MANAGE_THREADS == 0 {
				self
					.read(&format!(
						"/channels/{}/thread-members/{id}",
						text(channel, "id", 20)?
					))
					.await
					.context("Private thread membership is required")?;
			}
		}
		Ok(())
	}
	async fn target(&self, id: &str, hierarchy: bool) -> Result<Value> {
		ensure_id(id)?;
		if hierarchy {
			ensure!(
				id != self.bot_id,
				"This moderation action cannot target the bot itself"
			);
			ensure!(
				self.guild["owner_id"].as_str() != Some(id),
				"Cannot moderate the Discord guild owner"
			);
		}
		let member = self
			.read(&format!("/guilds/{}/members/{id}", self.guild_id))
			.await?;
		if hierarchy {
			for (actor, actor_id) in [
				(&self.bot, self.bot_id.as_str()),
				(&self.owner, self.owner_id.as_str()),
			] {
				ensure!(
					self.guild["owner_id"].as_str() == Some(actor_id)
						|| outranks(actor, &member, self.roles()),
					"Target is above or equal to the bot or human owner in the role hierarchy"
				);
			}
		}
		Ok(member)
	}
	async fn voice(&self, user: &str) -> Result<Value> {
		ensure_id(user)?;
		let voice = self
			.read(&format!("/guilds/{}/voice-states/{user}", self.guild_id))
			.await?;
		ensure!(
			voice["channel_id"].as_str().is_some(),
			"Member is no longer connected to voice"
		);
		Ok(voice)
	}
	async fn parent(&self, id: &str) -> Result<Value> {
		let parent = self.channel(id).await?;
		ensure!(parent["type"] == 4, "Parent must be a category");
		self.channel_permission(&parent, MANAGE_CHANNELS).await?;
		Ok(parent)
	}
	async fn message(&self, channel: &str, message: &str) -> Result<Value> {
		ensure_id(message)?;
		let result = self
			.read(&format!("/channels/{channel}/messages/{message}"))
			.await?;
		ensure!(
			result["channel_id"].as_str() == Some(channel),
			"Message moved or no longer belongs to this channel"
		);
		Ok(result)
	}
}
fn ensure_id(id: &str) -> Result<()> {
	snowflake(&json!({"id":id}), "id")?;
	Ok(())
}
fn role_permissions(role: &Value) -> u64 {
	role["permissions"]
		.as_str()
		.and_then(|s| s.parse().ok())
		.unwrap_or(0)
}
fn role_ids(member: &Value) -> HashSet<&str> {
	member["roles"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.collect()
}
fn timed_out(member: &Value) -> bool {
	member["communication_disabled_until"]
		.as_str()
		.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
		.is_some_and(|deadline| deadline > chrono::Utc::now())
}
fn effective_permissions(
	guild: &Value,
	user: &str,
	member: &Value,
	channel: Option<&Value>,
) -> u64 {
	if guild["owner_id"].as_str() == Some(user) {
		return u64::MAX;
	}
	let ids = role_ids(member);
	let guild_id = guild["id"].as_str().unwrap_or("");
	let mut permissions = 0;
	for role in guild["roles"].as_array().into_iter().flatten() {
		if role["id"].as_str() == Some(guild_id)
			|| role["id"].as_str().is_some_and(|id| ids.contains(id))
		{
			permissions |= role_permissions(role)
		}
	}
	if permissions & ADMIN != 0 {
		return u64::MAX;
	}
	if let Some(channel) = channel {
		let overwrites: Vec<_> = channel["permission_overwrites"]
			.as_array()
			.into_iter()
			.flatten()
			.collect();
		let apply = |permissions: u64, deny: u64, allow: u64| (permissions & !deny) | allow;
		let flags = |item: &Value, key: &str| {
			item[key]
				.as_str()
				.and_then(|s| s.parse::<u64>().ok())
				.unwrap_or(0)
		};
		for overwrite in &overwrites {
			if overwrite["id"].as_str() == Some(guild_id) && overwrite["type"] == 0 {
				permissions = apply(
					permissions,
					flags(overwrite, "deny"),
					flags(overwrite, "allow"),
				)
			}
		}
		let (mut deny, mut allow) = (0, 0);
		for overwrite in &overwrites {
			if overwrite["type"] == 0
				&& overwrite["id"]
					.as_str()
					.is_some_and(|id| id != guild_id && ids.contains(id))
			{
				deny |= flags(overwrite, "deny");
				allow |= flags(overwrite, "allow")
			}
		}
		permissions = apply(permissions, deny, allow);
		for overwrite in &overwrites {
			if overwrite["id"].as_str() == Some(user) && overwrite["type"] == 1 {
				permissions = apply(
					permissions,
					flags(overwrite, "deny"),
					flags(overwrite, "allow"),
				)
			}
		}
	}
	if timed_out(member) {
		permissions &= VIEW | HISTORY
	}
	permissions
}
fn top_role<'a>(member: &Value, roles: &'a [Value]) -> Option<&'a Value> {
	let ids = role_ids(member);
	roles
		.iter()
		.filter(|r| r["id"].as_str().is_some_and(|id| ids.contains(id)))
		.max_by(|a, b| compare_roles(a, b))
}
fn compare_roles(a: &Value, b: &Value) -> std::cmp::Ordering {
	a["position"]
		.as_i64()
		.unwrap_or(0)
		.cmp(&b["position"].as_i64().unwrap_or(0))
		.then_with(|| {
			// Discord puts the older (smaller snowflake) role above equal positions.
			let aid = a["id"]
				.as_str()
				.and_then(|s| s.parse::<u64>().ok())
				.unwrap_or(u64::MAX);
			let bid = b["id"]
				.as_str()
				.and_then(|s| s.parse::<u64>().ok())
				.unwrap_or(u64::MAX);
			bid.cmp(&aid)
		})
}
fn outranks(actor: &Value, target: &Value, roles: &[Value]) -> bool {
	match (top_role(actor, roles), top_role(target, roles)) {
		(Some(a), Some(b)) => compare_roles(a, b).is_gt(),
		(Some(_), None) => true,
		_ => false,
	}
}

pub(super) async fn resolve(
	engine: &Engine,
	source: &ChatMessage,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	let client = Discord::new(engine, source, cancel).await?;
	let query = text(args, "query", 100)?;
	let members = if ensure_id(query).is_ok() {
		vec![
			client
				.read(&format!("/guilds/{}/members/{query}", client.guild_id))
				.await?,
		]
	} else {
		let value = request(
			client
				.req(
					Method::GET,
					&format!("/guilds/{}/members/search", client.guild_id),
				)
				.query(&[("query", query), ("limit", "25")]),
			"Discord",
			false,
			&client.cancel,
		)
		.await?;
		value
			.as_array()
			.context("Invalid Discord member search response")?
			.clone()
	};
	let candidates: Vec<_> = members.iter().map(member_summary).collect();
	Ok(receipt(
		"verified",
		json!({"guildId":client.guild_id,"ambiguous":candidates.len()!=1,"candidates":candidates}),
	))
}
fn member_summary(member: &Value) -> Value {
	json!({"id":member["user"]["id"],"username":member["user"]["username"],"displayName":member["nick"].as_str().or(member["user"]["global_name"].as_str()).or(member["user"]["username"].as_str()),"roleIds":member["roles"]})
}
async fn inspect(client: &Discord<'_>, args: &Value) -> Result<Value> {
	let limit = if args["limit"].is_null() {
		50
	} else {
		number(args, "limit", 1, 100)?
	};
	let kind = text(args, "kind", 20)?;
	let data = match kind {
		"channels" => {
			let all = client
				.read(&format!("/guilds/{}/channels", client.guild_id))
				.await?;
			let mut visible = Vec::new();
			for channel in all.as_array().context("Invalid channel catalog")? {
				if client.channel_permission(channel, 0).await.is_ok() {
					visible.push(json!({"id":channel["id"],"name":channel["name"],"type":channel["type"],"parentId":channel["parent_id"],"topic":channel["topic"]}))
				}
			}
			json!({"channels":visible})
		}
		"roles" => {
			let roles: Vec<_> = client
				.roles()
				.iter()
				.map(
					|r| json!({"id":r["id"],"name":r["name"],"position":r["position"],"managed":r["managed"]}),
				)
				.collect();
			json!({"roles":roles})
		}
		"members" => {
			let query = optional_text(args, "query", 100)?;
			let mut request = client
				.req(
					Method::GET,
					&format!(
						"/guilds/{}/members{}",
						client.guild_id,
						if query.is_some() { "/search" } else { "" }
					),
				)
				.query(&[("limit", limit.to_string())]);
			if let Some(query) = query {
				request = request.query(&[("query", query)])
			} else if !args["cursor"].is_null() {
				request = request.query(&[("after", snowflake(args, "cursor")?)])
			}
			let data = request_super(request, &client.cancel).await?;
			let items = data.as_array().context("Invalid member list")?;
			let members: Vec<_> = items.iter().map(member_summary).collect();
			json!({"members":members,"nextCursor":if query.is_none()&&items.len()==limit as usize{items.last().and_then(|m|m["user"]["id"].as_str())}else{None}})
		}
		"messages" => {
			let channel_id = snowflake(args, "channelId")?;
			let channel = client.channel(channel_id).await?;
			client.channel_permission(&channel, HISTORY).await?;
			let mut req = client
				.req(Method::GET, &format!("/channels/{channel_id}/messages"))
				.query(&[("limit", limit.to_string())]);
			if !args["cursor"].is_null() {
				req = req.query(&[("before", snowflake(args, "cursor")?)])
			}
			let data = request_super(req, &client.cancel).await?;
			let items = data.as_array().context("Invalid message list")?;
			let messages:Vec<_>=items.iter().map(|m|json!({"id":m["id"],"authorId":m["author"]["id"],"authorName":m["author"]["username"],"content":m["content"],"pinned":m["pinned"]})).collect();
			json!({"channelId":channel_id,"messages":messages,"nextCursor":if items.len()==limit as usize{items.last().and_then(|m|m["id"].as_str())}else{None}})
		}
		_ => anyhow::bail!("Unsupported Discord resource kind"),
	};
	Ok(receipt(
		"verified",
		json!({"guildId":client.guild_id,"resources":data}),
	))
}
async fn request_super(
	builder: reqwest::RequestBuilder,
	cancel: &CancellationToken,
) -> Result<Value> {
	request(builder, "Discord", false, cancel).await
}

/// Read-only permission inspection. The executor repeats these checks just
/// before mutation. REST failures and invalid targets remain hard errors.
pub(super) async fn preflight(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	cancel: CancellationToken,
) -> Result<()> {
	let client = Discord::new(engine, source, cancel).await?;
	if let Some(guild) = args["guildId"].as_str() {
		ensure!(
			guild == client.guild_id,
			"Approved guild differs from current Settings; request a new action"
		);
	}
	match name {
		"createDiscordChannel" => {
			client.guild_permission(MANAGE_CHANNELS)?;
			if let Some(parent) = args["parentId"].as_str() {
				client.parent(parent).await?;
			}
		}
		"editDiscordChannel" | "deleteDiscordChannel" => {
			let channel = client.channel(snowflake(args, "channelId")?).await?;
			let thread = matches!(channel["type"].as_u64(), Some(10..=12));
			ensure!(
				name != "editDiscordChannel" || !thread,
				"Use channel editing only for non-thread channels"
			);
			client
				.channel_permission(
					&channel,
					if thread {
						MANAGE_THREADS
					} else {
						MANAGE_CHANNELS
					},
				)
				.await?;
			if name == "editDiscordChannel" {
				if let Some(parent) = args["parentId"].as_str() {
					client.parent(parent).await?;
				}
			}
		}
		"moveDiscordVoiceUser" | "disconnectDiscordVoiceUser" | "discordStageControlUser" => {
			let user = snowflake(args, "userId")?;
			client.target(user, false).await?;
			let voice = client.voice(user).await?;
			let channel = client.channel(text(&voice, "channel_id", 20)?).await?;
			if name == "discordStageControlUser" {
				ensure!(
					channel["type"] == 13 && channel["id"] == args["channelId"],
					"Member is no longer in the approved Stage channel"
				);
				client.channel_permission(&channel, MUTE).await?;
			} else {
				client.channel_permission(&channel, MOVE).await?;
				if name == "moveDiscordVoiceUser" {
					let target = client.channel(snowflake(args, "channelId")?).await?;
					ensure!(
						matches!(target["type"].as_u64(), Some(2 | 13)),
						"Destination is not a voice channel"
					);
					client.channel_permission(&target, MOVE | CONNECT).await?;
				}
			}
		}
		"timeoutDiscordUser" | "banDiscordUser" | "discordManageRole" | "setDiscordNickname" => {
			let user = snowflake(args, "userId")?;
			let target = client.target(user, true).await?;
			if name == "timeoutDiscordUser" {
				ensure!(
					effective_permissions(&client.guild, user, &target, None) & ADMIN == 0,
					"Cannot timeout administrators"
				);
			}
			if name == "discordManageRole" {
				let role = snowflake(args, "roleId")?;
				ensure!(
					client.roles().iter().any(|r| r["id"] == role),
					"Role no longer exists"
				);
			}
			client.guild_permission(match name {
				"timeoutDiscordUser" => MODERATE,
				"banDiscordUser" => BAN,
				"discordManageRole" => MANAGE_ROLES,
				_ => MANAGE_NICKNAMES,
			})?;
		}
		"deleteDiscordMessage" | "pinDiscordMessage" | "unpinDiscordMessage" => {
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			client.message(id, snowflake(args, "messageId")?).await?;
			client
				.channel_permission(
					&channel,
					HISTORY
						| if name == "deleteDiscordMessage" {
							MANAGE_MESSAGES
						} else {
							PIN
						},
				)
				.await?;
		}
		"createDiscordThread" => {
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			ensure!(
				matches!(channel["type"].as_u64(), Some(0 | 5)),
				"Public thread creation requires a text/news channel"
			);
			if let Some(message) = args["messageId"].as_str() {
				client.message(id, message).await?;
			}
			client
				.channel_permission(&channel, PUBLIC_THREADS | SEND)
				.await?;
		}
		"inspectDiscordResources" if args["kind"] == "messages" => {
			let channel = client.channel(snowflake(args, "channelId")?).await?;
			client.channel_permission(&channel, HISTORY).await?;
		}
		"deliverMessage" if args["target"] == "discord_channel" => {
			let channel = client.channel(snowflake(args, "destinationId")?).await?;
			client
				.channel_permission(
					&channel,
					if matches!(channel["type"].as_u64(), Some(10..=12)) {
						SEND_THREADS
					} else {
						SEND
					},
				)
				.await?;
		}
		_ => {}
	}
	Ok(())
}

pub(super) async fn execute(
	engine: &Engine,
	source: &ChatMessage,
	name: &str,
	args: &Value,
	cancel: CancellationToken,
) -> Result<Value> {
	let client = Discord::new(engine, source, cancel).await?;
	if name == "inspectDiscordResources" {
		return inspect(&client, args).await;
	}
	ensure!(
		snowflake(args, "guildId")? == client.guild_id,
		"Approved guild differs from current Settings; rediscover before requesting this action"
	);
	let group = if matches!(
		name,
		"createDiscordChannel"
			| "editDiscordChannel"
			| "deleteDiscordChannel"
			| "createDiscordThread"
			| "deleteDiscordMessage"
			| "pinDiscordMessage"
			| "unpinDiscordMessage"
	) {
		"discord_resources"
	} else {
		"discord_moderation"
	};
	grant(engine, group)?;
	let reason = optional_text(args, "reason", 300)?;
	let guild = &client.guild_id;
	let data = match name {
		"createDiscordChannel" => {
			client.guild_permission(MANAGE_CHANNELS)?;
			let name = text(args, "name", 100)?;
			let kind = match text(args, "type", 20)? {
				"text" => 0,
				"voice" => 2,
				"category" => 4,
				"stage" => 13,
				"forum" => 15,
				_ => anyhow::bail!("Unsupported channel type"),
			};
			let mut body = json!({"name":name,"type":kind});
			if !args["parentId"].is_null() {
				let parent = snowflake(args, "parentId")?;
				ensure!(kind != 4, "A category cannot have a parent");
				client.parent(parent).await?;
				body["parent_id"] = json!(parent)
			}
			if let Some(topic) = optional_text(args, "topic", 1024)? {
				ensure!(
					matches!(kind, 0 | 15),
					"This channel type has no text topic"
				);
				body["topic"] = json!(topic)
			}
			client
				.write(
					Method::POST,
					&format!("/guilds/{guild}/channels"),
					Some(body),
					group,
					reason,
				)
				.await?
		}
		"editDiscordChannel" => {
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			client.channel_permission(&channel, MANAGE_CHANNELS).await?;
			ensure!(
				!matches!(channel["type"].as_u64(), Some(10..=12)),
				"Use channel editing only for non-thread channels"
			);
			let mut body = json!({});
			for (key, target) in [("name", "name"), ("topic", "topic")] {
				if let Some(value) = optional_text(args, key, if key == "name" { 100 } else { 1024 })? {
					ensure!(
						key != "name" || !value.is_empty(),
						"Channel name cannot be empty"
					);
					body[target] = json!(value)
				}
			}
			let remove = args["removeParent"]
				.as_bool()
				.context("removeParent must be boolean")?;
			ensure!(
				!remove || args["parentId"].is_null(),
				"Cannot set and remove parent together"
			);
			if remove {
				body["parent_id"] = Value::Null
			} else if !args["parentId"].is_null() {
				let parent = snowflake(args, "parentId")?;
				ensure!(parent != id, "A channel cannot parent itself");
				client.parent(parent).await?;
				body["parent_id"] = json!(parent)
			}
			for (key, target, min, max) in [
				("position", "position", 0, 1000),
				("slowmodeSeconds", "rate_limit_per_user", 0, 21600),
				("userLimit", "user_limit", 0, 99),
			] {
				if !args[key].is_null() {
					body[target] = json!(number(args, key, min, max)?)
				}
			}
			if !args["nsfw"].is_null() {
				body["nsfw"] = json!(args["nsfw"].as_bool().context("nsfw must be boolean")?)
			}
			ensure!(
				!body.as_object().unwrap().is_empty(),
				"No channel changes supplied"
			);
			client
				.write(
					Method::PATCH,
					&format!("/channels/{id}"),
					Some(body),
					group,
					reason,
				)
				.await?
		}
		"deleteDiscordChannel" => {
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			client
				.channel_permission(
					&channel,
					if matches!(channel["type"].as_u64(), Some(10..=12)) {
						MANAGE_THREADS
					} else {
						MANAGE_CHANNELS
					},
				)
				.await?;
			client
				.write(
					Method::DELETE,
					&format!("/channels/{id}"),
					None,
					group,
					reason,
				)
				.await?
		}
		"moveDiscordVoiceUser" | "disconnectDiscordVoiceUser" => {
			let user = snowflake(args, "userId")?;
			client.target(user, false).await?;
			let voice = client.voice(user).await?;
			let current = client.channel(text(&voice, "channel_id", 20)?).await?;
			client.channel_permission(&current, MOVE).await?;
			let destination = if name == "moveDiscordVoiceUser" {
				let id = snowflake(args, "channelId")?;
				let target = client.channel(id).await?;
				ensure!(
					matches!(target["type"].as_u64(), Some(2 | 13)),
					"Destination is not a voice channel"
				);
				client.channel_permission(&target, MOVE | CONNECT).await?;
				json!(id)
			} else {
				Value::Null
			};
			client
				.write(
					Method::PATCH,
					&format!("/guilds/{guild}/members/{user}"),
					Some(json!({"channel_id":destination})),
					group,
					reason,
				)
				.await?
		}
		"timeoutDiscordUser" => {
			client.guild_permission(MODERATE)?;
			let user = snowflake(args, "userId")?;
			let target = client.target(user, true).await?;
			ensure!(
				effective_permissions(&client.guild, user, &target, None) & ADMIN == 0,
				"Cannot timeout administrators"
			);
			let duration = number(args, "durationSeconds", 0, 2419200)?;
			let until = if duration == 0 {
				Value::Null
			} else {
				json!((chrono::Utc::now() + chrono::Duration::seconds(duration)).to_rfc3339())
			};
			client
				.write(
					Method::PATCH,
					&format!("/guilds/{guild}/members/{user}"),
					Some(json!({"communication_disabled_until":until})),
					group,
					reason,
				)
				.await?
		}
		"banDiscordUser" => {
			client.guild_permission(BAN)?;
			let user = snowflake(args, "userId")?;
			client.target(user, true).await?;
			let seconds = if args["deleteMessageSeconds"].is_null() {
				0
			} else {
				number(args, "deleteMessageSeconds", 0, 604800)?
			};
			client
				.write(
					Method::PUT,
					&format!("/guilds/{guild}/bans/{user}"),
					Some(json!({"delete_message_seconds":seconds})),
					group,
					reason,
				)
				.await?
		}
		"discordManageRole" => {
			client.guild_permission(MANAGE_ROLES)?;
			let user = snowflake(args, "userId")?;
			client.target(user, true).await?;
			let role_id = snowflake(args, "roleId")?;
			let role = client
				.roles()
				.iter()
				.find(|r| r["id"].as_str() == Some(role_id))
				.context("Role no longer exists")?;
			ensure!(
				role_id != guild && role["managed"] != true,
				"Cannot modify the everyone role or an integration-managed role"
			);
			for (actor, actor_id) in [
				(&client.bot, client.bot_id.as_str()),
				(&client.owner, client.owner_id.as_str()),
			] {
				ensure!(
					client.guild["owner_id"].as_str() == Some(actor_id)
						|| top_role(actor, client.roles())
							.is_some_and(|top| compare_roles(top, role).is_gt()),
					"Role is above or equal to the bot or human owner's highest role"
				)
			}
			let method = match text(args, "action", 10)? {
				"add" => Method::PUT,
				"remove" => Method::DELETE,
				_ => anyhow::bail!("Role action must be add or remove"),
			};
			client
				.write(
					method,
					&format!("/guilds/{guild}/members/{user}/roles/{role_id}"),
					None,
					group,
					reason,
				)
				.await?
		}
		"deleteDiscordMessage" | "pinDiscordMessage" | "unpinDiscordMessage" => {
			let id = snowflake(args, "channelId")?;
			let message = snowflake(args, "messageId")?;
			let channel = client.channel(id).await?;
			client
				.channel_permission(
					&channel,
					HISTORY
						| if name == "deleteDiscordMessage" {
							MANAGE_MESSAGES
						} else {
							PIN
						},
				)
				.await?;
			client.message(id, message).await?;
			let (method, path) = match name {
				"deleteDiscordMessage" => {
					(Method::DELETE, format!("/channels/{id}/messages/{message}"))
				}
				"pinDiscordMessage" => (
					Method::PUT,
					format!("/channels/{id}/messages/pins/{message}"),
				),
				_ => (
					Method::DELETE,
					format!("/channels/{id}/messages/pins/{message}"),
				),
			};
			client.write(method, &path, None, group, reason).await?
		}
		"setDiscordNickname" => {
			client.guild_permission(MANAGE_NICKNAMES)?;
			let user = snowflake(args, "userId")?;
			client.target(user, true).await?;
			let nickname = optional_text(args, "nickname", 32)?;
			client
				.write(
					Method::PATCH,
					&format!("/guilds/{guild}/members/{user}"),
					Some(json!({"nick":nickname})),
					group,
					reason,
				)
				.await?
		}
		"createDiscordThread" => {
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			ensure!(
				matches!(channel["type"].as_u64(), Some(0 | 5)),
				"Public thread creation requires a text/news channel"
			);
			client
				.channel_permission(&channel, PUBLIC_THREADS | SEND)
				.await?;
			let mut body = json!({"name":text(args,"name",100)?,"auto_archive_duration":number(args,"autoArchiveMinutes",60,10080)?});
			ensure!(
				[60, 1440, 4320, 10080].contains(&body["auto_archive_duration"].as_i64().unwrap()),
				"Unsupported thread archive duration"
			);
			let path = if !args["messageId"].is_null() {
				let message = snowflake(args, "messageId")?;
				client.message(id, message).await?;
				format!("/channels/{id}/messages/{message}/threads")
			} else {
				ensure!(
					channel["type"] == 0,
					"News threads must begin at an existing message"
				);
				body["type"] = json!(11);
				format!("/channels/{id}/threads")
			};
			client
				.write(Method::POST, &path, Some(body), group, reason)
				.await?
		}
		"discordStageControlUser" => {
			let user = snowflake(args, "userId")?;
			client.target(user, false).await?;
			let id = snowflake(args, "channelId")?;
			let channel = client.channel(id).await?;
			ensure!(channel["type"] == 13, "Channel must be a Stage");
			client.channel_permission(&channel, MUTE).await?;
			let voice = client.voice(user).await?;
			ensure!(
				voice["channel_id"].as_str() == Some(id),
				"Member is no longer in the approved Stage channel"
			);
			match text(args, "action", 20)? {
				"speaker" | "audience" => {
					client
						.write(
							Method::PATCH,
							&format!("/guilds/{guild}/voice-states/{user}"),
							Some(json!({"channel_id":id,"suppress":args["action"]=="audience"})),
							group,
							reason,
						)
						.await?
				}
				"mute" | "unmute" => {
					client
						.write(
							Method::PATCH,
							&format!("/guilds/{guild}/members/{user}"),
							Some(json!({"mute":args["action"]=="mute"})),
							group,
							reason,
						)
						.await?
				}
				_ => anyhow::bail!("Unsupported Stage action"),
			}
		}
		_ => anyhow::bail!("Unknown Discord action"),
	};
	Ok(receipt("accepted", json!({"guildId":guild,"result":data})))
}

pub(super) async fn send_message(
	engine: &Engine,
	source: &ChatMessage,
	channel_id: &str,
	body: &str,
) -> Result<Value> {
	ensure!(
		body.chars().count() > 0 && body.chars().count() <= 2000,
		"Discord message must contain 1 to 2000 characters"
	);
	let client = Discord::new(engine, source, CancellationToken::new()).await?;
	let channel = client.channel(channel_id).await?;
	client
		.channel_permission(
			&channel,
			if matches!(channel["type"].as_u64(), Some(10..=12)) {
				SEND_THREADS
			} else {
				SEND
			},
		)
		.await?;
	let result = client
		.write(
			Method::POST,
			&format!("/channels/{channel_id}/messages"),
			Some(json!({"content":body,"allowed_mentions":{"parse":[]}})),
			"discord_resources",
			None,
		)
		.await?;
	Ok(receipt(
		"accepted",
		json!({"guildId":client.guild_id,"channelId":channel_id,"messageId":result["id"]}),
	))
}

#[cfg(test)]
mod tests {
	use super::*;
	fn guild() -> Value {
		json!({"id":"1","owner_id":"9","roles":[{"id":"1","permissions":(VIEW|HISTORY|SEND).to_string(),"position":0},{"id":"2","permissions":MANAGE_MESSAGES.to_string(),"position":1},{"id":"3","permissions":"0","position":2},{"id":"4","permissions":ADMIN.to_string(),"position":3}]})
	}
	#[test]
	fn overwrites_combine_roles_then_apply_member_override() {
		let guild = guild();
		let member = json!({"roles":["2","3"]});
		let channel = json!({"permission_overwrites":[{"id":"1","type":0,"deny":SEND.to_string(),"allow":"0"},{"id":"2","type":0,"deny":MANAGE_MESSAGES.to_string(),"allow":"0"},{"id":"3","type":0,"deny":"0","allow":MANAGE_MESSAGES.to_string()},{"id":"7","type":1,"deny":MANAGE_MESSAGES.to_string(),"allow":SEND.to_string()}]});
		let permissions = effective_permissions(&guild, "7", &member, Some(&channel));
		assert_ne!(permissions & SEND, 0);
		assert_eq!(permissions & MANAGE_MESSAGES, 0);
		assert_ne!(
			effective_permissions(&guild, "8", &member, Some(&channel)) & MANAGE_MESSAGES,
			0
		);
	}
	#[test]
	fn owner_and_admin_bypass_overwrites_but_timeout_removes_actions() {
		let guild = guild();
		let channel = json!({"permission_overwrites":[{"id":"1","type":0,"deny":u64::MAX.to_string(),"allow":"0"}]});
		assert_eq!(
			effective_permissions(&guild, "9", &json!({"roles":[]}), Some(&channel)),
			u64::MAX
		);
		assert_eq!(
			effective_permissions(&guild, "8", &json!({"roles":["4"]}), Some(&channel)),
			u64::MAX
		);
		let member = json!({"roles":["2"],"communication_disabled_until":(chrono::Utc::now()+chrono::Duration::minutes(5)).to_rfc3339()});
		assert_eq!(
			effective_permissions(&guild, "8", &member, None),
			VIEW | HISTORY
		);
	}
	#[test]
	fn role_hierarchy_is_not_permission_superset() {
		let guild = guild();
		let roles = guild["roles"].as_array().unwrap();
		assert!(outranks(
			&json!({"roles":["3"]}),
			&json!({"roles":["2"]}),
			roles
		));
		assert!(!outranks(
			&json!({"roles":["2"]}),
			&json!({"roles":["3"]}),
			roles
		));
		assert!(!outranks(
			&json!({"roles":["2"]}),
			&json!({"roles":["2"]}),
			roles
		));
		assert!(
			compare_roles(
				&json!({"id":"8","position":2}),
				&json!({"id":"9","position":2})
			)
			.is_gt()
		);
	}
	#[test]
	fn observed_permission_loss_is_repairable_but_restored_roles_pass_same_check() {
		let guild = guild();
		let mut channel =
			json!({"permission_overwrites":[{"id":"8","type":1,"deny":SEND.to_string(),"allow":"0"}]});
		let member = json!({"roles":["2"]});
		let observed = effective_permissions(&guild, "8", &member, Some(&channel));
		let error = require_permission("bot", "channel", observed, SEND | VIEW).unwrap_err();
		assert!(
			error
				.downcast_ref::<super::super::super::access::PermissionRequired>()
				.is_some()
		);
		channel["permission_overwrites"] = json!([]);
		let restored = effective_permissions(&guild, "8", &member, Some(&channel));
		require_permission("bot", "channel", restored, SEND | VIEW).unwrap();
		// An unrelated ordinary API/target failure has no repairable marker.
		assert!(
			anyhow::anyhow!("Discord: Resource no longer exists (HTTP 404)")
				.downcast_ref::<super::super::super::access::PermissionRequired>()
				.is_none()
		);
	}
	#[test]
	fn newer_pin_permission_is_distinct_from_message_moderation() {
		assert_eq!(PIN & MANAGE_MESSAGES, 0);
		assert_eq!(PIN, 1_u64 << 51);
	}
}
