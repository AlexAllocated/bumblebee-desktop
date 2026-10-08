use super::{Providers, check_response};
use crate::model::ChatMessage;
use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::{
	MaybeTlsStream, WebSocketStream, connect_async_with_config,
	tungstenite::{Message, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;

impl Providers {
	pub async fn twitch_chat(
		self: Arc<Self>,
		messages: mpsc::Sender<ChatMessage>,
		cancellation: CancellationToken,
	) -> Result<()> {
		let mut backoff = 1u64;
		while !cancellation.is_cancelled() {
			let result = self.twitch_session(&messages, &cancellation).await;
			if cancellation.is_cancelled() {
				break;
			}
			self.status(
				"twitch",
				"connection_failed",
				result
					.err()
					.map(|e| e.to_string())
					.unwrap_or_else(|| "Twitch connection closed; reconnecting".into()),
			);
			tokio::select! {_=cancellation.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(backoff))=>{}}
			backoff = (backoff * 2).min(60);
		}
		self.status("twitch", "disconnected", "Twitch chat disconnected");
		Ok(())
	}

	async fn twitch_session(
		&self,
		messages: &mpsc::Sender<ChatMessage>,
		cancel: &CancellationToken,
	) -> Result<()> {
		let mut tokens = self.tokens("twitch").await?;
		self.validate_twitch_tokens(&mut tokens).await?;
		let settings = self.store.settings()?;
		let channel = if settings.twitch_channel.trim().is_empty() {
			tokens.login.as_str()
		} else {
			settings.twitch_channel.trim().trim_start_matches('#')
		};
		let response = self
			.http
			.get("https://api.twitch.tv/helix/users")
			.bearer_auth(&tokens.access_token)
			.header("Client-Id", &tokens.client_id)
			.query(&[("login", channel)])
			.send()
			.await?;
		check_response("twitch", &response)?;
		let data: serde_json::Value = response.json().await?;
		let broadcaster = data["data"][0]["id"]
			.as_str()
			.context("Twitch channel not found")?
			.to_owned();
		let mut socket = tokio::select! {_=cancel.cancelled()=>return Ok(()),result=open_twitch_socket("wss://eventsub.wss.twitch.tv/ws")=>result?};
		let mut keepalive = Duration::from_secs(40);
		let mut validation = tokio::time::interval(Duration::from_secs(3600));
		validation.tick().await;
		let mut transferred = false;
		let mut buffered_welcome = None;
		loop {
			let event = if let Some(welcome) = buffered_welcome.take() {
				welcome
			} else {
				tokio::select! {
					 _=cancel.cancelled()=>return Ok(()),
					 _=validation.tick()=>{let mut current=self.tokens("twitch").await?;self.validate_twitch_tokens(&mut current).await?;ensure!(current.account_id==tokens.account_id,"Twitch account changed; restart the session");continue},
					 event=tokio::time::timeout(keepalive,read_twitch_event(&mut socket))=>event.context("Twitch heartbeat timed out")??,
				}
			};
			match event["metadata"]["message_type"].as_str().unwrap_or("") {
				"session_welcome" => {
					let session = &event["payload"]["session"];
					keepalive = Duration::from_secs(
						session["keepalive_timeout_seconds"]
							.as_u64()
							.unwrap_or(30)
							.clamp(10, 600)
							+ 10,
					);
					if !transferred {
						let id = session["id"]
							.as_str()
							.context("Missing Twitch session ID")?;
						let response=self.http.post("https://api.twitch.tv/helix/eventsub/subscriptions").bearer_auth(&tokens.access_token).header("Client-Id",&tokens.client_id)
                            .json(&serde_json::json!({"type":"channel.chat.message","version":"1","condition":{"broadcaster_user_id":broadcaster,"user_id":tokens.account_id},"transport":{"method":"websocket","session_id":id}})).send().await?;
						check_response("twitch", &response)?;
					}
					self.status(
						"twitch",
						"connected",
						format!("Listening to {channel} as {}", tokens.login),
					);
				}
				"session_reconnect" => {
					let destination = event["payload"]["session"]["reconnect_url"]
						.as_str()
						.context("Missing Twitch reconnect URL")?;
					let url = url::Url::parse(destination)?;
					ensure!(
						url.scheme() == "wss"
							&& url.host_str() == Some("eventsub.wss.twitch.tv")
							&& url.username().is_empty()
							&& url.password().is_none(),
						"Invalid Twitch reconnect destination"
					);
					// Continue processing the old connection through the new
					// connection's welcome; only then close the old socket.
					let (next, welcome) = transfer_twitch_socket(
						&mut socket,
						open_twitch_socket(destination),
						|event| self.twitch_notification(event, &tokens, &broadcaster, messages, cancel),
						cancel,
					)
					.await?;
					socket = next;
					buffered_welcome = Some(welcome);
					transferred = true;
				}
				_ => {
					self
						.twitch_notification(event, &tokens, &broadcaster, messages, cancel)
						.await?
				}
			}
		}
	}
	async fn twitch_notification(
		&self,
		event: serde_json::Value,
		tokens: &super::oauth::Tokens,
		broadcaster: &str,
		messages: &mpsc::Sender<ChatMessage>,
		cancel: &CancellationToken,
	) -> Result<()> {
		match event["metadata"]["message_type"].as_str().unwrap_or("") {
			"notification" => {
				let e = &event["payload"]["event"];
				let Some(user) = e["chatter_user_id"].as_str() else {
					return Ok(());
				};
				let Some(id) = e["message_id"].as_str() else {
					return Ok(());
				};
				let Some(text) = e["message"]["text"].as_str() else {
					return Ok(());
				};
				if user == tokens.account_id
					&& self
						.is_echo("twitch", broadcaster, id, text, cancel)
						.await?
				{
					return Ok(());
				}
				let message = ChatMessage {
					platform: "twitch".into(),
					user_id: user.into(),
					display_name: e["chatter_user_name"].as_str().unwrap_or(user).into(),
					message_id: id.into(),
					channel_id: broadcaster.into(),
					text: text.chars().take(8000).collect(),
					is_owner: user == tokens.account_id,
				};
				tokio::select! {_=cancel.cancelled()=>return Ok(()),result=messages.send(message)=>result.context("Chat processing stopped")?};
			}
			"revocation" => anyhow::bail!("Twitch chat permission revoked; reconnect in Settings"),
			_ => {}
		}
		Ok(())
	}

	pub async fn youtube_chat(
		self: Arc<Self>,
		messages: mpsc::Sender<ChatMessage>,
		cancel: CancellationToken,
	) -> Result<()> {
		let mut page = String::new();
		let mut live_chat = String::new();
		let mut interval = 5u64;
		let mut failures = 0;
		while !cancel.is_cancelled() {
			let result = self
				.youtube_page(&mut live_chat, &mut page, &messages, &cancel)
				.await;
			match result {
				Ok(ms) => {
					interval = ms.max(1000).div_ceil(1000);
					failures = 0
				}
				Err(error) => {
					failures += 1;
					self.status("youtube", "connection_failed", error.to_string());
					interval = (interval * 2).min(60);
					if failures > 3 {
						live_chat.clear();
						page.clear();
					}
				}
			}
			tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(interval))=>{}}
		}
		self.status("youtube", "disconnected", "YouTube chat disconnected");
		Ok(())
	}
	async fn youtube_page(
		&self,
		live_chat: &mut String,
		page: &mut String,
		messages: &mpsc::Sender<ChatMessage>,
		cancel: &CancellationToken,
	) -> Result<u64> {
		let tokens = self.tokens("google").await?;
		if live_chat.is_empty() {
			let setting = self.store.settings()?.youtube_live_chat_id;
			*live_chat = if !setting.trim().is_empty() {
				setting.trim().to_owned()
			} else {
				let response = self
					.http
					.get("https://www.googleapis.com/youtube/v3/liveBroadcasts")
					.bearer_auth(&tokens.access_token)
					.query(&[
						("part", "snippet"),
						("broadcastStatus", "active"),
						("broadcastType", "all"),
						("maxResults", "50"),
					])
					.send()
					.await?;
				check_response("youtube", &response)?;
				let body: serde_json::Value = response.json().await?;
				select_youtube_live_chat(&body, &tokens.account_id)?
			};
		}
		let mut query = vec![
			("part", "snippet,authorDetails"),
			("liveChatId", live_chat.as_str()),
			("maxResults", "200"),
		];
		if !page.is_empty() {
			query.push(("pageToken", page.as_str()));
		}
		let response = tokio::select! {_=cancel.cancelled()=>return Ok(5000),r=self.http.get("https://www.googleapis.com/youtube/v3/liveChat/messages").bearer_auth(&tokens.access_token).query(&query).send()=>r?};
		check_response("youtube", &response)?;
		let body: serde_json::Value = response.json().await?;
		let first = page.is_empty();
		*page = body["nextPageToken"]
			.as_str()
			.context("YouTube paging token missing")?
			.into();
		self.status(
			"youtube",
			"connected",
			format!("Listening as {}", tokens.login),
		);
		// The first response contains chat history: do not execute old commands on reconnect.
		if !first {
			if let Some(items) = body["items"].as_array() {
				for item in items {
					if item["snippet"]["type"].as_str() != Some("textMessageEvent") {
						continue;
					}
					let Some(user) = item["authorDetails"]["channelId"].as_str() else {
						continue;
					};
					let Some(id) = item["id"].as_str() else {
						continue;
					};
					let Some(text) = item["snippet"]["textMessageDetails"]["messageText"].as_str()
					else {
						continue;
					};
					if user == tokens.account_id
						&& self.is_echo("youtube", live_chat, id, text, cancel).await?
					{
						continue;
					}
					let message = ChatMessage {
						platform: "youtube".into(),
						user_id: user.into(),
						display_name: item["authorDetails"]["displayName"]
							.as_str()
							.unwrap_or(user)
							.into(),
						message_id: id.into(),
						channel_id: live_chat.clone(),
						text: text.chars().take(8000).collect(),
						is_owner: user == tokens.account_id,
					};
					tokio::select! {_=cancel.cancelled()=>return Ok(5000),r=messages.send(message)=>r.context("Chat processing stopped")?};
				}
			}
		}
		Ok(body["pollingIntervalMillis"]
			.as_u64()
			.unwrap_or(5000)
			.max(1000))
	}
	pub async fn send_stream_chat(
		&self,
		platform: &str,
		channel: &str,
		text: &str,
	) -> Result<String> {
		ensure!(!text.trim().is_empty(), "Cannot send an empty chat message");
		match platform {
			"twitch" => {
				ensure!(
					text.chars().count() <= 500,
					"Twitch messages must be at most 500 characters"
				);
				let tokens = self.tokens("twitch").await?;
				let echo = self.expect_echo(platform, channel, text);
				let response = self
					.http
					.post("https://api.twitch.tv/helix/chat/messages")
					.bearer_auth(&tokens.access_token)
					.header("Client-Id", &tokens.client_id)
					.json(
						&serde_json::json!({"broadcaster_id":channel,"sender_id":tokens.account_id,"message":text}),
					)
					.send()
					.await
					.context("Twitch send outcome is unknown; do not retry automatically")?;
				if response.status().is_client_error()
					&& response.status() != reqwest::StatusCode::REQUEST_TIMEOUT
				{
					echo.rejected();
				}
				check_response("twitch", &response)?;
				let body: serde_json::Value = response.json().await?;
				if body["data"][0]["is_sent"].as_bool() == Some(false) {
					echo.rejected();
				}
				ensure!(
					body["data"][0]["is_sent"].as_bool() == Some(true),
					"Twitch did not acknowledge the message"
				);
				let id = body["data"][0]["message_id"]
					.as_str()
					.context("Twitch message acknowledgement missing")?;
				self.record_sent_echo(&echo, id)?;
				Ok(id.into())
			}
			"youtube" => {
				ensure!(
					text.chars().count() <= 200,
					"YouTube messages must be at most 200 characters"
				);
				let tokens = self.tokens("google").await?;
				let echo = self.expect_echo(platform, channel, text);
				let response=self.http.post("https://www.googleapis.com/youtube/v3/liveChat/messages").bearer_auth(&tokens.access_token).query(&[("part","snippet")]).json(&serde_json::json!({"snippet":{"liveChatId":channel,"type":"textMessageEvent","textMessageDetails":{"messageText":text}}})).send().await.context("YouTube send outcome is unknown; do not retry automatically")?;
				if response.status().is_client_error()
					&& response.status() != reqwest::StatusCode::REQUEST_TIMEOUT
				{
					echo.rejected();
				}
				check_response("youtube", &response)?;
				let body: serde_json::Value = response.json().await?;
				let id = body["id"]
					.as_str()
					.context("YouTube message acknowledgement missing")?;
				self.record_sent_echo(&echo, id)?;
				Ok(id.into())
			}
			_ => anyhow::bail!("Unknown streaming platform"),
		}
	}
}

