use crate::{
    ActorDetectionV1, ActorIdentityEngineV1, EmbeddingMetadataV1, EmbeddingModelV1, EngineError,
    FrameActorsV1, FrameIdentityUpdateV1, IdentityConfigV1, IdentityGalleryV1, IdentityReferenceV1,
    NormalizedEmbeddingV1, SubjectIdentityV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use thiserror::Error;

pub const QUALIFIED_IDENTITY_SCHEMA_VERSION: u32 = 1;
const MAX_REFERENCE_DIMENSIONS: usize = 65_536;
const MAX_FRAME_OBSERVATIONS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceSourceClassV1 {
    /// A reference supplied by the user, private to one local user and never
    /// eligible for distribution or provider egress.
    UserPrivate,
    /// Original synthetic artwork whose license is recorded explicitly.
    OriginalSynthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableTensorTransportV1 {
    /// Raw finite little-endian f32 values. Object serializers such as pickle
    /// are intentionally absent from the closed transport enum.
    F32Le,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedIdentityQualificationV1 {
    pub qualification_id: String,
    pub model: EmbeddingModelV1,
    pub detector_id: String,
    pub detector_revision: String,
    pub preprocessing: String,
    pub calibration_fixture_sha256: String,
    pub calibrated_config: IdentityConfigV1,
}

impl PinnedIdentityQualificationV1 {
    pub fn validate(&self) -> Result<(), QualifiedIdentityError> {
        self.model.validate()?;
        self.calibrated_config.validate()?;
        if !valid_identifier(&self.qualification_id)
            || self.detector_id.trim().is_empty()
            || self.detector_revision.trim().is_empty()
            || self.preprocessing.trim().is_empty()
            || !valid_sha256(&self.calibration_fixture_sha256)
        {
            return Err(QualifiedIdentityError::InvalidQualification);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedReferenceProvenanceV1 {
    pub game_profile_id: String,
    pub subject_id: String,
    pub reference_id: String,
    pub source_class: ReferenceSourceClassV1,
    pub source_content_sha256: String,
    pub owner_user_id: Option<String>,
    pub original_work_license: Option<String>,
    pub explicit_user_consent: bool,
    pub local_only: bool,
    pub imported_at_ms: u64,
}

impl QualifiedReferenceProvenanceV1 {
    fn validate(&self, game_profile_id: &str) -> Result<(), QualifiedIdentityError> {
        if self.game_profile_id != game_profile_id
            || !valid_identifier(&self.game_profile_id)
            || !valid_identifier(&self.subject_id)
            || !valid_identifier(&self.reference_id)
            || !valid_sha256(&self.source_content_sha256)
        {
            return Err(QualifiedIdentityError::InvalidReferenceProvenance);
        }
        match self.source_class {
            ReferenceSourceClassV1::UserPrivate => {
                if !self.explicit_user_consent
                    || !self.local_only
                    || self
                        .owner_user_id
                        .as_deref()
                        .is_none_or(|owner| !valid_identifier(owner))
                    || self.original_work_license.is_some()
                {
                    return Err(QualifiedIdentityError::InvalidReferenceProvenance);
                }
            }
            ReferenceSourceClassV1::OriginalSynthetic => {
                if !self.local_only
                    || self.owner_user_id.is_some()
                    || self
                        .original_work_license
                        .as_deref()
                        .is_none_or(|license| license.trim().is_empty())
                {
                    return Err(QualifiedIdentityError::InvalidReferenceProvenance);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableReferenceImportV1 {
    pub schema_version: u32,
    pub provenance: QualifiedReferenceProvenanceV1,
    pub subject_display_name: String,
    pub model: EmbeddingModelV1,
    pub metadata: EmbeddingMetadataV1,
    pub transport: PortableTensorTransportV1,
    pub tensor_sha256: String,
    /// Bounded raw f32le bytes. Serde encodes this as a data array; no object
    /// deserialization or executable model payload is accepted.
    pub tensor_f32le: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedIdentityGalleryV1 {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub qualification: PinnedIdentityQualificationV1,
    pub gallery: IdentityGalleryV1,
    pub reference_provenance: BTreeMap<String, QualifiedReferenceProvenanceV1>,
}

impl QualifiedIdentityGalleryV1 {
    pub fn new(
        game_profile_id: impl Into<String>,
        qualification: PinnedIdentityQualificationV1,
    ) -> Result<Self, QualifiedIdentityError> {
        let game_profile_id = game_profile_id.into();
        if !valid_identifier(&game_profile_id) {
            return Err(QualifiedIdentityError::InvalidGameScope);
        }
        qualification.validate()?;
        let gallery = IdentityGalleryV1::new(qualification.model.clone())?;
        Ok(Self {
            schema_version: QUALIFIED_IDENTITY_SCHEMA_VERSION,
            game_profile_id,
            qualification,
            gallery,
            reference_provenance: BTreeMap::new(),
        })
    }

    pub fn validate(&self) -> Result<(), QualifiedIdentityError> {
        if self.schema_version != QUALIFIED_IDENTITY_SCHEMA_VERSION
            || !valid_identifier(&self.game_profile_id)
        {
            return Err(QualifiedIdentityError::InvalidGallerySchema);
        }
        self.qualification.validate()?;
        self.gallery.validate()?;
        if self.gallery.model != self.qualification.model {
            return Err(QualifiedIdentityError::ModelNotPinned);
        }
        let mut gallery_reference_ids = BTreeSet::new();
        for subject in self.gallery.subjects.values() {
            for reference in &subject.references {
                gallery_reference_ids.insert(reference.reference_id.as_str());
                let provenance = self
                    .reference_provenance
                    .get(&reference.reference_id)
                    .ok_or(QualifiedIdentityError::MissingReferenceProvenance)?;
                provenance.validate(&self.game_profile_id)?;
                if provenance.subject_id != subject.subject_id {
                    return Err(QualifiedIdentityError::InvalidReferenceProvenance);
                }
            }
        }
        if gallery_reference_ids.len() != self.reference_provenance.len()
            || self
                .reference_provenance
                .keys()
                .any(|id| !gallery_reference_ids.contains(id.as_str()))
        {
            return Err(QualifiedIdentityError::MissingReferenceProvenance);
        }
        Ok(())
    }

    pub fn import_reference(
        &mut self,
        import: PortableReferenceImportV1,
    ) -> Result<(), QualifiedIdentityError> {
        self.validate()?;
        if import.schema_version != QUALIFIED_IDENTITY_SCHEMA_VERSION
            || import.model != self.qualification.model
            || import.metadata.detector_id != self.qualification.detector_id
            || import.metadata.detector_revision != self.qualification.detector_revision
            || import.metadata.preprocessing != self.qualification.preprocessing
            || !valid_sha256(&import.tensor_sha256)
            || sha256_hex(&import.tensor_f32le) != import.tensor_sha256
            || import.tensor_f32le.len() != import.model.dimensions.saturating_mul(4)
            || import.model.dimensions > MAX_REFERENCE_DIMENSIONS
            || import.subject_display_name.trim().is_empty()
            || import.subject_display_name.len() > 256
        {
            return Err(QualifiedIdentityError::InvalidPortableTensor);
        }
        import.provenance.validate(&self.game_profile_id)?;
        if import.metadata.source_digest_sha256.as_deref()
            != Some(import.provenance.source_content_sha256.as_str())
        {
            return Err(QualifiedIdentityError::SourceDigestMismatch);
        }
        if self
            .reference_provenance
            .contains_key(&import.provenance.reference_id)
        {
            return Err(QualifiedIdentityError::DuplicateReference);
        }

        let values = import
            .tensor_f32le
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect::<Vec<_>>();
        let embedding = NormalizedEmbeddingV1::new(import.model, import.metadata, values)?;
        let reference = IdentityReferenceV1 {
            reference_id: import.provenance.reference_id.clone(),
            provenance: format!(
                "qualified:{}:{}",
                self.qualification.qualification_id,
                match import.provenance.source_class {
                    ReferenceSourceClassV1::UserPrivate => "user_private",
                    ReferenceSourceClassV1::OriginalSynthetic => "original_synthetic",
                }
            ),
            embedding,
        };
        match self
            .gallery
            .subjects
            .entry(import.provenance.subject_id.clone())
        {
            Entry::Vacant(entry) => {
                entry.insert(SubjectIdentityV1 {
                    subject_id: import.provenance.subject_id.clone(),
                    display_name: import.subject_display_name,
                    references: vec![reference],
                });
            }
            Entry::Occupied(mut entry) => {
                if entry.get().display_name != import.subject_display_name {
                    return Err(QualifiedIdentityError::SubjectDisplayNameMismatch);
                }
                entry.get_mut().references.push(reference);
            }
        }
        self.reference_provenance
            .insert(import.provenance.reference_id.clone(), import.provenance);
        self.validate()
    }

    pub fn into_engine(
        self,
        tracker: crate::ActorTrackerV1,
    ) -> Result<ActorIdentityEngineV1, QualifiedIdentityError> {
        self.validate()?;
        let resolver = crate::IdentityResolverV1::new(
            self.qualification.calibrated_config.clone(),
            self.gallery,
        )?;
        Ok(ActorIdentityEngineV1::new(tracker, resolver))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedCaptureTargetV1 {
    pub capture_session_id: String,
    pub process_id: u32,
    pub window_handle: u64,
    pub executable_name: String,
}

impl TrustedCaptureTargetV1 {
    fn validate(&self) -> Result<(), QualifiedIdentityError> {
        if !valid_identifier(&self.capture_session_id)
            || self.process_id == 0
            || self.window_handle == 0
            || !self.executable_name.to_ascii_lowercase().ends_with(".exe")
            || self.executable_name.contains(['/', '\\', ':'])
        {
            return Err(QualifiedIdentityError::InvalidCaptureTarget);
        }
        Ok(())
    }
}

/// Authenticated WGC frame state plus already-qualified detector observations.
/// WGC pixels alone never create an identity decision: actor detections and
/// pinned-model embeddings must be supplied by a separate native inference
/// stage and remain subject to consensus, ambiguity, and hysteresis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedWgcIdentityFrameV1 {
    pub schema_version: u32,
    pub target: TrustedCaptureTargetV1,
    pub frame_sequence: u64,
    pub device_generation: u64,
    pub geometry_epoch: u64,
    pub source_frame_qpc: u64,
    pub qpc_frequency: u64,
    pub captured_at_ms: u64,
    pub content_sha256: String,
    pub advancing_frame_verified: bool,
    pub overlay_capture_excluded: bool,
    pub protected_online_detected: bool,
    pub anti_cheat_detected: bool,
    pub observations: Vec<ActorDetectionV1>,
}

#[derive(Clone, Debug)]
pub struct TrustedWgcEvidenceAdapterV1 {
    expected_target: TrustedCaptureTargetV1,
    qualification: PinnedIdentityQualificationV1,
    last_frame_sequence: Option<u64>,
    last_source_frame_qpc: Option<u64>,
    device_generation: Option<u64>,
    geometry_epoch: Option<u64>,
}

impl TrustedWgcEvidenceAdapterV1 {
    pub fn new(
        expected_target: TrustedCaptureTargetV1,
        qualification: PinnedIdentityQualificationV1,
    ) -> Result<Self, QualifiedIdentityError> {
        expected_target.validate()?;
        qualification.validate()?;
        Ok(Self {
            expected_target,
            qualification,
            last_frame_sequence: None,
            last_source_frame_qpc: None,
            device_generation: None,
            geometry_epoch: None,
        })
    }

    pub fn adapt(
        &mut self,
        frame: TrustedWgcIdentityFrameV1,
    ) -> Result<FrameActorsV1, QualifiedIdentityError> {
        frame.target.validate()?;
        if frame.schema_version != QUALIFIED_IDENTITY_SCHEMA_VERSION
            || frame.target != self.expected_target
            || frame.frame_sequence == 0
            || frame.device_generation == 0
            || frame.geometry_epoch == 0
            || frame.source_frame_qpc == 0
            || frame.qpc_frequency == 0
            || !valid_sha256(&frame.content_sha256)
            || !frame.advancing_frame_verified
            || !frame.overlay_capture_excluded
            || frame.protected_online_detected
            || frame.anti_cheat_detected
            || frame.observations.len() > MAX_FRAME_OBSERVATIONS
            || self
                .last_frame_sequence
                .is_some_and(|last| frame.frame_sequence <= last)
            || self
                .last_source_frame_qpc
                .is_some_and(|last| frame.source_frame_qpc <= last)
        {
            return Err(QualifiedIdentityError::UntrustedCaptureEvidence);
        }
        if self.device_generation.is_some_and(|generation| {
            generation != frame.device_generation
                && self
                    .last_frame_sequence
                    .is_some_and(|sequence| sequence != 0)
        }) {
            return Err(QualifiedIdentityError::CaptureGenerationChanged);
        }
        if self
            .geometry_epoch
            .is_some_and(|epoch| frame.geometry_epoch < epoch)
        {
            return Err(QualifiedIdentityError::UntrustedCaptureEvidence);
        }
        for observation in &frame.observations {
            observation.bounds.validate()?;
            if !observation.confidence.is_finite()
                || !(0.0..=1.0).contains(&observation.confidence)
                || observation
                    .embedding
                    .as_ref()
                    .is_some_and(|embedding| embedding.model != self.qualification.model)
            {
                return Err(QualifiedIdentityError::UnqualifiedObservation);
            }
            if let Some(embedding) = &observation.embedding {
                embedding.validate()?;
                if embedding.metadata.source_frame_index != frame.frame_sequence
                    || embedding.metadata.detector_id != self.qualification.detector_id
                    || embedding.metadata.detector_revision != self.qualification.detector_revision
                    || embedding.metadata.preprocessing != self.qualification.preprocessing
                    || embedding.metadata.source_digest_sha256.as_deref()
                        != Some(frame.content_sha256.as_str())
                {
                    return Err(QualifiedIdentityError::UnqualifiedObservation);
                }
            }
        }
        self.last_frame_sequence = Some(frame.frame_sequence);
        self.last_source_frame_qpc = Some(frame.source_frame_qpc);
        self.device_generation = Some(frame.device_generation);
        self.geometry_epoch = Some(frame.geometry_epoch);
        Ok(FrameActorsV1 {
            frame_index: frame.frame_sequence,
            timestamp_ms: frame.captured_at_ms,
            detections: frame.observations,
        })
    }

    pub fn process(
        &mut self,
        engine: &mut ActorIdentityEngineV1,
        frame: TrustedWgcIdentityFrameV1,
    ) -> Result<FrameIdentityUpdateV1, QualifiedIdentityError> {
        let frame = self.adapt(frame)?;
        engine.process_frame(frame).map_err(Into::into)
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum QualifiedIdentityError {
    #[error("identity qualification is invalid")]
    InvalidQualification,
    #[error("qualified gallery schema or game scope is invalid")]
    InvalidGallerySchema,
    #[error("identity gallery game scope is invalid")]
    InvalidGameScope,
    #[error("reference provenance is incomplete or violates its privacy class")]
    InvalidReferenceProvenance,
    #[error("portable identity tensor is malformed, unpinned, or corrupted")]
    InvalidPortableTensor,
    #[error("reference source digest does not match the qualified crop metadata")]
    SourceDigestMismatch,
    #[error("identity reference already exists")]
    DuplicateReference,
    #[error("subject display name conflicts with the existing game-scoped subject")]
    SubjectDisplayNameMismatch,
    #[error("gallery reference provenance is missing or orphaned")]
    MissingReferenceProvenance,
    #[error("gallery model differs from the pinned qualified model")]
    ModelNotPinned,
    #[error("trusted capture target is invalid")]
    InvalidCaptureTarget,
    #[error("WGC frame evidence is stale, unsafe, mismatched, or unverified")]
    UntrustedCaptureEvidence,
    #[error("WGC device generation changed; start a fresh adapter epoch")]
    CaptureGenerationChanged,
    #[error("actor observation does not use the pinned model and exact source frame")]
    UnqualifiedObservation,
    #[error(transparent)]
    Embedding(#[from] crate::EmbeddingError),
    #[error(transparent)]
    Identity(#[from] crate::IdentityError),
    #[error(transparent)]
    Geometry(#[from] crate::GeometryError),
    #[error(transparent)]
    Engine(#[from] EngineError),
}
