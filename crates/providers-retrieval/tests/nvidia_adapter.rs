use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use npc_providers_retrieval::{
    EmbeddingInputRole, EmbeddingProvider, EmbeddingRequestV1, HttpRequest, HttpResponse,
    HttpTransport, MemorySecretResolver, NvidiaEmbeddingModel, NvidiaNimAdapter, NvidiaNimConfig,
    NvidiaRerankModel, RequestContext, RerankProvider, RerankRequestV1, RetrievalErrorKind,
    SecretReference, SecretResolver, SecretString, TransportError, TruncationPolicy,
    NVIDIA_EMBEDDINGS_ENDPOINT, NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID, NVIDIA_RERANKING_ENDPOINT,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use url::Url;

const EMBEDDING_MODEL: &str = NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID;
const RERANK_MODEL: &str = "nvidia/llama-nemotron-rerank-1b-v2";

#[derive(Clone, Debug)]
struct CapturedRequest {
    url: String,
    body: Value,
    auth_was_present: bool,
}

#[derive(Default)]
struct MockTransport {
    responses: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    captured: Mutex<Vec<CapturedRequest>>,
    wait_for_cancellation: bool,
}

struct HangingSecretResolver;

#[async_trait]
impl SecretResolver for HangingSecretResolver {
    async fn resolve(
        &self,
        _reference: &SecretReference,
    ) -> Result<SecretString, npc_providers_retrieval::RetrievalError> {
        std::future::pending().await
    }
}

impl MockTransport {
    fn with_responses(responses: Vec<HttpResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            captured: Mutex::default(),
            wait_for_cancellation: false,
        }
    }

    fn pending() -> Self {
        Self {
            wait_for_cancellation: true,
            ..Self::default()
        }
    }

    fn captured(&self) -> Vec<CapturedRequest> {
        self.captured.lock().expect("captured lock").clone()
    }
}

#[async_trait]
impl HttpTransport for MockTransport {
    async fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, TransportError> {
        let body = serde_json::from_slice(request.json_body).expect("adapter produced JSON");
        self.captured
            .lock()
            .expect("captured lock")
            .push(CapturedRequest {
                url: request.url.as_str().to_owned(),
                body,
                auth_was_present: format!("{:?}", request.auth).contains("REDACTED"),
            });
        if self.wait_for_cancellation {
            std::future::pending::<()>().await;
        }
        self.responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .unwrap_or(Err(TransportError::Other))
    }
}

fn response(status: u16, body: Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&body).expect("fixture JSON"),
    }
}

