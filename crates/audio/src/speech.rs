//! Azure SDK synthesis with real word timings, cancellable off the audio executor.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
	ffi::{CStr, CString, c_char, c_void},
	sync::Arc,
	time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
	pub text: String,
	pub start_ms: u64,
	pub duration_ms: u64,
}
#[derive(Debug, Clone)]
pub struct SynthesizedSpeech {
	pub wav: Vec<u8>,
	pub words: Vec<WordTiming>,
}

/// Inputs originate in the Rust provider layer. SSML must be built with escaped
/// text and validated catalog voice IDs. Keys are never logged or serialized.
pub async fn synthesize(
	key: String,
	region: String,
	ssml: String,
	cancellation: CancellationToken,
) -> Result<SynthesizedSpeech> {
	anyhow::ensure!(ssml.len() <= 64 * 1024, "speech request exceeds 64 KiB");
	crate::resources::configured()?;
	let cancel_on_drop = CancelOnDrop(cancellation.child_token());
	let token = cancel_on_drop.0.clone();
	tokio::task::spawn_blocking(move || {
		let key = CString::new(key).context("invalid speech key")?;
		let region = CString::new(region).context("invalid speech region")?;
		let ssml = CString::new(ssml).context("invalid speech SSML")?;
		let library = Arc::new(crate::resources::load_speech_library()?);
		unsafe {
			let start: libloading::Symbol<
				unsafe extern "C" fn(
					*const c_char,
					*const c_char,
					*const c_char,
					*mut *mut c_void,
				) -> i32,
			> = library.get(b"bb_tts_start\0")?;
			let poll: libloading::Symbol<unsafe extern "C" fn(*mut c_void, u32) -> i32> =
				library.get(b"bb_tts_poll\0")?;
			let audio: libloading::Symbol<
				unsafe extern "C" fn(*mut c_void, *mut *const u8, *mut u32) -> i32,
			> = library.get(b"bb_tts_audio\0")?;
			let word: libloading::Symbol<
				unsafe extern "C" fn(*mut c_void, u32, *mut c_char, u32, *mut u64, *mut u64) -> i32,
			> = library.get(b"bb_tts_word\0")?;
			let close: libloading::Symbol<unsafe extern "C" fn(*mut c_void) -> i32> =
				library.get(b"bb_tts_close\0")?;
			let error: libloading::Symbol<unsafe extern "C" fn() -> *const c_char> =
				library.get(b"bb_last_error\0")?;
			let check = |code| -> Result<()> {
				if code == 0 {
					return Ok(());
				}
				let ptr = error();
				let detail = if ptr.is_null() {
					"native synthesis error".into()
				} else {
					CStr::from_ptr(ptr).to_string_lossy().into_owned()
				};
				bail!("{detail}")
			};
			anyhow::ensure!(!token.is_cancelled(), "speech synthesis canceled");
			let mut ptr = std::ptr::null_mut();
			check(start(
				key.as_ptr(),
				region.as_ptr(),
				ssml.as_ptr(),
				&mut ptr,
			))?;
			anyhow::ensure!(!ptr.is_null(), "speech SDK returned no synthesis handle");
			let handle = SynthesisHandle { ptr, close: *close };
			let deadline = Instant::now() + Duration::from_secs(120);
			loop {
				anyhow::ensure!(!token.is_cancelled(), "speech synthesis canceled");
				anyhow::ensure!(Instant::now() < deadline, "speech synthesis timed out");
				let code = poll(handle.ptr, 25);
				if code == 1 {
					continue;
				}
				check(code)?;
				break;
			}
			let mut bytes = std::ptr::null();
			let mut length = 0;
			check(audio(handle.ptr, &mut bytes, &mut length))?;
			anyhow::ensure!(
				!bytes.is_null() && length > 44 && length <= 32 * 1024 * 1024,
				"speech SDK returned invalid or oversized audio"
			);
			let wav = std::slice::from_raw_parts(bytes, length as usize).to_vec();
			let mut words = Vec::new();
			for index in 0..100_000 {
				let mut text = [0i8; 8192];
				let mut start_ms = 0;
				let mut duration_ms = 0;
				let code = word(
					handle.ptr,
					index,
					text.as_mut_ptr(),
					text.len() as u32,
					&mut start_ms,
					&mut duration_ms,
				);
				if code == 1 {
					return Ok(SynthesizedSpeech { wav, words });
				}
				check(code)?;
				words.push(WordTiming {
					text: CStr::from_ptr(text.as_ptr()).to_str()?.into(),
					start_ms,
					duration_ms,
				});
			}
			bail!("speech SDK returned excessive word boundaries")
		}
	})
	.await
	.context("native synthesis worker failed")?
}
struct SynthesisHandle {
	ptr: *mut c_void,
	close: unsafe extern "C" fn(*mut c_void) -> i32,
}
impl Drop for SynthesisHandle {
	fn drop(&mut self) {
		unsafe {
			(self.close)(self.ptr);
		}
	}
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
	fn drop(&mut self) {
		self.0.cancel();
	}
}
