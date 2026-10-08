//! TEN VAD with the original 10 ms / 16 kHz window and threshold.
//! Distributed separately under upstream Apache-derived terms with additional conditions.
use std::{
	path::PathBuf,
	sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};
use tracing::info;

const HOP_SAMPLES: usize = 160;
const HOP_BYTES: usize = HOP_SAMPLES * 2;
const DEFAULT_THRESHOLD: f32 = 0.5;

type TenVadCreate = unsafe extern "C" fn(*mut *mut std::ffi::c_void, u64, f32) -> i32;
type TenVadProcess =
	unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, u64, *mut f32, *mut i32) -> i32;
type TenVadDestroy = unsafe extern "C" fn(*mut *mut std::ffi::c_void) -> i32;
type TenVadGetVersion = unsafe extern "C" fn() -> *const std::ffi::c_char;

#[derive(Clone)]
struct TenVadApi {
	_library: Arc<Library>,
	create: TenVadCreate,
	process: TenVadProcess,
	destroy: TenVadDestroy,
	get_version: TenVadGetVersion,
}

static TEN_VAD_API: OnceLock<TenVadApi> = OnceLock::new();
static TEN_VAD_LOGGED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct VadFrameResult {
	pub vad_active: bool,
	pub rms: f32,
}

pub struct VoiceActivityDetector {
	handle: *mut std::ffi::c_void,
	api: TenVadApi,
	carry: Vec<u8>,
	closed: bool,
}

unsafe impl Send for VoiceActivityDetector {}

impl VoiceActivityDetector {
	pub fn new() -> Result<Self> {
		super::blocking::run("vad.create", Self::create)
	}

	fn create() -> Result<Self> {
		let api = get_ten_vad_api()?;
		let mut handle = std::ptr::null_mut();
		let status = unsafe { (api.create)(&mut handle, HOP_SAMPLES as u64, DEFAULT_THRESHOLD) };
		if status != 0 {
			bail!("ten_vad_create failed with status {status}");
		}
		if handle.is_null() {
			bail!("ten_vad_create returned a null handle");
		}

		let instance = Self {
			handle,
			api,
			carry: Vec::new(),
			closed: false,
		};
		if TEN_VAD_LOGGED.get().is_none() {
			if let Ok(version) = instance.version() {
				info!(
					  version = %version,
					  library_path = %ten_vad_lib_path().display(),
					  hop_size = HOP_SAMPLES,
					  "using TEN VAD"
				);
				let _ = TEN_VAD_LOGGED.set(());
			}
		}
		Ok(instance)
	}

	pub fn version(&self) -> Result<String> {
		let raw = unsafe { (self.api.get_version)() };
		if raw.is_null() {
			bail!("ten_vad_get_version returned null");
		}
		let text = unsafe { std::ffi::CStr::from_ptr(raw) }
			.to_str()
			.context("decode ten_vad_get_version")?;
		Ok(text.to_string())
	}

	pub fn process_frame(&mut self, pcm16le: &[u8]) -> Result<VadFrameResult> {
		super::blocking::run("vad.process", || self.process_native(pcm16le))
	}

	fn process_native(&mut self, pcm16le: &[u8]) -> Result<VadFrameResult> {
		if pcm16le.len() % 2 != 0 {
			bail!("PCM16 audio must contain complete samples");
		}
		if pcm16le.len() > 32000 {
			bail!("VAD frame exceeds one second");
		}
		if self.closed || pcm16le.is_empty() {
			return Ok(VadFrameResult {
				vad_active: false,
				rms: 0.0,
			});
		}

		let mut merged = Vec::with_capacity(self.carry.len() + pcm16le.len());
		merged.extend_from_slice(&self.carry);
		merged.extend_from_slice(pcm16le);

		let mut offset = 0;
		let mut any_speech = false;

		while offset + HOP_BYTES <= merged.len() {
			let window = &merged[offset..offset + HOP_BYTES];
			let mut probability = 0.0_f32;
			let mut flag = 0_i32;
			let status = unsafe {
				(self.api.process)(
					self.handle,
					window.as_ptr(),
					HOP_SAMPLES as u64,
					&mut probability,
					&mut flag,
				)
			};
			if status != 0 {
				bail!("ten_vad_process failed with status {status}");
			}
			any_speech |= flag == 1;
			offset += HOP_BYTES;
		}

		self.carry.clear();
		self.carry.extend_from_slice(&merged[offset..]);

		Ok(VadFrameResult {
			vad_active: any_speech,
			rms: compute_rms(pcm16le),
		})
	}

