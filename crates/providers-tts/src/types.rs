use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{CredentialResolveError, SensitiveString, TtsError};

pub const MAX_PUSH_TEXT_CHARS: usize = 16_384;
pub const MAX_AUDIO_CHUNK_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedTtsProviderId {
    Cartesia,
    ElevenLabs,
    Inworld,
    Deepgram,
    NvidiaNimMagpie,
}

impl HostedTtsProviderId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cartesia => "cartesia",
            Self::ElevenLabs => "elevenlabs",
            Self::Inworld => "inworld",
            Self::Deepgram => "deepgram",
            Self::NvidiaNimMagpie => "nvidia-nim-magpie",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PcmEncoding {
    PcmS16Le,
    MuLaw,
    ALaw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    pub encoding: PcmEncoding,
    pub sample_rate_hz: u32,
    pub channels: u16,
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self {
            encoding: PcmEncoding::PcmS16Le,
            sample_rate_hz: 24_000,
            channels: 1,
        }
    }
}

impl AudioFormat {
    pub fn validate(self) -> Result<Self, &'static str> {
        if !(8_000..=96_000).contains(&self.sample_rate_hz) {
            return Err("invalid_sample_rate");
        }
        if self.channels == 0 || self.channels > 2 {
            return Err("invalid_channel_count");
        }
        Ok(self)
    }
}

/// Provider-neutral qualities. No demographic inference or actor imitation is
/// represented in the contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceIntent {
    pub id: String,
    pub locale: String,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    pub pace: f32,
    pub pitch_semitones: f32,
    pub warmth: f32,
    pub energy: f32,
    pub roughness: f32,
    pub expressiveness: f32,
}

impl VoiceIntent {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.trim().is_empty() || self.id.len() > 128 {
            return Err("invalid_voice_intent_id");
        }
        if self.locale.trim().is_empty() || self.locale.len() > 35 {
            return Err("invalid_locale");
        }
        for value in [
            self.warmth,
            self.energy,
            self.roughness,
            self.expressiveness,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err("invalid_voice_dimension");
            }
        }
        if !self.pace.is_finite() || !(0.7..=1.5).contains(&self.pace) {
            return Err("invalid_pace");
        }
        if !self.pitch_semitones.is_finite() || !(-12.0..=12.0).contains(&self.pitch_semitones) {
            return Err("invalid_pitch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceBinding {
    pub intent_id: String,
    pub provider_id: HostedTtsProviderId,
    pub voice_id: String,
    pub model_id: String,
    #[serde(default)]
    pub provider_options: BTreeMap<String, String>,
}

impl VoiceBinding {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.intent_id.trim().is_empty() || self.intent_id.len() > 128 {
            return Err("invalid_voice_intent_id");
        }
        if self.voice_id.trim().is_empty() || self.voice_id.len() > 256 {
            return Err("invalid_voice_id");
        }
        if self.model_id.trim().is_empty() || self.model_id.len() > 256 {
            return Err("invalid_model_id");
        }
        if self.provider_options.len() > 32
            || self
                .provider_options
                .iter()
                .any(|(key, value)| key.len() > 64 || value.len() > 256)
        {
            return Err("invalid_provider_options");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct VoiceBindings {
    entries: BTreeMap<(String, HostedTtsProviderId), VoiceBinding>,
}

impl VoiceBindings {
    pub fn new(bindings: impl IntoIterator<Item = VoiceBinding>) -> Result<Self, &'static str> {
        let mut entries = BTreeMap::new();
        for binding in bindings {
            binding.validate()?;
            let key = (binding.intent_id.clone(), binding.provider_id);
            if entries.insert(key, binding).is_some() {
                return Err("duplicate_voice_binding");
            }
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn resolve(
        &self,
        intent_id: &str,
        provider_id: HostedTtsProviderId,
    ) -> Option<&VoiceBinding> {
        self.entries.get(&(intent_id.to_owned(), provider_id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    pub session_id: String,
    pub turn_id: String,
    pub cancellation_generation: u64,
}

impl SessionIdentity {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.session_id.trim().is_empty() || self.session_id.len() > 128 {
            return Err("invalid_session_id");
        }
        if self.turn_id.trim().is_empty() || self.turn_id.len() > 128 {
            return Err("invalid_turn_id");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct TtsSessionRequest {
    pub identity: SessionIdentity,
    pub locale: String,
    pub voice_intent_id: String,
    pub output: AudioFormat,
    pub request_alignment: bool,
    pub request_visemes: bool,
    pub clause_policy: crate::SemanticClausePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    AcceptingText,
    Finishing,
    Completed,
    Cancelled,
    Faulted,
}

impl SessionState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Faulted)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    pub accepted_chars: usize,
    pub clauses_submitted: usize,
    pub buffered_chars: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcmChunk {
    pub sequence: u64,
    pub format: AudioFormat,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordAlignment {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text_start: Option<usize>,
    pub source_text_length: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimingSymbolKind {
    /// A provider-defined mouth-shape category, not a phoneme.
    ProviderViseme,
    /// A spoken phoneme which requires an explicit provider/model mapping.
    Phoneme,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisemeEvent {
    pub symbol_kind: TimingSymbolKind,
    pub symbol: String,
    pub start_ms: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageEvent {
    pub processed_characters: u64,
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TtsEvent {
    Audio(PcmChunk),
    Alignment(Vec<WordAlignment>),
    Viseme(Vec<VisemeEvent>),
    Usage(UsageEvent),
    Interrupted { reason: &'static str },
    Completed,
}

/// Resolves a provider credential for a single connection attempt. Production
/// implementations should read Windows Credential Manager here and return a new
/// zeroizing buffer. Providers retain this resolver, never the secret itself.
#[async_trait::async_trait]
pub trait ProviderCredentialResolver: Send + Sync {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError>;
}

#[async_trait::async_trait]
pub trait StreamingTtsProvider: Send + Sync {
    fn id(&self) -> HostedTtsProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError>;
}

#[async_trait::async_trait]
pub trait StreamingTtsSession: Send {
    fn provider_id(&self) -> HostedTtsProviderId;
    fn identity(&self) -> &SessionIdentity;
    fn state(&self) -> SessionState;
    fn has_started_utterance(&self) -> bool;
    async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError>;
    async fn finish(&mut self) -> Result<(), TtsError>;
    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>>;
    async fn cancel(&mut self) -> Result<(), TtsError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub streaming_input: bool,
    pub streaming_pcm: bool,
    pub alignment: bool,
    pub visemes_or_phonemes: bool,
    pub cancellation: bool,
    pub usage: bool,
}
