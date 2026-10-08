use std::{
	ffi::{CStr, CString, c_char, c_void},
	path::Path,
	sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};

const SAMPLE_RATE: u32 = 16_000;
const BITS_PER_SAMPLE: u8 = 16;
const CHANNELS: u8 = 1;
const STATUS_TIMEOUT: i32 = 1;
const POLL_TEXT_CAPACITY: usize = 512;

type BbLastError = unsafe extern "C" fn() -> *const c_char;
type BbKeywordCreate = unsafe extern "C" fn(*const c_char, u32, u8, u8, *mut *mut c_void) -> i32;
type BbKeywordWrite = unsafe extern "C" fn(*mut c_void, *const u8, u32) -> i32;
type BbKeywordPoll = unsafe extern "C" fn(*mut c_void, u32, *mut c_char, u32, *mut u32) -> i32;
type BbKeywordClose = unsafe extern "C" fn(*mut c_void) -> i32;

#[derive(Clone)]
struct AzureSpeechApi {
	_library: Arc<Library>,
	last_error: BbLastError,
	keyword_create: BbKeywordCreate,
	keyword_write: BbKeywordWrite,
	keyword_poll: BbKeywordPoll,
	keyword_close: BbKeywordClose,
}

static AZURE_SPEECH_API: OnceLock<AzureSpeechApi> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct KeywordPollResult {
	pub status: KeywordPollStatus,
	pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordPollStatus {
	Ready,
	Timeout,
}

pub struct KeywordRecognizer {
	handle: *mut c_void,
	api: AzureSpeechApi,
	closed: bool,
}

unsafe impl Send for KeywordRecognizer {}

impl KeywordRecognizer {
	pub fn new(model_path: &Path) -> Result<Self> {
		super::blocking::run("keyword.create", || Self::create(model_path))
	}

	fn create(model_path: &Path) -> Result<Self> {
		let api = get_azure_speech_api()?;
		let model_path = CString::new(model_path.to_string_lossy().as_bytes())
			.context("build model path CString")?;
		let mut handle = std::ptr::null_mut();
		let status = unsafe {
			(api.keyword_create)(
				model_path.as_ptr(),
				SAMPLE_RATE,
				BITS_PER_SAMPLE,
				CHANNELS,
				&mut handle,
			)
		};
		check_status(&api, status, "bb_keyword_create")?;
		if handle.is_null() {
			bail!("bb_keyword_create returned a null handle");
		}

		Ok(Self {
			handle,
			api,
			closed: false,
		})
	}

	pub fn write(&self, pcm: &[u8]) -> Result<()> {
		super::blocking::run("keyword.write", || self.write_native(pcm))
	}

	fn write_native(&self, pcm: &[u8]) -> Result<()> {
		if self.closed || pcm.is_empty() {
			return Ok(());
		}
		let status = unsafe { (self.api.keyword_write)(self.handle, pcm.as_ptr(), pcm.len() as u32) };
		check_status(&self.api, status, "bb_keyword_write")
	}

	pub fn poll(&self, timeout_ms: u32) -> Result<KeywordPollResult> {
		super::blocking::run("keyword.poll", || self.poll_native(timeout_ms))
	}

	fn poll_native(&self, timeout_ms: u32) -> Result<KeywordPollResult> {
		if self.closed {
			bail!("keyword recognizer is closed");
		}

		let mut reason = 0_u32;
		let mut text = vec![0_u8; POLL_TEXT_CAPACITY];
		let status = unsafe {
			(self.api.keyword_poll)(
				self.handle,
				timeout_ms,
				text.as_mut_ptr().cast::<c_char>(),
				text.len() as u32,
				&mut reason,
			)
		};

		if status == STATUS_TIMEOUT {
			return Ok(KeywordPollResult {
				status: KeywordPollStatus::Timeout,
				text: String::new(),
			});
		}

		check_status(&self.api, status, "bb_keyword_poll")?;
		let end = text.iter().position(|it| *it == 0).unwrap_or(text.len());
		Ok(KeywordPollResult {
			status: KeywordPollStatus::Ready,
			text: String::from_utf8_lossy(&text[..end]).to_string(),
		})
	}

	pub fn close(&mut self) -> Result<()> {
		super::blocking::run("keyword.close", || self.close_native())
	}

	fn close_native(&mut self) -> Result<()> {
		if self.closed {
			return Ok(());
		}
		self.closed = true;
		let status = unsafe { (self.api.keyword_close)(self.handle) };
		self.handle = std::ptr::null_mut();
		check_status(&self.api, status, "bb_keyword_close")
	}
}

impl Drop for KeywordRecognizer {
	fn drop(&mut self) {
		let _ = self.close();
	}
}

fn get_azure_speech_api() -> Result<AzureSpeechApi> {
	if let Some(api) = AZURE_SPEECH_API.get() {
		return Ok(api.clone());
	}

	let library = crate::resources::load_speech_library()?;
	let library = Arc::new(library);

	let api = AzureSpeechApi {
		last_error: load_symbol::<BbLastError>(&library, b"bb_last_error\0")?,
		keyword_create: load_symbol::<BbKeywordCreate>(&library, b"bb_keyword_create\0")?,
		keyword_write: load_symbol::<BbKeywordWrite>(&library, b"bb_keyword_write\0")?,
		keyword_poll: load_symbol::<BbKeywordPoll>(&library, b"bb_keyword_poll\0")?,
		keyword_close: load_symbol::<BbKeywordClose>(&library, b"bb_keyword_close\0")?,
		_library: library,
	};

	let _ = AZURE_SPEECH_API.set(api.clone());
	Ok(api)
}

fn load_symbol<T>(library: &Arc<Library>, name: &[u8]) -> Result<T>
where
	T: Copy,
{
	let symbol: Symbol<'_, T> = unsafe { library.get(name) }
		.with_context(|| format!("load symbol {}", String::from_utf8_lossy(name)))?;
	Ok(*symbol)
}

fn check_status(api: &AzureSpeechApi, status: i32, label: &str) -> Result<()> {
	if status == 0 {
		return Ok(());
	}
	let detail = unsafe { (api.last_error)() };
	let suffix = if detail.is_null() {
		String::new()
	} else {
		format!(
			": {}",
			unsafe { CStr::from_ptr(detail) }
				.to_str()
				.unwrap_or("unreadable azure speech error")
		)
	};
	bail!("{label} failed with status {status}{suffix}")
}
