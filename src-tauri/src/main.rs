#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod artifacts;
mod commands;
mod secrets;
mod smoke;
mod transport;

use bumblebee_core::{
	model::OverlayEvent,
	providers::Providers,
	runtime::{Engine, EnginePaths},
	storage::Store,
};
use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};
use tauri::{
	Emitter, Manager,
	menu::{Menu, MenuItem},
	tray::TrayIconBuilder,
};
use tokio::sync::{Mutex, broadcast, watch};
use tokio_util::sync::CancellationToken;

struct Runtime {
	store: Arc<Store>,
	providers: Arc<Providers>,
	engine: Arc<Engine>,
	transport: Arc<transport::OverlayTransport>,
	server: Mutex<Option<tokio::task::JoinHandle<anyhow::Result<()>>>>,
	shutdown: CancellationToken,
	quitting: AtomicBool,
	overlay_error: Option<String>,
	configuration: Mutex<()>,
}

async fn shutdown_app(app: tauri::AppHandle, state: Arc<Runtime>) {
	if state.quitting.swap(true, Ordering::SeqCst) {
		return;
	}
	let _configuration = state.configuration.lock().await;
	state.providers.cancel_authorizations();
	state.engine.cancel().await;
	if let Err(error) = state.engine.stop().await {
		tracing::error!("Session shutdown: {error}");
	}
	state.shutdown.cancel();
	if let Some(server) = state.server.lock().await.take() {
		let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server).await;
	}
	app.exit(0);
}
fn show_window(app: &tauri::AppHandle) {
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.show();
		let _ = window.unminimize();
		let _ = window.set_focus();
	}
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
	let handle = app.handle().clone();
	let smoke_mode = std::env::args().any(|arg| arg == "--smoke-test");
	let smoke_id = uuid::Uuid::new_v4();
	let data = if smoke_mode {
		std::env::temp_dir().join(format!("bumblebee-installed-smoke-{smoke_id}"))
	} else {
		app.path().app_data_dir()?
	};
	std::fs::create_dir_all(&data)?;
	if smoke_mode {
		if let Some(window) = app.get_webview_window("main") {
			window.hide()?;
		}
	}
	for directory in ["images", "media"] {
		std::fs::create_dir_all(data.join(directory))?;
	}
	let store = Arc::new(Store::open(&data.join("bumblebee.sqlite3"))?);
	let keyring_service = if smoke_mode {
		format!("buzz.bumblebee.desktop.smoke-{smoke_id}")
	} else {
		"buzz.bumblebee.desktop".into()
	};
	let providers = Providers::new(
		store.clone(),
		Arc::new(secrets::OsSecrets::new(keyring_service)),
	)?;
	let shutdown = CancellationToken::new();
	let (events, _) = broadcast::channel(256);
	let mut overlay_rx = events.subscribe();
	let mut status_rx = providers.status_events.subscribe();
	let (token, _) = watch::channel(store.overlay_token()?);
	let asset_app = handle.clone();
	let assets = Arc::new(move |path: &str| {
		asset_app
			.asset_resolver()
			.get(path.to_owned())
			.map(|asset| (asset.bytes, asset.mime_type))
			.or_else(|| {
				#[cfg(dev)]
				{
					let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist");
					std::fs::read(root.join(path)).ok().map(|bytes| {
						(
							bytes,
							mime_guess::from_path(path)
								.first_or_octet_stream()
								.to_string(),
						)
					})
				}
				#[cfg(not(dev))]
				{
					None
				}
			})
	});
	let transport = Arc::new(transport::OverlayTransport {
		port: store.settings()?.overlay_port,
		token,
		events: events.clone(),
		store: store.clone(),
		images: data.join("images"),
		media: data.join("media"),
		assets,
		shutdown: shutdown.clone(),
	});
	#[cfg(dev)]
	let native_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
	#[cfg(not(dev))]
	let native_dir = app.path().resource_dir()?;
	let engine = Engine::new(
		providers.clone(),
		EnginePaths {
			data_dir: data,
			native_dir,
		},
		events,
	)?;
	let (server, overlay_error) = match tauri::async_runtime::block_on(transport.clone().start()) {
		Ok(server) => (Some(server), None),
		Err(error) => (
			None,
			Some(format!(
				"Cannot start the OBS endpoint on port {}: {error}. Change its port in Settings → Application, then restart Bumblebee.",
				transport.port
			)),
		),
	};
	let state = Arc::new(Runtime {
		store,
		providers,
		engine,
		transport,
		server: Mutex::new(server),
		shutdown: shutdown.clone(),
		quitting: AtomicBool::new(false),
		overlay_error,
		configuration: Mutex::new(()),
	});
	app.manage(state.clone());
	if smoke_mode {
		let app = app.handle().clone();
		tauri::async_runtime::spawn(async move {
			let result = smoke::run(state.clone()).await;
			let status = if let Err(error) = result {
				eprintln!("Installed-package smoke test failed: {error:#}");
				1
			} else {
				println!(
					"Installed-package smoke test passed: SQLite, keyring, native libraries, loopback assets."
				);
				0
			};
			state.quitting.store(true, Ordering::SeqCst);
			let _ = state.engine.stop().await;
			state.shutdown.cancel();
			let _ = std::fs::remove_dir_all(&state.engine.paths.data_dir);
			app.exit(status);
		});
	}
	tauri::async_runtime::spawn(async move {
		loop {
			tokio::select! {
				 _=shutdown.cancelled()=>break,
				 event=overlay_rx.recv()=>match event {Ok(event)=>{let _=handle.emit("bumblebee:overlay",event);},Err(broadcast::error::RecvError::Lagged(_))=>{let _=handle.emit("bumblebee:overlay",OverlayEvent::StopSpeech);},Err(_)=>break},
				 status=status_rx.recv()=>match status {Ok(status)=>{let _=handle.emit("bumblebee:provider-status",status);},Err(broadcast::error::RecvError::Lagged(_))=>{},Err(_)=>break},
			}
		}
	});
	let show = MenuItem::with_id(app, "show", "Open Bumblebee", true, None::<&str>)?;
	let quit = MenuItem::with_id(app, "quit", "Quit Bumblebee", true, None::<&str>)?;
	let menu = Menu::with_items(app, &[&show, &quit])?;
	let mut tray = TrayIconBuilder::new()
		.tooltip("Bumblebee")
		.menu(&menu)
		.show_menu_on_left_click(true)
		.on_menu_event(|app, event| match event.id.as_ref() {
			"show" => show_window(app),
			"quit" => {
				let state = app.state::<Arc<Runtime>>().inner().clone();
				let app = app.clone();
				tauri::async_runtime::spawn(shutdown_app(app, state));
			}
			_ => {}
		});
	if let Some(icon) = app.default_window_icon() {
		tray = tray.icon(icon.clone());
	}
	tray.build(app)?;
	Ok(())
}

