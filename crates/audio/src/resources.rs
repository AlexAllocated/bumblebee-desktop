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
#[cfg(not(windows))]
static SPEECH_CORE: OnceLock<libloading::Library> = OnceLock::new();

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
		// Exercise delayed SDK dependencies as well as the loader. Both engines
		// consume synthetic silence; no microphone, playback device, credentials,
		// or provider connection is involved.
		let mut detector =
			crate::native::vad::VoiceActivityDetector::new().context("initialize packaged TEN VAD")?;
		let silence = [0_u8; 6400];
		let activity = detector.process_frame(&silence)?;
		anyhow::ensure!(
			!activity.vad_active && activity.rms == 0.0,
			"packaged TEN VAD did not recognize synthetic silence"
		);
		detector.close().context("close packaged TEN VAD")?;
		let mut keyword = crate::native::azure_speech::KeywordRecognizer::new(
			&self
				.keyword_models
				.join("default/hey_bumblebee_default.table"),
		)
		.context("initialize packaged Speech keyword model")?;
		keyword.write(&silence)?;
		let result = keyword.poll(100)?;
		anyhow::ensure!(
			result.status == crate::native::azure_speech::KeywordPollStatus::Timeout,
			"packaged Speech keyword recognition ended unexpectedly while processing silence"
		);
		keyword
			.close()
			.context("close packaged Speech keyword model")?;
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

#[cfg(test)]
mod tests {
	#[test]
	#[ignore = "requires prepared packaged native resources"]
	fn installed_probe_exercises_silent_native_lifecycles() {
		super::NativeResources::from_bundle(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources"),
		)
		.probe()
		.unwrap();
	}

	#[cfg(target_os = "linux")]
	#[test]
	#[ignore = "requires prepared packaged native resources"]
	fn installed_probe_ignores_appimage_shadow_core_without_extensions() {
		use std::{
			process::{Command, Stdio},
			time::{Duration, Instant, SystemTime, UNIX_EPOCH},
		};
		let native =
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/native");
		let shadow = std::env::temp_dir().join(format!(
			"bumblebee-shadow-speech-{}-{}",
			std::process::id(),
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.unwrap()
				.as_nanos(),
		));
		std::fs::create_dir(&shadow).unwrap();
		std::fs::copy(
			native.join("libMicrosoft.CognitiveServices.Speech.core.so"),
			shadow.join("libMicrosoft.CognitiveServices.Speech.core.so"),
		)
		.unwrap();
		let mut paths = vec![shadow.clone()];
		if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
			paths.extend(std::env::split_paths(&existing));
		}
		// A fresh process is essential: the dynamic loader and native APIs keep
		// libraries cached. This recreates AppRun's shadow core without its KWS
		// extensions and exercises the actual packaged create/poll/close path.
		let mut child = Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"resources::tests::installed_probe_exercises_silent_native_lifecycles",
				"--ignored",
				"--nocapture",
			])
			.env("LD_LIBRARY_PATH", std::env::join_paths(paths).unwrap())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		let deadline = Instant::now() + Duration::from_secs(30);
		while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(20));
		}
		if child.try_wait().unwrap().is_none() {
			child.kill().unwrap();
		}
		let result = child.wait_with_output().unwrap();
		std::fs::remove_dir_all(shadow).unwrap();
		assert!(
			result.status.success(),
			"AppImage shadow core broke native lifecycle: {} {}",
			String::from_utf8_lossy(&result.stdout),
			String::from_utf8_lossy(&result.stderr)
		);
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
	// AppImage adds usr/lib to LD_LIBRARY_PATH and linuxdeploy may copy the
	// Speech core there without its delayed keyword extensions. Load the core
	// beside those extensions first so the wrapper's dependency resolves to
	// that exact runtime, even when the launcher shadows its $ORIGIN path.
	// Retain one successful load for the process lifetime. A failed load leaves
	// the cell empty so a retry remains possible; racing extra handles drop.
	#[cfg(not(windows))]
	if SPEECH_CORE.get().is_none() {
		let core = load_native_library("libMicrosoft.CognitiveServices.Speech.core.so")?;
		let _ = SPEECH_CORE.set(core);
	}
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
