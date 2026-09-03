use crate::BoundingBoxV1;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const EMBEDDING_SCHEMA_VERSION: u32 = 1;
const UNIT_NORM_TOLERANCE: f32 = 1.0e-3;

/// Fully versioned description of the model space containing an embedding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingModelV1 {
    pub provider: String,
    pub model_id: String,
    pub revision: String,
    pub dimensions: usize,
}

impl EmbeddingModelV1 {
    pub fn validate(&self) -> Result<(), EmbeddingError> {
        if self.provider.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.revision.trim().is_empty()
            || self.dimensions == 0
            || self.dimensions > 65_536
        {
            return Err(EmbeddingError::InvalidModelDescriptor);
        }
        Ok(())
    }
}

/// Audit metadata retained alongside the tensor. It intentionally contains no
/// demographic labels or executable/object-serialization fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingMetadataV1 {
    pub source_frame_index: u64,
    pub crop_bounds: BoundingBoxV1,
    pub detector_id: String,
    pub detector_revision: String,
    pub preprocessing: String,
    pub source_digest_sha256: Option<String>,
}

impl EmbeddingMetadataV1 {
    pub fn validate(&self) -> Result<(), EmbeddingError> {
        self.crop_bounds.validate()?;
        if self.detector_id.trim().is_empty()
            || self.detector_revision.trim().is_empty()
            || self.preprocessing.trim().is_empty()
        {
            return Err(EmbeddingError::InvalidMetadata);
        }
        if self.source_digest_sha256.as_ref().is_some_and(|digest| {
            digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(EmbeddingError::InvalidSourceDigest);
        }
        Ok(())
    }
}

/// A normalized one-dimensional f32 tensor with a stable JSON representation.
///
/// `new` normalizes caller values. `validate` must be called after deserializing
/// untrusted data; gallery and tracker entry points do this automatically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedEmbeddingV1 {
    pub schema_version: u32,
    pub model: EmbeddingModelV1,
    pub metadata: EmbeddingMetadataV1,
    pub values: Vec<f32>,
}

impl NormalizedEmbeddingV1 {
    pub fn new(
        model: EmbeddingModelV1,
        metadata: EmbeddingMetadataV1,
        mut values: Vec<f32>,
    ) -> Result<Self, EmbeddingError> {
        model.validate()?;
        metadata.validate()?;
        validate_raw_values(&model, &values)?;
        let norm = l2_norm(&values);
        if norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }
        for value in &mut values {
            *value /= norm;
        }
        let embedding = Self {
            schema_version: EMBEDDING_SCHEMA_VERSION,
            model,
            metadata,
            values,
        };
        embedding.validate()?;
        Ok(embedding)
    }

    pub fn validate(&self) -> Result<(), EmbeddingError> {
        if self.schema_version != EMBEDDING_SCHEMA_VERSION {
            return Err(EmbeddingError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        self.model.validate()?;
        self.metadata.validate()?;
        validate_raw_values(&self.model, &self.values)?;
        let norm = l2_norm(&self.values);
        if norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }
        if (norm - 1.0).abs() > UNIT_NORM_TOLERANCE {
            return Err(EmbeddingError::NotNormalized);
        }
        Ok(())
    }

    /// Cosine similarity in `[-1, 1]`, available only inside the exact same
    /// versioned model space.
    pub fn cosine_similarity(&self, other: &Self) -> Result<f32, EmbeddingError> {
        self.validate()?;
        other.validate()?;
        if self.model != other.model {
            return Err(EmbeddingError::IncompatibleModelSpace);
        }
        Ok(self
            .values
            .iter()
            .zip(&other.values)
            .map(|(left, right)| left * right)
            .sum::<f32>()
            .clamp(-1.0, 1.0))
    }
}

fn validate_raw_values(model: &EmbeddingModelV1, values: &[f32]) -> Result<(), EmbeddingError> {
    if values.len() != model.dimensions {
        return Err(EmbeddingError::DimensionMismatch {
            expected: model.dimensions,
            actual: values.len(),
        });
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::NonFiniteValue);
    }
    Ok(())
}

fn l2_norm(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum::<f32>().sqrt()
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum EmbeddingError {
    #[error("unsupported embedding schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("embedding model descriptor is incomplete or has an invalid dimension count")]
    InvalidModelDescriptor,
    #[error("embedding metadata is incomplete")]
    InvalidMetadata,
    #[error("embedding source digest must be a 64-character hexadecimal SHA-256")]
    InvalidSourceDigest,
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("embedding contains a non-finite value")]
    NonFiniteValue,
    #[error("embedding has zero norm")]
    ZeroNorm,
    #[error("embedding tensor is not L2-normalized")]
    NotNormalized,
    #[error("embeddings use different model IDs, revisions, providers, or dimensions")]
    IncompatibleModelSpace,
    #[error(transparent)]
    InvalidGeometry(#[from] crate::GeometryError),
}
