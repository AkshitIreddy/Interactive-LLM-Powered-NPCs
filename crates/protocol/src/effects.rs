use crate::{ActorId, MAX_TEXT_BYTES};
use prost::{Enumeration, Message};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum Emotion {
    Unspecified = 0,
    Neutral = 1,
    Joy = 2,
    Sadness = 3,
    Anger = 4,
    Fear = 5,
    Surprise = 6,
    Disgust = 7,
    Contempt = 8,
    Concern = 9,
    Amusement = 10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum VoiceStyle {
    Unspecified = 0,
    Neutral = 1,
    Warm = 2,
    Tense = 3,
    Somber = 4,
    Excited = 5,
    Whisper = 6,
    Shout = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum AnimationCueKind {
    Unspecified = 0,
    Nod = 1,
    ShakeHead = 2,
    LookAtPlayer = 3,
    LookAway = 4,
    GestureOpen = 5,
    GesturePoint = 6,
    IdleShift = 7,
    VisemeStream = 8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum InterruptionPolicy {
    Unspecified = 0,
    BargeInAllowed = 1,
    FinishClause = 2,
    Uninterruptible = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum MemoryProposalKind {
    Unspecified = 0,
    Episodic = 1,
    Fact = 2,
    Preference = 3,
    QuestState = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum RelationshipAxis {
    Unspecified = 0,
    Affinity = 1,
    Trust = 2,
    Respect = 3,
    Fear = 4,
    Familiarity = 5,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct AnimationCueV1 {
    #[prost(enumeration = "AnimationCueKind", tag = "1")]
    pub kind: i32,
    #[prost(uint64, tag = "2")]
    pub start_offset_ms: u64,
    #[prost(uint64, tag = "3")]
    pub duration_ms: u64,
    #[prost(float, tag = "4")]
    pub intensity: f32,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct MemoryProposalV1 {
    #[prost(enumeration = "MemoryProposalKind", tag = "1")]
    pub kind: i32,
    #[prost(string, tag = "2")]
    pub content: String,
    #[prost(float, tag = "3")]
    pub importance: f32,
    #[prost(uint64, optional, tag = "4")]
    pub expires_after_ms: Option<u64>,
    #[prost(string, repeated, tag = "5")]
    pub evidence_turn_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct RelationshipProposalV1 {
    #[prost(message, optional, tag = "1")]
    pub subject_actor_id: Option<ActorId>,
    #[prost(enumeration = "RelationshipAxis", tag = "2")]
    pub axis: i32,
    #[prost(float, tag = "3")]
    pub delta: f32,
    #[prost(string, tag = "4")]
    pub reason: String,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct ActionProposalV1 {
    /// Stable allowlisted action identifier, never source code or a command line.
    #[prost(string, tag = "1")]
    pub action_id: String,
    /// UTF-8 JSON object interpreted only by the selected game adapter.
    #[prost(string, tag = "2")]
    pub parameters_json: String,
    /// Human-readable explanation for diagnostics and optional confirmation UI.
    #[prost(string, tag = "3")]
    pub rationale: String,
    #[prost(bool, tag = "4")]
    pub requires_confirmation: bool,
}

/// Structured suggestions produced independently from spoken text.
///
/// These values are proposals. The runtime applies policy, game capability, and
/// user confirmation after decoding; adapters must never execute them directly.
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct NpcEffectsV1 {
    #[prost(enumeration = "Emotion", tag = "1")]
    pub emotion: i32,
    #[prost(float, tag = "2")]
    pub valence: f32,
    #[prost(float, tag = "3")]
    pub arousal: f32,
    #[prost(float, tag = "4")]
    pub intensity: f32,
    #[prost(enumeration = "VoiceStyle", tag = "5")]
    pub voice_style: i32,
    #[prost(float, tag = "6")]
    pub speaking_rate: f32,
    #[prost(float, tag = "7")]
    pub pitch_semitones: f32,
    #[prost(float, tag = "8")]
    pub energy: f32,
    #[prost(message, repeated, tag = "9")]
    pub animation_cues: Vec<AnimationCueV1>,
    #[prost(enumeration = "InterruptionPolicy", tag = "10")]
    pub interruption_policy: i32,
    #[prost(message, repeated, tag = "11")]
    pub memory_proposals: Vec<MemoryProposalV1>,
    #[prost(message, repeated, tag = "12")]
    pub relationship_proposals: Vec<RelationshipProposalV1>,
    #[prost(message, repeated, tag = "13")]
    pub action_proposals: Vec<ActionProposalV1>,
}

impl NpcEffectsV1 {
    #[must_use]
    pub fn neutral() -> Self {
        Self {
            emotion: Emotion::Neutral as i32,
            valence: 0.0,
            arousal: 0.0,
            intensity: 0.0,
            voice_style: VoiceStyle::Neutral as i32,
            speaking_rate: 1.0,
            pitch_semitones: 0.0,
            energy: 1.0,
            animation_cues: Vec::new(),
            interruption_policy: InterruptionPolicy::BargeInAllowed as i32,
            memory_proposals: Vec::new(),
            relationship_proposals: Vec::new(),
            action_proposals: Vec::new(),
        }
    }

    pub fn validate(&self, policy: &EffectsValidationPolicy) -> Result<(), EffectsValidationError> {
        known_nonzero::<Emotion>(self.emotion, "emotion")?;
        known_nonzero::<VoiceStyle>(self.voice_style, "voice style")?;
        known_nonzero::<InterruptionPolicy>(self.interruption_policy, "interruption policy")?;
        finite_range(self.valence, -1.0, 1.0, "valence")?;
        finite_range(self.arousal, 0.0, 1.0, "arousal")?;
        finite_range(self.intensity, 0.0, 1.0, "intensity")?;
        finite_range(self.speaking_rate, 0.5, 2.0, "speaking rate")?;
        finite_range(self.pitch_semitones, -12.0, 12.0, "pitch")?;
        finite_range(self.energy, 0.0, 2.0, "energy")?;

        bounded_count(
            self.animation_cues.len(),
            policy.max_animation_cues,
            "animation cues",
        )?;
        bounded_count(
            self.memory_proposals.len(),
            policy.max_memory_proposals,
            "memory proposals",
        )?;
        bounded_count(
            self.relationship_proposals.len(),
            policy.max_relationship_proposals,
            "relationship proposals",
        )?;
        bounded_count(
            self.action_proposals.len(),
            policy.max_action_proposals,
            "action proposals",
        )?;

        for cue in &self.animation_cues {
            known_nonzero::<AnimationCueKind>(cue.kind, "animation cue")?;
            if cue.duration_ms == 0 || cue.duration_ms > policy.max_animation_duration_ms {
                return Err(EffectsValidationError::InvalidDuration("animation cue"));
            }
            finite_range(cue.intensity, 0.0, 1.0, "animation intensity")?;
        }

        for proposal in &self.memory_proposals {
            known_nonzero::<MemoryProposalKind>(proposal.kind, "memory kind")?;
            validate_content(&proposal.content, policy.max_proposal_text_bytes, "memory")?;
            finite_range(proposal.importance, 0.0, 1.0, "memory importance")?;
            bounded_count(proposal.evidence_turn_ids.len(), 32, "memory evidence")?;
            if proposal.evidence_turn_ids.iter().any(|id| id.len() > 64) {
                return Err(EffectsValidationError::TextTooLarge("memory evidence"));
            }
        }

        for proposal in &self.relationship_proposals {
            known_nonzero::<RelationshipAxis>(proposal.axis, "relationship axis")?;
            proposal
                .subject_actor_id
                .as_ref()
                .ok_or(EffectsValidationError::MissingActor)?
                .validate()
                .map_err(|_| EffectsValidationError::MissingActor)?;
            finite_range(proposal.delta, -1.0, 1.0, "relationship delta")?;
            validate_content(
                &proposal.reason,
                policy.max_proposal_text_bytes,
                "relationship reason",
            )?;
        }

        for proposal in &self.action_proposals {
            if !policy.allowed_action_ids.contains(&proposal.action_id) {
                return Err(EffectsValidationError::ActionNotAllowed(
                    proposal.action_id.clone(),
                ));
            }
            if proposal.parameters_json.len() > policy.max_action_json_bytes {
                return Err(EffectsValidationError::TextTooLarge("action parameters"));
            }
            let parameters: serde_json::Value = serde_json::from_str(&proposal.parameters_json)
                .map_err(|_| EffectsValidationError::InvalidActionJson)?;
            if !parameters.is_object() {
                return Err(EffectsValidationError::InvalidActionJson);
            }
            if proposal.rationale.len() > policy.max_proposal_text_bytes {
                return Err(EffectsValidationError::TextTooLarge("action rationale"));
            }
        }
        Ok(())
    }

    /// Enforces the safe failure contract: one malformed field neutralizes the
    /// entire structured path, while spoken text remains unaffected.
    #[must_use]
    pub fn validated_or_neutral(
        self,
        policy: &EffectsValidationPolicy,
    ) -> EffectsValidationOutcome {
        match self.validate(policy) {
            Ok(()) => EffectsValidationOutcome {
                effects: self,
                neutralized: false,
                error: None,
            },
            Err(error) => EffectsValidationOutcome {
                effects: Self::neutral(),
                neutralized: true,
                error: Some(error),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectsValidationPolicy {
    pub max_animation_cues: usize,
    pub max_animation_duration_ms: u64,
    pub max_memory_proposals: usize,
    pub max_relationship_proposals: usize,
    pub max_action_proposals: usize,
    pub max_proposal_text_bytes: usize,
    pub max_action_json_bytes: usize,
    pub allowed_action_ids: BTreeSet<String>,
}

impl Default for EffectsValidationPolicy {
    fn default() -> Self {
        Self {
            max_animation_cues: 32,
            max_animation_duration_ms: 60_000,
            max_memory_proposals: 16,
            max_relationship_proposals: 16,
            max_action_proposals: 8,
            max_proposal_text_bytes: 8 * 1024,
            max_action_json_bytes: 16 * 1024,
            allowed_action_ids: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectsValidationOutcome {
    pub effects: NpcEffectsV1,
    pub neutralized: bool,
    pub error: Option<EffectsValidationError>,
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum EffectsValidationError {
    #[error("unknown or unspecified {0}")]
    UnknownEnum(&'static str),
    #[error("{0} is non-finite or outside its permitted range")]
    InvalidRange(&'static str),
    #[error("too many {0}")]
    TooMany(&'static str),
    #[error("invalid duration for {0}")]
    InvalidDuration(&'static str),
    #[error("{0} is empty")]
    EmptyText(&'static str),
    #[error("{0} is too large")]
    TextTooLarge(&'static str),
    #[error("relationship proposal is missing a valid subject actor")]
    MissingActor,
    #[error("action `{0}` is not allowlisted")]
    ActionNotAllowed(String),
    #[error("action parameters must be a valid JSON object")]
    InvalidActionJson,
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
    Emotion,
    VoiceStyle,
    AnimationCueKind,
    InterruptionPolicy,
    MemoryProposalKind,
    RelationshipAxis,
);

fn known_nonzero<T: KnownEnum>(
    value: i32,
    name: &'static str,
) -> Result<(), EffectsValidationError> {
    if value == 0 || T::from_i32(value).is_none() {
        Err(EffectsValidationError::UnknownEnum(name))
    } else {
        Ok(())
    }
}

fn finite_range(
    value: f32,
    minimum: f32,
    maximum: f32,
    name: &'static str,
) -> Result<(), EffectsValidationError> {
    if value.is_finite() && (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(EffectsValidationError::InvalidRange(name))
    }
}

fn bounded_count(
    actual: usize,
    maximum: usize,
    name: &'static str,
) -> Result<(), EffectsValidationError> {
    if actual <= maximum {
        Ok(())
    } else {
        Err(EffectsValidationError::TooMany(name))
    }
}

fn validate_content(
    content: &str,
    maximum: usize,
    name: &'static str,
) -> Result<(), EffectsValidationError> {
    if content.trim().is_empty() {
        Err(EffectsValidationError::EmptyText(name))
    } else if content.len() > maximum.min(MAX_TEXT_BYTES) {
        Err(EffectsValidationError::TextTooLarge(name))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn neutral_effects_are_valid_under_default_policy() {
        assert_eq!(
            NpcEffectsV1::neutral().validate(&EffectsValidationPolicy::default()),
            Ok(())
        );
    }

    #[test]
    fn non_allowlisted_action_neutralizes_all_effects() {
        let mut effects = NpcEffectsV1::neutral();
        effects.emotion = Emotion::Joy as i32;
        effects.action_proposals.push(ActionProposalV1 {
            action_id: "spawn_process".into(),
            parameters_json: "{}".into(),
            rationale: "unsafe".into(),
            requires_confirmation: false,
        });
        let outcome = effects.validated_or_neutral(&EffectsValidationPolicy::default());
        assert!(outcome.neutralized);
        assert_eq!(outcome.effects, NpcEffectsV1::neutral());
        assert!(matches!(
            outcome.error,
            Some(EffectsValidationError::ActionNotAllowed(_))
        ));
    }

    #[test]
    fn allowlisted_action_requires_json_object() {
        let mut policy = EffectsValidationPolicy::default();
        policy.allowed_action_ids.insert("wave".into());
        let mut effects = NpcEffectsV1::neutral();
        effects.action_proposals.push(ActionProposalV1 {
            action_id: "wave".into(),
            parameters_json: "[]".into(),
            rationale: String::new(),
            requires_confirmation: false,
        });
        assert_eq!(
            effects.validate(&policy),
            Err(EffectsValidationError::InvalidActionJson)
        );
    }

    proptest! {
        #[test]
        fn every_non_finite_affect_value_is_rejected(value in prop_oneof![Just(f32::NAN), Just(f32::INFINITY), Just(f32::NEG_INFINITY)]) {
            let mut effects = NpcEffectsV1::neutral();
            effects.valence = value;
            prop_assert!(matches!(effects.validate(&EffectsValidationPolicy::default()), Err(EffectsValidationError::InvalidRange("valence"))));
        }

        #[test]
        fn out_of_range_intensity_always_neutralizes(value in prop_oneof![-1000.0f32..-0.001, 1.001f32..1000.0]) {
            let mut effects = NpcEffectsV1::neutral();
            effects.intensity = value;
            let outcome = effects.validated_or_neutral(&EffectsValidationPolicy::default());
            prop_assert!(outcome.neutralized);
            prop_assert_eq!(outcome.effects, NpcEffectsV1::neutral());
        }
    }
}
