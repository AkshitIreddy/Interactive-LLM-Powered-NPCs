use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalErrorKind {
    Cancelled,
    DeadlineExceeded,
    Authentication,
    PaymentRequired,
    RateLimited,
    InvalidRequest,
    Unavailable,
    PollRequired,
    MalformedResponse,
    CredentialUnavailable,
}

/// A bounded, serializable error safe for diagnostics and UI display.
///
/// Provider bodies, request content, endpoints and credential values never enter this type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{provider_id}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct RetrievalError {
    pub provider_id: String,
    pub kind: RetrievalErrorKind,
    pub message: String,
    pub retryable: bool,
    pub http_status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub provider_request_id: Option<String>,
}

impl RetrievalError {
    pub(crate) fn new(kind: RetrievalErrorKind, message: &'static str) -> Self {
        Self {
            provider_id: "nvidia-nim".to_owned(),
            kind,
            message: message.to_owned(),
            retryable: matches!(
                kind,
                RetrievalErrorKind::Cancelled
                    | RetrievalErrorKind::DeadlineExceeded
                    | RetrievalErrorKind::RateLimited
                    | RetrievalErrorKind::Unavailable
                    | RetrievalErrorKind::PollRequired
            ),
            http_status: None,
            retry_after: None,
            provider_request_id: None,
        }
    }

    pub(crate) fn from_status(status: u16) -> Self {
        let (kind, message) = match status {
            202 => (
                RetrievalErrorKind::PollRequired,
                "provider accepted the operation but requires polling",
            ),
            400 | 422 => (
                RetrievalErrorKind::InvalidRequest,
                "provider rejected the retrieval request",
            ),
            401 | 403 => (
                RetrievalErrorKind::Authentication,
                "provider authentication failed",
            ),
            402 => (
                RetrievalErrorKind::PaymentRequired,
                "provider account has no usable entitlement",
            ),
            408 | 504 => (
                RetrievalErrorKind::DeadlineExceeded,
                "provider request exceeded its deadline",
            ),
            429 => (
                RetrievalErrorKind::RateLimited,
                "provider rate limit reached",
            ),
            500..=599 => (
                RetrievalErrorKind::Unavailable,
                "provider service is unavailable",
            ),
            _ => (
                RetrievalErrorKind::MalformedResponse,
                "provider returned an unsupported HTTP status",
            ),
        };
        let mut error = Self::new(kind, message);
        error.http_status = Some(status);
        error
    }

    pub(crate) fn invalid_request() -> Self {
        Self::new(
            RetrievalErrorKind::InvalidRequest,
            "retrieval request failed local validation",
        )
    }

    pub(crate) fn malformed(message: &'static str) -> Self {
        Self::new(RetrievalErrorKind::MalformedResponse, message)
    }
}