	pub fn close(&mut self) -> Result<()> {
		super::blocking::run("vad.close", || self.close_native())
	}

	fn close_native(&mut self) -> Result<()> {
		if self.closed {
			return Ok(());
		}
		self.closed = true;
		if self.handle.is_null() {
			return Ok(());
		}
		let mut handle = self.handle;
		let status = unsafe { (self.api.destroy)(&mut handle) };
		self.handle = std::ptr::null_mut();
		if status != 0 && !handle.is_null() {
			bail!("ten_vad_destroy failed with status {status}");
		}
		Ok(())
	}
}

impl Drop for VoiceActivityDetector {
	fn drop(&mut self) {
		let _ = self.close();
	}
}

fn get_ten_vad_api() -> Result<TenVadApi> {
	if let Some(api) = TEN_VAD_API.get() {
		return Ok(api.clone());
	}

	let library = crate::resources::load_native_library(crate::resources::vad_library_name())?;
	let library = Arc::new(library);

	let create = load_symbol::<TenVadCreate>(&library, b"ten_vad_create\0")?;
	let process = load_symbol::<TenVadProcess>(&library, b"ten_vad_process\0")?;
	let destroy = load_symbol::<TenVadDestroy>(&library, b"ten_vad_destroy\0")?;
	let get_version = load_symbol::<TenVadGetVersion>(&library, b"ten_vad_get_version\0")?;

	let api = TenVadApi {
		_library: library,
		create,
		process,
		destroy,
		get_version,
	};

	let _ = TEN_VAD_API.set(api.clone());
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

fn ten_vad_lib_path() -> PathBuf {
	crate::resources::configured()
		.map(|r| r.libraries.join(crate::resources::vad_library_name()))
		.unwrap_or_default()
}

fn compute_rms(pcm16le: &[u8]) -> f32 {
	let sample_count = pcm16le.len() / 2;
	if sample_count == 0 {
		return 0.0;
	}

	let mut sum_squares = 0.0_f64;
	for chunk in pcm16le.chunks_exact(2) {
		let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f64;
		sum_squares += sample * sample;
	}

	((sum_squares / sample_count as f64).sqrt() / 32768.0) as f32
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn measures_pcm_level_without_alignment_assumptions() {
		assert_eq!(compute_rms(&[]), 0.0);
		assert_eq!(compute_rms(&8192_i16.to_le_bytes()), 0.25);
	}
	#[test]
	#[ignore = "requires prepared packaged TEN VAD resources"]
	fn real_ten_vad_preserves_windowing_and_bounded_carry() {
		crate::NativeResources::from_bundle(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources"),
		)
		.initialize()
		.unwrap();
		let mut detector = VoiceActivityDetector::new().unwrap();
		for size in [2, 158, 320, 640, 3200] {
			let result = detector.process_frame(&vec![0; size]).unwrap();
			assert!(!result.vad_active);
			assert_eq!(result.rms, 0.0);
			assert!(detector.carry.len() < HOP_BYTES);
		}
		assert!(detector.process_frame(&[0]).is_err());
		assert!(detector.process_frame(&vec![0; 32002]).is_err());
		detector.close().unwrap();
		detector.close().unwrap();
	}
}
