//! Bounded Cartesia Sonic WebSocket transport.

use std::{fmt, sync::Arc, time::Duration};

use crate::{
    hosted_websocket::{connect_cartesia_pooled, CartesiaSocketPool, HostedWebSocketLimits},
    AudioFormat, CartesiaConnectionStats, OpenRequest, PcmEncoding, TransportError, TtsConnection,
    TtsTransport,
};

/// Exact Cartesia route qualified by the 2026-09-05 live transport probe.
pub const CARTESIA_QUALIFIED_MODEL_ID: &str = "sonic-3.6";
pub const CARTESIA_QUALIFIED_STOCK_VOICE_ID: &str = "a0e99841-438c-4a64-b679-ae501e7d6091";
pub const CARTESIA_QUALIFIED_API_VERSION: &str = "2026-03-01";
pub const CARTESIA_QUALIFIED_AUDIO_FORMAT: AudioFormat = AudioFormat {
    encoding: PcmEncoding::PcmS16Le,
    sample_rate_hz: 24_000,
    channels: 1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CartesiaWebSocketTransportConfig {
    pub limits: HostedWebSocketLimits,
    /// A completed socket may be leased by one subsequent context during this
    /// window. Expired sockets are closed before a fresh authenticated upgrade.
    pub idle_timeout: Duration,
}

impl Default for CartesiaWebSocketTransportConfig {
    fn default() -> Self {
        Self {
            limits: HostedWebSocketLimits::default(),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct CartesiaWebSocketTransport {
    config: CartesiaWebSocketTransportConfig,
    pool: Arc<CartesiaSocketPool>,
}

impl CartesiaWebSocketTransport {
    pub fn new(config: CartesiaWebSocketTransportConfig) -> Result<Self, TransportError> {
        config.limits.validate()?;
        if config.idle_timeout.is_zero() || config.idle_timeout > Duration::from_secs(300) {
            return Err(TransportError::Protocol);
        }
        Ok(Self {
            config,
            pool: Arc::new(CartesiaSocketPool::default()),
        })
    }

    /// Sanitized connection counters for native receipts and latency audits.
    #[must_use]
    pub fn connection_stats(&self) -> CartesiaConnectionStats {
        self.pool.snapshot()
    }
}

impl Default for CartesiaWebSocketTransport {
    fn default() -> Self {
        Self::new(CartesiaWebSocketTransportConfig::default())
            .expect("default Cartesia WebSocket limits are valid")
    }
}

impl fmt::Debug for CartesiaWebSocketTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CartesiaWebSocketTransport")
            .field("config", &self.config)
            .field("connection_stats", &self.connection_stats())
            .finish()
    }
}

#[async_trait::async_trait]
impl TtsTransport for CartesiaWebSocketTransport {
    async fn connect(
        &self,
        request: OpenRequest,
    ) -> Result<Box<dyn TtsConnection>, TransportError> {
        connect_cartesia_pooled(
            request,
            self.config.limits,
            self.config.idle_timeout,
            Arc::clone(&self.pool),
        )
        .await
    }
}
