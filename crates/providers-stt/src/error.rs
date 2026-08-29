use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SttErrorKind {
    Cancelled,
    Timeout,
    Authentication,
    RateLimited,
    QuotaExceeded,
    InvalidRequest,
    PrivacyPolicy,
    Unavailable,
    Protocol,
}

/// Sanitized adapter error. It never contains credentials, audio, transcripts, context, or raw
/// provider frames.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{provider_id}: {message}")]
pub struct SttError {
    pub provider_id: &'static str,
    pub kind: SttErrorKind,
    pub message: &'static str,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
    pub provider_code: Option<String>,
}

impl SttError {
    pub(crate) fn new(
        provider_id: &'static str,
        kind: SttErrorKind,
        message: &'static str,
        retryable: bool,
        provider_code: Option<&str>,
    ) -> Self {
        Self {
            provider_id,
            kind,
            message,
            retryable,
            retry_after: None,
            provider_code: provider_code.map(sanitize_code),
        }
    }

    pub fn invalid_request(message: &'static str) -> Self {
        Self::new("stt", SttErrorKind::InvalidRequest, message, false, None)
    }

    pub(crate) fn cancelled(provider_id: &'static str) -> Self {
        Self::new(
            provider_id,
            SttErrorKind::Cancelled,
            "recognition session was cancelled",
            false,
            None,
        )
    }

    pub(crate) fn timeout(provider_id: &'static str, phase: &'static str) -> Self {
        Self::new(provider_id, SttErrorKind::Timeout, phase, true, None)
    }

    pub(crate) fn protocol(provider_id: &'static str, code: Option<&str>) -> Self {
        Self::new(
            provider_id,
            SttErrorKind::Protocol,
            "provider returned an invalid protocol message",
            false,
            code,
        )
    }

    pub(crate) fn transport(provider_id: &'static str) -> Self {
        Self::new(
            provider_id,
            SttErrorKind::Unavailable,
            "streaming transport is unavailable",
            true,
            None,
        )
    }
}

fn sanitize_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(64)
        .collect()
}
