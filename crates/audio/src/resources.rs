//! Resolve packaged native resources explicitly, never relative to a source checkout.
use anyhow::{Context, Result, bail};
use std::{
	path::{Path, PathBuf},
	sync::OnceLock,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeResources {
	pub libraries: PathBuf,
	pub keyword_models: PathBuf,
}
static RESOURCES: OnceLock<NativeResources> = OnceLock::new();

impl NativeResources {
	/// `root` contains `native/` and `keyword_models/` in the application bundle.
	pub fn from_bundle(root: impl AsRef<Path>) -> Self {
		Self {
			libraries: root.as_ref().join("native"),
			keyword_models: root.as_ref().join("keyword_models"),
		}
	}
	pub fn initialize(&self) -> Result<()> {
		if let Some(existing) = RESOURCES.get() {
			anyhow::ensure!(
				existing == self,
				"native audio resources are already initialized at another path"
			);
			return Ok(());
		}
		if RESOURCES.set(self.clone()).is_err() {
			anyhow::ensure!(
				RESOURCES.get() == Some(self),
				"native resource initialization raced with another path"
			);
		}
		Ok(())
	}
	/// Verify installed resources and the actual native loader without credentials.
	pub fn probe(&self) -> Result<()> {
		self.initialize()?;
		self.validate()?;
		let library = load_speech_library()?;
		for symbol in [
			b"bb_keyword_create\0".as_slice(),
			b"bb_keyword_write\0",
			b"bb_keyword_poll\0",
			b"bb_keyword_close\0",
			b"bb_tts_start\0",
			b"bb_tts_poll\0",
			b"bb_tts_audio\0",
			b"bb_tts_word\0",
			b"bb_tts_close\0",
		] {
			let _: libloading::Symbol<'_, unsafe extern "C" fn()> =
				unsafe { library.get(symbol) }.context("native speech bridge is incompatible")?;
		}
		let vad = load_native_library(vad_library_name())?;
		for symbol in [
			b"ten_vad_create\0".as_slice(),
			b"ten_vad_process\0",
			b"ten_vad_destroy\0",
			b"ten_vad_get_version\0",
		] {
			let _: libloading::Symbol<'_, unsafe extern "C" fn()> =
				unsafe { vad.get(symbol) }.context("TEN VAD library is incompatible")?;
		}
		Ok(())
	}

	pub fn validate(&self) -> Result<()> {
		for file in [
			self.libraries.join(speech_library_name()),
			self.libraries.join(vad_library_name()),
			self
				.keyword_models
				.join("default/hey_bumblebee_default.table"),
		] {
			if !file.is_file() {
				bail!("native audio resource is missing: {}", file.display());
			}
		}
		Ok(())
	}
}
pub(crate) fn configured() -> Result<&'static NativeResources> {
	RESOURCES
		.get()
		.context("native audio resources have not been initialized")
}
pub(crate) fn speech_library_name() -> &'static str {
	if cfg!(windows) {
		"bumblebee_speech_wrapper.dll"
	} else {
		"libbumblebee_speech_wrapper.so"
	}
}

pub(crate) fn load_speech_library() -> Result<libloading::Library> {
	load_native_library(speech_library_name())
}
pub(crate) fn vad_library_name() -> &'static str {
	if cfg!(windows) {
		"ten_vad.dll"
	} else {
		"libten_vad.so"
	}
}
pub(crate) fn load_native_library(name: &str) -> Result<libloading::Library> {
	let path = configured()?.libraries.join(name);
	#[cfg(windows)]
	let library = unsafe {
		// Resolve Speech SDK DLL dependencies beside the wrapper while excluding
		// the process working directory from the DLL search path.
		libloading::os::windows::Library::load_with_flags(&path, 0x00000100 | 0x00001000)
			.map(libloading::Library::from)
	};
	#[cfg(not(windows))]
	let library = unsafe { libloading::Library::new(&path) };
	library.with_context(|| format!("load native audio library at {}", path.display()))
}
