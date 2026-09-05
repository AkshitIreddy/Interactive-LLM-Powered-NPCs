//! Bounded Deepgram Aura `/v1/speak` and Flux `/v2/speak` WebSocket transport.

use std::fmt;

use url::Url;

use crate::{
    hosted_websocket::{connect, HostedWebSocketLimits, WireFlavor},
    AudioFormat, OpenRequest, PcmEncoding, TransportError, TtsConnection, TtsTransport,
};

pub const DEEPGRAM_QUALIFIED_AURA2_MODEL_ID: &str = "aura-2-arcas-en";
pub const DEEPGRAM_QUALIFIED_FLUX_MODEL_ID: &str = "flux-miles-en";
pub const DEEPGRAM_QUALIFIED_AUDIO_FORMAT: AudioFormat = AudioFormat {
    encoding: PcmEncoding::PcmS16Le,
    sample_rate_hz: 24_000,
    channels: 1,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeepgramWebSocketTransportConfig {
    pub limits: HostedWebSocketLimits,
}

#[derive(Clone)]
pub struct DeepgramWebSocketTransport {
    config: DeepgramWebSocketTransportConfig,
}

impl DeepgramWebSocketTransport {
    pub fn new(config: DeepgramWebSocketTransportConfig) -> Result<Self, TransportError> {
        config.limits.validate()?;
        Ok(Self { config })
    }
}

impl Default for DeepgramWebSocketTransport {
    fn default() -> Self {
        Self::new(DeepgramWebSocketTransportConfig::default())
            .expect("default Deepgram WebSocket limits are valid")
    }
}

impl fmt::Debug for DeepgramWebSocketTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeepgramWebSocketTransport")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait::async_trait]
impl TtsTransport for DeepgramWebSocketTransport {
    async fn connect(
        &self,
        request: OpenRequest,
    ) -> Result<Box<dyn TtsConnection>, TransportError> {
        let endpoint = Url::parse(&request.endpoint).map_err(|_| TransportError::Protocol)?;
        let model = request.query.get("model").map(String::as_str);
        let flavor = match (endpoint.path(), model) {
            ("/v1/speak", Some(DEEPGRAM_QUALIFIED_AURA2_MODEL_ID)) => WireFlavor::DeepgramV1,
            ("/v2/speak", Some(DEEPGRAM_QUALIFIED_FLUX_MODEL_ID)) => WireFlavor::DeepgramV2,
            _ => return Err(TransportError::Protocol),
        };
        if request.query.get("encoding").map(String::as_str) != Some("linear16")
            || request.query.get("sample_rate").map(String::as_str) != Some("24000")
            || request.query.get("mip_opt_out").map(String::as_str) != Some("true")
            || request.query.len() != 4
        {
            return Err(TransportError::ProtocolStage(
                "deepgram_route_not_qualified",
            ));
        }
        connect(request, flavor, self.config.limits).await
    }
}
