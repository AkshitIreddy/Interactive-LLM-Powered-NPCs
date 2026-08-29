use std::{collections::BTreeSet, fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::time::timeout_at;
use url::Url;

use crate::{
    EmbeddingInputRole, EmbeddingProvider, EmbeddingRequestV1, EmbeddingResponseV1,
    EmbeddingVector, HttpMethod, HttpRequest, HttpTransport, ProviderLimits, ProviderPrivacy,
    RequestContext, RerankProvider, RerankRequestV1, RerankResponseV1, RerankScore,
    RetrievalCapabilitiesV1, RetrievalError, RetrievalErrorKind, SecretReference, SecretResolver,
    TokenUsage, TransportAuth, TransportError, TruncationPolicy,
};

pub const NVIDIA_PROVIDER_ID: &str = "nvidia-nim";
/// Exact model ID verified by the current curated catalog and synthetic endpoint smoke.
pub const NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID: &str = "nvidia/nemotron-3-embed-1b";
/// NVIDIA hosted embeddings endpoint from the official NIM API reference:
/// <https://docs.api.nvidia.com/nim/reference/nvidia-nemotron-3-embed-1b-infer>
pub const NVIDIA_EMBEDDINGS_ENDPOINT: &str = "https://integrate.api.nvidia.com/v1/embeddings";
/// NVIDIA hosted reranking route from the official NIM API reference. Availability is separately
/// gated by curated metadata because hosted routes can be model-specific or temporarily absent:
/// <https://docs.api.nvidia.com/nim/reference/nvidia-nv-rerankqa-mistral-4b-v3-infer>
pub const NVIDIA_RERANKING_ENDPOINT: &str =
    "https://ai.api.nvidia.com/v1/retrieval/nvidia/reranking";

const MAX_MODEL_ID_BYTES: usize = 512;
const MAX_TEXT_BYTES: usize = 128 * 1_024;
const MAX_REQUEST_TEXT_BYTES: usize = 4 * 1_048_576;
const MAX_EMBEDDING_INPUTS: usize = 512;
const MAX_RERANK_PASSAGES: usize = 512;
const MAX_DIMENSIONS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvidiaEmbeddingModel {
    pub model_id: String,
    pub expected_dimensions: Option<usize>,
    pub maximum_inputs: usize,
    pub maximum_text_bytes: usize,
    pub supports_dynamic_dimensions: bool,
}

impl NvidiaEmbeddingModel {
    pub fn new(
        model_id: impl Into<String>,
        expected_dimensions: Option<usize>,
    ) -> Result<Self, RetrievalError> {
        let model = Self {
            model_id: model_id.into(),
            expected_dimensions,
            maximum_inputs: MAX_EMBEDDING_INPUTS,
            maximum_text_bytes: MAX_TEXT_BYTES,
            supports_dynamic_dimensions: false,
        };
        model.validate()?;
        Ok(model)
    }

    fn validate(&self) -> Result<(), RetrievalError> {
        if !valid_model_id(&self.model_id)
            || self.maximum_inputs == 0
            || self.maximum_inputs > MAX_EMBEDDING_INPUTS
            || self.maximum_text_bytes == 0
            || self.maximum_text_bytes > MAX_TEXT_BYTES
            || self
                .expected_dimensions
                .is_some_and(|dimensions| dimensions == 0 || dimensions > MAX_DIMENSIONS)
        {
            return Err(RetrievalError::invalid_request());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NvidiaRerankEndpoint {
    Shared,
    /// Exact endpoint sourced from signed curated metadata. Arbitrary URLs are rejected.
    CuratedModelSpecific(CuratedNvidiaRerankEndpoint),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CuratedNvidiaRerankEndpoint {
    url: Url,
}

impl NvidiaRerankEndpoint {
    fn resolve(&self) -> Result<Url, RetrievalError> {
        match self {
            Self::Shared => {
                Url::parse(NVIDIA_RERANKING_ENDPOINT).map_err(|_| RetrievalError::invalid_request())
            }
            Self::CuratedModelSpecific(endpoint)
                if valid_model_specific_rerank_url(&endpoint.url) =>
            {
                Ok(endpoint.url.clone())
            }
            Self::CuratedModelSpecific(_) => Err(RetrievalError::invalid_request()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvidiaRerankModel {
    pub model_id: String,
    pub maximum_passages: usize,
    pub maximum_text_bytes: usize,
    pub endpoint: NvidiaRerankEndpoint,
}

impl NvidiaRerankModel {
    pub fn shared(model_id: impl Into<String>) -> Result<Self, RetrievalError> {
        let model = Self {
            model_id: model_id.into(),
            maximum_passages: MAX_RERANK_PASSAGES,
            maximum_text_bytes: MAX_TEXT_BYTES,
            endpoint: NvidiaRerankEndpoint::Shared,
        };
        model.validate()?;
        Ok(model)
    }

    pub fn from_curated_metadata(
        model_id: impl Into<String>,
        exact_endpoint: Url,
    ) -> Result<Self, RetrievalError> {
        let model = Self {
            model_id: model_id.into(),
            maximum_passages: MAX_RERANK_PASSAGES,
            maximum_text_bytes: MAX_TEXT_BYTES,
            endpoint: NvidiaRerankEndpoint::CuratedModelSpecific(CuratedNvidiaRerankEndpoint {
                url: exact_endpoint,
            }),
        };
        model.validate()?;
        Ok(model)
    }

    fn validate(&self) -> Result<(), RetrievalError> {
        if !valid_model_id(&self.model_id)
            || self.maximum_passages == 0
            || self.maximum_passages > MAX_RERANK_PASSAGES
            || self.maximum_text_bytes == 0
            || self.maximum_text_bytes > MAX_TEXT_BYTES
        {
            return Err(RetrievalError::invalid_request());
        }
        let _ = self.endpoint.resolve()?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct NvidiaNimConfig {
    pub credential: SecretReference,
    pub secrets: Arc<dyn SecretResolver>,
    pub embedding_models: Vec<NvidiaEmbeddingModel>,
    pub rerank_models: Vec<NvidiaRerankModel>,
}

impl fmt::Debug for NvidiaNimConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NvidiaNimConfig")
            .field("credential", &self.credential)
            .field("secrets", &"[SECRET_RESOLVER]")
            .field("embedding_models", &self.embedding_models)
            .field("rerank_models", &self.rerank_models)
            .finish()
    }
}

pub struct NvidiaNimAdapter {
    config: NvidiaNimConfig,
    transport: Arc<dyn HttpTransport>,
    capabilities: Arc<RetrievalCapabilitiesV1>,
    embeddings_endpoint: Url,
}

impl fmt::Debug for NvidiaNimAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NvidiaNimAdapter")
            .field("config", &self.config)
            .field("transport", &"[HTTP_TRANSPORT]")
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl NvidiaNimAdapter {
    pub fn new(
        config: NvidiaNimConfig,
        transport: Arc<dyn HttpTransport>,
    ) -> Result<Self, RetrievalError> {
        if config.embedding_models.is_empty() && config.rerank_models.is_empty() {
            return Err(RetrievalError::invalid_request());
        }
        let mut model_ids = BTreeSet::new();
        for model in &config.embedding_models {
            model.validate()?;
            if !model_ids.insert((&model.model_id, "embed")) {
                return Err(RetrievalError::invalid_request());
            }
        }
        for model in &config.rerank_models {
            model.validate()?;
            if !model_ids.insert((&model.model_id, "rerank")) {
                return Err(RetrievalError::invalid_request());
            }
        }
        let embeddings_endpoint = Url::parse(NVIDIA_EMBEDDINGS_ENDPOINT)
            .map_err(|_| RetrievalError::invalid_request())?;
        let capabilities = Arc::new(RetrievalCapabilitiesV1 {
            schema_version: "1.0.0".to_owned(),
            provider_id: NVIDIA_PROVIDER_ID.to_owned(),
            display_name: "NVIDIA NIM".to_owned(),
            embeddings: !config.embedding_models.is_empty(),
            reranking: !config.rerank_models.is_empty(),
            cancellation: true,
            deadlines: true,
            exact_model_ids: true,
            privacy: ProviderPrivacy {
                query_leaves_device: true,
                passages_leave_device: true,
                provider_may_retain_data: true,
                no_automatic_fallback: true,
            },
            limits: ProviderLimits {
                maximum_embedding_inputs: MAX_EMBEDDING_INPUTS,
                maximum_rerank_passages: MAX_RERANK_PASSAGES,
                maximum_text_bytes: MAX_TEXT_BYTES,
                maximum_request_text_bytes: MAX_REQUEST_TEXT_BYTES,
                maximum_embedding_dimensions: MAX_DIMENSIONS,
            },
        });
        Ok(Self {
            config,
            transport,
            capabilities,
            embeddings_endpoint,
        })
    }

    fn embedding_model(&self, requested: &str) -> Result<&NvidiaEmbeddingModel, RetrievalError> {
        self.config
            .embedding_models
            .iter()
            .find(|model| model.model_id == requested)
            .ok_or_else(RetrievalError::invalid_request)
    }

    fn rerank_model(&self, requested: &str) -> Result<&NvidiaRerankModel, RetrievalError> {
        self.config
            .rerank_models
            .iter()
            .find(|model| model.model_id == requested)
            .ok_or_else(RetrievalError::invalid_request)
    }

    async fn dispatch(
        &self,
        url: &Url,
        body: &[u8],
        context: &RequestContext,
    ) -> Result<crate::HttpResponse, RetrievalError> {
        if context.cancellation.is_cancelled() {
            return Err(RetrievalError::new(
                RetrievalErrorKind::Cancelled,
                "retrieval request was cancelled",
            ));
        }
        if context.deadline <= tokio::time::Instant::now() {
            return Err(RetrievalError::new(
                RetrievalErrorKind::DeadlineExceeded,
                "retrieval request deadline elapsed",
            ));
        }

        // Credential material exists only for this request scope and is dropped after dispatch.
        // Resolution shares the operation's cancellation and deadline budget.
        let resolve = self.config.secrets.resolve(&self.config.credential);
        let secret = tokio::select! {
            biased;
            () = context.cancellation.cancelled() => return Err(RetrievalError::new(
                RetrievalErrorKind::Cancelled,
                "retrieval request was cancelled",
            )),
            result = timeout_at(context.deadline, resolve) => match result {
                Ok(result) => result?,
                Err(_) => return Err(RetrievalError::new(
                    RetrievalErrorKind::DeadlineExceeded,
                    "retrieval request deadline elapsed",
                )),
            },
        };
        let execute = self.transport.execute(HttpRequest {
            method: HttpMethod::Post,
            url,
            auth: TransportAuth::Bearer(&secret),
            json_body: body,
        });
        tokio::select! {
            biased;
            () = context.cancellation.cancelled() => Err(RetrievalError::new(
                RetrievalErrorKind::Cancelled,
                "retrieval request was cancelled",
            )),
            result = timeout_at(context.deadline, execute) => match result {
                Ok(Ok(response)) => normalize_http_response(response),
                Ok(Err(error)) => Err(map_transport_error(error)),
                Err(_) => Err(RetrievalError::new(
                    RetrievalErrorKind::DeadlineExceeded,
                    "retrieval request deadline elapsed",
                )),
            },
        }
    }
}

#[async_trait]
impl EmbeddingProvider for NvidiaNimAdapter {
    fn capabilities(&self) -> Arc<RetrievalCapabilitiesV1> {
        Arc::clone(&self.capabilities)
    }

    async fn embed(
        &self,
        request: EmbeddingRequestV1,
        context: RequestContext,
    ) -> Result<EmbeddingResponseV1, RetrievalError> {
        let model = self.embedding_model(&request.model_id)?;
        validate_embedding_request(&request, model)?;
        let payload = NvidiaEmbeddingRequest {
            input: &request.inputs,
            model: &request.model_id,
            input_type: match request.role {
                EmbeddingInputRole::Query => "query",
                EmbeddingInputRole::Passage => "passage",
            },
            encoding_format: "float",
            truncate: truncation_wire(request.truncation),
            dimensions: request.dimensions,
        };
        let body = serde_json::to_vec(&payload).map_err(|_| RetrievalError::invalid_request())?;
        let response = self
            .dispatch(&self.embeddings_endpoint, &body, &context)
            .await?;
        parse_embedding_response(&request, model, response)
    }
}

#[async_trait]
impl RerankProvider for NvidiaNimAdapter {
    fn capabilities(&self) -> Arc<RetrievalCapabilitiesV1> {
        Arc::clone(&self.capabilities)
    }

    async fn rerank(
        &self,
        request: RerankRequestV1,
        context: RequestContext,
    ) -> Result<RerankResponseV1, RetrievalError> {
        let model = self.rerank_model(&request.model_id)?;
        validate_rerank_request(&request, model)?;
        let endpoint = model.endpoint.resolve()?;
        let query = TextPayload {
            text: &request.query,
        };
        let passages = request
            .passages
            .iter()
            .map(|text| TextPayload { text })
            .collect::<Vec<_>>();
        let payload = NvidiaRerankRequest {
            model: &request.model_id,
            query,
            passages,
            truncate: rerank_truncation_wire(request.truncation)?,
        };
        let body = serde_json::to_vec(&payload).map_err(|_| RetrievalError::invalid_request())?;
        let response = self.dispatch(&endpoint, &body, &context).await?;
        parse_rerank_response(&request, response)
    }
}

fn validate_embedding_request(
    request: &EmbeddingRequestV1,
    model: &NvidiaEmbeddingModel,
) -> Result<(), RetrievalError> {
    if request.model_id != model.model_id
        || request.inputs.is_empty()
        || request.inputs.len() > model.maximum_inputs
        || request.dimensions.is_some_and(|value| {
            value == 0 || !model.supports_dynamic_dimensions || value > MAX_DIMENSIONS
        })
    {
        return Err(RetrievalError::invalid_request());
    }
    validate_texts(&request.inputs, model.maximum_text_bytes)
}

fn validate_rerank_request(
    request: &RerankRequestV1,
    model: &NvidiaRerankModel,
) -> Result<(), RetrievalError> {
    if request.model_id != model.model_id
        || request.passages.is_empty()
        || request.passages.len() > model.maximum_passages
        || request.truncation == TruncationPolicy::Start
    {
        return Err(RetrievalError::invalid_request());
    }
    validate_texts(
        std::slice::from_ref(&request.query),
        model.maximum_text_bytes,
    )?;
    validate_texts(&request.passages, model.maximum_text_bytes)?;
    let total = request
        .passages
        .iter()
        .fold(request.query.len(), |size, passage| {
            size.saturating_add(passage.len())
        });
    if total > MAX_REQUEST_TEXT_BYTES {
        return Err(RetrievalError::invalid_request());
    }
    Ok(())
}

fn validate_texts(texts: &[String], per_text_limit: usize) -> Result<(), RetrievalError> {
    let mut total = 0usize;
    for text in texts {
        total = total.saturating_add(text.len());
        if text.trim().is_empty()
            || text.len() > per_text_limit
            || text.chars().any(|character| character == '\0')
            || total > MAX_REQUEST_TEXT_BYTES
        {
            return Err(RetrievalError::invalid_request());
        }
    }
    Ok(())
}

fn normalize_http_response(
    response: crate::HttpResponse,
) -> Result<crate::HttpResponse, RetrievalError> {
    if response.body.len() > crate::transport::MAX_RESPONSE_BYTES {
        return Err(RetrievalError::malformed(
            "provider response exceeded the bounded size",
        ));
    }
    if response.status == 200 {
        return Ok(response);
    }
    let mut error = RetrievalError::from_status(response.status);
    error.retry_after = parse_retry_after(response.header("retry-after"));
    error.provider_request_id = request_id(&response);
    Err(error)
}

fn map_transport_error(error: TransportError) -> RetrievalError {
    match error {
        TransportError::Timeout => RetrievalError::new(
            RetrievalErrorKind::DeadlineExceeded,
            "provider transport timed out",
        ),
        TransportError::Connect | TransportError::Other => {
            RetrievalError::new(RetrievalErrorKind::Unavailable, "provider transport failed")
        }
        TransportError::ResponseTooLarge => {
            RetrievalError::malformed("provider response exceeded the bounded size")
        }
    }
}

fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    value?
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds <= 86_400)
        .map(Duration::from_secs)
}

fn request_id(response: &crate::HttpResponse) -> Option<String> {
    response
        .header("x-request-id")
        .or_else(|| response.header("request-id"))
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 256
                && value.chars().all(|character| !character.is_control())
        })
        .map(str::to_owned)
}

fn parse_embedding_response(
    request: &EmbeddingRequestV1,
    model: &NvidiaEmbeddingModel,
    response: crate::HttpResponse,
) -> Result<EmbeddingResponseV1, RetrievalError> {
    let request_id = request_id(&response);
    let payload: NvidiaEmbeddingResponse = serde_json::from_slice(&response.body)
        .map_err(|_| RetrievalError::malformed("provider returned malformed embedding JSON"))?;
    if payload.model != request.model_id || payload.data.len() != request.inputs.len() {
        return Err(RetrievalError::malformed(
            "provider returned mismatched embedding metadata",
        ));
    }
    let expected_dimensions = request.dimensions.or(model.expected_dimensions);
    let mut slots: Vec<Option<Vec<f32>>> = vec![None; request.inputs.len()];
    let mut observed_dimensions = None;
    for item in payload.data {
        if item.index >= slots.len()
            || item.embedding.is_empty()
            || item.embedding.len() > MAX_DIMENSIONS
            || item.embedding.iter().any(|value| !value.is_finite())
            || slots[item.index].is_some()
        {
            return Err(RetrievalError::malformed(
                "provider returned invalid embedding vectors",
            ));
        }
        if expected_dimensions.is_some_and(|expected| item.embedding.len() != expected)
            || observed_dimensions.is_some_and(|observed| item.embedding.len() != observed)
        {
            return Err(RetrievalError::malformed(
                "provider returned inconsistent embedding dimensions",
            ));
        }
        observed_dimensions = Some(item.embedding.len());
        slots[item.index] = Some(item.embedding);
    }
    let dimensions = observed_dimensions
        .ok_or_else(|| RetrievalError::malformed("provider returned no embedding dimensions"))?;
    let vectors = slots
        .into_iter()
        .enumerate()
        .map(|(input_index, values)| {
            values
                .map(|values| EmbeddingVector {
                    input_index,
                    values,
                })
                .ok_or_else(|| RetrievalError::malformed("provider omitted an embedding vector"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EmbeddingResponseV1 {
        model_id: payload.model,
        dimensions,
        vectors,
        usage: payload.usage.into(),
        provider_request_id: request_id,
    })
}

fn parse_rerank_response(
    request: &RerankRequestV1,
    response: crate::HttpResponse,
) -> Result<RerankResponseV1, RetrievalError> {
    let request_id = request_id(&response);
    let payload: NvidiaRerankResponse = serde_json::from_slice(&response.body)
        .map_err(|_| RetrievalError::malformed("provider returned malformed reranking JSON"))?;
    if payload.rankings.len() != request.passages.len() {
        return Err(RetrievalError::malformed(
            "provider returned the wrong number of reranking scores",
        ));
    }
    let mut seen = vec![false; request.passages.len()];
    let mut previous = f32::INFINITY;
    let mut scores = Vec::with_capacity(payload.rankings.len());
    for ranking in payload.rankings {
        if ranking.index >= seen.len()
            || seen[ranking.index]
            || !ranking.logit.is_finite()
            || ranking.logit > previous
        {
            return Err(RetrievalError::malformed(
                "provider returned invalid reranking order",
            ));
        }
        seen[ranking.index] = true;
        previous = ranking.logit;
        scores.push(RerankScore {
            passage_index: ranking.index,
            score: ranking.logit,
        });
    }
    if seen.iter().any(|value| !value) {
        return Err(RetrievalError::malformed(
            "provider omitted a reranking score",
        ));
    }
    Ok(RerankResponseV1 {
        model_id: request.model_id.clone(),
        scores,
        usage: payload.usage.into(),
        provider_request_id: request_id,
    })
}

fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_MODEL_ID_BYTES
        && !value.contains("..")
        && !value.starts_with('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn valid_model_specific_rerank_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("ai.api.nvidia.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with("/v1/retrieval/nvidia/")
        && url.path().ends_with("/reranking")
        && url.path() != "/v1/retrieval/nvidia/reranking"
        && !url.path().contains("//")
        && !url.path().contains("..")
        && !url.path().contains('%')
}

fn truncation_wire(policy: TruncationPolicy) -> &'static str {
    match policy {
        TruncationPolicy::None => "NONE",
        TruncationPolicy::Start => "START",
        TruncationPolicy::End => "END",
    }
}

fn rerank_truncation_wire(policy: TruncationPolicy) -> Result<&'static str, RetrievalError> {
    match policy {
        TruncationPolicy::None => Ok("NONE"),
        TruncationPolicy::End => Ok("END"),
        TruncationPolicy::Start => Err(RetrievalError::invalid_request()),
    }
}

#[derive(Serialize)]
struct NvidiaEmbeddingRequest<'request> {
    input: &'request [String],
    model: &'request str,
    input_type: &'static str,
    encoding_format: &'static str,
    truncate: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct NvidiaEmbeddingResponse {
    model: String,
    data: Vec<NvidiaEmbeddingItem>,
    #[serde(default)]
    usage: NvidiaUsage,
}

#[derive(Deserialize)]
struct NvidiaEmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Serialize)]
struct NvidiaRerankRequest<'request> {
    model: &'request str,
    query: TextPayload<'request>,
    passages: Vec<TextPayload<'request>>,
    truncate: &'static str,
}

#[derive(Serialize)]
struct TextPayload<'request> {
    text: &'request str,
}

#[derive(Deserialize)]
struct NvidiaRerankResponse {
    rankings: Vec<NvidiaRanking>,
    #[serde(default)]
    usage: NvidiaUsage,
}

#[derive(Deserialize)]
struct NvidiaRanking {
    index: usize,
    logit: f32,
}

#[derive(Default, Deserialize)]
struct NvidiaUsage {
    #[serde(default, alias = "input_tokens")]
    prompt_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

impl From<NvidiaUsage> for TokenUsage {
    fn from(value: NvidiaUsage) -> Self {
        Self {
            input_tokens: value.prompt_tokens,
            total_tokens: value.total_tokens,
        }
    }
}
