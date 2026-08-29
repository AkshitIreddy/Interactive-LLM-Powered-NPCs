use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::RetrievalError;

/// Versioned capability contract shared by hosted and future local retrieval providers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalCapabilitiesV1 {
    pub schema_version: String,
    pub provider_id: String,
    pub display_name: String,
    pub embeddings: bool,
    pub reranking: bool,
    pub cancellation: bool,
    pub deadlines: bool,
    pub exact_model_ids: bool,
    pub privacy: ProviderPrivacy,
    pub limits: ProviderLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPrivacy {
    /// Queries are transmitted to the configured external provider.
    pub query_leaves_device: bool,
    /// Passage text is transmitted for passage embeddings and reranking.
    pub passages_leave_device: bool,
    /// Provider-side retention is governed by the provider account and current terms.
    pub provider_may_retain_data: bool,
    /// Retrieval failures never reroute content to another provider automatically.
    pub no_automatic_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimits {
    pub maximum_embedding_inputs: usize,
    pub maximum_rerank_passages: usize,
    pub maximum_text_bytes: usize,
    pub maximum_request_text_bytes: usize,
    pub maximum_embedding_dimensions: usize,
}

/// The role is semantically significant for asymmetric embedding models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingInputRole {
    Query,
    Passage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TruncationPolicy {
    #[default]
    None,
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingRequestV1 {
    pub model_id: String,
    pub role: EmbeddingInputRole,
    pub inputs: Vec<String>,
    #[serde(default)]
    pub truncation: TruncationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<usize>,
}

/// A vector is always identified by its original input index. Responses are normalized to
/// ascending `input_index` order before they cross the provider boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingVector {
    pub input_index: usize,
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingResponseV1 {
    pub model_id: String,
    pub dimensions: usize,
    pub vectors: Vec<EmbeddingVector>,
    pub usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankRequestV1 {
    pub model_id: String,
    pub query: String,
    pub passages: Vec<String>,
    #[serde(default)]
    pub truncation: TruncationPolicy,
}

/// Scores are normalized to descending relevance order. `passage_index` always refers to the
/// original request, allowing callers to reorder immutable passage records safely.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankScore {
    pub passage_index: usize,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankResponseV1 {
    pub model_id: String,
    pub scores: Vec<RerankScore>,
    pub usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub total_tokens: u64,
}

/// Per-operation control state. The generation token is supplied by the caller so late results
/// can be correlated with the turn generation even when cancellation races with completion.
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub deadline: Instant,
    pub cancellation: CancellationToken,
    pub cancellation_generation: u64,
}

impl RequestContext {
    pub fn with_timeout(timeout: Duration, cancellation_generation: u64) -> Self {
        Self {
            deadline: Instant::now() + timeout,
            cancellation: CancellationToken::new(),
            cancellation_generation,
        }
    }
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn capabilities(&self) -> Arc<RetrievalCapabilitiesV1>;

    async fn embed(
        &self,
        request: EmbeddingRequestV1,
        context: RequestContext,
    ) -> Result<EmbeddingResponseV1, RetrievalError>;
}

#[async_trait]
pub trait RerankProvider: Send + Sync {
    fn capabilities(&self) -> Arc<RetrievalCapabilitiesV1>;

    async fn rerank(
        &self,
        request: RerankRequestV1,
        context: RequestContext,
    ) -> Result<RerankResponseV1, RetrievalError>;
}