fn main() {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| "bumblebee=info,warn".into()),
		)
		.init();
	let builder = tauri::Builder::default();
	// A smoke invocation must run its checks, never silently become a second-instance notification.
	let builder = if std::env::args().any(|argument| argument == "--smoke-test") {
		builder
	} else {
		builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
			show_window(app)
		}))
	};
	let app = builder
		.plugin(tauri_plugin_opener::init())
		.plugin(tauri_plugin_autostart::Builder::new().build())
		.invoke_handler(tauri::generate_handler![
			commands::get_snapshot,
			commands::save_overlay_settings,
			commands::get_activity,
			commands::save_settings,
			commands::set_secret,
			commands::delete_secret,
			commands::validate_provider,
			commands::authorize_provider,
			commands::start_session,
			commands::stop_session,
			commands::preview_speech,
			commands::cancel_speech,
			commands::search_chatters,
			commands::review_image,
			commands::update_chatter,
			commands::rotate_overlay_token,
			commands::quit_app,
			commands::answer_pending,
			commands::dismiss_interrupted,
			artifacts::preview_artifact,
			artifacts::preview_submission,
			artifacts::reveal_artifact,
			artifacts::show_artifact,
			artifacts::hide_artifact
		])
		.setup(setup)
		.on_window_event(|window, event| {
			if let tauri::WindowEvent::CloseRequested { api, .. } = event {
				let state = window.state::<Arc<Runtime>>().inner().clone();
				api.prevent_close();
				if state.quitting.load(Ordering::SeqCst) {
					return;
				}
				if state.engine.is_active() {
					let _ = window.hide();
				} else {
					let app = window.app_handle().clone();
					tauri::async_runtime::spawn(shutdown_app(app, state));
				}
			}
		})
		.build(tauri::generate_context!())
		.expect("Bumblebee could not start");
	app.run(|app, event| {
		if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
			let state = app.state::<Arc<Runtime>>().inner().clone();
			if !state.quitting.load(Ordering::SeqCst) || code.is_none() {
				api.prevent_exit();
				tauri::async_runtime::spawn(shutdown_app(app.clone(), state));
			}
		}
	});
}
