use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Stable error categories consumed by routing and circuit-breaker policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Cancelled,
    Timeout,
    Authentication,
    RateLimited,
    InvalidRequest,
    Unavailable,
    Protocol,
    SecretUnavailable,
    PollingRequired,
    Internal,
}

/// A sanitized provider failure. Provider response bodies are intentionally not
/// included because a hostile or misconfigured endpoint can reflect credentials.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq, Serialize, Deserialize)]
#[error("{provider_id}: {message}")]
pub struct ProviderError {
    pub provider_id: String,
    pub kind: ErrorKind,
    pub message: String,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
    pub http_status: Option<u16>,
    pub request_id: Option<String>,
}

impl ProviderError {
    pub(crate) fn new(
        provider_id: impl Into<String>,
        kind: ErrorKind,
        message: impl Into<String>,
    ) -> Self {
        let retryable = matches!(
            kind,
            ErrorKind::Cancelled
                | ErrorKind::Timeout
                | ErrorKind::RateLimited
                | ErrorKind::Unavailable
        );
        Self {
            provider_id: provider_id.into(),
            kind,
            message: message.into(),
            retryable,
            retry_after: None,
            http_status: None,
            request_id: None,
        }
    }

    pub(crate) fn cancelled(provider_id: &str) -> Self {
        Self::new(provider_id, ErrorKind::Cancelled, "request cancelled")
    }

    pub(crate) fn timeout(provider_id: &str) -> Self {
        Self::new(
            provider_id,
            ErrorKind::Timeout,
            "provider request timed out",
        )
    }

    pub(crate) fn protocol(provider_id: &str, message: &'static str) -> Self {
        Self::new(provider_id, ErrorKind::Protocol, message)
    }

    pub(crate) fn from_http(provider_id: &str, status: reqwest::StatusCode) -> Self {
        let kind = match status.as_u16() {
            401 | 403 | 498 => ErrorKind::Authentication,
            408 | 504 => ErrorKind::Timeout,
            499 => ErrorKind::Cancelled,
            429 => ErrorKind::RateLimited,
            400..=499 => ErrorKind::InvalidRequest,
            500..=599 => ErrorKind::Unavailable,
            _ => ErrorKind::Protocol,
        };
        let mut error = Self::new(
            provider_id,
            kind,
            format!("provider returned HTTP status {}", status.as_u16()),
        );
        error.http_status = Some(status.as_u16());
        error
    }

    pub(crate) fn transport(provider_id: &str, error: &reqwest::Error) -> Self {
        // Do not use reqwest::Error's Display representation: it may contain a
        // user-configured URL. Classify it locally and keep diagnostics bounded.
        if error.is_timeout() {
            Self::timeout(provider_id)
        } else if error.is_connect() {
            Self::new(
                provider_id,
                ErrorKind::Unavailable,
                "provider connection failed",
            )
        } else if error.is_request() || error.is_builder() {
            Self::new(
                provider_id,
                ErrorKind::InvalidRequest,
                "provider request could not be constructed",
            )
        } else {
            Self::new(
                provider_id,
                ErrorKind::Unavailable,
                "provider transport failed",
            )
        }
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("credential reference was not found")]
    NotFound,
    #[error("credential service is unavailable")]
    Unavailable,
    #[error("credential bytes are invalid")]
    Invalid,
}
