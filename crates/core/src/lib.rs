pub mod catalog;
pub mod cache;
pub mod agent_storage;
pub mod agent;
pub mod model;
pub mod storage;
pub mod images;
pub mod commands;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub mod providers;
pub mod runtime;
pub mod speech;