fn response_with_headers(
    status: u16,
    headers: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> HttpResponse {
    HttpResponse {
        status,
        headers: headers
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        body: b"provider error body with nvapi-should-not-escape".to_vec(),
    }
}

fn fixture_adapter(transport: Arc<dyn HttpTransport>) -> NvidiaNimAdapter {
    let reference = SecretReference::new("provider/nvidia-nim/default").expect("reference");
    let secrets = MemorySecretResolver::default();
    secrets
        .insert(
            reference.clone(),
            SecretString::new("nvapi-fixture-secret").expect("secret"),
        )
        .expect("insert secret");
    let mut embedding =
        NvidiaEmbeddingModel::new(EMBEDDING_MODEL, Some(3)).expect("embedding model");
    embedding.maximum_inputs = 8;
    embedding.maximum_text_bytes = 1_024;
    let mut rerank = NvidiaRerankModel::shared(RERANK_MODEL).expect("rerank model");
    rerank.maximum_passages = 8;
    rerank.maximum_text_bytes = 1_024;
    NvidiaNimAdapter::new(
        NvidiaNimConfig {
            credential: reference,
            secrets: Arc::new(secrets),
            embedding_models: vec![embedding],
            rerank_models: vec![rerank],
        },
        transport,
    )
    .expect("adapter")
}

fn context() -> RequestContext {
    RequestContext::with_timeout(Duration::from_secs(2), 7)
}

fn embedding_request(role: EmbeddingInputRole) -> EmbeddingRequestV1 {
    EmbeddingRequestV1 {
        model_id: EMBEDDING_MODEL.to_owned(),
        role,
        inputs: vec!["Where is the relic?".to_owned(), "A passage".to_owned()],
        truncation: TruncationPolicy::None,
        dimensions: None,
    }
}

fn rerank_request() -> RerankRequestV1 {
    RerankRequestV1 {
        model_id: RERANK_MODEL.to_owned(),
        query: "Where is the relic?".to_owned(),
        passages: vec!["In the vault".to_owned(), "At the market".to_owned()],
        truncation: TruncationPolicy::End,
    }
}

#[tokio::test]
async fn embedding_query_preserves_original_input_order() {
    let transport = Arc::new(MockTransport::with_responses(vec![response(
        200,
        json!({
            "model": EMBEDDING_MODEL,
            "data": [
                {"index": 1, "embedding": [4.0, 5.0, 6.0]},
                {"index": 0, "embedding": [1.0, 2.0, 3.0]}
            ],
            "usage": {"prompt_tokens": 6, "total_tokens": 6}
        }),
    )]));
    let adapter = fixture_adapter(transport.clone());

    let result = adapter
        .embed(embedding_request(EmbeddingInputRole::Query), context())
        .await
        .expect("embedding succeeds");

    assert_eq!(result.dimensions, 3);
    assert_eq!(result.vectors[0].input_index, 0);
    assert_eq!(result.vectors[0].values, vec![1.0, 2.0, 3.0]);
    assert_eq!(result.vectors[1].input_index, 1);
    assert_eq!(result.usage.input_tokens, 6);
    let captured = transport.captured();
    assert_eq!(captured[0].url, NVIDIA_EMBEDDINGS_ENDPOINT);
    assert_eq!(captured[0].body["input_type"], "query");
    assert_eq!(captured[0].body["encoding_format"], "float");
    assert!(captured[0].auth_was_present);
    assert!(!format!("{:?}", captured[0]).contains("fixture-secret"));
}

#[tokio::test]
async fn embedding_passage_sends_the_semantically_distinct_role() {
    let transport = Arc::new(MockTransport::with_responses(vec![response(
        200,
        json!({
            "model": EMBEDDING_MODEL,
            "data": [
                {"index": 0, "embedding": [1.0, 0.0, 0.0]},
                {"index": 1, "embedding": [0.0, 1.0, 0.0]}
            ]
        }),
    )]));
    let adapter = fixture_adapter(transport.clone());

    adapter
        .embed(embedding_request(EmbeddingInputRole::Passage), context())
        .await
        .expect("embedding succeeds");

    assert_eq!(transport.captured()[0].body["input_type"], "passage");
}

#[tokio::test]
async fn reranking_returns_validated_descending_original_indexes() {
    let transport = Arc::new(MockTransport::with_responses(vec![response(
        200,
        json!({
            "rankings": [
                {"index": 1, "logit": 2.25},
                {"index": 0, "logit": -1.5}
            ],
            "usage": {"prompt_tokens": 12, "total_tokens": 12}
        }),
    )]));
    let adapter = fixture_adapter(transport.clone());

    let result = adapter
        .rerank(rerank_request(), context())
        .await
        .expect("rerank succeeds");

    assert_eq!(result.scores[0].passage_index, 1);
    assert_eq!(result.scores[0].score, 2.25);
    assert_eq!(result.scores[1].passage_index, 0);
    let captured = transport.captured();
    assert_eq!(captured[0].url, NVIDIA_RERANKING_ENDPOINT);
    assert_eq!(captured[0].body["query"]["text"], "Where is the relic?");
    assert_eq!(captured[0].body["passages"][0]["text"], "In the vault");
    assert_eq!(captured[0].body["truncate"], "END");
}

#[tokio::test]
async fn local_validation_blocks_hostile_sizes_before_transport() {
    let transport = Arc::new(MockTransport::default());
    let adapter = fixture_adapter(transport.clone());
    let mut embedding = embedding_request(EmbeddingInputRole::Query);
    embedding.inputs = vec!["x".repeat(1_025)];
    let error = adapter
        .embed(embedding, context())
        .await
        .expect_err("oversize text rejected");
    assert_eq!(error.kind, RetrievalErrorKind::InvalidRequest);

    let mut rerank = rerank_request();
    rerank.passages = (0..9).map(|index| format!("passage {index}")).collect();
    let error = adapter
        .rerank(rerank, context())
        .await
        .expect_err("passage count rejected");
    assert_eq!(error.kind, RetrievalErrorKind::InvalidRequest);
    assert!(transport.captured().is_empty());
}

#[tokio::test]
async fn accepted_response_becomes_poll_required_without_following_location() {
    let transport = Arc::new(MockTransport::with_responses(vec![response_with_headers(
        202,
        [("retry-after", "3"), ("x-request-id", "request-202")],
    )]));
    let adapter = fixture_adapter(transport);

    let error = adapter
        .embed(embedding_request(EmbeddingInputRole::Query), context())
        .await
        .expect_err("202 is not a completed embedding");

    assert_eq!(error.kind, RetrievalErrorKind::PollRequired);
    assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
    assert_eq!(error.provider_request_id.as_deref(), Some("request-202"));
    assert!(error.retryable);
}

#[tokio::test]
async fn status_codes_are_sanitized_and_typed() {
    let statuses = [
        (400, RetrievalErrorKind::InvalidRequest),
        (401, RetrievalErrorKind::Authentication),
        (402, RetrievalErrorKind::PaymentRequired),
        (422, RetrievalErrorKind::InvalidRequest),
        (429, RetrievalErrorKind::RateLimited),
        (503, RetrievalErrorKind::Unavailable),
    ];
    let transport = Arc::new(MockTransport::with_responses(
        statuses
            .iter()
            .map(|(status, _)| response_with_headers(*status, []))
            .collect(),
    ));
    let adapter = fixture_adapter(transport);

    for (status, kind) in statuses {
        let error = adapter
            .embed(embedding_request(EmbeddingInputRole::Query), context())
            .await
            .expect_err("fixture status fails");
        assert_eq!(error.kind, kind);
        assert_eq!(error.http_status, Some(status));
        let serialized = serde_json::to_string(&error).expect("error serializes");
        assert!(!serialized.contains("nvapi-should-not-escape"));
    }
}

#[tokio::test]
async fn malformed_embedding_dimensions_and_indexes_are_rejected() {
    let transport = Arc::new(MockTransport::with_responses(vec![
        response(
            200,
            json!({
                "model": EMBEDDING_MODEL,
                "data": [
                    {"index": 0, "embedding": [1.0, 2.0]},
                    {"index": 1, "embedding": [3.0, 4.0]}
                ]
            }),
        ),
        response(
            200,
            json!({
                "model": EMBEDDING_MODEL,
                "data": [
                    {"index": 0, "embedding": [1.0, 2.0, 3.0]},
                    {"index": 0, "embedding": [4.0, 5.0, 6.0]}
                ]
            }),
        ),
    ]));
    let adapter = fixture_adapter(transport);

    for _ in 0..2 {
        let error = adapter
            .embed(embedding_request(EmbeddingInputRole::Query), context())
            .await
            .expect_err("malformed response rejected");
        assert_eq!(error.kind, RetrievalErrorKind::MalformedResponse);
    }
}

#[tokio::test]
async fn malformed_rerank_order_duplicates_and_nonfinite_scores_are_rejected() {
    let transport = Arc::new(MockTransport::with_responses(vec![
        response(
            200,
            json!({"rankings": [
                {"index": 0, "logit": 0.1},
                {"index": 1, "logit": 0.9}
            ]}),
        ),
        response(
            200,
            json!({"rankings": [
                {"index": 0, "logit": 0.9},
                {"index": 0, "logit": 0.1}
            ]}),
        ),
        response(
            200,
            json!({"rankings": [
                {"index": 0, "logit": "NaN"},
                {"index": 1, "logit": 0.1}
            ]}),
        ),
    ]));
    let adapter = fixture_adapter(transport);

    for _ in 0..3 {
        let error = adapter
            .rerank(rerank_request(), context())
            .await
            .expect_err("malformed rerank rejected");
        assert_eq!(error.kind, RetrievalErrorKind::MalformedResponse);
    }
}

#[tokio::test]
async fn cancellation_wins_over_a_late_transport() {
    let transport = Arc::new(MockTransport::pending());
    let adapter = Arc::new(fixture_adapter(transport));
    let cancellation = CancellationToken::new();
    let task_context = RequestContext {
        deadline: tokio::time::Instant::now() + Duration::from_secs(5),
        cancellation: cancellation.clone(),
        cancellation_generation: 9,
    };
    let task_adapter = adapter.clone();
    let task = tokio::spawn(async move {
        task_adapter
            .embed(embedding_request(EmbeddingInputRole::Query), task_context)
            .await
    });
    tokio::task::yield_now().await;
    cancellation.cancel();

    let error = task
        .await
        .expect("task joined")
        .expect_err("operation cancelled");
    assert_eq!(error.kind, RetrievalErrorKind::Cancelled);
}

#[tokio::test]
async fn cancellation_also_interrupts_hung_credential_resolution() {
    let reference = SecretReference::new("provider/nvidia-nim/hanging").expect("reference");
    let embedding = NvidiaEmbeddingModel::new(EMBEDDING_MODEL, Some(3)).expect("embedding model");
    let adapter = Arc::new(
        NvidiaNimAdapter::new(
            NvidiaNimConfig {
                credential: reference,
                secrets: Arc::new(HangingSecretResolver),
                embedding_models: vec![embedding],
                rerank_models: vec![],
            },
            Arc::new(MockTransport::default()),
        )
        .expect("adapter"),
    );
    let cancellation = CancellationToken::new();
    let task_context = RequestContext {
        deadline: tokio::time::Instant::now() + Duration::from_secs(5),
        cancellation: cancellation.clone(),
        cancellation_generation: 11,
    };
    let task_adapter = adapter.clone();
    let task = tokio::spawn(async move {
        task_adapter
            .embed(embedding_request(EmbeddingInputRole::Query), task_context)
            .await
    });
    tokio::task::yield_now().await;
    cancellation.cancel();

    let error = task
        .await
        .expect("task joined")
        .expect_err("operation cancelled");
    assert_eq!(error.kind, RetrievalErrorKind::Cancelled);
}

#[tokio::test]
async fn elapsed_deadline_is_rejected_before_resolving_or_sending() {
    let transport = Arc::new(MockTransport::default());
    let adapter = fixture_adapter(transport.clone());
    let context = RequestContext {
        deadline: tokio::time::Instant::now() - Duration::from_millis(1),
        cancellation: CancellationToken::new(),
        cancellation_generation: 1,
    };

    let error = adapter
        .embed(embedding_request(EmbeddingInputRole::Query), context)
        .await
        .expect_err("deadline rejected");
    assert_eq!(error.kind, RetrievalErrorKind::DeadlineExceeded);
    assert!(transport.captured().is_empty());
}

#[test]
fn curated_endpoints_reject_arbitrary_egress_and_accept_exact_nvidia_paths() {
    let accepted = Url::parse(
        "https://ai.api.nvidia.com/v1/retrieval/nvidia/llama-nemotron-rerank-1b-v2/reranking",
    )
    .expect("URL");
    assert!(NvidiaRerankModel::from_curated_metadata(RERANK_MODEL, accepted).is_ok());

    for hostile in [
        "https://evil.example/v1/retrieval/nvidia/model/reranking",
        "http://ai.api.nvidia.com/v1/retrieval/nvidia/model/reranking",
        "https://ai.api.nvidia.com/v1/retrieval/nvidia/../model/reranking",
        "https://ai.api.nvidia.com/v1/retrieval/nvidia/%2e%2e/model/reranking",
        "https://ai.api.nvidia.com/v1/retrieval/nvidia/model/reranking?token=leak",
    ] {
        let url = Url::parse(hostile).expect("fixture URL");
        assert!(NvidiaRerankModel::from_curated_metadata(RERANK_MODEL, url).is_err());
    }
}

#[tokio::test]
async fn exact_model_ids_are_not_remapped_to_aliases() {
    let transport = Arc::new(MockTransport::default());
    let adapter = fixture_adapter(transport.clone());
    let mut request = embedding_request(EmbeddingInputRole::Query);
    request.model_id = "nvidia/latest-embedding".to_owned();

    let error = adapter
        .embed(request, context())
        .await
        .expect_err("unlisted alias rejected");
    assert_eq!(error.kind, RetrievalErrorKind::InvalidRequest);
    assert!(transport.captured().is_empty());
}

#[test]
fn capabilities_make_external_egress_and_no_fallback_explicit() {
    let adapter = fixture_adapter(Arc::new(MockTransport::default()));
    let capabilities = EmbeddingProvider::capabilities(&adapter);

    assert!(capabilities.embeddings);
    assert!(capabilities.reranking);
    assert!(capabilities.cancellation);
    assert!(capabilities.deadlines);
    assert!(capabilities.exact_model_ids);
    assert!(capabilities.privacy.query_leaves_device);
    assert!(capabilities.privacy.passages_leave_device);
    assert!(capabilities.privacy.provider_may_retain_data);
    assert!(capabilities.privacy.no_automatic_fallback);
}
