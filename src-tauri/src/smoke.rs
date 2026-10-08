//! Isolated installed-package checks. No provider credentials or production data are used.
use crate::Runtime;
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

/// Only the trusted desktop window can acknowledge this. Ordinary runs have no smoke state.
#[tauri::command]
pub fn frontend_probe_required(state: tauri::State<'_, Arc<Runtime>>) -> bool {
	state.smoke_frontend.is_some()
}

#[tauri::command]
pub fn frontend_result(state: tauri::State<'_, Arc<Runtime>>, error: Option<String>) {
	if let Some(frontend) = &state.smoke_frontend {
		frontend.send_replace(Some(error.map_or(Ok(()), Err)));
	}
}

pub async fn run(state: Arc<Runtime>) -> Result<()> {
	ensure!(state.overlay_error.is_none(), "OBS endpoint could not bind");
	let temporary = std::env::temp_dir().join(format!("bumblebee-smoke-{}", uuid::Uuid::new_v4()));
	std::fs::create_dir_all(&temporary)?;
	let database = temporary.join("smoke.sqlite3");
	let result = async {
		let mut frontend = state
			.smoke_frontend
			.as_ref()
			.context("Not an isolated smoke run")?
			.subscribe();
		tokio::time::timeout(
			Duration::from_secs(60),
			frontend.wait_for(|result| result.is_some()),
		)
		.await
		.context("Desktop interface did not load its model and render a frame within 60 seconds")??
		.as_ref()
		.expect("waited for frontend result")
		.as_ref()
		.map_err(|error| anyhow::anyhow!("Desktop renderer failed: {error}"))?;
		{
			let store = bumblebee_core::storage::Store::open(&database)?;
			store.set("smoke", &"persistent")?;
		}
		{
			let store = bumblebee_core::storage::Store::open(&database)?;
			ensure!(
				store.get::<String>("smoke")?.as_deref() == Some("persistent"),
				"SQLite state did not survive reopening"
			);
		}
		tokio::task::spawn_blocking(|| -> Result<()> {
			let name = format!("smoke-{}", uuid::Uuid::new_v4());
			let secret = uuid::Uuid::new_v4().to_string();
			let entry = keyring::Entry::new("buzz.bumblebee.desktop.smoke-test", &name)?;
			entry
				.set_password(&secret)
				.context("OS credential store write failed")?;
			let read = entry.get_password();
			let removed = entry.delete_credential();
			ensure!(
				read? == secret,
				"OS credential store did not preserve the temporary entry"
			);
			removed.context("Cannot remove temporary keyring entry")?;
			Ok(())
		})
		.await??;
		bumblebee_audio::NativeResources::from_bundle(&state.engine.paths.native_dir).probe()?;
		let http = reqwest::Client::builder()
			.no_proxy()
			.timeout(Duration::from_secs(5))
			.build()?;
		let response = http.get(state.transport.url()).send().await?;
		ensure!(
			response.status().is_success(),
			"Packaged overlay was not available through loopback"
		);
		let html = response.text().await?;
		ensure!(
			html.contains("Bumblebee Overlay") && html.contains("assets/"),
			"Overlay assets were not built and bundled"
		);
		let base = state.transport.url().replace("overlay.html", "");
		let mut assets: Vec<String> = html
			.split('"')
			.filter_map(|value| {
				value
					.strip_prefix("./assets/")
					.map(|name| format!("assets/{name}"))
			})
			.collect();
		ensure!(
			!assets.is_empty(),
			"Packaged overlay has no script/style references"
		);
		assets.push("models/bumblebee.cb67e11b.glb".into());
		assets.push(format!(
			"puppets/images/{}.png",
			bumblebee_core::catalog::puppets()[0].id
		));
		for asset in assets {
			let response = http.get(format!("{base}{asset}")).send().await?;
			ensure!(
				response.status().is_success(),
				"Missing packaged rendering asset: {asset}"
			);
			ensure!(
				!response.bytes().await?.is_empty(),
				"Empty packaged asset: {asset}"
			);
		}
		for forbidden in ["index.html", "settings", "../bumblebee.sqlite3"] {
			ensure!(
				http
					.get(format!("{base}{forbidden}"))
					.send()
					.await?
					.status() == 404,
				"Local endpoint exposed non-rendering route {forbidden}"
			);
		}
		let mut request =
			format!("{}events", base.replace("http://", "ws://")).into_client_request()?;
		request.headers_mut().insert(
			"Origin",
			format!("http://127.0.0.1:{}", state.transport.port).parse()?,
		);
		let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
		let initial = tokio::time::timeout(Duration::from_secs(5), socket.next())
			.await?
			.context("Overlay socket closed without initial settings")??;
		let initial: serde_json::Value = serde_json::from_str(initial.to_text()?)?;
		ensure!(
			initial["type"] == "overlay_settings",
			"Overlay did not receive its persisted layout"
		);
		state
			.transport
			.events
			.send(bumblebee_core::model::OverlayEvent::Status {
				message: "isolated smoke event".into(),
			})?;
		let event = tokio::time::timeout(Duration::from_secs(5), socket.next())
			.await?
			.context("Overlay did not receive event")??;
		ensure!(
			event.to_text()?.contains("isolated smoke event"),
			"Overlay event transport failed"
		);
		let original = state.transport.url();
		state.transport.token.send_replace(format!(
			"{}{}",
			uuid::Uuid::new_v4().simple(),
			uuid::Uuid::new_v4().simple()
		));
		let closed = tokio::time::timeout(Duration::from_secs(5), socket.next()).await?;
		ensure!(
			matches!(closed, None | Some(Ok(Message::Close(_)))),
			"Revocation did not close the existing overlay socket"
		);
		ensure!(
			http.get(original).send().await?.status() == 404,
			"Revoked overlay token still works"
		);
		ensure!(
			http
				.get(state.transport.url())
				.send()
				.await?
				.status()
				.is_success(),
			"Replacement overlay token did not work"
		);
		// Leave the verified frame on screen briefly so native compositor captures are meaningful.
		tokio::time::sleep(Duration::from_millis(500)).await;
		Ok(())
	}
	.await;
	let _ = std::fs::remove_dir_all(temporary);
	result
}
