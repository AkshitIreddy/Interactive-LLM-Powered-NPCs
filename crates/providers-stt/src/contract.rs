use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};

use crate::SttError;

/// PCM formats accepted by the first hosted-adapter release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEncoding {
    PcmS16Le,
    G711Ulaw,
    G711Alaw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    pub encoding: AudioEncoding,
    pub sample_rate_hz: u32,
    pub channels: u16,
}

impl AudioFormat {
    pub const PCM_16KHZ_MONO: Self = Self {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 16_000,
        channels: 1,
    };

    pub fn validate(self) -> Result<(), SttError> {
        if !(8_000..=48_000).contains(&self.sample_rate_hz) {
            return Err(SttError::invalid_request(
                "sample rate must be between 8 kHz and 48 kHz",
            ));
        }
        if self.channels != 1 {
            return Err(SttError::invalid_request(
                "hosted realtime STT requires mono audio",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointingMode {
    Manual,
    ProviderVad,
}

/// How the user's retention preference is satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    /// Provider-standard handling is acceptable after the UI disclosed it.
    ProviderDefaultAllowed,
    /// A provider request flag must disable storage/model-improvement use.
    RequireRequestLevelOptOut,
    /// The account/organization has an externally managed zero-retention agreement.
    AccountPolicyConfirmed,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionConfig {
    pub model: String,
    pub audio: AudioFormat,
    /// BCP-47 language hints. Empty means automatic detection.
    pub languages: Vec<String>,
    pub endpointing: EndpointingMode,
    pub interim_results: bool,
    pub keyword_hints: Vec<String>,
    /// Short non-secret conversational/domain context. Never added to errors or logs by this crate.
    pub context_hint: Option<String>,
    pub retention: RetentionPolicy,
    pub timeouts: SessionTimeouts,
}

impl fmt::Debug for RecognitionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecognitionConfig")
            .field("model", &self.model)
            .field("audio", &self.audio)
            .field("languages", &self.languages)
            .field("endpointing", &self.endpointing)
            .field("interim_results", &self.interim_results)
            .field("keyword_hint_count", &self.keyword_hints.len())
            .field("context_hint_present", &self.context_hint.is_some())
            .field("retention", &self.retention)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl RecognitionConfig {
    pub fn validate(&self) -> Result<(), SttError> {
        self.audio.validate()?;
        if self.model.trim().is_empty() || self.model.len() > 256 {
            return Err(SttError::invalid_request("model is missing or too long"));
        }
        if self.languages.len() > 16
            || self
                .languages
                .iter()
                .any(|value| value.is_empty() || value.len() > 64)
        {
            return Err(SttError::invalid_request("invalid language hints"));
        }
        if self.keyword_hints.len() > 100
            || self
                .keyword_hints
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 128)
        {
            return Err(SttError::invalid_request("invalid keyword hints"));
        }
        if self
            .context_hint
            .as_ref()
            .is_some_and(|value| value.len() > 4_096)
        {
            return Err(SttError::invalid_request("context hint is too long"));
        }
        self.timeouts.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimeouts {
    pub connect: Duration,
    pub send: Duration,
    pub receive_idle: Duration,
    pub flush: Duration,
}

impl Default for SessionTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            send: Duration::from_secs(5),
            receive_idle: Duration::from_secs(30),
            flush: Duration::from_secs(10),
        }
    }
}

impl SessionTimeouts {
    fn validate(self) -> Result<(), SttError> {
        let values = [self.connect, self.send, self.receive_idle, self.flush];
        if values
            .iter()
            .any(|value| value.is_zero() || *value > Duration::from_secs(300))
        {
            return Err(SttError::invalid_request(
                "timeouts must be greater than zero and at most five minutes",
            ));
        }
        Ok(())
    }
}

impl Default for RecognitionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            audio: AudioFormat::PCM_16KHZ_MONO,
            languages: Vec::new(),
            endpointing: EndpointingMode::Manual,
            interim_results: true,
            keyword_hints: Vec::new(),
            context_hint: None,
            retention: RetentionPolicy::ProviderDefaultAllowed,
            timeouts: SessionTimeouts::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionControl {
    RequestLevel,
    AccountLevel,
    ProviderPolicyOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLifecycle {
    Stable,
    Experimental,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecognizerCapabilities {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    pub default_model: &'static str,
    pub capability_revision: &'static str,
    pub languages: &'static [&'static str],
    pub accepted_audio: &'static [AudioFormat],
    pub partial_revisions: bool,
    pub provider_turn_detection: bool,
    pub manual_flush: bool,
    pub dynamic_keyword_hints: bool,
    pub context_hints: bool,
    pub resume_events: bool,
    pub retention_control: RetentionControl,
    pub sends_audio_to_provider: bool,
    pub sends_context_to_provider: bool,
    pub privacy_policy_url: &'static str,
    pub lifecycle: ProviderLifecycle,
    /// Curated disclosure shown by the UI; never provider-generated content.
    pub availability_note: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptStatus {
    Partial,
    /// Stable enough for speculative work, but may be revoked by `TurnResumed`.
    EagerFinal,
    Final,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    ProviderEndpoint,
    ManualFlush,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEvent {
    /// Provider turn/item identifier, normalized to a stable string.
    pub turn_id: String,
    /// Monotonic per-turn revision assigned by this adapter, beginning at one.
    pub revision: u64,
    /// Complete current text for this turn, never a delta.
    pub text: String,
    pub status: TranscriptStatus,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub audio_start_ms: Option<u64>,
    pub audio_end_ms: Option<u64>,
}

impl fmt::Debug for TranscriptEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TranscriptEvent")
            .field("turn_id", &self.turn_id)
            .field("revision", &self.revision)
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecognitionEvent {
    SessionStarted {
        provider_session_id: Option<String>,
    },
    TurnStarted {
        turn_id: String,
    },
    Transcript(TranscriptEvent),
    /// Invalidates speculative work based on an earlier eager-final revision.
    TurnResumed {
        turn_id: String,
        revision: u64,
    },
    TurnEnded {
        turn_id: String,
        revision: u64,
        reason: TurnEndReason,
    },
    Warning {
        code: String,
        message: String,
    },
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlushReason {
    PushToTalkReleased,
    ManualStop,
    MaximumDuration,
}
