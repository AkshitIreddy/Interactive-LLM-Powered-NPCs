use crate::{encounter::hex_sha256, CharacterDbError, CHARACTER_DB_SCHEMA_VERSION};
use npc_memory::EmbeddingInput;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorElementFormat {
    F32Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceMetric {
    Cosine,
}

/// Serializable metadata for a raw, finite f32 tensor. The tensor itself is a
/// bounded `.f32le` blob; executable serializers such as pickle are not part of
/// the schema and therefore cannot be selected by data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingTensorMetadataV1 {
    pub schema_version: String,
    pub metadata_id: String,
    pub item_id: String,
    pub generation: i64,
    pub model_id: String,
    pub model_revision: String,
    pub preprocessing_revision: String,
    pub dimensions: u32,
    pub element_format: TensorElementFormat,
    pub distance_metric: DistanceMetric,
    pub source_content_sha256: String,
    pub tensor_sha256: String,
    pub tensor_byte_length: u64,
}

pub fn encode_embedding_tensor(
    item_id: &str,
    generation: i64,
    model_id: &str,
    model_revision: &str,
    preprocessing_revision: &str,
    source_content_sha256: &str,
    values: &[f32],
) -> Result<(EmbeddingTensorMetadataV1, Vec<u8>), CharacterDbError> {
    validate_fields(
        item_id,
        generation,
        model_id,
        model_revision,
        preprocessing_revision,
        source_content_sha256,
        values,
    )?;
    let bytes = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let tensor_sha256 = hex_sha256(&bytes);
    let identity = format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        item_id,
        generation,
        model_id,
        model_revision,
        preprocessing_revision,
        source_content_sha256
    );
    let metadata_id = format!("embedding-{}", &hex_sha256(identity.as_bytes())[..24]);
    let metadata = EmbeddingTensorMetadataV1 {
        schema_version: CHARACTER_DB_SCHEMA_VERSION.to_owned(),
        metadata_id,
        item_id: item_id.to_owned(),
        generation,
        model_id: model_id.to_owned(),
        model_revision: model_revision.to_owned(),
        preprocessing_revision: preprocessing_revision.to_owned(),
        dimensions: values.len() as u32,
        element_format: TensorElementFormat::F32Le,
        distance_metric: DistanceMetric::Cosine,
        source_content_sha256: source_content_sha256.to_owned(),
        tensor_sha256,
        tensor_byte_length: bytes.len() as u64,
    };
    Ok((metadata, bytes))
}

pub fn decode_embedding_tensor(
    metadata: &EmbeddingTensorMetadataV1,
    bytes: &[u8],
) -> Result<EmbeddingInput, CharacterDbError> {
    if metadata.schema_version != CHARACTER_DB_SCHEMA_VERSION
        || metadata.element_format != TensorElementFormat::F32Le
        || metadata.distance_metric != DistanceMetric::Cosine
        || metadata.generation < 0
        || metadata.dimensions == 0
        || metadata.dimensions > 65_536
        || metadata.tensor_byte_length != bytes.len() as u64
        || bytes.len() != metadata.dimensions as usize * 4
        || hex_sha256(bytes) != metadata.tensor_sha256
    {
        return Err(CharacterDbError::InvalidInput(
            "embedding tensor metadata does not match the bounded f32le payload".to_owned(),
        ));
    }
    let values = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    validate_fields(
        &metadata.item_id,
        metadata.generation,
        &metadata.model_id,
        &metadata.model_revision,
        &metadata.preprocessing_revision,
        &metadata.source_content_sha256,
        &values,
    )?;
    Ok(EmbeddingInput {
        item_id: metadata.item_id.clone(),
        generation: metadata.generation,
        model_id: format!("{}@{}", metadata.model_id, metadata.model_revision),
        values,
    })
}

fn validate_fields(
    item_id: &str,
    generation: i64,
    model_id: &str,
    model_revision: &str,
    preprocessing_revision: &str,
    source_content_sha256: &str,
    values: &[f32],
) -> Result<(), CharacterDbError> {
    if generation < 0
        || [item_id, model_id, model_revision, preprocessing_revision]
            .iter()
            .any(|value| value.trim().is_empty())
        || source_content_sha256.len() != 64
        || !source_content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || values.is_empty()
        || values.len() > 65_536
        || values.iter().any(|value| !value.is_finite())
    {
        return Err(CharacterDbError::InvalidInput(
            "embedding fields, source hash, or values are invalid".to_owned(),
        ));
    }
    let norm = values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if norm <= f64::EPSILON {
        return Err(CharacterDbError::InvalidInput(
            "zero-length embedding is not searchable".to_owned(),
        ));
    }
    Ok(())
}
