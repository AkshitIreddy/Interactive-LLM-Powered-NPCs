use crate::{
    EmbeddingError, EmbeddingModelV1, NormalizedEmbeddingV1, TrackEpoch, TrackId, TrackPhaseV1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub const IDENTITY_GALLERY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityReferenceV1 {
    pub reference_id: String,
    pub provenance: String,
    pub embedding: NormalizedEmbeddingV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectIdentityV1 {
    pub subject_id: String,
    pub display_name: String,
    pub references: Vec<IdentityReferenceV1>,
}

impl SubjectIdentityV1 {
    fn validate(&self, model: &EmbeddingModelV1) -> Result<(), IdentityError> {
        validate_identifier(&self.subject_id, "subject ID")?;
        if self.display_name.trim().is_empty() || self.display_name.len() > 256 {
            return Err(IdentityError::InvalidDisplayName);
        }
        if self.references.is_empty() {
            return Err(IdentityError::SubjectHasNoReferences(
                self.subject_id.clone(),
            ));
        }
        let mut reference_ids = BTreeSet::new();
        for reference in &self.references {
            validate_identifier(&reference.reference_id, "reference ID")?;
            if !reference_ids.insert(&reference.reference_id) {
                return Err(IdentityError::DuplicateReferenceId(
                    reference.reference_id.clone(),
                ));
            }
            if reference.provenance.trim().is_empty() || reference.provenance.len() > 2_048 {
                return Err(IdentityError::InvalidProvenance);
            }
            reference.embedding.validate()?;
            if &reference.embedding.model != model {
                return Err(IdentityError::IncompatibleGalleryModel);
            }
        }
        Ok(())
    }
}

/// A portable, versioned gallery of normalized tensors. Its serde contract is
/// data-only JSON and never depends on language-specific object serialization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityGalleryV1 {
    pub schema_version: u32,
    pub model: EmbeddingModelV1,
    pub subjects: BTreeMap<String, SubjectIdentityV1>,
}

impl IdentityGalleryV1 {
    pub fn new(model: EmbeddingModelV1) -> Result<Self, IdentityError> {
        model.validate()?;
        Ok(Self {
            schema_version: IDENTITY_GALLERY_SCHEMA_VERSION,
            model,
            subjects: BTreeMap::new(),
        })
    }

    pub fn validate(&self) -> Result<(), IdentityError> {
        if self.schema_version != IDENTITY_GALLERY_SCHEMA_VERSION {
            return Err(IdentityError::UnsupportedGallerySchema(self.schema_version));
        }
        self.model.validate()?;
        for (key, subject) in &self.subjects {
            if key != &subject.subject_id {
                return Err(IdentityError::SubjectMapKeyMismatch {
                    key: key.clone(),
                    subject_id: subject.subject_id.clone(),
                });
            }
            subject.validate(&self.model)?;
        }
        Ok(())
    }

    pub fn insert_subject(&mut self, subject: SubjectIdentityV1) -> Result<(), IdentityError> {
        subject.validate(&self.model)?;
        if self.subjects.contains_key(&subject.subject_id) {
            return Err(IdentityError::DuplicateSubjectId(subject.subject_id));
        }
        self.subjects.insert(subject.subject_id.clone(), subject);
        Ok(())
    }

    pub fn contains_subject(&self, subject_id: &str) -> bool {
        self.subjects.contains_key(subject_id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityConfigV1 {
    pub consensus_window_frames: usize,
    pub minimum_consensus_frames: usize,
    pub minimum_similarity: f32,
    pub ambiguity_margin: f32,
    pub hysteresis_retention_similarity: f32,
    pub hysteresis_switch_margin: f32,
}

impl Default for IdentityConfigV1 {
    fn default() -> Self {
        Self {
            consensus_window_frames: 5,
            minimum_consensus_frames: 3,
            minimum_similarity: 0.72,
            ambiguity_margin: 0.08,
            hysteresis_retention_similarity: 0.66,
            hysteresis_switch_margin: 0.12,
        }
    }
}

impl IdentityConfigV1 {
    pub fn validate(&self) -> Result<(), IdentityError> {
        if self.consensus_window_frames == 0
            || self.minimum_consensus_frames == 0
            || self.minimum_consensus_frames > self.consensus_window_frames
        {
            return Err(IdentityError::InvalidConfiguration);
        }
        for value in [
            self.minimum_similarity,
            self.ambiguity_margin,
            self.hysteresis_retention_similarity,
            self.hysteresis_switch_margin,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(IdentityError::InvalidConfiguration);
            }
        }
        if self.hysteresis_retention_similarity > self.minimum_similarity {
            return Err(IdentityError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Conservative identity state for one actor track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum IdentityDecisionV1 {
    Pending {
        encounter_id: String,
        observed_frames: usize,
        required_frames: usize,
    },
    Explicit {
        encounter_id: String,
        subject_id: String,
    },
    Matched {
        encounter_id: String,
        subject_id: String,
        subject_similarity: f32,
        top_candidate_subject_id: String,
        top_candidate_similarity: f32,
        runner_up_similarity: Option<f32>,
        top1_top2_margin: f32,
        supporting_frames: usize,
        window_frames: usize,
        held_by_hysteresis: bool,
    },
    Ambiguous {
        encounter_id: String,
        top_subject_id: String,
        runner_up_subject_id: Option<String>,
        top_similarity: f32,
        runner_up_similarity: Option<f32>,
        top1_top2_margin: f32,
        supporting_frames: usize,
        window_frames: usize,
    },
    NoMatch {
        encounter_id: String,
        best_subject_id: Option<String>,
        best_similarity: Option<f32>,
        observed_frames: usize,
    },
    Offscreen {
        encounter_id: String,
        last_confirmed_subject_id: Option<String>,
        track_epoch: TrackEpoch,
    },
}

impl IdentityDecisionV1 {
    pub fn encounter_id(&self) -> &str {
        match self {
            Self::Pending { encounter_id, .. }
            | Self::Explicit { encounter_id, .. }
            | Self::Matched { encounter_id, .. }
            | Self::Ambiguous { encounter_id, .. }
            | Self::NoMatch { encounter_id, .. }
            | Self::Offscreen { encounter_id, .. } => encounter_id,
        }
    }

    pub fn resolved_subject_id(&self) -> Option<&str> {
        match self {
            Self::Explicit { subject_id, .. } | Self::Matched { subject_id, .. } => {
                Some(subject_id)
            }
            Self::Offscreen {
                last_confirmed_subject_id,
                ..
            } => last_confirmed_subject_id.as_deref(),
            Self::Pending { .. } | Self::Ambiguous { .. } | Self::NoMatch { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
struct ScoreFrame {
    scores: BTreeMap<String, f32>,
    top_subject_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct TrackIdentityHistory {
    score_frames: VecDeque<ScoreFrame>,
    confirmed_subject_id: Option<String>,
    last_decision: Option<IdentityDecisionV1>,
}

#[derive(Clone, Debug)]
pub struct IdentityResolverV1 {
    config: IdentityConfigV1,
    gallery: IdentityGalleryV1,
    histories: BTreeMap<TrackId, TrackIdentityHistory>,
    explicit_assignments: BTreeMap<TrackId, String>,
}

impl IdentityResolverV1 {
    pub fn new(
        config: IdentityConfigV1,
        gallery: IdentityGalleryV1,
    ) -> Result<Self, IdentityError> {
        config.validate()?;
        gallery.validate()?;
        Ok(Self {
            config,
            gallery,
            histories: BTreeMap::new(),
            explicit_assignments: BTreeMap::new(),
        })
    }

    pub fn config(&self) -> &IdentityConfigV1 {
        &self.config
    }

    pub fn gallery(&self) -> &IdentityGalleryV1 {
        &self.gallery
    }

    /// Assigning an actor explicitly is authoritative until cleared or retired.
    pub fn assign_explicit(
        &mut self,
        track_id: TrackId,
        subject_id: impl Into<String>,
    ) -> Result<(), IdentityError> {
        let subject_id = subject_id.into();
        if !self.gallery.contains_subject(&subject_id) {
            return Err(IdentityError::UnknownSubject(subject_id));
        }
        self.explicit_assignments.insert(track_id, subject_id);
        Ok(())
    }

    pub fn clear_explicit(&mut self, track_id: TrackId) {
        self.explicit_assignments.remove(&track_id);
    }

    pub fn retire_track(&mut self, track_id: TrackId) {
        self.explicit_assignments.remove(&track_id);
        self.histories.remove(&track_id);
    }

    pub fn observe(
        &mut self,
        track_id: TrackId,
        embedding: Option<&NormalizedEmbeddingV1>,
        encounter_id: &str,
    ) -> Result<IdentityDecisionV1, IdentityError> {
        if let Some(subject_id) = self.explicit_assignments.get(&track_id) {
            let decision = IdentityDecisionV1::Explicit {
                encounter_id: encounter_id.to_owned(),
                subject_id: subject_id.clone(),
            };
            self.histories.entry(track_id).or_default().last_decision = Some(decision.clone());
            return Ok(decision);
        }

        if let Some(embedding) = embedding {
            embedding.validate()?;
            if embedding.model != self.gallery.model {
                return Err(IdentityError::IncompatibleObservationModel);
            }
            let score_frame = score_embedding(&self.gallery, embedding)?;
            let history = self.histories.entry(track_id).or_default();
            history.score_frames.push_back(score_frame);
            while history.score_frames.len() > self.config.consensus_window_frames {
                history.score_frames.pop_front();
            }
        }

        let history = self.histories.entry(track_id).or_default();
        if embedding.is_none() {
            if let Some(last) = &history.last_decision {
                return Ok(last.clone());
            }
        }
        let decision = decide(&self.config, history, encounter_id);
        history.last_decision = Some(decision.clone());
        Ok(decision)
    }

    pub fn decision_for_phase(
        &mut self,
        track_id: TrackId,
        track_epoch: TrackEpoch,
        phase: TrackPhaseV1,
        encounter_id: &str,
    ) -> IdentityDecisionV1 {
        if phase == TrackPhaseV1::Offscreen {
            let last_confirmed_subject_id = self
                .explicit_assignments
                .get(&track_id)
                .cloned()
                .or_else(|| {
                    self.histories
                        .get(&track_id)
                        .and_then(|history| history.confirmed_subject_id.clone())
                });
            return IdentityDecisionV1::Offscreen {
                encounter_id: encounter_id.to_owned(),
                last_confirmed_subject_id,
                track_epoch,
            };
        }
        self.observe(track_id, None, encounter_id)
            .unwrap_or_else(|_| IdentityDecisionV1::Pending {
                encounter_id: encounter_id.to_owned(),
                observed_frames: 0,
                required_frames: self.config.minimum_consensus_frames,
            })
    }
}

fn score_embedding(
    gallery: &IdentityGalleryV1,
    embedding: &NormalizedEmbeddingV1,
) -> Result<ScoreFrame, IdentityError> {
    let mut scores = BTreeMap::new();
    for subject in gallery.subjects.values() {
        let mut best = f32::NEG_INFINITY;
        for reference in &subject.references {
            best = best.max(embedding.cosine_similarity(&reference.embedding)?);
        }
        scores.insert(subject.subject_id.clone(), best);
    }
    let top_subject_id = ranked_scores(&scores)
        .into_iter()
        .next()
        .map(|(subject_id, _)| subject_id);
    Ok(ScoreFrame {
        scores,
        top_subject_id,
    })
}

fn decide(
    config: &IdentityConfigV1,
    history: &mut TrackIdentityHistory,
    encounter_id: &str,
) -> IdentityDecisionV1 {
    let observed_frames = history.score_frames.len();
    if observed_frames < config.minimum_consensus_frames {
        return IdentityDecisionV1::Pending {
            encounter_id: encounter_id.to_owned(),
            observed_frames,
            required_frames: config.minimum_consensus_frames,
        };
    }

    let mut sums = BTreeMap::<String, f32>::new();
    let mut supports = BTreeMap::<String, usize>::new();
    for frame in &history.score_frames {
        for (subject_id, score) in &frame.scores {
            *sums.entry(subject_id.clone()).or_default() += score;
        }
        if let Some(subject_id) = &frame.top_subject_id {
            *supports.entry(subject_id.clone()).or_default() += 1;
        }
    }
    let averages: BTreeMap<String, f32> = sums
        .into_iter()
        .map(|(subject_id, sum)| (subject_id, sum / observed_frames as f32))
        .collect();
    let ranked = ranked_scores(&averages);
    let Some((top_subject_id, top_similarity)) = ranked.first().cloned() else {
        history.confirmed_subject_id = None;
        return IdentityDecisionV1::NoMatch {
            encounter_id: encounter_id.to_owned(),
            best_subject_id: None,
            best_similarity: None,
            observed_frames,
        };
    };
    let runner_up = ranked.get(1).cloned();
    let runner_up_similarity = runner_up.as_ref().map(|(_, score)| *score);
    let margin = runner_up_similarity.map_or(1.0, |score| top_similarity - score);
    let top_support = supports.get(&top_subject_id).copied().unwrap_or(0);

    if let Some(confirmed_subject_id) = history.confirmed_subject_id.clone() {
        let confirmed_similarity = averages
            .get(&confirmed_subject_id)
            .copied()
            .unwrap_or(f32::NEG_INFINITY);
        let challenger_advantage = top_similarity - confirmed_similarity;
        if confirmed_similarity >= config.hysteresis_retention_similarity
            && (top_subject_id == confirmed_subject_id
                || challenger_advantage < config.hysteresis_switch_margin)
        {
            let confirmed_support = supports.get(&confirmed_subject_id).copied().unwrap_or(0);
            let held_by_hysteresis = top_subject_id != confirmed_subject_id;
            return IdentityDecisionV1::Matched {
                encounter_id: encounter_id.to_owned(),
                subject_id: confirmed_subject_id,
                subject_similarity: confirmed_similarity,
                top_candidate_subject_id: top_subject_id.clone(),
                top_candidate_similarity: top_similarity,
                runner_up_similarity,
                top1_top2_margin: margin,
                supporting_frames: confirmed_support,
                window_frames: observed_frames,
                held_by_hysteresis,
            };
        }
    }

    if top_similarity < config.minimum_similarity {
        history.confirmed_subject_id = None;
        return IdentityDecisionV1::NoMatch {
            encounter_id: encounter_id.to_owned(),
            best_subject_id: Some(top_subject_id),
            best_similarity: Some(top_similarity),
            observed_frames,
        };
    }

    if margin < config.ambiguity_margin || top_support < config.minimum_consensus_frames {
        return IdentityDecisionV1::Ambiguous {
            encounter_id: encounter_id.to_owned(),
            top_subject_id,
            runner_up_subject_id: runner_up.map(|(subject_id, _)| subject_id),
            top_similarity,
            runner_up_similarity,
            top1_top2_margin: margin,
            supporting_frames: top_support,
            window_frames: observed_frames,
        };
    }

    history.confirmed_subject_id = Some(top_subject_id.clone());
    IdentityDecisionV1::Matched {
        encounter_id: encounter_id.to_owned(),
        subject_id: top_subject_id.clone(),
        subject_similarity: top_similarity,
        top_candidate_subject_id: top_subject_id,
        top_candidate_similarity: top_similarity,
        runner_up_similarity,
        top1_top2_margin: margin,
        supporting_frames: top_support,
        window_frames: observed_frames,
        held_by_hysteresis: false,
    }
}

fn ranked_scores(scores: &BTreeMap<String, f32>) -> Vec<(String, f32)> {
    let mut ranked: Vec<_> = scores
        .iter()
        .map(|(subject_id, score)| (subject_id.clone(), *score))
        .collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked
}

fn validate_identifier(value: &str, kind: &'static str) -> Result<(), IdentityError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(IdentityError::InvalidIdentifier(kind));
    }
    Ok(())
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum IdentityError {
    #[error("unsupported identity-gallery schema version {0}")]
    UnsupportedGallerySchema(u32),
    #[error("identity configuration is invalid")]
    InvalidConfiguration,
    #[error("invalid {0}")]
    InvalidIdentifier(&'static str),
    #[error("subject display name is empty or too long")]
    InvalidDisplayName,
    #[error("identity reference provenance is empty or too long")]
    InvalidProvenance,
    #[error("subject {0:?} has no reference embeddings")]
    SubjectHasNoReferences(String),
    #[error("duplicate subject ID {0:?}")]
    DuplicateSubjectId(String),
    #[error("duplicate reference ID {0:?}")]
    DuplicateReferenceId(String),
    #[error("gallery map key {key:?} does not match embedded subject ID {subject_id:?}")]
    SubjectMapKeyMismatch { key: String, subject_id: String },
    #[error("identity reference does not use the gallery's exact model space")]
    IncompatibleGalleryModel,
    #[error("observation does not use the gallery's exact model space")]
    IncompatibleObservationModel,
    #[error("unknown subject {0:?}")]
    UnknownSubject(String),
    #[error(transparent)]
    InvalidEmbedding(#[from] EmbeddingError),
}
