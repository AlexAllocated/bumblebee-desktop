//! Shared file transcription for voice turns and explicitly enabled streamer captions.
use super::Providers;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

const TRANSCRIPTION_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

impl Providers {
	pub(crate) async fn transcribe_audio(
		&self,
		model: &str,
		wav: Vec<u8>,
		language: Option<&str>,
		cancel: &CancellationToken,
	) -> Result<String> {
		ensure!(!cancel.is_cancelled(), "Transcription canceled");
		transcribe_at(
			&self.http,
			TRANSCRIPTION_URL,
			&self.secret("openai")?,
			model,
			wav,
			language,
			cancel,
		)
		.await
	}
}

async fn transcribe_at(
	client: &reqwest::Client,
	url: &str,
	key: &str,
	model: &str,
	wav: Vec<u8>,
	language: Option<&str>,
	cancel: &CancellationToken,
) -> Result<String> {
	ensure!(!cancel.is_cancelled(), "Transcription canceled");
	let mut form = reqwest::multipart::Form::new()
		.text("model", model.to_owned())
		.text("response_format", "json")
		.part(
			"file",
			reqwest::multipart::Part::bytes(wav)
				.file_name("voice.wav")
				.mime_str("audio/wav")?,
		);
	if let Some(language) = language {
		// The current transcription model accepts language hints as an array;
		// the retained 4o/Whisper models use the singular language parameter.
		let field = if model == "gpt-transcribe" || model.starts_with("gpt-transcribe-") {
			"languages[]"
		} else {
			"language"
		};
		form = form.text(field, language.to_owned());
	}
	let response = tokio::select! {
		biased;
		_ = cancel.cancelled() => bail!("Transcription canceled"),
		response = client.post(url).bearer_auth(key).multipart(form).send() => response.context("OpenAI transcription connection failed")?,
	};
	let status = response.status();
	let body = tokio::select! {
		biased;
		_ = cancel.cancelled() => bail!("Transcription canceled"),
		body = response.bytes() => body.context("OpenAI transcription response could not be read")?,
	};
	let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
	if !status.is_success() {
		let code = json.pointer("/error/code").and_then(Value::as_str);
		bail!(transcription_error(status, code, model));
	}
	json["text"]
		.as_str()
		.map(str::to_owned)
		.context("OpenAI transcription returned no text")
}

fn transcription_error(status: reqwest::StatusCode, code: Option<&str>, model: &str) -> String {
	if code == Some("model_not_found") {
		return format!(
			"OpenAI transcription model '{model}' is unavailable or this project does not have access. Choose an available transcription model in AI settings."
		);
	}
	match status.as_u16() {
		401 => {
			"OpenAI transcription authorization is invalid or expired. Update the saved OpenAI key."
				.into()
		}
		403 => format!(
			"OpenAI transcription permission was denied for model '{model}'. Check this project's API key permissions and model access."
		),
		429 => {
			"OpenAI transcription rate limit or quota reached. Check the project's limits and billing."
				.into()
		}
		_ => format!(
			"OpenAI transcription failed for model '{model}' (HTTP {}).",
			status.as_u16()
		),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::{Router, body::Bytes, http::StatusCode, response::IntoResponse, routing::post};
	use std::sync::{Arc, Mutex};
	async fn server(
		status: StatusCode,
		response: Value,
	) -> (
		String,
		Arc<Mutex<Vec<Vec<u8>>>>,
		tokio::task::JoinHandle<()>,
	) {
		let requests = Arc::new(Mutex::new(Vec::new()));
		let seen = requests.clone();
		let app = Router::new().route(
			"/audio/transcriptions",
			post(move |body: Bytes| {
				let seen = seen.clone();
				let response = response.clone();
				async move {
					seen.lock().unwrap().push(body.to_vec());
					(status, axum::Json(response)).into_response()
				}
			}),
		);
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let url = format!(
			"http://{}/audio/transcriptions",
			listener.local_addr().unwrap()
		);
		let job = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
		(url, requests, job)
	}
	#[tokio::test]
	async fn chosen_model_and_wav_reach_the_multipart_provider_request_without_fallback() {
		let (url, requests, job) = server(
			StatusCode::OK,
			serde_json::json!({"text":"Hello Bumblebee"}),
		)
		.await;
		for model in [
			"gpt-transcribe",
			"gpt-transcribe-2026-01-01",
			"gpt-4o-mini-transcribe",
			"gpt-4o-transcribe",
			"whisper-1",
		] {
			let result = transcribe_at(
				&reqwest::Client::new(),
				&url,
				"test-key",
				model,
				b"RIFF-test-WAV".to_vec(),
				Some("en"),
				&CancellationToken::new(),
			)
			.await
			.unwrap();
			assert_eq!(result, "Hello Bumblebee");
			let wire = String::from_utf8(requests.lock().unwrap().last().unwrap().clone()).unwrap();
			assert!(wire.contains(&format!("name=\"model\"\r\n\r\n{model}\r\n")));
			assert!(wire.contains("name=\"response_format\"\r\n\r\njson\r\n"));
			let field = if model == "gpt-transcribe" || model.starts_with("gpt-transcribe-") {
				"languages[]"
			} else {
				"language"
			};
			assert!(wire.contains(&format!("name=\"{field}\"\r\n\r\nen\r\n")));
			assert!(
				wire.contains("filename=\"voice.wav\"")
					&& wire.contains("audio/wav")
					&& wire.contains("RIFF-test-WAV")
			);
		}
		assert_eq!(requests.lock().unwrap().len(), 5);
		job.abort();
	}
	#[tokio::test]
	async fn forbidden_model_error_reports_model_access_and_does_not_retry() {
		let (url, requests, job) = server(
			StatusCode::FORBIDDEN,
			serde_json::json!({"error":{"code":"model_not_found","message":"private provider details"}}),
		)
		.await;
		let error = transcribe_at(
			&reqwest::Client::new(),
			&url,
			"test-key",
			"whisper-1",
			vec![0],
			None,
			&CancellationToken::new(),
		)
		.await
		.unwrap_err()
		.to_string();
		assert!(
			error.contains("whisper-1")
				&& error.contains("does not have access")
				&& error.contains("AI settings")
		);
		assert!(!error.contains("private provider details") && !error.contains("test-key"));
		assert_eq!(requests.lock().unwrap().len(), 1);
		job.abort();
	}
	#[tokio::test]
	async fn canceled_transcription_never_sends_audio() {
		let (url, requests, job) = server(StatusCode::OK, serde_json::json!({"text":"No"})).await;
		let cancel = CancellationToken::new();
		cancel.cancel();
		assert!(
			transcribe_at(
				&reqwest::Client::new(),
				&url,
				"test-key",
				"gpt-4o-mini-transcribe",
				vec![0],
				None,
				&cancel
			)
			.await
			.is_err()
		);
		assert!(requests.lock().unwrap().is_empty());
		job.abort();
	}
}
