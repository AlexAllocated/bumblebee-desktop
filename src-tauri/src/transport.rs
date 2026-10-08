//! Rendering-only loopback transport. All administrative operations remain Tauri commands.
use axum::{
	Router,
	body::Body,
	extract::{
		Path, State, WebSocketUpgrade,
		ws::{Message, WebSocket},
	},
	http::{HeaderMap, StatusCode, header},
	response::{IntoResponse, Response},
	routing::get,
};
use bumblebee_core::{images::valid_hash, model::OverlayEvent};
use futures_util::{SinkExt, StreamExt};
use std::{path::PathBuf, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::{
	sync::{broadcast, watch},
	task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OverlayTransport {
	pub port: u16,
	pub token: watch::Sender<String>,
	pub events: broadcast::Sender<OverlayEvent>,
	pub store: Arc<bumblebee_core::storage::Store>,
	pub images: PathBuf,
	pub media: PathBuf,
	pub assets: Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>,
	pub shutdown: CancellationToken,
}

impl OverlayTransport {
	pub fn url(&self) -> String {
		format!(
			"http://127.0.0.1:{}/{}/overlay.html",
			self.port,
			self.token.borrow().as_str()
		)
	}
	pub async fn start(self: Arc<Self>) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
		let listener =
			tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, self.port)).await?;
		let router = self.clone().router();
		let shutdown = self.shutdown.clone();
		Ok(tokio::spawn(async move {
			axum::serve(listener, router)
				.with_graceful_shutdown(shutdown.cancelled_owned())
				.await?;
			Ok(())
		}))
	}
	fn router(self: Arc<Self>) -> Router {
		Router::new()
			.route("/{token}/events", get(websocket))
			.route("/{token}/{*path}", get(asset))
			.with_state(self)
	}
	fn authorized(&self, token: &str, headers: &HeaderMap) -> bool {
		valid_request(&self.token.borrow(), token, headers, self.port)
	}
}

fn valid_request(expected: &str, token: &str, headers: &HeaderMap, port: u16) -> bool {
	let expected_host = format!("127.0.0.1:{port}");
	headers.get(header::HOST).and_then(|s| s.to_str().ok()) == Some(expected_host.as_str())
		&& expected.as_bytes().ct_eq(token.as_bytes()).into()
}

fn safe_relative_path(path: &str) -> bool {
	!path.is_empty()
		&& !path.contains(['\\', '\0', '%'])
		&& path
			.split('/')
			.all(|p| !p.is_empty() && p != "." && p != "..")
}

async fn websocket(
	State(state): State<Arc<OverlayTransport>>,
	Path(token): Path<String>,
	headers: HeaderMap,
	ws: WebSocketUpgrade,
) -> Response {
	let origin = format!("http://127.0.0.1:{}", state.port);
	if !state.authorized(&token, &headers)
		|| headers.get(header::ORIGIN).and_then(|s| s.to_str().ok()) != Some(origin.as_str())
	{
		return StatusCode::NOT_FOUND.into_response();
	}
	ws.max_message_size(4096)
		.max_frame_size(4096)
		.on_upgrade(move |socket| serve_socket(socket, state, token))
}

async fn serve_socket(socket: WebSocket, state: Arc<OverlayTransport>, expected_token: String) {
	let mut events = state.events.subscribe();
	let mut token_changes = state.token.subscribe();
	if token_changes.borrow().as_str() != expected_token {
		return;
	}
	let (mut sender, mut receiver) = socket.split();
	let store = state.store.clone();
	let initial = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<OverlayEvent>> {
		Ok(vec![
			OverlayEvent::OverlaySettings {
				settings: store.get("overlay_settings")?.unwrap_or_default(),
			},
			OverlayEvent::AudioSettings {
				settings: store.settings()?.audio_mix(),
			},
		])
	})
	.await;
	if token_changes.has_changed().unwrap_or(true) || state.shutdown.is_cancelled() {
		return;
	}
	let Ok(Ok(initial)) = initial else {
		return;
	};
	for event in initial {
		let Ok(json) = serde_json::to_string(&event) else {
			return;
		};
		if !matches!(
			tokio::time::timeout(
				std::time::Duration::from_secs(2),
				sender.send(Message::Text(json.into()))
			)
			.await,
			Ok(Ok(()))
		) {
			return;
		}
	}

	loop {
		tokio::select! {
			 _ = state.shutdown.cancelled() => break,
			 _ = token_changes.changed() => break,
			 incoming = receiver.next() => match incoming {
				  Some(Ok(Message::Ping(bytes))) => { if sender.send(Message::Pong(bytes)).await.is_err() { break; } }
				  Some(Ok(Message::Pong(_))) => {},
				  // This protocol deliberately accepts no application messages.
				  _ => break,
			 },
			 event = events.recv() => match event {
				  Ok(event) => if let Ok(json) = serde_json::to_string(&event) {
						// A slow renderer must not accumulate an unbounded audio backlog.
						match tokio::time::timeout(std::time::Duration::from_secs(2), sender.send(Message::Text(json.into()))).await {
							 Ok(Ok(())) => {}, _ => break,
						}
				  },
				  Err(_) => break,
			 }
		}
	}
	let _ = sender.close().await;
}

