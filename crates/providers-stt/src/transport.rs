use std::fmt;

use async_trait::async_trait;
use zeroize::{Zeroize, Zeroizing};

/// Non-cloneable UTF-8 secret material that clears its allocation before release.
///
/// This type intentionally implements neither `Clone` nor `Serialize`. It is borrowed only for
/// the WebSocket handshake and is dropped as soon as `TransportFactory::connect` returns.
///
/// ```compile_fail
/// use npc_providers_stt::SecretString;
/// let secret = SecretString::new("never serialize me");
/// let _ = serde_json::to_string(&secret);
/// ```
///
/// ```compile_fail
/// use npc_providers_stt::SecretString;
/// let secret = SecretString::new("never clone me");
/// let _copy = secret.clone();
/// ```
pub struct SecretString(Zeroizing<Box<[u8]>>);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into().into_bytes().into_boxed_slice()))
    }

    /// Only the transport boundary should reveal a secret to construct authentication headers.
    pub fn expose(&self) -> &str {
        // Construction accepts a String, so this invariant cannot be violated by safe callers.
        std::str::from_utf8(self.0.as_ref()).expect("SecretString is valid UTF-8")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // Zeroizing repeats this operation in its own Drop. Clearing explicitly lets the unit
        // test observe the bytes after zeroization but before the allocation is released.
        self.0.zeroize();
        #[cfg(test)]
        drop_observer::observe(self.0.as_ref());
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Debug)]
pub enum TransportAuth<'secret> {
    Header {
        name: &'static str,
        scheme: Option<&'static str>,
        value: &'secret SecretString,
    },
}

pub struct ConnectRequest<'secret> {
    pub url: &'static str,
    /// Query pairs are separate so the actual transport performs escaping.
    pub query: Vec<(String, String)>,
    pub auth: TransportAuth<'secret>,
}

impl fmt::Debug for ConnectRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectRequest")
            .field("url", &self.url)
            .field(
                "query_keys",
                &self.query.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            )
            .field("auth", &self.auth)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ClientFrame {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

impl fmt::Debug for ClientFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => formatter
                .debug_tuple("Text")
                .field(&format_args!("[REDACTED; {} bytes]", value.len()))
                .finish(),
            Self::Binary(value) => formatter
                .debug_tuple("Binary")
                .field(&format_args!("[REDACTED; {} bytes]", value.len()))
                .finish(),
            Self::Close => formatter.write_str("Close"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ServerFrame {
    Text(String),
    Binary(Vec<u8>),
    Closed { code: Option<u16> },
}

impl fmt::Debug for ServerFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => formatter
                .debug_tuple("Text")
                .field(&format_args!("[REDACTED; {} bytes]", value.len()))
                .finish(),
            Self::Binary(value) => formatter
                .debug_tuple("Binary")
                .field(&format_args!("[REDACTED; {} bytes]", value.len()))
                .finish(),
            Self::Closed { code } => formatter
                .debug_struct("Closed")
                .field("code", code)
                .finish(),
        }
    }
}

/// A deliberately content-free transport failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("websocket transport failed")]
pub struct TransportError;

#[async_trait]
pub trait StreamingTransport: Send {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), TransportError>;
    async fn receive(&mut self) -> Result<Option<ServerFrame>, TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
}

#[async_trait]
pub trait TransportFactory: Send + Sync {
    /// Performs the authenticated WebSocket handshake.
    ///
    /// Implementations must reveal the borrowed credential only while constructing the handshake
    /// headers. They must not copy it into the returned transport, an error, telemetry, or logs.
    async fn connect(
        &self,
        request: ConnectRequest<'_>,
    ) -> Result<Box<dyn StreamingTransport>, TransportError>;
}

#[cfg(test)]
mod drop_observer {
    use std::sync::Mutex;

    static OBSERVED: Mutex<Option<Vec<u8>>> = Mutex::new(None);

    pub(super) fn observe(bytes: &[u8]) {
        *OBSERVED.lock().expect("drop observer lock") = Some(bytes.to_vec());
    }

    pub(super) fn take() -> Option<Vec<u8>> {
        OBSERVED.lock().expect("drop observer lock").take()
    }
}

#[cfg(test)]
mod tests {
    use super::{drop_observer, SecretString};

    #[test]
    fn secret_drop_observes_only_zeroed_bytes() {
        drop(SecretString::new("credential bytes"));
        let observed = drop_observer::take().expect("drop was observed");
        assert_eq!(observed.len(), "credential bytes".len());
        assert!(observed.iter().all(|byte| *byte == 0));
    }
}
