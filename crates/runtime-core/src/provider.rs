use std::{pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_core::Stream;
use tokio_util::sync::CancellationToken;

use crate::types::{
    CharacterIdentity, DeliveredSentence, EffectsRequest, GenerationRequest, LlmDelta,
    MemoryContext, NpcEffectsV1, PlaybackReceipt, ProviderDescriptor, SpeechRequest,
    SpeechStreamItem, TurnIdentity, TurnRequest,
};

pub type ProviderStream<T> = Pin<Box<dyn Stream<Item = Result<T, ProviderError>> + Send + 'static>>;
pub type LlmStream = ProviderStream<LlmDelta>;
pub type SpeechStream = ProviderStream<SpeechStreamItem>;
pub type RecognitionStream = ProviderStream<RecognitionEvent>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Cancelled,
    Timeout,
    RateLimited,
    Authentication,
    InvalidRequest,
    Unavailable,
    Protocol,
    Internal,
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("{provider_id}: {message}")]
pub struct ProviderError {
    pub provider_id: String,
    pub kind: ProviderErrorKind,
    pub message: String,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
}

impl ProviderError {
    pub fn cancelled(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            kind: ProviderErrorKind::Cancelled,
            message: "request cancelled".into(),
            retryable: true,
            retry_after: None,
        }
    }

    pub fn unavailable(provider_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            kind: ProviderErrorKind::Unavailable,
            message: message.into(),
            retryable: true,
            retry_after: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecognitionConfig {
    pub locale: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub push_to_talk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioInputChunk {
    pub sequence: u64,
    pub pcm_s16le: Vec<u8>,
    pub end_of_utterance: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecognitionEvent {
    SpeechStarted,
    Partial { text: String, stable: bool },
    Final { text: String },
    SpeechEnded,
}

#[async_trait]
pub trait RecognitionSession: Send {
    async fn push_audio(&mut self, chunk: AudioInputChunk) -> Result<(), ProviderError>;
    async fn finish(&mut self) -> Result<(), ProviderError>;
    fn events(&mut self) -> RecognitionStream;
    fn cancel(&self);
}

#[async_trait]
pub trait StreamingRecognizer: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;
    async fn start_session(
        &self,
        config: RecognitionConfig,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn RecognitionSession>, ProviderError>;
}

#[async_trait]
pub trait LanguageModelProvider: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;

    /// Starts a text stream. Once the first non-empty delta is emitted, the
    /// supervisor pins the turn to this provider and will not replay the prompt on
    /// another provider; doing so could duplicate user-visible dialogue.
    async fn stream_response(
        &self,
        request: GenerationRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmStream, ProviderError>;
}

#[async_trait]
pub trait EffectsProvider: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;
    async fn derive_effects(
        &self,
        request: EffectsRequest,
        cancellation: CancellationToken,
    ) -> Result<NpcEffectsV1, ProviderError>;
}

#[async_trait]
pub trait TtsSession: Send {
    fn provider(&self) -> &ProviderDescriptor;
    async fn synthesize(
        &mut self,
        request: SpeechRequest,
        cancellation: CancellationToken,
    ) -> Result<SpeechStream, ProviderError>;
    async fn close(&mut self) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    fn descriptor(&self) -> &ProviderDescriptor;
    async fn start_session(
        &self,
        identity: &TurnIdentity,
        locale: &str,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn TtsSession>, ProviderError>;
}

#[async_trait]
pub trait IdentityResolver: Send + Sync {
    async fn resolve(
        &self,
        request: &TurnRequest,
        cancellation: CancellationToken,
    ) -> Result<CharacterIdentity, RuntimeDependencyError>;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn retrieve(
        &self,
        request: &TurnRequest,
        cancellation: CancellationToken,
    ) -> Result<MemoryContext, RuntimeDependencyError>;

    /// Commits only sentences that reached an audible or visible delivery surface.
    async fn commit_delivered(
        &self,
        identity: &TurnIdentity,
        transcript: &str,
        delivered: &[DeliveredSentence],
        cancellation: CancellationToken,
    ) -> Result<(), RuntimeDependencyError>;
}

#[async_trait]
pub trait AudioSink: Send + Sync {
    /// Consumes provider audio incrementally. On barge-in, implementations must stop
    /// the device promptly and return a receipt describing what was actually heard.
    async fn play(
        &self,
        identity: &TurnIdentity,
        sentence_id: u64,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<PlaybackReceipt, RuntimeDependencyError>;

    async fn stop(&self, identity: &TurnIdentity) -> Result<(), RuntimeDependencyError>;
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum RuntimeDependencyError {
    #[error("cancelled")]
    Cancelled,
    #[error("temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("invalid dependency response: {0}")]
    Invalid(String),
    #[error("dependency failed: {0}")]
    Internal(String),
}

impl RuntimeDependencyError {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Unavailable(_))
    }
}

#[derive(Clone, Default)]
pub struct ProviderPool {
    pub recognizers: Vec<Arc<dyn StreamingRecognizer>>,
    pub language_models: Vec<Arc<dyn LanguageModelProvider>>,
    pub effects: Vec<Arc<dyn EffectsProvider>>,
    pub speech: Vec<Arc<dyn TtsProvider>>,
}

#[derive(Clone)]
pub struct RuntimeDependencies {
    pub providers: ProviderPool,
    pub identity: Arc<dyn IdentityResolver>,
    pub memory: Arc<dyn MemoryStore>,
    pub audio: Arc<dyn AudioSink>,
}