fn select_youtube_live_chat(body: &serde_json::Value, account_id: &str) -> Result<String> {
	ensure!(
		body["nextPageToken"].as_str().is_none_or(str::is_empty),
		"YouTube returned more active broadcasts; enter the intended live chat ID in Settings"
	);
	let mut chats = std::collections::BTreeSet::new();
	if let Some(items) = body["items"].as_array() {
		for item in items {
			if item["snippet"]["channelId"].as_str() != Some(account_id) {
				continue;
			}
			if let Some(id) = item["snippet"]["liveChatId"]
				.as_str()
				.filter(|id| !id.is_empty())
			{
				chats.insert(id);
			}
		}
	}
	ensure!(
		!chats.is_empty(),
		"No active YouTube chat found for the authorized channel; start a broadcast or enter a live chat ID"
	);
	ensure!(
		chats.len() == 1,
		"Multiple YouTube broadcasts are active; enter the intended live chat ID in Settings"
	);
	Ok(chats.into_iter().next().unwrap().to_owned())
}

async fn open_twitch_socket(
	url: &str,
) -> Result<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>> {
	let config = WebSocketConfig::default()
		.max_message_size(Some(1024 * 1024))
		.max_frame_size(Some(1024 * 1024));
	let (socket, _) = connect_async_with_config(url, Some(config), false)
		.await
		.context("Cannot connect to Twitch EventSub")?;
	Ok(socket)
}
async fn read_twitch_event<S>(socket: &mut WebSocketStream<S>) -> Result<serde_json::Value>
where
	S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
	loop {
		let frame = socket
			.next()
			.await
			.context("Twitch connection ended")?
			.context("Twitch connection failed")?;
		match frame {
			Message::Text(text) => {
				ensure!(text.len() <= 1024 * 1024, "Twitch event too large");
				return serde_json::from_str(&text).context("Invalid Twitch event");
			}
			Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await?,
			Message::Close(_) => anyhow::bail!("Twitch connection closed"),
			_ => {}
		}
	}
}
async fn transfer_twitch_socket<S, F, H, HF>(
	old: &mut WebSocketStream<S>,
	connecting: F,
	mut incoming: H,
	cancel: &CancellationToken,
) -> Result<(WebSocketStream<S>, serde_json::Value)>
where
	S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
	F: std::future::Future<Output = Result<WebSocketStream<S>>> + Send,
	H: FnMut(serde_json::Value) -> HF + Send,
	HF: std::future::Future<Output = Result<()>> + Send,
{
	let handoff = async {
		tokio::pin!(connecting);
		let mut next = loop {
			tokio::select! {
				 connected=&mut connecting=>break connected?,
				 event=read_twitch_event(old)=>incoming(event?).await?,
			}
		};
		let welcome = loop {
			tokio::select! {
				 biased;
				 event=read_twitch_event(&mut next)=>{let event=event?;ensure!(event["metadata"]["message_type"]=="session_welcome","Replacement Twitch socket did not send a welcome");break event},
				 event=read_twitch_event(old)=>incoming(event?).await?,
			}
		};
		old.close(None)
			.await
			.context("Close transferred Twitch socket")?;
		Ok((next, welcome))
	};
	tokio::select! {_=cancel.cancelled()=>anyhow::bail!("Twitch reconnect canceled"),result=tokio::time::timeout(Duration::from_secs(30),handoff)=>result.context("Twitch reconnect deadline expired")?}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::DuplexStream;
	use tokio_tungstenite::tungstenite::protocol::Role;
	fn broadcast(owner: &str, chat: &str) -> serde_json::Value {
		serde_json::json!({"snippet":{"channelId":owner,"liveChatId":chat}})
	}
	#[test]
	fn youtube_discovery_requires_one_owned_chat_and_rejects_ambiguous_pages() {
		use serde_json::json;
		assert!(select_youtube_live_chat(&json!({"items":[]}), "owner").is_err());
		assert!(
			select_youtube_live_chat(&json!({"items":[broadcast("other", "elsewhere")]}), "owner")
				.is_err()
		);
		assert_eq!(
			select_youtube_live_chat(
				&json!({"items":[broadcast("other", "elsewhere"),broadcast("owner", "chosen")]}),
				"owner"
			)
			.unwrap(),
			"chosen"
		);
		assert!(
			select_youtube_live_chat(
				&json!({"items":[broadcast("owner", "one"),broadcast("owner", "two")]}),
				"owner"
			)
			.unwrap_err()
			.to_string()
			.contains("Multiple")
		);
		assert!(
			select_youtube_live_chat(
				&json!({"items":[broadcast("owner", "first-page")],"nextPageToken":"more"}),
				"owner"
			)
			.unwrap_err()
			.to_string()
			.contains("more active broadcasts")
		);
		assert_eq!(
			select_youtube_live_chat(
				&json!({"items":[broadcast("owner", "same"),broadcast("owner", "same")],"nextPageToken":""}),
				"owner"
			)
			.unwrap(),
			"same"
		);
	}
	async fn sockets() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
		let (a, b) = tokio::io::duplex(8192);
		tokio::join!(
			WebSocketStream::from_raw_socket(a, Role::Client, None),
			WebSocketStream::from_raw_socket(b, Role::Server, None)
		)
	}
	#[tokio::test]
	async fn reconnect_delivers_old_socket_messages_until_replacement_welcome() {
		let (mut old, mut old_server) = sockets().await;
		let (next, mut next_server) = sockets().await;
		let seen = Arc::new(tokio::sync::Notify::new());
		let server_seen = seen.clone();
		let old_task = tokio::spawn(async move {
			old_server.send(Message::Text(serde_json::json!({"metadata":{"message_type":"notification"},"payload":{"message":"last-old-message"}}).to_string().into())).await.unwrap();
			let message = old_server.next().await.unwrap().unwrap();
			assert!(matches!(message, Message::Close(_)));
		});
		let next_task = tokio::spawn(async move {
			// The new welcome is deliberately held until the old notification is
			// actually consumed. Closing the old socket early makes this fail.
			server_seen.notified().await;
			next_server.send(Message::Text(serde_json::json!({"metadata":{"message_type":"session_welcome"},"payload":{"session":{"id":"new-session"}}}).to_string().into())).await.unwrap();
			next_server
		});
		let mut messages = Vec::new();
		let (_, welcome) = tokio::time::timeout(
			Duration::from_secs(2),
			transfer_twitch_socket(
				&mut old,
				async { Ok(next) },
				|event| {
					messages.push(event);
					seen.notify_one();
					std::future::ready(Ok(()))
				},
				&CancellationToken::new(),
			),
		)
		.await
		.unwrap()
		.unwrap();
		assert_eq!(messages.len(), 1);
		assert_eq!(messages[0]["payload"]["message"], "last-old-message");
		assert_eq!(welcome["payload"]["session"]["id"], "new-session");
		old_task.await.unwrap();
		next_task.await.unwrap();
	}
	#[tokio::test]
	async fn reconnect_cancel_stops_waiting_for_unresponsive_replacement() {
		let (mut old, _old_server) = sockets().await;
		let cancel = CancellationToken::new();
		let copy = cancel.clone();
		let task = tokio::spawn(async move {
			transfer_twitch_socket(
				&mut old,
				std::future::pending::<Result<WebSocketStream<DuplexStream>>>(),
				|_| std::future::ready(Ok(())),
				&copy,
			)
			.await
		});
		cancel.cancel();
		assert!(
			tokio::time::timeout(Duration::from_secs(1), task)
				.await
				.unwrap()
				.unwrap()
				.is_err()
		);
	}
}
