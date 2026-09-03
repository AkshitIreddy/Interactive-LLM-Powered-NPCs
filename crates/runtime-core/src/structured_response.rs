//! Versioned, provider-neutral NPC response and delivered-span contracts.
//!
//! The spoken response is the required lane. Every other field is a proposal:
//! malformed emotion, voice, animation, memory, interruption, or action data is
//! neutralized by [`ProviderResponseAdapter`] without delaying or discarding valid
//! speech. Callers that require an all-or-nothing contract can use
//! [`NpcResponseEnvelopeV1::from_json_strict`].

use std::{collections::BTreeSet, ops::Range, time::Duration};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    sentence::{SentenceSegmenter, SentenceSegmenterConfig, SentenceSpan},
    types::{ActionProposal, MemoryProposal, NpcEffectsV1, RelationshipProposal},
};

pub const NPC_RESPONSE_SCHEMA_V1: &str = "npc_response.v1";
pub const MAX_PROVIDER_RESPONSE_BYTES: usize = 65_536;
pub const MAX_SPOKEN_RESPONSE_BYTES: usize = 16_384;

const TOP_LEVEL_FIELDS: &[&str] = &[
    "schema_version",
    "spoken_response",
    "emotion",
    "voice_style",
    "animation_cues",
    "memory_proposals",
    "interruption_behavior",
    "actions",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpcResponseSchemaVersion {
    #[default]
    #[serde(rename = "npc_response.v1")]
    V1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpokenResponseV1 {
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmotionKindV1 {
    #[default]
    Neutral,
    Joy,
    Sadness,
    Anger,
    Fear,
    Surprise,
    Disgust,
    Contempt,
    Concern,
    Amusement,
}

impl EmotionKindV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Joy => "joy",
            Self::Sadness => "sadness",
            Self::Anger => "anger",
            Self::Fear => "fear",
            Self::Surprise => "surprise",
            Self::Disgust => "disgust",
            Self::Contempt => "contempt",
            Self::Concern => "concern",
            Self::Amusement => "amusement",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmotionStateV1 {
    pub kind: EmotionKindV1,
    pub valence: f32,
    pub arousal: f32,
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStyleKindV1 {
    #[default]
    Neutral,
    Warm,
    Tense,
    Somber,
    Excited,
    Whisper,
    Shout,
}

impl VoiceStyleKindV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Warm => "warm",
            Self::Tense => "tense",
            Self::Somber => "somber",
            Self::Excited => "excited",
            Self::Whisper => "whisper",
            Self::Shout => "shout",
        }
    }
}

/// Provider-neutral prosody. Provider-specific voice IDs and model parameters do
/// not belong in model output and are resolved after validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceStyleV1 {
    pub kind: VoiceStyleKindV1,
    pub speaking_rate: f32,
    pub pitch_semitones: f32,
    pub energy: f32,
}

