use std::{collections::BTreeMap, fmt, time::Duration};

use serde::{Deserialize, Serialize};

/// Stable identity attached to every event emitted by one foreground turn.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnIdentity {
    pub session_id: String,
    pub turn_id: String,
    /// Monotonic process-local generation. Events from older generations are stale.
    pub cancellation_generation: u64,
}

impl fmt::Display for TurnIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}@{}",
            self.session_id, self.turn_id, self.cancellation_generation
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Cloud,
    Hybrid,
    FullyLocal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderModality {
    Recognition,
    LanguageModel,
    Effects,
    Speech,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderLocation {
    Local,
    Cloud { service: String },
    ExternalLocalServer,
}

impl ProviderLocation {
    pub fn is_networked(&self) -> bool {
        matches!(self, Self::Cloud { .. })
    }

    pub fn is_local(&self) -> bool {
        !self.is_networked()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: String,
    pub display_name: String,
    pub modality: ProviderModality,
    pub location: ProviderLocation,
    /// True when provider terms allow submitted data to be retained.
    pub may_retain_data: bool,
    /// Data categories leaving the process if this provider is selected.
    #[serde(default)]
    pub transmitted_data: Vec<DataClass>,
    #[serde(default)]
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Transcript,
    PromptContext,
    Audio,
    EffectsSchema,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnLifecycle {
    Accepted,
    Listening,
    Transcribing,
    Identifying,
    Remembering,
    Responding,
    Voicing,
    Animating,
    Committing,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnLane {
    SpokenResponse,
    StructuredEffects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneLifecycle {
    Pending,
    Running,
    Completed,
    Neutralized,
    Cancelled,
    Failed,
}

impl TurnLifecycle {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnRequest {
    pub session_id: String,
    pub turn_id: String,
    pub transcript: String,
    pub character_hint: Option<String>,
    pub game_id: String,
    pub locale: String,
    pub execution_mode: ExecutionMode,
    pub network_policy: NetworkPolicy,
    /// Exact cloud provider IDs authorized for this turn. Empty means no cloud egress.
    #[serde(default)]
    pub authorized_cloud_providers: Vec<String>,
    pub allow_provider_fallback: bool,
    pub allow_local_to_cloud_fallback: bool,
    pub allow_retaining_providers: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl TurnRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.session_id.trim().is_empty() {
            return Err(ValidationError::Missing("session_id"));
        }
        if self.turn_id.trim().is_empty() {
            return Err(ValidationError::Missing("turn_id"));
        }
        if self.transcript.trim().is_empty() {
            return Err(ValidationError::Missing("transcript"));
        }
        if self.game_id.trim().is_empty() {
            return Err(ValidationError::Missing("game_id"));
        }
        if self.locale.trim().is_empty() {
            return Err(ValidationError::Missing("locale"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("required field is missing: {0}")]
    Missing(&'static str),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterIdentity {
    pub character_id: Option<String>,
    pub display_name: String,
    pub confidence: f32,
    pub evidence: Vec<String>,
    pub explicit_selection: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryContext {
    pub working_context: Vec<String>,
    pub episodic_memories: Vec<String>,
    pub canon_facts: Vec<String>,
    pub relationship_summary: Option<String>,
    pub retrieval_degraded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub identity: TurnIdentity,
    pub transcript: String,
    pub character: CharacterIdentity,
    pub memory: MemoryContext,
    pub locale: String,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectsRequest {
    pub generation: GenerationRequest,
    /// Effects are derived independently and must not mutate this spoken text.
    pub response_text_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmDelta {
    pub text: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechRequest {
    pub identity: TurnIdentity,
    pub sentence_id: u64,
    pub text: String,
    pub locale: String,
    pub voice_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioChunk {
    pub sequence: u64,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub pcm_s16le: Vec<u8>,
    pub end_of_stream: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlignmentEvent {
    pub text_offset: usize,
    pub text_length: usize,
    pub audio_offset: Duration,
    pub viseme: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SpeechStreamItem {
    Audio(AudioChunk),
    Alignment(AlignmentEvent),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NpcEffectsV1 {
    pub emotion: Option<String>,
    pub valence: Option<f32>,
    pub arousal: Option<f32>,
    pub intensity: Option<f32>,
    pub voice_style: Option<String>,
    #[serde(default)]
    pub animation_cues: Vec<String>,
    pub interruption_policy: Option<String>,
    #[serde(default)]
    pub memory_proposals: Vec<MemoryProposal>,
    #[serde(default)]
    pub relationship_proposals: Vec<RelationshipProposal>,
    #[serde(default)]
    pub action_proposals: Vec<ActionProposal>,
}

impl NpcEffectsV1 {
    pub fn neutral() -> Self {
        Self::default()
    }

    pub fn validate(mut self, allowed_actions: &[String]) -> Result<Self, EffectsValidationError> {
        validate_optional_range("valence", self.valence, -1.0, 1.0)?;
        validate_optional_range("arousal", self.arousal, 0.0, 1.0)?;
        validate_optional_range("intensity", self.intensity, 0.0, 1.0)?;
        if self.memory_proposals.iter().any(|proposal| {
            proposal.content.trim().is_empty()
                || proposal.content.len() > 2_000
                || !proposal.confidence.is_finite()
                || !(0.0..=1.0).contains(&proposal.confidence)
        }) {
            return Err(EffectsValidationError::InvalidProposal("memory"));
        }
        if self.relationship_proposals.iter().any(|proposal| {
            proposal.dimension.trim().is_empty()
                || proposal.dimension.len() > 64
                || proposal.reason.len() > 500
                || !proposal.delta.is_finite()
                || !(-1.0..=1.0).contains(&proposal.delta)
        }) {
            return Err(EffectsValidationError::InvalidProposal("relationship"));
        }
        self.animation_cues
            .retain(|cue| !cue.trim().is_empty() && cue.len() <= 64);
        self.action_proposals.retain(|proposal| {
            allowed_actions
                .iter()
                .any(|value| value == &proposal.action)
                && proposal.arguments.len() <= 16
                && proposal
                    .arguments
                    .iter()
                    .all(|(key, value)| key.len() <= 64 && value.len() <= 1_000)
        });
        Ok(self)
    }
}

fn validate_optional_range(
    name: &'static str,
    value: Option<f32>,
    minimum: f32,
    maximum: f32,
) -> Result<(), EffectsValidationError> {
    if value.is_some_and(|value| !value.is_finite() || !(minimum..=maximum).contains(&value)) {
        return Err(EffectsValidationError::OutOfRange(name));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub content: String,
    pub confidence: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationshipProposal {
    pub dimension: String,
    pub delta: f32,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionProposal {
    pub action: String,
    pub arguments: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EffectsValidationError {
    #[error("effect value is outside its accepted range: {0}")]
    OutOfRange(&'static str),
    #[error("effect contains an invalid {0} proposal")]
    InvalidProposal(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    Audio,
    Subtitle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeliveredSentence {
    pub sentence_id: u64,
    pub text: String,
    pub delivery: DeliveryMode,
    pub audible_frames: u64,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackReceipt {
    pub audible_frames: u64,
    pub duration: Duration,
    pub completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Degradation {
    IdentityToExplicitSelection,
    MemoryWithoutVectorRetrieval,
    NeutralEffects,
    VisualsDisabled,
    AudioToSubtitles,
    MemoryCommitDeferred,
    ProviderFallback { from: String, to: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub identity: TurnIdentity,
    pub lifecycle: TurnLifecycle,
    pub full_response: String,
    pub effects: NpcEffectsV1,
    pub delivered: Vec<DeliveredSentence>,
    pub degradations: Vec<Degradation>,
    pub selected_llm_provider: Option<String>,
    pub selected_tts_providers: Vec<String>,
    pub error: Option<TurnFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    Lifecycle {
        identity: TurnIdentity,
        stage: TurnLifecycle,
    },
    LaneLifecycle {
        identity: TurnIdentity,
        lane: TurnLane,
        stage: LaneLifecycle,
    },
    CharacterResolved {
        identity: TurnIdentity,
        character: CharacterIdentity,
    },
    MemoryReady {
        identity: TurnIdentity,
        context: MemoryContext,
    },
    TextDelta {
        identity: TurnIdentity,
        delta: LlmDelta,
    },
    SentenceReady {
        identity: TurnIdentity,
        sentence_id: u64,
        text: String,
    },
    SpeechStarted {
        identity: TurnIdentity,
        sentence_id: u64,
        provider_id: String,
    },
    SpeechDelivered {
        identity: TurnIdentity,
        sentence: DeliveredSentence,
    },
    EffectsReady {
        identity: TurnIdentity,
        effects: NpcEffectsV1,
    },
    Degraded {
        identity: TurnIdentity,
        degradation: Degradation,
        reason: String,
    },
    ProviderRejected {
        identity: TurnIdentity,
        provider_id: String,
        reason: String,
    },
    Timing {
        identity: TurnIdentity,
        span: crate::timing::TimingSpan,
    },
    Terminal {
        outcome: TurnOutcome,
    },
}

impl TurnEvent {
    pub fn identity(&self) -> &TurnIdentity {
        match self {
            Self::Lifecycle { identity, .. }
            | Self::LaneLifecycle { identity, .. }
            | Self::CharacterResolved { identity, .. }
            | Self::MemoryReady { identity, .. }
            | Self::TextDelta { identity, .. }
            | Self::SentenceReady { identity, .. }
            | Self::SpeechStarted { identity, .. }
            | Self::SpeechDelivered { identity, .. }
            | Self::EffectsReady { identity, .. }
            | Self::Degraded { identity, .. }
            | Self::ProviderRejected { identity, .. }
            | Self::Timing { identity, .. } => identity,
            Self::Terminal { outcome } => &outcome.identity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_reject_invalid_numeric_proposals_and_filter_actions() {
        let invalid = NpcEffectsV1 {
            intensity: Some(1.2),
            ..Default::default()
        };
        assert_eq!(
            invalid.validate(&[]),
            Err(EffectsValidationError::OutOfRange("intensity"))
        );

        let filtered = NpcEffectsV1 {
            action_proposals: vec![ActionProposal {
                action: "unsafe".into(),
                arguments: BTreeMap::new(),
            }],
            ..Default::default()
        }
        .validate(&["wave".into()])
        .unwrap();
        assert!(filtered.action_proposals.is_empty());
    }
}
