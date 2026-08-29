#![forbid(unsafe_code)]

mod contract;
mod error;
mod nvidia;
mod secret;
mod transport;

pub use contract::{
    EmbeddingInputRole, EmbeddingProvider, EmbeddingRequestV1, EmbeddingResponseV1,
    EmbeddingVector, ProviderLimits, ProviderPrivacy, RequestContext, RerankProvider,
    RerankRequestV1, RerankResponseV1, RerankScore, RetrievalCapabilitiesV1, TokenUsage,
    TruncationPolicy,
};
pub use error::{RetrievalError, RetrievalErrorKind};
pub use nvidia::{
    CuratedNvidiaRerankEndpoint, NvidiaEmbeddingModel, NvidiaNimAdapter, NvidiaNimConfig,
    NvidiaRerankEndpoint, NvidiaRerankModel, NVIDIA_EMBEDDINGS_ENDPOINT,
    NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID, NVIDIA_PROVIDER_ID, NVIDIA_RERANKING_ENDPOINT,
};
pub use secret::{MemorySecretResolver, SecretReference, SecretResolver, SecretString};
pub use transport::{
    HttpMethod, HttpRequest, HttpResponse, HttpTransport, ReqwestTransport, TransportAuth,
    TransportError,
};
