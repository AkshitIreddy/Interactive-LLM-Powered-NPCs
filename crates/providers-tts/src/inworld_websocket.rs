//! Bounded Inworld multi-context TTS WebSocket transport.

use std::fmt;

use crate::{
    hosted_websocket::{connect, HostedWebSocketLimits, WireFlavor},
    AudioFormat, OpenRequest, PcmEncoding, TransportError, TtsConnection, TtsTransport,
};

pub const INWORLD_QUALIFIED_MODEL_ID: &str = "inworld-tts-2";
pub const INWORLD_QUALIFIED_FLASH_MODEL_ID: &str = "inworld-tts-2-flash";
pub const INWORLD_QUALIFIED_STOCK_VOICE_ID: &str = "Dennis";
pub const INWORLD_QUALIFIED_AUDIO_FORMAT: AudioFormat = AudioFormat {
    encoding: PcmEncoding::PcmS16Le,
    sample_rate_hz: 24_000,
    channels: 1,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InworldWebSocketTransportConfig {
    pub limits: HostedWebSocketLimits,
}

#[derive(Clone)]
pub struct InworldWebSocketTransport {
    config: InworldWebSocketTransportConfig,
}

impl InworldWebSocketTransport {
    pub fn new(config: InworldWebSocketTransportConfig) -> Result<Self, TransportError> {
        config.limits.validate()?;
        Ok(Self { config })
    }
}

impl Default for InworldWebSocketTransport {
    fn default() -> Self {
        Self::new(InworldWebSocketTransportConfig::default())
            .expect("default Inworld WebSocket limits are valid")
    }
}

impl fmt::Debug for InworldWebSocketTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InworldWebSocketTransport")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait::async_trait]
impl TtsTransport for InworldWebSocketTransport {
    async fn connect(
        &self,
        request: OpenRequest,
    ) -> Result<Box<dyn TtsConnection>, TransportError> {
        connect(request, WireFlavor::Inworld, self.config.limits).await
    }
}
