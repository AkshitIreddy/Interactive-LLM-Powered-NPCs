mod assemblyai;
mod deepgram;
mod elevenlabs;
mod openai;

use std::fmt;

use crate::{
    ClientFrame, ConnectRequest, FlushReason, RecognitionConfig, RecognizerCapabilities,
    SecretString, ServerFrame, SttError, TranscriptStatus, TurnEndReason,
};

pub use assemblyai::AssemblyAi;
pub use deepgram::DeepgramFlux;
pub use elevenlabs::ElevenLabsScribe;
pub use openai::OpenAiRealtime;

pub trait ProviderProtocol: Send + Sync + 'static {
    fn capabilities(&self) -> &'static RecognizerCapabilities;
    fn create_session(
        &self,
        config: &RecognitionConfig,
    ) -> Result<Box<dyn ProviderSessionProtocol>, SttError>;
}

pub trait ProviderSessionProtocol: Send {
    fn provider_id(&self) -> &'static str;
    fn connect_request<'secret>(
        &self,
        config: &RecognitionConfig,
        credential: &'secret SecretString,
    ) -> Result<ConnectRequest<'secret>, SttError>;
    fn start_frames(&mut self, config: &RecognitionConfig) -> Result<Vec<ClientFrame>, SttError>;
    fn audio_frame(&mut self, audio: &[u8]) -> Result<ClientFrame, SttError>;
    fn flush_frame(&mut self, reason: FlushReason) -> Result<ClientFrame, SttError>;
    fn cancel_frame(&mut self) -> Option<ClientFrame>;
    fn close_frame(&mut self) -> Option<ClientFrame>;
    fn parse_frame(&mut self, frame: ServerFrame) -> Result<Vec<WireEvent>, SttError>;
}

#[derive(Clone, PartialEq)]
pub struct WireTranscript {
    pub turn_id: String,
    pub text: String,
    pub status: TranscriptStatus,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub audio_start_ms: Option<u64>,
    pub audio_end_ms: Option<u64>,
}

impl fmt::Debug for WireTranscript {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WireTranscript")
            .field("turn_id", &self.turn_id)
            .field(
                "text",
                &format_args!("[REDACTED; {} bytes]", self.text.len()),
            )
            .field("status", &self.status)
            .field("language", &self.language)
            .field("confidence", &self.confidence)
            .field("audio_start_ms", &self.audio_start_ms)
            .field("audio_end_ms", &self.audio_end_ms)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum WireEvent {
    SessionStarted {
        provider_session_id: Option<String>,
    },
    TurnStarted {
        turn_id: String,
    },
    Transcript(WireTranscript),
    TurnResumed {
        turn_id: String,
    },
    TurnEnded {
        turn_id: String,
        reason: TurnEndReason,
    },
    Warning {
        code: String,
        message: String,
    },
}

pub(super) fn json_frame(
    provider_id: &'static str,
    frame: ServerFrame,
) -> Result<serde_json::Value, SttError> {
    let ServerFrame::Text(text) = frame else {
        return Err(SttError::protocol(provider_id, Some("unexpected_binary")));
    };
    serde_json::from_str(&text).map_err(|_| SttError::protocol(provider_id, Some("invalid_json")))
}

pub(super) fn text_frame(value: serde_json::Value) -> Result<ClientFrame, SttError> {
    serde_json::to_string(&value)
        .map(ClientFrame::Text)
        .map_err(|_| SttError::invalid_request("provider request could not be encoded"))
}

pub(super) fn seconds_to_ms(value: Option<f64>) -> Option<u64> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| (value * 1_000.0).round() as u64)
}

pub(super) fn safe_warning(code: &str, message: &'static str) -> WireEvent {
    WireEvent::Warning {
        code: code
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            .take(64)
            .collect(),
        message: message.to_owned(),
    }
}
