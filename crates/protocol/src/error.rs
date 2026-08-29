use prost::{Enumeration, Message};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum ErrorCode {
    Unspecified = 0,
    InvalidArgument = 1,
    UnsupportedVersion = 2,
    UnauthorizedPeer = 3,
    StaleLaunch = 4,
    WrongSession = 5,
    OutOfOrder = 6,
    SequenceGap = 7,
    DeadlineExceeded = 8,
    Cancelled = 9,
    PayloadTooLarge = 10,
    MalformedPayload = 11,
    ProviderUnavailable = 12,
    ProviderRateLimited = 13,
    ModelUnavailable = 14,
    DeviceUnavailable = 15,
    WorkerQuarantined = 16,
    PolicyBlocked = 17,
    Internal = 18,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct IpcErrorV1 {
    #[prost(enumeration = "ErrorCode", tag = "1")]
    pub code: i32,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(bool, tag = "3")]
    pub retryable: bool,
    #[prost(uint64, optional, tag = "4")]
    pub retry_after_ms: Option<u64>,
    #[prost(string, tag = "5")]
    pub stage: String,
    #[prost(map = "string, string", tag = "6")]
    pub details: HashMap<String, String>,
}

impl IpcErrorV1 {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code as i32,
            message: message.into(),
            retryable: false,
            retry_after_ms: None,
            stage: String::new(),
            details: HashMap::new(),
        }
    }

    #[must_use]
    pub fn retryable(mut self, retry_after_ms: Option<u64>) -> Self {
        self.retryable = true;
        self.retry_after_ms = retry_after_ms;
        self
    }

    #[must_use]
    pub fn at_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = stage.into();
        self
    }

    #[must_use]
    pub fn error_code(&self) -> ErrorCode {
        ErrorCode::try_from(self.code).unwrap_or(ErrorCode::Unspecified)
    }

    pub fn validate(&self) -> Result<(), IpcErrorValidationError> {
        if self.error_code() == ErrorCode::Unspecified {
            return Err(IpcErrorValidationError::UnknownCode);
        }
        if self.message.trim().is_empty() || self.message.len() > 16_384 {
            return Err(IpcErrorValidationError::InvalidMessage);
        }
        if self.stage.len() > 128
            || self.details.len() > 64
            || self
                .details
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 128 || value.len() > 4_096)
        {
            return Err(IpcErrorValidationError::InvalidDetails);
        }
        if !self.retryable && self.retry_after_ms.is_some() {
            return Err(IpcErrorValidationError::ContradictoryRetry);
        }
        if self
            .retry_after_ms
            .is_some_and(|delay| delay == 0 || delay > 86_400_000)
        {
            return Err(IpcErrorValidationError::InvalidRetryDelay);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum IpcErrorValidationError {
    #[error("IPC error code is unknown or unspecified")]
    UnknownCode,
    #[error("IPC error message is empty or too large")]
    InvalidMessage,
    #[error("IPC error diagnostic details exceed their bounds")]
    InvalidDetails,
    #[error("non-retryable IPC error carries a retry delay")]
    ContradictoryRetry,
    #[error("IPC retry delay is outside the accepted range")]
    InvalidRetryDelay,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_metadata_is_internally_consistent() {
        let error = IpcErrorV1::new(ErrorCode::ProviderRateLimited, "slow down")
            .retryable(Some(1_000))
            .at_stage("responding");
        assert_eq!(error.validate(), Ok(()));
    }

    #[test]
    fn non_retryable_error_cannot_smuggle_retry_delay() {
        let mut error = IpcErrorV1::new(ErrorCode::PolicyBlocked, "blocked");
        error.retry_after_ms = Some(1_000);
        assert_eq!(
            error.validate(),
            Err(IpcErrorValidationError::ContradictoryRetry)
        );
    }
}
