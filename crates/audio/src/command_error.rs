use std::{error::Error, fmt};

/// Invalid channel configuration requires a settings or permissions change.
#[derive(Debug)]
pub struct VoiceConfigurationError(pub String);

impl fmt::Display for VoiceConfigurationError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}
impl Error for VoiceConfigurationError {}

/// Binds a failed access check to the exact native route observed before I/O.
#[derive(Debug)]
pub struct VoiceAccessCheckError {
	pub revision: u64,
	source: anyhow::Error,
}
impl fmt::Display for VoiceAccessCheckError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#}", self.source)
	}
}
impl Error for VoiceAccessCheckError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		Some(self.source.as_ref())
	}
}
pub fn bind_access_check(error: anyhow::Error, revision: u64) -> anyhow::Error {
	if is_destination_access_error(&error) {
		anyhow::Error::new(VoiceAccessCheckError {
			revision,
			source: error,
		})
	} else {
		error
	}
}

/// Only destination-local access/configuration failures are expected account state.
/// Invalid bot credentials (401), malformed commands (400), rate limits and service
/// errors must continue through operational error reporting.
pub fn is_destination_access_error(error: &anyhow::Error) -> bool {
	if error.downcast_ref::<VoiceAccessCheckError>().is_some() {
		return true;
	}
	if error.downcast_ref::<VoiceConfigurationError>().is_some() {
		return true;
	}
	error.chain().any(|cause| {
		cause
			.downcast_ref::<serenity::http::HttpError>()
			.and_then(|http| http.status_code())
			.is_some_and(|status| matches!(status.as_u16(), 403 | 404))
	})
}

pub fn is_retryable(error: &anyhow::Error) -> bool {
	if error.downcast_ref::<VoiceAccessCheckError>().is_some() {
		return false;
	}
	if error.downcast_ref::<VoiceConfigurationError>().is_some() {
		return false;
	}
	for cause in error.chain() {
		if let Some(http) = cause.downcast_ref::<serenity::http::HttpError>() {
			if let Some(status) = http.status_code() {
				return retryable_status(status.as_u16());
			}
		}
	}
	true
}

fn retryable_status(status: u16) -> bool {
	!(400..500).contains(&status) || matches!(status, 408 | 429)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn configuration_errors_remain_terminal_through_context() {
		let error = anyhow::Error::new(VoiceConfigurationError(
			"Connect permission required".into(),
		))
		.context("starting source");
		assert!(!is_retryable(&error));
		assert!(is_destination_access_error(&error));
		assert!(!is_destination_access_error(&anyhow::anyhow!(
			"Missing Access"
		)));
		assert!(!is_destination_access_error(&anyhow::anyhow!(
			"gateway temporarily unavailable"
		)));
		assert!(is_retryable(&anyhow::anyhow!(
			"gateway temporarily unavailable"
		)));
	}

	#[test]
	fn permission_and_missing_resources_do_not_retry_but_rate_limits_and_servers_do() {
		for status in [400, 401, 403, 404] {
			assert!(!retryable_status(status));
		}
		for status in [408, 429, 500, 502, 503] {
			assert!(retryable_status(status));
		}
	}
}
