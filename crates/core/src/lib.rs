pub mod agent;
pub mod agent_storage;
pub mod cache;
pub mod catalog;
pub mod commands;
pub mod images;
pub mod model;
pub mod storage;

pub fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as i64
}

pub mod providers;
pub mod runtime;
pub mod speech;

pub mod settings;

pub mod overlay;

pub mod preferences;