async fn asset(
	State(state): State<Arc<OverlayTransport>>,
	Path((token, path)): Path<(String, String)>,
	headers: HeaderMap,
) -> Response {
	if !state.authorized(&token, &headers) || !safe_relative_path(&path) {
		return StatusCode::NOT_FOUND.into_response();
	}
	let result: Option<(Vec<u8>, String)> = if let Some(name) = path.strip_prefix("images/") {
		if let Some(hash) = name.strip_suffix(".png").filter(|hash| valid_hash(hash)) {
			let store = state.store.clone();
			let hash = hash.to_owned();
			let approved = tokio::task::spawn_blocking(move || store.is_approved_image(&hash)).await;
			if !matches!(approved, Ok(Ok(true))) {
				return StatusCode::NOT_FOUND.into_response();
			}
			tokio::fs::read(state.images.join(name))
				.await
				.ok()
				.map(|b| (b, "image/png".into()))
		} else {
			None
		}
	} else if let Some(name) = path.strip_prefix("media/") {
		// Providers write content-addressed files here; never allow nested paths.
		if name.len() <= 160
			&& name
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
			&& !name.starts_with('.')
		{
			tokio::fs::read(state.media.join(name)).await.ok().map(|b| {
				(
					b,
					mime_guess::from_path(name)
						.first_or_octet_stream()
						.to_string(),
				)
			})
		} else {
			None
		}
	} else if path == "overlay.html"
		|| path.starts_with("assets/")
		|| path.starts_with("puppets/")
		|| path.starts_with("models/")
        || path.starts_with("audio/")
	{
		(state.assets)(&path)
	} else {
		None
	};
	let Some((bytes, mime)) = result else {
		return StatusCode::NOT_FOUND.into_response();
	};
	let mut response = Response::new(Body::from(bytes));
	let headers = response.headers_mut();
	if let Ok(mime) = mime.parse() {
		headers.insert(header::CONTENT_TYPE, mime);
	}
	headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
	headers.insert("x-content-type-options", "nosniff".parse().unwrap());
	headers.insert("referrer-policy", "no-referrer".parse().unwrap());
	// The desktop preview reads these rendering-only resources using its Tauri origin.
	headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
	headers.insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; worker-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'".parse().unwrap());
	response
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::http::Request;
	use tower::ServiceExt;
	#[tokio::test]
	async fn rendering_routes_are_capability_scoped_and_revocable() {
		let directory = tempfile::tempdir().unwrap();
		let (token, _) = watch::channel("good-token".to_owned());
		let (events, _) = broadcast::channel(4);
		let state = Arc::new(OverlayTransport {
			port: 2899,
			token,
			events,
			store: Arc::new(
				bumblebee_core::storage::Store::open(&directory.path().join("test.sqlite")).unwrap(),
			),
			images: directory.path().join("images"),
			media: directory.path().join("media"),
			assets: Arc::new(|path| Some((path.as_bytes().to_vec(), "text/plain".into()))),
			shutdown: CancellationToken::new(),
		});
		let request = |path: &str| {
			Request::builder()
				.uri(path)
				.header(header::HOST, "127.0.0.1:2899")
				.body(Body::empty())
				.unwrap()
		};
		let app = state.clone().router();
		assert_eq!(
			app.clone()
				.oneshot(request("/good-token/overlay.html"))
				.await
				.unwrap()
				.status(),
			StatusCode::OK
		);
		for path in [
			"/wrong/overlay.html",
			"/good-token/index.html",
			"/good-token/settings",
			"/good-token/../secrets",
			"/good-token/assets/../secrets",
		] {
			assert_eq!(
				app.clone().oneshot(request(path)).await.unwrap().status(),
				StatusCode::NOT_FOUND,
				"{path}"
			);
		}
		// Pending bytes exist locally but cannot enter the rendering endpoint before approval.
		std::fs::create_dir_all(&state.images).unwrap();
		let hash = "a".repeat(64);
		std::fs::write(state.images.join(format!("{hash}.png")), b"saved-image").unwrap();
		state
			.store
			.ensure_chatter("twitch", "viewer", "Viewer")
			.unwrap();
		let submission = state.store.submit_image("twitch", "viewer", &hash).unwrap();
		let image_url = format!("/good-token/images/{hash}.png");
		assert_eq!(
			app.clone()
				.oneshot(request(&image_url))
				.await
				.unwrap()
				.status(),
			StatusCode::NOT_FOUND
		);
		state.store.review_image(&submission, true).unwrap();
		assert_eq!(
			app.clone()
				.oneshot(request(&image_url))
				.await
				.unwrap()
				.status(),
			StatusCode::OK
		);
		state.store.reset_chatter("twitch", "viewer").unwrap();
		assert_eq!(
			app.clone()
				.oneshot(request(&image_url))
				.await
				.unwrap()
				.status(),
			StatusCode::NOT_FOUND
		);
		state.token.send_replace("new-token".into());
		assert_eq!(
			app.clone()
				.oneshot(request("/good-token/overlay.html"))
				.await
				.unwrap()
				.status(),
			StatusCode::NOT_FOUND
		);
		assert_eq!(
			app.oneshot(request("/new-token/overlay.html"))
				.await
				.unwrap()
				.status(),
			StatusCode::OK
		);
	}
	#[test]
	fn local_address_and_token_are_both_required() {
		let mut headers = HeaderMap::new();
		headers.insert(header::HOST, "127.0.0.1:2899".parse().unwrap());
		assert!(valid_request("correct", "correct", &headers, 2899));
		assert!(!valid_request("correct", "incorrect", &headers, 2899));
		headers.insert(header::HOST, "attacker.example:2899".parse().unwrap());
		assert!(!valid_request("correct", "correct", &headers, 2899));
	}
	#[test]
	fn resource_paths_cannot_escape_the_render_assets() {
		for invalid in [
			"../settings.json",
			"images/../../private",
			"/absolute",
			"a\\b",
			"a/%2fsecret",
			"a//b",
			"a/./b",
		] {
			assert!(!safe_relative_path(invalid), "{invalid}");
		}
		assert!(safe_relative_path("images/valid.png"));
	}
}
