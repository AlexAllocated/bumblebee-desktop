use serde_json::Value;

// The application configures tracing. No external log transport, credentials or Redis.
pub fn log_event(level: tracing::Level, event: &str, message: &str, fields: Value) {
	match level {
		tracing::Level::ERROR => tracing::error!(event, message, fields = %fields),
		tracing::Level::WARN => tracing::warn!(event, message, fields = %fields),
		tracing::Level::INFO => tracing::info!(event, message, fields = %fields),
		tracing::Level::DEBUG => tracing::debug!(event, message, fields = %fields),
		tracing::Level::TRACE => tracing::trace!(event, message, fields = %fields),
	}
}
