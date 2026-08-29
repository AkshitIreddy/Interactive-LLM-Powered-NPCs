use crate::{
    AudioFormatV1, IpcErrorV1, LlmEventV1, RequestId, SessionId, SttEventV1, TtsEventV1, TurnId,
    MAX_AUDIO_CHUNK_BYTES, MAX_TEXT_BYTES,
};
use prost::{Enumeration, Message};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum ProviderService {
    Unspecified = 0,
    SpeechToText = 1,
    LanguageModel = 2,
    TextToSpeech = 3,
    Embeddings = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum DeploymentKind {
    Unspecified = 0,
    Hosted = 1,
    LocalManaged = 2,
    LocalExternal = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum PrivacyClass {
    Unspecified = 0,
    FullyLocal = 1,
    ProviderNoTraining = 2,
    ProviderRetention = 3,
    UserConfiguredEndpoint = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum PricingUnit {
    Unspecified = 0,
    MillionInputTokens = 1,
    MillionOutputTokens = 2,
    AudioInputMinute = 3,
    AudioOutputMinute = 4,
    ThousandCharacters = 5,
    Request = 6,
    FreeLocalCompute = 7,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct PriceV1 {
    #[prost(enumeration = "PricingUnit", tag = "1")]
    pub unit: i32,
    #[prost(double, tag = "2")]
    pub amount: f64,
    #[prost(string, tag = "3")]
    pub currency: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct PricingDescriptorV1 {
    #[prost(message, repeated, tag = "1")]
    pub prices: Vec<PriceV1>,
    #[prost(string, tag = "2")]
    pub effective_date: String,
    #[prost(string, tag = "3")]
    pub source_url: String,
    #[prost(bool, tag = "4")]
    pub estimated_only: bool,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct ProviderFeatureFlagsV1 {
    #[prost(bool, tag = "1")]
    pub streaming_input: bool,
    #[prost(bool, tag = "2")]
    pub streaming_output: bool,
    #[prost(bool, tag = "3")]
    pub cancellation: bool,
    #[prost(bool, tag = "4")]
    pub model_discovery: bool,
    #[prost(bool, tag = "5")]
    pub language_discovery: bool,
    #[prost(bool, tag = "6")]
    pub json_schema: bool,
    #[prost(bool, tag = "7")]
    pub word_alignment: bool,
    #[prost(bool, tag = "8")]
    pub visemes: bool,
    #[prost(bool, tag = "9")]
    pub usage_reporting: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ProviderCapabilityDescriptorV1 {
    /// Stable lowercase identifier such as `openai`, `kokoro-local`, or `custom`.
    #[prost(string, tag = "1")]
    pub provider_id: String,
    #[prost(string, tag = "2")]
    pub display_name: String,
    #[prost(enumeration = "DeploymentKind", tag = "3")]
    pub deployment: i32,
    #[prost(enumeration = "PrivacyClass", tag = "4")]
    pub privacy: i32,
    #[prost(enumeration = "ProviderService", repeated, tag = "5")]
    pub services: Vec<i32>,
    #[prost(message, optional, tag = "6")]
    pub features: Option<ProviderFeatureFlagsV1>,
    #[prost(string, repeated, tag = "7")]
    pub model_ids: Vec<String>,
    /// BCP-47 tags; `*` means provider-advertised multilingual support.
    #[prost(string, repeated, tag = "8")]
    pub languages: Vec<String>,
    #[prost(message, repeated, tag = "9")]
    pub accepted_audio: Vec<AudioFormatV1>,
    #[prost(message, repeated, tag = "10")]
    pub produced_audio: Vec<AudioFormatV1>,
    #[prost(uint64, optional, tag = "11")]
    pub maximum_context_tokens: Option<u64>,
    #[prost(uint64, optional, tag = "12")]
    pub maximum_output_tokens: Option<u64>,
    #[prost(message, optional, tag = "13")]
    pub pricing: Option<PricingDescriptorV1>,
    /// IDs of providers that are semantically safe fallbacks for equivalent work.
    /// Selection still requires explicit user authorization for every cloud route.
    #[prost(string, repeated, tag = "14")]
    pub compatible_fallback_provider_ids: Vec<String>,
    #[prost(string, tag = "15")]
    pub capability_revision: String,
}

impl ProviderCapabilityDescriptorV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        validate_stable_id(&self.provider_id)?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err(ProviderValidationError::InvalidDisplayName);
        }
        nonzero_enum::<DeploymentKind>(self.deployment, "deployment")?;
        nonzero_enum::<PrivacyClass>(self.privacy, "privacy")?;
        if self.services.is_empty() || self.services.len() > 8 {
            return Err(ProviderValidationError::InvalidServices);
        }
        for service in &self.services {
            nonzero_enum::<ProviderService>(*service, "service")?;
        }
        if self.features.is_none() {
            return Err(ProviderValidationError::MissingFeatures);
        }
        if self.model_ids.len() > 4_096
            || self.languages.len() > 1_024
            || self.compatible_fallback_provider_ids.len() > 64
        {
            return Err(ProviderValidationError::TooManyEntries);
        }
        for id in &self.compatible_fallback_provider_ids {
            validate_stable_id(id)?;
            if id == &self.provider_id {
                return Err(ProviderValidationError::SelfFallback);
            }
        }
        for format in self.accepted_audio.iter().chain(&self.produced_audio) {
            format
                .validate()
                .map_err(|_| ProviderValidationError::InvalidAudioFormat)?;
        }
        if let Some(pricing) = &self.pricing {
            pricing.validate()?;
        }
        if self.capability_revision.is_empty() || self.capability_revision.len() > 128 {
            return Err(ProviderValidationError::InvalidRevision);
        }
        Ok(())
    }

    #[must_use]
    pub fn supports(&self, service: ProviderService) -> bool {
        self.services.contains(&(service as i32))
    }

    #[must_use]
    pub fn is_cloud(&self) -> bool {
        self.deployment == DeploymentKind::Hosted as i32
    }

    /// Returns true only for a declared route. Policy must additionally verify
    /// that the user explicitly authorized the target provider.
    #[must_use]
    pub fn declares_fallback(&self, target_provider_id: &str) -> bool {
        self.compatible_fallback_provider_ids
            .iter()
            .any(|id| id == target_provider_id)
    }
}

impl PricingDescriptorV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        if self.prices.len() > 32 || self.effective_date.len() > 32 || self.source_url.len() > 2_048
        {
            return Err(ProviderValidationError::InvalidPricing);
        }
        for price in &self.prices {
            nonzero_enum::<PricingUnit>(price.unit, "pricing unit")?;
            if !price.amount.is_finite() || price.amount < 0.0 || price.currency.len() != 3 {
                return Err(ProviderValidationError::InvalidPricing);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum ChatRole {
    Unspecified = 0,
    System = 1,
    User = 2,
    Assistant = 3,
    Tool = 4,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct ChatMessageV1 {
    #[prost(enumeration = "ChatRole", tag = "1")]
    pub role: i32,
    #[prost(string, tag = "2")]
    pub content: String,
    #[prost(string, optional, tag = "3")]
    pub name: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct StreamingRecognitionRequestV1 {
    #[prost(message, optional, tag = "1")]
    pub request_id: Option<RequestId>,
    #[prost(message, optional, tag = "2")]
    pub session_id: Option<SessionId>,
    #[prost(message, optional, tag = "3")]
    pub turn_id: Option<TurnId>,
    #[prost(message, optional, tag = "4")]
    pub input_format: Option<AudioFormatV1>,
    #[prost(string, tag = "5")]
    pub language: String,
    #[prost(bool, tag = "6")]
    pub interim_results: bool,
    #[prost(bool, tag = "7")]
    pub provider_endpointing: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct LanguageModelRequestV1 {
    #[prost(message, optional, tag = "1")]
    pub request_id: Option<RequestId>,
    #[prost(message, optional, tag = "2")]
    pub session_id: Option<SessionId>,
    #[prost(message, optional, tag = "3")]
    pub turn_id: Option<TurnId>,
    #[prost(string, tag = "4")]
    pub model_id: String,
    #[prost(message, repeated, tag = "5")]
    pub messages: Vec<ChatMessageV1>,
    #[prost(uint64, tag = "6")]
    pub maximum_output_tokens: u64,
    #[prost(float, tag = "7")]
    pub temperature: f32,
    #[prost(string, optional, tag = "8")]
    pub response_json_schema: Option<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TtsRequestV1 {
    #[prost(message, optional, tag = "1")]
    pub request_id: Option<RequestId>,
    #[prost(message, optional, tag = "2")]
    pub session_id: Option<SessionId>,
    #[prost(message, optional, tag = "3")]
    pub turn_id: Option<TurnId>,
    #[prost(string, tag = "4")]
    pub sentence_id: String,
    #[prost(string, tag = "5")]
    pub text: String,
    #[prost(string, tag = "6")]
    pub voice_id: String,
    #[prost(string, tag = "7")]
    pub language: String,
    #[prost(message, optional, tag = "8")]
    pub requested_format: Option<AudioFormatV1>,
    #[prost(bool, tag = "9")]
    pub request_alignment: bool,
}

/// Bounded audio input unit submitted to a streaming recognizer.
#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct RecognitionAudioChunkV1 {
    #[prost(uint64, tag = "1")]
    pub chunk_sequence: u64,
    #[prost(uint64, tag = "2")]
    pub start_sample: u64,
    #[prost(bytes = "vec", tag = "3")]
    pub data: Vec<u8>,
}

impl RecognitionAudioChunkV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        if self.chunk_sequence == 0
            || self.data.is_empty()
            || self.data.len() > MAX_AUDIO_CHUNK_BYTES
        {
            Err(ProviderValidationError::InvalidAudioChunk)
        } else {
            Ok(())
        }
    }
}

/// Result of a non-blocking submission to a persistent provider adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmitOutcome {
    Accepted,
    Backpressured,
}

/// Non-blocking normalized STT boundary used by the runtime.
///
/// Implementations enqueue work and return promptly; network/native async work
/// remains inside the adapter. `try_next_event` never blocks. The runtime owns
/// deadlines and discards events from obsolete cancellation generations.
pub trait StreamingRecognizer: Send {
    fn capabilities(&self) -> &ProviderCapabilityDescriptorV1;
    fn start(&mut self, request: StreamingRecognitionRequestV1) -> Result<(), IpcErrorV1>;
    fn submit_audio(&mut self, chunk: RecognitionAudioChunkV1)
        -> Result<SubmitOutcome, IpcErrorV1>;
    fn finish_input(&mut self) -> Result<(), IpcErrorV1>;
    fn cancel(&mut self, cancellation_generation: u64) -> Result<(), IpcErrorV1>;
    fn try_next_event(&mut self) -> Option<SttEventV1>;
}

/// Non-blocking normalized streaming LLM boundary used by the runtime.
pub trait LanguageModelProvider: Send {
    fn capabilities(&self) -> &ProviderCapabilityDescriptorV1;
    fn start(&mut self, request: LanguageModelRequestV1) -> Result<(), IpcErrorV1>;
    fn cancel(&mut self, cancellation_generation: u64) -> Result<(), IpcErrorV1>;
    fn try_next_event(&mut self) -> Option<LlmEventV1>;
}

/// One persistent TTS session. Sentence requests may be queued while earlier
/// sentences stream; the adapter reports explicit backpressure rather than
/// accumulating an unbounded queue.
pub trait TtsSession: Send {
    fn capabilities(&self) -> &ProviderCapabilityDescriptorV1;
    fn submit(&mut self, request: TtsRequestV1) -> Result<SubmitOutcome, IpcErrorV1>;
    fn cancel(&mut self, cancellation_generation: u64) -> Result<(), IpcErrorV1>;
    fn try_next_event(&mut self) -> Option<TtsEventV1>;
}

impl StreamingRecognitionRequestV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        validate_request_ids(
            self.request_id.as_ref(),
            self.session_id.as_ref(),
            self.turn_id.as_ref(),
        )?;
        self.input_format
            .as_ref()
            .ok_or(ProviderValidationError::InvalidAudioFormat)?
            .validate()
            .map_err(|_| ProviderValidationError::InvalidAudioFormat)?;
        if self.language.len() > 64 {
            return Err(ProviderValidationError::InvalidRequest);
        }
        Ok(())
    }
}

impl LanguageModelRequestV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        validate_request_ids(
            self.request_id.as_ref(),
            self.session_id.as_ref(),
            self.turn_id.as_ref(),
        )?;
        if self.model_id.is_empty() || self.model_id.len() > 512 || self.messages.is_empty() {
            return Err(ProviderValidationError::InvalidRequest);
        }
        if self.messages.len() > 4_096
            || self.maximum_output_tokens == 0
            || self.maximum_output_tokens > 1_000_000
            || !self.temperature.is_finite()
            || !(0.0..=2.0).contains(&self.temperature)
        {
            return Err(ProviderValidationError::InvalidRequest);
        }
        for message in &self.messages {
            nonzero_enum::<ChatRole>(message.role, "chat role")?;
            if message.content.len() > MAX_TEXT_BYTES
                || message.name.as_ref().is_some_and(|v| v.len() > 128)
            {
                return Err(ProviderValidationError::InvalidRequest);
            }
        }
        if self.response_json_schema.as_ref().is_some_and(|schema| {
            schema.len() > 256 * 1024 || serde_json::from_str::<serde_json::Value>(schema).is_err()
        }) {
            return Err(ProviderValidationError::InvalidJsonSchema);
        }
        Ok(())
    }
}

impl TtsRequestV1 {
    pub fn validate(&self) -> Result<(), ProviderValidationError> {
        validate_request_ids(
            self.request_id.as_ref(),
            self.session_id.as_ref(),
            self.turn_id.as_ref(),
        )?;
        if self.sentence_id.is_empty()
            || self.sentence_id.len() > 256
            || self.text.trim().is_empty()
            || self.text.len() > MAX_TEXT_BYTES
            || self.voice_id.is_empty()
            || self.voice_id.len() > 512
            || self.language.len() > 64
        {
            return Err(ProviderValidationError::InvalidRequest);
        }
        self.requested_format
            .as_ref()
            .ok_or(ProviderValidationError::InvalidAudioFormat)?
            .validate()
            .map_err(|_| ProviderValidationError::InvalidAudioFormat)
    }
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum ProviderValidationError {
    #[error("stable identifier is malformed")]
    InvalidStableId,
    #[error("display name is missing or too large")]
    InvalidDisplayName,
    #[error("unknown or unspecified {0}")]
    UnknownEnum(&'static str),
    #[error("provider service set is empty or invalid")]
    InvalidServices,
    #[error("provider feature descriptor is missing")]
    MissingFeatures,
    #[error("descriptor contains too many entries")]
    TooManyEntries,
    #[error("provider cannot list itself as a fallback")]
    SelfFallback,
    #[error("audio format is invalid")]
    InvalidAudioFormat,
    #[error("streaming audio chunk is empty, oversized, or unsequenced")]
    InvalidAudioChunk,
    #[error("pricing data is invalid")]
    InvalidPricing,
    #[error("capability revision is missing or too large")]
    InvalidRevision,
    #[error("normalized provider request is invalid")]
    InvalidRequest,
    #[error("JSON schema is invalid")]
    InvalidJsonSchema,
}

trait KnownEnum: Sized {
    fn from_i32(value: i32) -> Option<Self>;
}

macro_rules! impl_known_enum {
    ($($type:ty),+ $(,)?) => {
        $(impl KnownEnum for $type {
            fn from_i32(value: i32) -> Option<Self> {
                <$type>::try_from(value).ok()
            }
        })+
    };
}

impl_known_enum!(
    ProviderService,
    DeploymentKind,
    PrivacyClass,
    PricingUnit,
    ChatRole,
);

fn nonzero_enum<T: KnownEnum>(
    value: i32,
    name: &'static str,
) -> Result<(), ProviderValidationError> {
    if value == 0 || T::from_i32(value).is_none() {
        Err(ProviderValidationError::UnknownEnum(name))
    } else {
        Ok(())
    }
}

fn validate_stable_id(value: &str) -> Result<(), ProviderValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        Err(ProviderValidationError::InvalidStableId)
    } else {
        Ok(())
    }
}

fn validate_request_ids(
    request: Option<&RequestId>,
    session: Option<&SessionId>,
    turn: Option<&TurnId>,
) -> Result<(), ProviderValidationError> {
    request
        .and_then(RequestId::to_uuid)
        .ok_or(ProviderValidationError::InvalidRequest)?;
    session
        .and_then(SessionId::to_uuid)
        .ok_or(ProviderValidationError::InvalidRequest)?;
    turn.and_then(TurnId::to_uuid)
        .ok_or(ProviderValidationError::InvalidRequest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioEncoding;

    fn pcm() -> AudioFormatV1 {
        AudioFormatV1 {
            encoding: AudioEncoding::PcmS16Le as i32,
            sample_rate_hz: 24_000,
            channels: 1,
            bits_per_sample: 16,
        }
    }

    #[test]
    fn fallback_is_declared_not_implicitly_authorized() {
        let descriptor = ProviderCapabilityDescriptorV1 {
            provider_id: "local-kokoro".into(),
            display_name: "Kokoro Local".into(),
            deployment: DeploymentKind::LocalManaged as i32,
            privacy: PrivacyClass::FullyLocal as i32,
            services: vec![ProviderService::TextToSpeech as i32],
            features: Some(ProviderFeatureFlagsV1 {
                streaming_input: false,
                streaming_output: true,
                cancellation: true,
                model_discovery: false,
                language_discovery: false,
                json_schema: false,
                word_alignment: false,
                visemes: false,
                usage_reporting: false,
            }),
            model_ids: vec!["kokoro-82m".into()],
            languages: vec!["en".into()],
            accepted_audio: vec![],
            produced_audio: vec![pcm()],
            maximum_context_tokens: None,
            maximum_output_tokens: None,
            pricing: None,
            compatible_fallback_provider_ids: vec!["elevenlabs".into()],
            capability_revision: "2026-08-28".into(),
        };
        assert_eq!(descriptor.validate(), Ok(()));
        assert!(descriptor.declares_fallback("elevenlabs"));
        assert!(!descriptor.declares_fallback("unknown-cloud"));
    }

    #[test]
    fn request_schema_must_be_parseable_json() {
        let request = LanguageModelRequestV1 {
            request_id: Some(RequestId::new()),
            session_id: Some(SessionId::new()),
            turn_id: Some(TurnId::new()),
            model_id: "fixture".into(),
            messages: vec![ChatMessageV1 {
                role: ChatRole::User as i32,
                content: "Hello".into(),
                name: None,
            }],
            maximum_output_tokens: 200,
            temperature: 0.4,
            response_json_schema: Some("not json".into()),
        };
        assert_eq!(
            request.validate(),
            Err(ProviderValidationError::InvalidJsonSchema)
        );
    }
}