impl Default for VoiceStyleV1 {
    fn default() -> Self {
        Self {
            kind: VoiceStyleKindV1::Neutral,
            speaking_rate: 1.0,
            pitch_semitones: 0.0,
            energy: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationCueKindV1 {
    Nod,
    ShakeHead,
    LookAtPlayer,
    LookAway,
    GestureOpen,
    GesturePoint,
    IdleShift,
    VisemeStream,
}

impl AnimationCueKindV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Nod => "nod",
            Self::ShakeHead => "shake_head",
            Self::LookAtPlayer => "look_at_player",
            Self::LookAway => "look_away",
            Self::GestureOpen => "gesture_open",
            Self::GesturePoint => "gesture_point",
            Self::IdleShift => "idle_shift",
            Self::VisemeStream => "viseme_stream",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationCueV1 {
    pub kind: AnimationCueKindV1,
    pub start_offset_ms: u64,
    pub duration_ms: u64,
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalKindV1 {
    Episodic,
    Fact,
    Preference,
    QuestState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProposalV1 {
    pub kind: MemoryProposalKindV1,
    pub content: String,
    pub importance: f32,
    #[serde(default)]
    pub evidence_turn_ids: Vec<String>,
    pub expires_after_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionBehaviorV1 {
    #[default]
    BargeInAllowed,
    FinishClause,
    Uninterruptible,
}

impl InterruptionBehaviorV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::BargeInAllowed => "barge_in_allowed",
            Self::FinishClause => "finish_clause",
            Self::Uninterruptible => "uninterruptible",
        }
    }
}

/// An inert proposal. It is never a command line, script, file path, provider
/// request, or game integration symbol, and it remains blocked until an external
/// policy allowlists its stable ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalActionV1 {
    pub action_id: String,
    #[serde(default)]
    pub arguments: std::collections::BTreeMap<String, String>,
    pub rationale: String,
    #[serde(default)]
    pub requires_confirmation: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcResponseEnvelopeV1 {
    pub schema_version: NpcResponseSchemaVersion,
    pub spoken_response: SpokenResponseV1,
    #[serde(default)]
    pub emotion: Option<EmotionStateV1>,
    #[serde(default)]
    pub voice_style: VoiceStyleV1,
    #[serde(default)]
    pub animation_cues: Vec<AnimationCueV1>,
    #[serde(default)]
    pub memory_proposals: Vec<MemoryProposalV1>,
    #[serde(default)]
    pub interruption_behavior: InterruptionBehaviorV1,
    #[serde(default)]
    pub actions: Vec<OptionalActionV1>,
}

impl NpcResponseEnvelopeV1 {
    pub fn plain_text(text: impl Into<String>) -> Self {
        Self {
            schema_version: NpcResponseSchemaVersion::V1,
            spoken_response: SpokenResponseV1 { text: text.into() },
            emotion: None,
            voice_style: VoiceStyleV1::default(),
            animation_cues: Vec::new(),
            memory_proposals: Vec::new(),
            interruption_behavior: InterruptionBehaviorV1::BargeInAllowed,
            actions: Vec::new(),
        }
    }

    pub fn from_json_strict(
        raw: &str,
        policy: &ResponseValidationPolicy,
    ) -> Result<Self, ResponseValidationError> {
        validate_provider_size(raw)?;
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| ResponseValidationError::MalformedProviderOutput(error.to_string()))?;
        let object = value
            .as_object()
            .ok_or(ResponseValidationError::ExpectedObject)?;
        if let Some(field) = unsupported_top_level_fields(object).next() {
            return Err(ResponseValidationError::UnsupportedField(field.to_owned()));
        }
        let response: Self = serde_json::from_value(value)
            .map_err(|error| ResponseValidationError::MalformedProviderOutput(error.to_string()))?;
        response.validate(policy)?;
        Ok(response)
    }

    pub fn validate(
        &self,
        policy: &ResponseValidationPolicy,
    ) -> Result<(), ResponseValidationError> {
        validate_spoken(&self.spoken_response, policy)?;
        if let Some(emotion) = &self.emotion {
            validate_emotion(emotion)?;
        }
        validate_voice_style(&self.voice_style)?;
        validate_animation(&self.animation_cues, policy)?;
        validate_memory(&self.memory_proposals, policy)?;
        validate_interruption(self.interruption_behavior, policy)?;
        validate_actions(&self.actions, policy)?;
        Ok(())
    }

    /// Lossless for spoken text and conservative for legacy stringly-typed
    /// effects. New runtimes should consume this envelope directly.
    pub fn to_legacy_effects(&self) -> NpcEffectsV1 {
        NpcEffectsV1 {
            emotion: self
                .emotion
                .as_ref()
                .map(|value| value.kind.as_str().into()),
            valence: self.emotion.as_ref().map(|value| value.valence),
            arousal: self.emotion.as_ref().map(|value| value.arousal),
            intensity: self.emotion.as_ref().map(|value| value.intensity),
            voice_style: Some(self.voice_style.kind.as_str().into()),
            animation_cues: self
                .animation_cues
                .iter()
                .map(|cue| cue.kind.as_str().to_owned())
                .collect(),
            interruption_policy: Some(self.interruption_behavior.as_str().into()),
            memory_proposals: self
                .memory_proposals
                .iter()
                .map(|proposal| MemoryProposal {
                    content: proposal.content.clone(),
                    confidence: proposal.importance,
                })
                .collect(),
            relationship_proposals: Vec::<RelationshipProposal>::new(),
            action_proposals: self
                .actions
                .iter()
                .map(|action| ActionProposal {
                    action: action.action_id.clone(),
                    arguments: action.arguments.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResponseValidationPolicy {
    pub max_spoken_response_bytes: usize,
    pub max_animation_cues: usize,
    pub max_animation_duration_ms: u64,
    pub max_memory_proposals: usize,
    pub max_memory_text_bytes: usize,
    pub max_evidence_turn_ids: usize,
    pub max_actions: usize,
    pub max_action_arguments: usize,
    pub max_action_value_bytes: usize,
    pub max_action_rationale_bytes: usize,
    /// Explicit product/profile permission. This is false by default so model
    /// output cannot disable push-to-talk barge-in by itself.
    pub allow_uninterruptible: bool,
    pub allowed_animation_cues: BTreeSet<AnimationCueKindV1>,
    pub allowed_action_ids: BTreeSet<String>,
}

impl Default for ResponseValidationPolicy {
    fn default() -> Self {
        Self {
            max_spoken_response_bytes: MAX_SPOKEN_RESPONSE_BYTES,
            max_animation_cues: 32,
            max_animation_duration_ms: 60_000,
            max_memory_proposals: 16,
            max_memory_text_bytes: 2_000,
            max_evidence_turn_ids: 32,
            max_actions: 8,
            max_action_arguments: 16,
            max_action_value_bytes: 1_000,
            max_action_rationale_bytes: 1_000,
            allow_uninterruptible: false,
            allowed_animation_cues: [
                AnimationCueKindV1::Nod,
                AnimationCueKindV1::ShakeHead,
                AnimationCueKindV1::LookAtPlayer,
                AnimationCueKindV1::LookAway,
                AnimationCueKindV1::GestureOpen,
                AnimationCueKindV1::GesturePoint,
                AnimationCueKindV1::IdleShift,
                AnimationCueKindV1::VisemeStream,
            ]
            .into_iter()
            .collect(),
            // No action can be enabled solely by provider output.
            allowed_action_ids: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResponseValidationError {
    #[error("provider response exceeds {maximum} bytes")]
    ProviderOutputTooLarge { maximum: usize },
    #[error("provider response must be a JSON object")]
    ExpectedObject,
    #[error("provider response is malformed: {0}")]
    MalformedProviderOutput(String),
    #[error("unsupported response field: {0}")]
    UnsupportedField(String),
    #[error("required text is empty: {0}")]
    EmptyText(&'static str),
    #[error("text exceeds its byte limit: {0}")]
    TextTooLarge(&'static str),
    #[error("text contains a disallowed control character: {0}")]
    InvalidText(&'static str),
    #[error("numeric field is outside its accepted finite range: {0}")]
    OutOfRange(&'static str),
    #[error("too many entries: {0}")]
    TooMany(&'static str),
    #[error("animation cue is not allowed: {0:?}")]
    UnsupportedAnimation(AnimationCueKindV1),
    #[error("action is not allowlisted: {0}")]
    UnsupportedAction(String),
    #[error("uninterruptible speech is disabled by policy")]
    UninterruptibleDisabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderResponseFormat {
    StructuredV1,
    PlainTextCompatibility,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderResponseIssue {
    UnsupportedTopLevelField {
        field: String,
    },
    OptionalSubsystemNeutralized {
        subsystem: &'static str,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedNpcResponse {
    pub response: NpcResponseEnvelopeV1,
    pub format: ProviderResponseFormat,
    pub issues: Vec<ProviderResponseIssue>,
}

#[derive(Clone, Debug, Default)]
pub struct ProviderResponseAdapter {
    policy: ResponseValidationPolicy,
}

/// Selected by trusted route metadata before provider bytes are consumed. There
/// is intentionally no auto-detect variant: a structured route can never fall
/// back to speaking its JSON as legacy text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingResponseFormatV1 {
    StructuredV1,
    LegacyPlainText,
}

impl StreamingResponseFormatV1 {
    pub const ROUTE_METADATA_KEY: &'static str = "npc_response_format";

    pub fn from_route_metadata(
        metadata: &std::collections::BTreeMap<String, String>,
    ) -> Result<Option<Self>, StreamingResponseError> {
        match metadata.get(Self::ROUTE_METADATA_KEY).map(String::as_str) {
            Some("structured_v1") => Ok(Some(Self::StructuredV1)),
            Some("legacy_plain_text") => Ok(Some(Self::LegacyPlainText)),
            Some(value) => Err(StreamingResponseError::UnsupportedRouteFormat(
                value.to_owned(),
            )),
            None => Ok(None),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamingResponseUpdateV1 {
    /// Complete spoken sentences that became safe for TTS during this call.
    /// Structured JSON routes always return an empty vector until `finish`.
    pub ready_sentences: Vec<SentenceSpan>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinalizedStreamingResponseV1 {
    pub response: NpcResponseEnvelopeV1,
    pub format: StreamingResponseFormatV1,
    /// Sentences newly released by `finish`. For a structured route this is every
    /// sentence; for legacy text this is only the final buffered fragment.
    pub ready_sentences: Vec<SentenceSpan>,
    /// Every sentence in the canonical spoken response, with ranges relative to
    /// `response.spoken_response.text`.
    pub all_sentences: Vec<SentenceSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamingAdapterState {
    Active,
    Cancelled,
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StreamingResponseError {
    #[error("stale cancellation generation: expected {expected}, received {received}")]
    StaleGeneration { expected: u64, received: u64 },
    #[error("response adapter is no longer accepting provider output")]
    Closed,
    #[error("provider delta sequence did not advance beyond {previous}: {received}")]
    NonMonotonicSequence { previous: u64, received: u64 },
    #[error("unsupported response route format: {0}")]
    UnsupportedRouteFormat(String),
    #[error(transparent)]
    InvalidResponse(#[from] ResponseValidationError),
}

/// Incremental provider boundary used before SentenceReady/TTS.
///
/// Structured JSON is only buffered during `push_delta`; no JSON fragment can
/// escape through `ready_sentences`. At `finish`, the complete buffer must pass
/// the strict v1 parser before its spoken field is segmented. Legacy plain text
/// keeps sentence-level latency, but callers must select that mode explicitly.
#[derive(Clone, Debug)]
pub struct StreamingResponseAdapterV1 {
    format: StreamingResponseFormatV1,
    policy: ResponseValidationPolicy,
    sentence_config: SentenceSegmenterConfig,
    sentence_segmenter: Option<SentenceSegmenter>,
    cancellation_generation: u64,
    state: StreamingAdapterState,
    last_sequence: Option<u64>,
    provider_buffer: String,
    released_sentences: Vec<SentenceSpan>,
}

impl StreamingResponseAdapterV1 {
    pub fn new(
        format: StreamingResponseFormatV1,
        policy: ResponseValidationPolicy,
        sentence_config: SentenceSegmenterConfig,
        cancellation_generation: u64,
    ) -> Self {
        Self {
            format,
            policy,
            sentence_config: sentence_config.clone(),
            sentence_segmenter: (format == StreamingResponseFormatV1::LegacyPlainText)
                .then(|| SentenceSegmenter::new(sentence_config)),
            cancellation_generation,
            state: StreamingAdapterState::Active,
            last_sequence: None,
            provider_buffer: String::new(),
            released_sentences: Vec::new(),
        }
    }

    pub fn format(&self) -> StreamingResponseFormatV1 {
        self.format
    }

    pub fn cancellation_generation(&self) -> u64 {
        self.cancellation_generation
    }

    pub fn push_delta(
        &mut self,
        observed_generation: u64,
        sequence: u64,
        delta: &str,
    ) -> Result<StreamingResponseUpdateV1, StreamingResponseError> {
        self.require_active_generation(observed_generation)?;
        if self
            .last_sequence
            .is_some_and(|previous| sequence <= previous)
        {
            return Err(StreamingResponseError::NonMonotonicSequence {
                previous: self.last_sequence.expect("checked as present"),
                received: sequence,
            });
        }

        let maximum = match self.format {
            StreamingResponseFormatV1::StructuredV1 => MAX_PROVIDER_RESPONSE_BYTES,
            StreamingResponseFormatV1::LegacyPlainText => self
                .policy
                .max_spoken_response_bytes
                .min(MAX_SPOKEN_RESPONSE_BYTES),
        };
        let next_size = self
            .provider_buffer
            .len()
            .checked_add(delta.len())
            .ok_or(ResponseValidationError::ProviderOutputTooLarge { maximum })?;
        if next_size > maximum {
            return Err(ResponseValidationError::ProviderOutputTooLarge { maximum }.into());
        }
        if self.format == StreamingResponseFormatV1::LegacyPlainText
            && delta
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        {
            return Err(ResponseValidationError::InvalidText("spoken response").into());
        }

        self.last_sequence = Some(sequence);
        self.provider_buffer.push_str(delta);
        let ready_sentences = if let Some(segmenter) = self.sentence_segmenter.as_mut() {
            let sentences = segmenter.push_spans(delta);
            self.released_sentences.extend(sentences.iter().cloned());
            sentences
        } else {
            Vec::new()
        };
        Ok(StreamingResponseUpdateV1 { ready_sentences })
    }

    pub fn finish(
        &mut self,
        observed_generation: u64,
    ) -> Result<FinalizedStreamingResponseV1, StreamingResponseError> {
        self.require_active_generation(observed_generation)?;

        let (response, ready_sentences, all_sentences) = match self.format {
            StreamingResponseFormatV1::StructuredV1 => {
                let response =
                    NpcResponseEnvelopeV1::from_json_strict(&self.provider_buffer, &self.policy)?;
                let mut segmenter = SentenceSegmenter::new(self.sentence_config.clone());
                let mut sentences = segmenter.push_spans(&response.spoken_response.text);
                sentences.extend(segmenter.finish_spans());
                (response, sentences.clone(), sentences)
            }
            StreamingResponseFormatV1::LegacyPlainText => {
                let response = NpcResponseEnvelopeV1::plain_text(self.provider_buffer.clone());
                response.validate(&self.policy)?;
                let segmenter = self
                    .sentence_segmenter
                    .as_mut()
                    .expect("legacy routes own a sentence segmenter");
                let ready = segmenter.finish_spans();
                self.released_sentences.extend(ready.iter().cloned());
                (response, ready, self.released_sentences.clone())
            }
        };
        debug_assert!(all_sentences.iter().all(|sentence| {
            response
                .spoken_response
                .text
                .get(sentence.text_start_bytes..sentence.text_end_bytes)
                == Some(sentence.text.as_str())
        }));
        self.state = StreamingAdapterState::Finalized;
        Ok(FinalizedStreamingResponseV1 {
            response,
            format: self.format,
            ready_sentences,
            all_sentences,
        })
    }

    pub fn cancel(&mut self, observed_generation: u64) -> Result<(), StreamingResponseError> {
        self.require_generation(observed_generation)?;
        match self.state {
            StreamingAdapterState::Active => {
                self.state = StreamingAdapterState::Cancelled;
                self.provider_buffer.clear();
                self.sentence_segmenter = None;
                Ok(())
            }
            StreamingAdapterState::Cancelled => Ok(()),
            StreamingAdapterState::Finalized => Err(StreamingResponseError::Closed),
        }
    }

    fn require_active_generation(
        &self,
        observed_generation: u64,
    ) -> Result<(), StreamingResponseError> {
        self.require_generation(observed_generation)?;
        if self.state == StreamingAdapterState::Active {
            Ok(())
        } else {
            Err(StreamingResponseError::Closed)
        }
    }

    fn require_generation(&self, observed_generation: u64) -> Result<(), StreamingResponseError> {
        if observed_generation == self.cancellation_generation {
            Ok(())
        } else {
            Err(StreamingResponseError::StaleGeneration {
                expected: self.cancellation_generation,
                received: observed_generation,
            })
        }
    }
}

impl ProviderResponseAdapter {
    pub fn new(policy: ResponseValidationPolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &ResponseValidationPolicy {
        &self.policy
    }

    /// Decodes structured JSON or safely wraps a legacy plain-text response.
    /// JSON-looking malformed output is never spoken as raw syntax. Only valid
    /// required speech can survive failure in optional proposal fields.
    pub fn decode(&self, raw: &str) -> Result<DecodedNpcResponse, ResponseValidationError> {
        validate_provider_size(raw)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ResponseValidationError::EmptyText("provider response"));
        }
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            self.decode_structured_with_isolation(trimmed)
        } else {
            let response = NpcResponseEnvelopeV1::plain_text(trimmed);
            response.validate(&self.policy)?;
            Ok(DecodedNpcResponse {
                response,
                format: ProviderResponseFormat::PlainTextCompatibility,
                issues: Vec::new(),
            })
        }
    }

    fn decode_structured_with_isolation(
        &self,
        raw: &str,
    ) -> Result<DecodedNpcResponse, ResponseValidationError> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| ResponseValidationError::MalformedProviderOutput(error.to_string()))?;
        let object = value
            .as_object()
            .ok_or(ResponseValidationError::ExpectedObject)?;

        let schema: NpcResponseSchemaVersion = decode_required(object, "schema_version")?;
        let spoken: SpokenResponseV1 = decode_required(object, "spoken_response")?;
        validate_spoken(&spoken, &self.policy)?;

        let mut issues = unsupported_top_level_fields(object)
            .map(|field| ProviderResponseIssue::UnsupportedTopLevelField {
                field: field.to_owned(),
            })
            .collect::<Vec<_>>();
        let mut response = NpcResponseEnvelopeV1 {
            schema_version: schema,
            spoken_response: spoken,
            emotion: None,
            voice_style: VoiceStyleV1::default(),
            animation_cues: Vec::new(),
            memory_proposals: Vec::new(),
            interruption_behavior: InterruptionBehaviorV1::BargeInAllowed,
            actions: Vec::new(),
        };

        response.emotion =
            decode_isolated_optional(object, "emotion", "emotion", &mut issues, |value| {
                validate_emotion(value)
            });
        response.voice_style = decode_isolated_default(
            object,
            "voice_style",
            "voice_style",
            &mut issues,
            VoiceStyleV1::default(),
            validate_voice_style,
        );
        response.animation_cues = decode_isolated_default(
            object,
            "animation_cues",
            "animation",
            &mut issues,
            Vec::new(),
            |value| validate_animation(value, &self.policy),
        );
        response.memory_proposals = decode_isolated_default(
            object,
            "memory_proposals",
            "memory",
            &mut issues,
            Vec::new(),
            |value| validate_memory(value, &self.policy),
        );
        response.interruption_behavior = decode_isolated_default(
            object,
            "interruption_behavior",
            "interruption",
            &mut issues,
            InterruptionBehaviorV1::BargeInAllowed,
            |value| validate_interruption(*value, &self.policy),
        );
        response.actions = decode_isolated_default(
            object,
            "actions",
            "actions",
            &mut issues,
            Vec::new(),
            |value| validate_actions(value, &self.policy),
        );

        // This is an invariant assertion over the isolated result, not a second
        // all-or-nothing parse. Any invalid optional field has already become a
        // neutral value with a diagnostic issue.
        response.validate(&self.policy)?;
        Ok(DecodedNpcResponse {
            response,
            format: ProviderResponseFormat::StructuredV1,
            issues,
        })
    }
}

fn validate_provider_size(raw: &str) -> Result<(), ResponseValidationError> {
    if raw.len() > MAX_PROVIDER_RESPONSE_BYTES {
        Err(ResponseValidationError::ProviderOutputTooLarge {
            maximum: MAX_PROVIDER_RESPONSE_BYTES,
        })
    } else {
        Ok(())
    }
}

fn unsupported_top_level_fields(object: &Map<String, Value>) -> impl Iterator<Item = &str> {
    object
        .keys()
        .map(String::as_str)
        .filter(|field| !TOP_LEVEL_FIELDS.contains(field))
}

fn decode_required<T: DeserializeOwned>(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<T, ResponseValidationError> {
    let value = object.get(field).ok_or_else(|| {
        ResponseValidationError::MalformedProviderOutput(format!("missing field `{field}`"))
    })?;
    serde_json::from_value(value.clone()).map_err(|error| {
        ResponseValidationError::MalformedProviderOutput(format!("{field}: {error}"))
    })
}

fn decode_isolated_optional<T, F>(
    object: &Map<String, Value>,
    field: &'static str,
    subsystem: &'static str,
    issues: &mut Vec<ProviderResponseIssue>,
    validate: F,
) -> Option<T>
where
    T: DeserializeOwned,
    F: FnOnce(&T) -> Result<(), ResponseValidationError>,
{
    let value = object.get(field)?;
    if value.is_null() {
        return None;
    }
    match serde_json::from_value::<T>(value.clone()) {
        Ok(decoded) => match validate(&decoded) {
            Ok(()) => Some(decoded),
            Err(error) => {
                issues.push(optional_issue(subsystem, error));
                None
            }
        },
        Err(error) => {
            issues.push(optional_issue(subsystem, error));
            None
        }
    }
}

fn decode_isolated_default<T, F, E>(
    object: &Map<String, Value>,
    field: &'static str,
    subsystem: &'static str,
    issues: &mut Vec<ProviderResponseIssue>,
    neutral: T,
    validate: F,
) -> T
where
    T: DeserializeOwned,
    F: FnOnce(&T) -> Result<(), E>,
    E: std::fmt::Display,
{
    let Some(value) = object.get(field) else {
        return neutral;
    };
    match serde_json::from_value::<T>(value.clone()) {
        Ok(decoded) => match validate(&decoded) {
            Ok(()) => decoded,
            Err(error) => {
                issues.push(optional_issue(subsystem, error));
                neutral
            }
        },
        Err(error) => {
            issues.push(optional_issue(subsystem, error));
            neutral
        }
    }
}

fn optional_issue(subsystem: &'static str, error: impl std::fmt::Display) -> ProviderResponseIssue {
    ProviderResponseIssue::OptionalSubsystemNeutralized {
        subsystem,
        reason: error.to_string(),
    }
}

fn validate_spoken(
    spoken: &SpokenResponseV1,
    policy: &ResponseValidationPolicy,
) -> Result<(), ResponseValidationError> {
    validate_text(
        &spoken.text,
        policy.max_spoken_response_bytes,
        "spoken response",
    )
}

fn validate_emotion(emotion: &EmotionStateV1) -> Result<(), ResponseValidationError> {
    finite_range(emotion.valence, -1.0, 1.0, "emotion valence")?;
    finite_range(emotion.arousal, 0.0, 1.0, "emotion arousal")?;
    finite_range(emotion.intensity, 0.0, 1.0, "emotion intensity")
}

fn validate_voice_style(style: &VoiceStyleV1) -> Result<(), ResponseValidationError> {
    finite_range(style.speaking_rate, 0.5, 2.0, "voice speaking rate")?;
    finite_range(style.pitch_semitones, -12.0, 12.0, "voice pitch semitones")?;
    finite_range(style.energy, 0.0, 2.0, "voice energy")
}

fn validate_animation(
    cues: &[AnimationCueV1],
    policy: &ResponseValidationPolicy,
) -> Result<(), ResponseValidationError> {
    bounded_count(cues.len(), policy.max_animation_cues, "animation cues")?;
    for cue in cues {
        if !policy.allowed_animation_cues.contains(&cue.kind) {
            return Err(ResponseValidationError::UnsupportedAnimation(cue.kind));
        }
        if cue.duration_ms == 0 || cue.duration_ms > policy.max_animation_duration_ms {
            return Err(ResponseValidationError::OutOfRange("animation duration"));
        }
        finite_range(cue.intensity, 0.0, 1.0, "animation intensity")?;
    }
    Ok(())
}

fn validate_memory(
    proposals: &[MemoryProposalV1],
    policy: &ResponseValidationPolicy,
) -> Result<(), ResponseValidationError> {
    bounded_count(
        proposals.len(),
        policy.max_memory_proposals,
        "memory proposals",
    )?;
    for proposal in proposals {
        validate_text(
            &proposal.content,
            policy.max_memory_text_bytes,
            "memory content",
        )?;
        finite_range(proposal.importance, 0.0, 1.0, "memory importance")?;
        bounded_count(
            proposal.evidence_turn_ids.len(),
            policy.max_evidence_turn_ids,
            "memory evidence turn IDs",
        )?;
        for id in &proposal.evidence_turn_ids {
            validate_text(id, 64, "memory evidence turn ID")?;
        }
    }
    Ok(())
}

fn validate_interruption(
    behavior: InterruptionBehaviorV1,
    policy: &ResponseValidationPolicy,
) -> Result<(), ResponseValidationError> {
    if behavior == InterruptionBehaviorV1::Uninterruptible && !policy.allow_uninterruptible {
        Err(ResponseValidationError::UninterruptibleDisabled)
    } else {
        Ok(())
    }
}

fn validate_actions(
    actions: &[OptionalActionV1],
    policy: &ResponseValidationPolicy,
) -> Result<(), ResponseValidationError> {
    bounded_count(actions.len(), policy.max_actions, "actions")?;
    for action in actions {
        validate_text(&action.action_id, 128, "action ID")?;
        if !policy.allowed_action_ids.contains(&action.action_id) {
            return Err(ResponseValidationError::UnsupportedAction(
                action.action_id.clone(),
            ));
        }
        bounded_count(
            action.arguments.len(),
            policy.max_action_arguments,
            "action arguments",
        )?;
        for (key, value) in &action.arguments {
            validate_text(key, 64, "action argument key")?;
            validate_text(
                value,
                policy.max_action_value_bytes,
                "action argument value",
            )?;
        }
        validate_text(
            &action.rationale,
            policy.max_action_rationale_bytes,
            "action rationale",
        )?;
    }
    Ok(())
}

fn validate_text(
    text: &str,
    maximum_bytes: usize,
    field: &'static str,
) -> Result<(), ResponseValidationError> {
    if text.trim().is_empty() {
        return Err(ResponseValidationError::EmptyText(field));
    }
    if text.len() > maximum_bytes {
        return Err(ResponseValidationError::TextTooLarge(field));
    }
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(ResponseValidationError::InvalidText(field));
    }
    Ok(())
}

fn finite_range(
    value: f32,
    minimum: f32,
    maximum: f32,
    field: &'static str,
) -> Result<(), ResponseValidationError> {
    if value.is_finite() && (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(ResponseValidationError::OutOfRange(field))
    }
}

fn bounded_count(
    actual: usize,
    maximum: usize,
    field: &'static str,
) -> Result<(), ResponseValidationError> {
    if actual <= maximum {
        Ok(())
    } else {
        Err(ResponseValidationError::TooMany(field))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveredSurfaceV1 {
    Audio,
    Subtitle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeliveryEvidenceV1 {
    pub cancellation_generation: u64,
    pub sentence_id: u64,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub surface: DeliveredSurfaceV1,
    pub audible_frames: u64,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommittedSpanV1 {
    pub sentence_id: u64,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub text: String,
    pub surface: DeliveredSurfaceV1,
    pub audible_frames: u64,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeliveredSpanCommitV1 {
    pub schema_version: NpcResponseSchemaVersion,
    pub cancellation_generation: u64,
    pub cancelled: bool,
    pub spans: Vec<CommittedSpanV1>,
}

impl DeliveredSpanCommitV1 {
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn fragments(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().map(|span| span.text.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryTrackerState {
    Active,
    Cancelled,
    Finalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryRecordOutcome {
    Recorded,
    DuplicateIgnored,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeliveryCommitError {
    #[error("stale cancellation generation: expected {expected}, received {received}")]
    StaleGeneration { expected: u64, received: u64 },
    #[error("delivery tracker is no longer accepting evidence")]
    Closed,
    #[error("delivered text range is empty or outside the spoken response")]
    InvalidRange,
    #[error("delivered text range is not aligned to UTF-8 boundaries")]
    InvalidUtf8Boundary,
    #[error("partial delivered text must end at a word or punctuation boundary")]
    InvalidSemanticBoundary,
    #[error("audio evidence must report non-zero audible frames and duration")]
    EmptyAudioEvidence,
    #[error("delivered spans overlap or arrive out of source order")]
    OverlappingOrOutOfOrder,
}

/// Generation-bound authority for durable dialogue. It records exact source
/// ranges acknowledged by a visible or audible surface and retains those ranges
/// across cancellation; queued or late work cannot become memory.
#[derive(Clone, Debug)]
pub struct DeliveredSpanTracker {
    cancellation_generation: u64,
    spoken_response: String,
    state: DeliveryTrackerState,
    spans: Vec<CommittedSpanV1>,
}

impl DeliveredSpanTracker {
    pub fn new(
        cancellation_generation: u64,
        spoken_response: impl Into<String>,
    ) -> Result<Self, ResponseValidationError> {
        let spoken_response = spoken_response.into();
        validate_text(
            &spoken_response,
            MAX_SPOKEN_RESPONSE_BYTES,
            "spoken response",
        )?;
        Ok(Self {
            cancellation_generation,
            spoken_response,
            state: DeliveryTrackerState::Active,
            spans: Vec::new(),
        })
    }

    pub fn cancellation_generation(&self) -> u64 {
        self.cancellation_generation
    }

    pub fn record(
        &mut self,
        evidence: DeliveryEvidenceV1,
    ) -> Result<DeliveryRecordOutcome, DeliveryCommitError> {
        self.require_generation(evidence.cancellation_generation)?;
        if self.state != DeliveryTrackerState::Active {
            return Err(DeliveryCommitError::Closed);
        }
        let range = evidence.text_start_bytes..evidence.text_end_bytes;
        self.validate_range(&range)?;
        if evidence.surface == DeliveredSurfaceV1::Audio
            && (evidence.audible_frames == 0 || evidence.duration.is_zero())
        {
            return Err(DeliveryCommitError::EmptyAudioEvidence);
        }

        if let Some(existing) = self.spans.iter().find(|span| {
            span.text_start_bytes == range.start
                && span.text_end_bytes == range.end
                && span.surface == evidence.surface
        }) {
            let same_receipt = existing.audible_frames == evidence.audible_frames
                && existing.duration == evidence.duration
                && existing.sentence_id == evidence.sentence_id;
            return if same_receipt {
                Ok(DeliveryRecordOutcome::DuplicateIgnored)
            } else {
                Err(DeliveryCommitError::OverlappingOrOutOfOrder)
            };
        }
        if self.spans.last().is_some_and(|last| {
            range.start < last.text_end_bytes || range.end <= last.text_end_bytes
        }) {
            return Err(DeliveryCommitError::OverlappingOrOutOfOrder);
        }

        self.spans.push(CommittedSpanV1 {
            sentence_id: evidence.sentence_id,
            text_start_bytes: range.start,
            text_end_bytes: range.end,
            text: self.spoken_response[range].to_owned(),
            surface: evidence.surface,
            audible_frames: evidence.audible_frames,
            duration: evidence.duration,
        });
        Ok(DeliveryRecordOutcome::Recorded)
    }

    pub fn cancel(&mut self, observed_generation: u64) -> Result<(), DeliveryCommitError> {
        self.require_generation(observed_generation)?;
        if self.state == DeliveryTrackerState::Active {
            self.state = DeliveryTrackerState::Cancelled;
        }
        Ok(())
    }

    pub fn finalize(&mut self, observed_generation: u64) -> Result<(), DeliveryCommitError> {
        self.require_generation(observed_generation)?;
        if self.state == DeliveryTrackerState::Active {
            self.state = DeliveryTrackerState::Finalized;
        }
        Ok(())
    }

    pub fn commit(&self) -> DeliveredSpanCommitV1 {
        DeliveredSpanCommitV1 {
            schema_version: NpcResponseSchemaVersion::V1,
            cancellation_generation: self.cancellation_generation,
            cancelled: self.state == DeliveryTrackerState::Cancelled,
            spans: self.spans.clone(),
        }
    }

    fn require_generation(&self, received: u64) -> Result<(), DeliveryCommitError> {
        if received == self.cancellation_generation {
            Ok(())
        } else {
            Err(DeliveryCommitError::StaleGeneration {
                expected: self.cancellation_generation,
                received,
            })
        }
    }

    fn validate_range(&self, range: &Range<usize>) -> Result<(), DeliveryCommitError> {
        if range.start >= range.end || range.end > self.spoken_response.len() {
            return Err(DeliveryCommitError::InvalidRange);
        }
        if !self.spoken_response.is_char_boundary(range.start)
            || !self.spoken_response.is_char_boundary(range.end)
        {
            return Err(DeliveryCommitError::InvalidUtf8Boundary);
        }
        if !is_semantic_boundary(&self.spoken_response, range.start)
            || !is_semantic_boundary(&self.spoken_response, range.end)
        {
            return Err(DeliveryCommitError::InvalidSemanticBoundary);
        }
        Ok(())
    }
}

fn is_semantic_boundary(text: &str, offset: usize) -> bool {
    if offset == 0 || offset == text.len() {
        return true;
    }
    let previous = text[..offset].chars().next_back();
    let next = text[offset..].chars().next();
    previous.is_some_and(is_word_boundary_char) || next.is_some_and(is_word_boundary_char)
}

fn is_word_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || ch.is_ascii_punctuation() || matches!(ch, '–' | '—' | '…')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CancellationGeneration;
    use proptest::prelude::*;

    fn valid_json() -> &'static str {
        r#"{
            "schema_version":"npc_response.v1",
            "spoken_response":{"text":"The west gate is still open."},
            "emotion":{"kind":"concern","valence":-0.2,"arousal":0.7,"intensity":0.6},
            "voice_style":{"kind":"tense","speaking_rate":1.1,"pitch_semitones":0.0,"energy":1.2},
            "animation_cues":[{"kind":"look_at_player","start_offset_ms":0,"duration_ms":800,"intensity":0.5}],
            "memory_proposals":[{"kind":"fact","content":"The west gate is open.","importance":0.7,"evidence_turn_ids":["turn-7"],"expires_after_ms":null}],
            "interruption_behavior":"finish_clause",
            "actions":[]
        }"#
    }

    #[test]
    fn strict_schema_accepts_valid_response_and_rejects_unsupported_fields() {
        let response = NpcResponseEnvelopeV1::from_json_strict(
            valid_json(),
            &ResponseValidationPolicy::default(),
        )
        .expect("valid response");
        assert_eq!(
            response.spoken_response.text,
            "The west gate is still open."
        );
        assert_eq!(
            response.emotion.expect("emotion").kind,
            EmotionKindV1::Concern
        );

        let unsupported = valid_json().replace(
            "\"actions\":[]",
            "\"actions\":[],\"provider_model\":\"secret\"",
        );
        assert_eq!(
            NpcResponseEnvelopeV1::from_json_strict(
                &unsupported,
                &ResponseValidationPolicy::default()
            ),
            Err(ResponseValidationError::UnsupportedField(
                "provider_model".into()
            ))
        );
    }

    #[test]
    fn malformed_optional_emotion_and_animation_are_isolated_from_speech() {
        let raw = r#"{
            "schema_version":"npc_response.v1",
            "spoken_response":{"text":"I can still answer safely."},
            "emotion":{"kind":"joy","valence":0.5,"arousal":0.5,"intensity":9.0},
            "animation_cues":[{"kind":"nod","start_offset_ms":0,"duration_ms":0,"intensity":0.5}],
            "unknown_future_field":{"ignored":true}
        }"#;
        let decoded = ProviderResponseAdapter::default()
            .decode(raw)
            .expect("required speech remains valid");
        assert_eq!(
            decoded.response.spoken_response.text,
            "I can still answer safely."
        );
        assert!(decoded.response.emotion.is_none());
        assert!(decoded.response.animation_cues.is_empty());
        assert!(decoded.issues.iter().any(|issue| matches!(
            issue,
            ProviderResponseIssue::OptionalSubsystemNeutralized {
                subsystem: "emotion",
                ..
            }
        )));
        assert!(decoded.issues.iter().any(|issue| matches!(
            issue,
            ProviderResponseIssue::OptionalSubsystemNeutralized {
                subsystem: "animation",
                ..
            }
        )));
        assert!(decoded.issues.iter().any(|issue| matches!(
            issue,
            ProviderResponseIssue::UnsupportedTopLevelField { field } if field == "unknown_future_field"
        )));
    }

    #[test]
    fn malformed_json_is_not_spoken_as_plain_text() {
        let error = ProviderResponseAdapter::default()
            .decode(r#"{"schema_version":"npc_response.v1","spoken_response": "#)
            .expect_err("JSON-looking syntax must fail closed");
        assert!(matches!(
            error,
            ResponseValidationError::MalformedProviderOutput(_)
        ));
    }

    #[test]
    fn plain_text_provider_compatibility_is_neutral_and_bounded() {
        let decoded = ProviderResponseAdapter::default()
            .decode("  Plain providers continue to work.  ")
            .expect("plain text adapter");
        assert_eq!(
            decoded.format,
            ProviderResponseFormat::PlainTextCompatibility
        );
        assert_eq!(
            decoded.response.spoken_response.text,
            "Plain providers continue to work."
        );
        assert!(decoded.response.emotion.is_none());
        assert!(decoded.response.animation_cues.is_empty());
        assert!(decoded.response.actions.is_empty());
    }

    #[test]
    fn unsupported_actions_are_neutralized_without_losing_speech() {
        let raw = r#"{
            "schema_version":"npc_response.v1",
            "spoken_response":{"text":"I will explain, not execute."},
            "actions":[{"action_id":"run_shell","arguments":{"command":"bad"},"rationale":"no","requires_confirmation":false}]
        }"#;
        let decoded = ProviderResponseAdapter::default().decode(raw).unwrap();
        assert!(decoded.response.actions.is_empty());
        assert!(decoded.issues.iter().any(|issue| matches!(
            issue,
            ProviderResponseIssue::OptionalSubsystemNeutralized {
                subsystem: "actions",
                ..
            }
        )));
    }

    #[test]
    fn structured_stream_releases_no_partial_json_and_finalizes_exact_spoken_spans() {
        let generation = 17;
        let mut adapter = StreamingResponseAdapterV1::new(
            StreamingResponseFormatV1::StructuredV1,
            ResponseValidationPolicy::default(),
            SentenceSegmenterConfig::default(),
            generation,
        );
        let raw = valid_json();
        let split = raw.find("west gate").expect("test phrase") + 4;
        assert!(adapter
            .push_delta(generation, 1, &raw[..split])
            .unwrap()
            .ready_sentences
            .is_empty());
        assert!(adapter
            .push_delta(generation, 2, &raw[split..])
            .unwrap()
            .ready_sentences
            .is_empty());

        let finalized = adapter.finish(generation).expect("strict final JSON");
        assert_eq!(finalized.format, StreamingResponseFormatV1::StructuredV1);
        assert!(!finalized.ready_sentences.is_empty());
        for sentence in &finalized.all_sentences {
            assert_eq!(
                finalized
                    .response
                    .spoken_response
                    .text
                    .get(sentence.text_start_bytes..sentence.text_end_bytes),
                Some(sentence.text.as_str())
            );
        }
    }

    #[test]
    fn structured_route_never_falls_back_to_plain_text() {
        let mut adapter = StreamingResponseAdapterV1::new(
            StreamingResponseFormatV1::StructuredV1,
            ResponseValidationPolicy::default(),
            SentenceSegmenterConfig::default(),
            8,
        );
        assert!(adapter
            .push_delta(8, 1, "This is plain provider text, not an envelope.")
            .unwrap()
            .ready_sentences
            .is_empty());
        assert!(matches!(
            adapter.finish(8),
            Err(StreamingResponseError::InvalidResponse(
                ResponseValidationError::MalformedProviderOutput(_)
            ))
        ));
    }

    #[test]
    fn explicitly_selected_legacy_route_streams_sentences_with_exact_utf8_ranges() {
        let source = "  Meet me beside the 界 gate. Bring the map when you come.";
        let mut adapter = StreamingResponseAdapterV1::new(
            StreamingResponseFormatV1::LegacyPlainText,
            ResponseValidationPolicy::default(),
            SentenceSegmenterConfig::default(),
            22,
        );
        assert!(adapter
            .push_delta(22, 40, "  Meet me beside the 界 gate.")
            .unwrap()
            .ready_sentences
            .is_empty());
        let released = adapter
            .push_delta(22, 41, " Bring the map when you come.")
            .unwrap();
        assert_eq!(released.ready_sentences.len(), 1);
        let finalized = adapter.finish(22).unwrap();
        assert_eq!(finalized.response.spoken_response.text, source);
        assert_eq!(finalized.all_sentences.len(), 2);
        for sentence in &finalized.all_sentences {
            assert_eq!(
                &source[sentence.text_start_bytes..sentence.text_end_bytes],
                sentence.text
            );
        }
    }

    #[test]
    fn cancelled_or_stale_generations_cannot_release_late_sentences() {
        let mut adapter = StreamingResponseAdapterV1::new(
            StreamingResponseFormatV1::LegacyPlainText,
            ResponseValidationPolicy::default(),
            SentenceSegmenterConfig::default(),
            4,
        );
        assert_eq!(
            adapter.push_delta(3, 1, "Stale output must not be buffered."),
            Err(StreamingResponseError::StaleGeneration {
                expected: 4,
                received: 3,
            })
        );
        adapter
            .push_delta(4, 1, "A current sentence is still pending.")
            .unwrap();
        adapter.cancel(4).unwrap();
        assert_eq!(
            adapter.push_delta(4, 2, " This is late."),
            Err(StreamingResponseError::Closed)
        );
        assert_eq!(adapter.finish(4), Err(StreamingResponseError::Closed));
        assert_eq!(adapter.cancel(4), Ok(()), "cancellation is idempotent");
    }

    #[test]
    fn provider_sequences_must_advance_before_buffer_mutation() {
        let mut adapter = StreamingResponseAdapterV1::new(
            StreamingResponseFormatV1::LegacyPlainText,
            ResponseValidationPolicy::default(),
            SentenceSegmenterConfig::default(),
            1,
        );
        adapter.push_delta(1, 9, "First safe sentence. ").unwrap();
        assert_eq!(
            adapter.push_delta(1, 9, "Duplicate text."),
            Err(StreamingResponseError::NonMonotonicSequence {
                previous: 9,
                received: 9,
            })
        );
        let finalized = adapter.finish(1).unwrap();
        assert!(!finalized
            .response
            .spoken_response
            .text
            .contains("Duplicate"));
    }

    #[test]
    fn partial_delivery_commits_only_exact_acknowledged_span_after_cancellation() {
        let generations = CancellationGeneration::default();
        let token = generations.next();
        let response = "Meet me at the west gate. Do not follow the road.";
        let delivered_end = "Meet me at the west gate.".len();
        let mut tracker = DeliveredSpanTracker::new(token.generation(), response).unwrap();
        tracker
            .record(DeliveryEvidenceV1 {
                cancellation_generation: token.generation(),
                sentence_id: 1,
                text_start_bytes: 0,
                text_end_bytes: delivered_end,
                surface: DeliveredSurfaceV1::Audio,
                audible_frames: 24_000,
                duration: Duration::from_secs(1),
            })
            .unwrap();
        token.cancel();
        tracker.cancel(token.generation()).unwrap();
        assert_eq!(
            tracker.record(DeliveryEvidenceV1 {
                cancellation_generation: token.generation(),
                sentence_id: 2,
                text_start_bytes: delivered_end + 1,
                text_end_bytes: response.len(),
                surface: DeliveredSurfaceV1::Subtitle,
                audible_frames: 0,
                duration: Duration::ZERO,
            }),
            Err(DeliveryCommitError::Closed)
        );
        let commit = tracker.commit();
        assert!(commit.cancelled);
        assert_eq!(
            commit.fragments().collect::<Vec<_>>(),
            vec!["Meet me at the west gate."]
        );
    }

    #[test]
    fn stale_generation_cannot_commit_late_delivery() {
        let generations = CancellationGeneration::default();
        let first = generations.next();
        let second = generations.next();
        let mut tracker =
            DeliveredSpanTracker::new(second.generation(), "Current response.").unwrap();
        assert_eq!(
            tracker.record(DeliveryEvidenceV1 {
                cancellation_generation: first.generation(),
                sentence_id: 1,
                text_start_bytes: 0,
                text_end_bytes: "Current response.".len(),
                surface: DeliveredSurfaceV1::Subtitle,
                audible_frames: 0,
                duration: Duration::ZERO,
            }),
            Err(DeliveryCommitError::StaleGeneration {
                expected: second.generation(),
                received: first.generation(),
            })
        );
        assert!(tracker.commit().is_empty());
    }

    proptest! {
        #[test]
        fn plain_text_round_trips_without_creating_effects(words in prop::collection::vec("[A-Za-z0-9]{1,12}", 1..80)) {
            let text = words.join(" ");
            prop_assume!(text.len() <= MAX_SPOKEN_RESPONSE_BYTES);
            let decoded = ProviderResponseAdapter::default().decode(&text).unwrap();
            prop_assert_eq!(decoded.response.spoken_response.text, text);
            prop_assert!(decoded.response.emotion.is_none());
            prop_assert!(decoded.response.animation_cues.is_empty());
            prop_assert!(decoded.response.memory_proposals.is_empty());
            prop_assert!(decoded.response.actions.is_empty());
        }

        #[test]
        fn arbitrary_delivery_offsets_never_slice_invalid_utf8(start in 0usize..32, end in 0usize..32) {
            let text = "Guard the 界 gate tonight.";
            let mut tracker = DeliveredSpanTracker::new(3, text).unwrap();
            let result = tracker.record(DeliveryEvidenceV1 {
                cancellation_generation: 3,
                sentence_id: 1,
                text_start_bytes: start,
                text_end_bytes: end,
                surface: DeliveredSurfaceV1::Subtitle,
                audible_frames: 0,
                duration: Duration::ZERO,
            });
            if result.is_ok() {
                let commit = tracker.commit();
                prop_assert_eq!(commit.spans.len(), 1);
                prop_assert_eq!(commit.spans[0].text.as_str(), &text[start..end]);
            }
        }
    }
}
