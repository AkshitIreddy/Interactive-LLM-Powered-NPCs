use std::{fmt, time::Duration};
use zeroize::Zeroizing;

/// A value whose contents must never be included in logs or diagnostics.
///
/// Transport implementations may explicitly call [`SensitiveString::expose`]
/// only while encoding an outbound request. `Debug` and `Display` are always
/// redacted.
/// This type intentionally implements neither `Clone` nor `Serialize`: there is
/// one owner for each secret/dialogue buffer, and crossing a wire requires an
/// explicit borrow by the transport encoder.
///
/// ```compile_fail
/// use npc_providers_tts::SensitiveString;
/// use serde::Serialize;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<SensitiveString>();
/// ```
pub struct SensitiveString(Zeroizing<Vec<u8>>);

impl SensitiveString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into().into_bytes()))
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        // Construction accepts `String`, and the buffer is never mutated except
        // by zeroization immediately before destruction.
        std::str::from_utf8(&self.0).expect("SensitiveString invariant violated")
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl PartialEq for SensitiveString {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice() == other.0.as_slice()
    }
}

impl Eq for SensitiveString {}

impl fmt::Debug for SensitiveString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl fmt::Display for SensitiveString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CredentialResolveError {
    #[error("provider credential is missing")]
    Missing,
    #[error("credential vault is unavailable")]
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtsErrorKind {
    Cancelled,
    InvalidState,
    InvalidRequest,
    Authentication,
    QuotaExceeded,
    RateLimited,
    Timeout,
    Unavailable,
    Protocol,
    Transport,
}

/// A deliberately content-free provider error safe to display and log.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{provider_id}: {code}")]
pub struct TtsError {
    pub provider_id: String,
    pub kind: TtsErrorKind,
    /// Stable, allowlisted diagnostic code; never a provider response body.
    pub code: &'static str,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
}

impl TtsError {
    #[must_use]
    pub fn new(
        provider_id: impl Into<String>,
        kind: TtsErrorKind,
        code: &'static str,
        retryable: bool,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            kind,
            code,
            retryable,
            retry_after: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("transport unavailable")]
    Unavailable,
    #[error("transport timed out")]
    Timeout,
    #[error("transport authentication failed")]
    Authentication,
    #[error("provider quota exceeded")]
    QuotaExceeded,
    #[error("transport rate limited")]
    RateLimited { retry_after: Option<Duration> },
    #[error("transport protocol failure")]
    Protocol,
    #[error("transport was closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroize;

    #[test]
    fn explicit_zeroize_clears_the_owned_buffer() {
        let mut secret = SensitiveString::new("observable-secret");
        secret.0.as_mut_slice().zeroize();
        assert!(secret.0.iter().all(|byte| *byte == 0));
    }
}
