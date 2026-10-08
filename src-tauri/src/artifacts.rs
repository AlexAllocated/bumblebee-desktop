use crate::Runtime;
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use bumblebee_core::{agent::Artifact, model::OverlayEvent};
use std::{path::PathBuf, sync::Arc};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

pub fn list(state: &Runtime) -> Result<Vec<Artifact>> {
	Ok(state
		.store
		.get::<Vec<Artifact>>("generated_artifacts")?
		.unwrap_or_default())
}
fn resolve(state: &Runtime, id: &str) -> Result<(Artifact, PathBuf)> {
	let artifact = list(state)?
		.into_iter()
		.find(|a| a.id == id)
		.context("Generated file no longer exists")?;
	ensure!(
		!artifact.filename.is_empty()
			&& artifact.filename.len() < 200
			&& !artifact.filename.starts_with('.')
			&& artifact
				.filename
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')),
		"Invalid generated filename"
	);
	let directory = state
		.engine
		.paths
		.data_dir
		.join("artifacts")
		.canonicalize()?;
	let path = directory.join(&artifact.filename).canonicalize()?;
	ensure!(
		path.parent() == Some(directory.as_path()),
		"Generated file escaped its data directory"
	);
	Ok((artifact, path))
}
fn image_bytes(state: &Runtime, id: &str) -> Result<(Artifact, Vec<u8>)> {
	let (artifact, path) = resolve(state, id)?;
	ensure!(
		["image/png", "image/jpeg", "image/webp"].contains(&artifact.media_type.as_str()),
		"This file has no image preview"
	);
	ensure!(
		path.metadata()?.len() <= 20 * 1024 * 1024,
		"Image exceeds the preview limit"
	);
	Ok((artifact, std::fs::read(path)?))
}
#[tauri::command]
pub async fn preview_artifact(
	state: State<'_, Arc<Runtime>>,
	id: String,
) -> Result<String, String> {
	let state = state.inner().clone();
	tokio::task::spawn_blocking(move || {
		let (artifact, bytes) = image_bytes(&state, &id).map_err(|e| e.to_string())?;
		Ok(format!(
			"data:{};base64,{}",
			artifact.media_type,
			base64::engine::general_purpose::STANDARD.encode(bytes)
		))
	})
	.await
	.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn reveal_artifact(
	app: tauri::AppHandle,
	state: State<'_, Arc<Runtime>>,
	id: String,
) -> Result<(), String> {
	let (_, path) = resolve(&state, &id).map_err(|e| e.to_string())?;
	app.opener()
		.reveal_item_in_dir(path)
		.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn show_artifact(state: State<'_, Arc<Runtime>>, id: String) -> Result<(), String> {
	let state = state.inner().clone();
	tokio::task::spawn_blocking(move || {
		let (artifact, bytes) = image_bytes(&state, &id).map_err(|e| e.to_string())?;
		let extension = match artifact.media_type.as_str() {
			"image/jpeg" => "jpg",
			"image/webp" => "webp",
			_ => "png",
		};
		let filename = format!("render-{}.{}", uuid::Uuid::new_v4(), extension);
		std::fs::write(state.transport.media.join(&filename), bytes).map_err(|e| e.to_string())?;
		let _ = state.transport.events.send(OverlayEvent::Image {
			id: artifact.id,
			image_path: filename,
			title: artifact.label,
		});
		Ok(())
	})
	.await
	.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn hide_artifact(state: State<'_, Arc<Runtime>>) -> Result<(), String> {
	let _ = state.transport.events.send(OverlayEvent::HideImage);
	Ok(())
}

/// Pending images are visible only through trusted desktop IPC, never the OBS server.
#[tauri::command]
pub async fn preview_submission(
	state: State<'_, Arc<Runtime>>,
	id: String,
) -> Result<String, String> {
	let state = state.inner().clone();
	tokio::task::spawn_blocking(move || -> Result<String, String> {
		let submission = state
			.store
			.pending_images()
			.map_err(|e| e.to_string())?
			.into_iter()
			.find(|item| item.id == id)
			.ok_or("This submission is no longer pending")?;
		if !bumblebee_core::images::valid_hash(&submission.image_hash) {
			return Err("Invalid saved image".into());
		}
		let path = state
			.transport
			.images
			.join(format!("{}.png", submission.image_hash));
		if path.metadata().map_err(|e| e.to_string())?.len() > 20 * 1024 * 1024 {
			return Err("Image exceeds preview limit".into());
		}
		let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
		Ok(format!(
			"data:image/png;base64,{}",
			base64::engine::general_purpose::STANDARD.encode(bytes)
		))
	})
	.await
	.map_err(|e| e.to_string())?
}
