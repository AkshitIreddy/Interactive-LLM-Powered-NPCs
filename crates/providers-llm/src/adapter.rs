use std::{collections::BTreeMap, fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Client, Response,
};
use serde_json::{json, Map, Value};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    parser::ProtocolParser, sse::SseDecoder, AdapterConfig, CapabilitySupport, ChatMessage,
    ChatRole, CompatibleOptions, ErrorKind, LlmEvent, LlmEventStream, LlmRequest,
    ModelCapabilities, ModelInfo, PrivacyCapabilities, ProviderCapabilities, ProviderError,
    ProviderProtocol, SecretBytes, SecretProvider, SecretReference, ToolDefinition,
};

const MAX_MODEL_LIST_BYTES: usize = 4 * 1_048_576;
pub const NVIDIA_NIM_HOSTED_BASE_URL: &str = "https://integrate.api.nvidia.com/";
pub const NVIDIA_NIM_DEFAULT_MODEL_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const NVIDIA_NIM_MIN_MODEL_CACHE_TTL: Duration = Duration::from_millis(100);
const NVIDIA_NIM_MAX_MODEL_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[async_trait]
pub trait HostedLanguageModel: Send + Sync {
    fn capabilities(&self) -> &ProviderCapabilities;
    async fn stream(
        &self,
        request: LlmRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmEventStream, ProviderError>;
    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError>;
}

#[derive(Clone)]
struct AdapterInner {
    config: AdapterConfig,
    capabilities: ProviderCapabilities,
    client: Client,
    compatible: Option<CompatibleOptions>,
}

impl fmt::Debug for AdapterInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdapterInner")
            .field("config", &self.config)
            .field("capabilities", &self.capabilities)
            .field("client", &"<HTTP_CLIENT>")
            .field("compatible", &self.compatible)
            .finish()
    }
}

impl AdapterInner {
    fn new(
        config: AdapterConfig,
        capabilities: ProviderCapabilities,
        compatible: Option<CompatibleOptions>,
    ) -> Result<Self, ProviderError> {
        config.validate(&capabilities.provider_id)?;
        capabilities.validate()?;
        let client = Client::builder()
            .connect_timeout(config.request_timeout.min(Duration::from_secs(20)))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("interactive-npcs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                ProviderError::new(
                    &capabilities.provider_id,
                    ErrorKind::Internal,
                    "HTTP client initialization failed",
                )
            })?;
        Ok(Self {
            config,
            capabilities,
            client,
            compatible,
        })
    }

    async fn stream(
        &self,
        request: LlmRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmEventStream, ProviderError> {
        let provider_id = self.capabilities.provider_id.clone();
        request.validate(&provider_id)?;
        if !request.tools.is_empty() && !self.capabilities.tool_calls {
            return Err(ProviderError::new(
                &provider_id,
                ErrorKind::InvalidRequest,
                "provider does not support tool calls",
            ));
        }
        if request.response_json_schema.is_some() && !self.capabilities.json_schema {
            return Err(ProviderError::new(
                &provider_id,
                ErrorKind::InvalidRequest,
                "provider does not support JSON schema output",
            ));
        }
        if self.capabilities.protocol == ProviderProtocol::CohereChat
            && !request.tools.is_empty()
            && request.response_json_schema.is_some()
        {
            return Err(ProviderError::new(
                &provider_id,
                ErrorKind::InvalidRequest,
                "Cohere JSON schema output cannot be combined with tools",
            ));
        }

        let deadline = Instant::now() + self.config.request_timeout;
        let secret = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(&provider_id)),
            result = tokio::time::timeout_at(deadline, self.config.secrets.resolve(&self.config.credential)) => {
                match result {
                    Ok(Ok(secret)) => secret,
                    Ok(Err(_)) => return Err(ProviderError::new(&provider_id, ErrorKind::SecretUnavailable, "provider credential is unavailable")),
                    Err(_) => return Err(ProviderError::timeout(&provider_id)),
                }
            }
        };
        let response = self
            .send_generation(&request, &secret, deadline, &cancellation)
            .await?;
        drop(secret);
        self.validate_stream_response(response, deadline, cancellation)
    }

    async fn send_generation(
        &self,
        request: &LlmRequest,
        secret: &SecretBytes,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Response, ProviderError> {
        let endpoint = generation_endpoint(
            &self.config.base_url,
            self.capabilities.protocol,
            &request.model,
        )?;
        let body = request_body(
            self.capabilities.protocol,
            request,
            self.compatible.as_ref(),
        )?;
        let headers = auth_headers(
            self.capabilities.protocol,
            secret,
            &self.capabilities.provider_id,
        )?;
        let future = self
            .client
            .post(endpoint)
            .headers(headers)
            .header(ACCEPT, "text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(&self.capabilities.provider_id)),
            result = tokio::time::timeout_at(deadline, future) => match result {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => return Err(ProviderError::transport(&self.capabilities.provider_id, &error)),
                Err(_) => return Err(ProviderError::timeout(&self.capabilities.provider_id)),
            }
        };
        if response.status() == reqwest::StatusCode::ACCEPTED {
            return Err(accepted_response_error(
                response,
                deadline,
                cancellation,
                &self.capabilities.provider_id,
                secret,
            )
            .await);
        }
        if !response.status().is_success() {
            return Err(http_error(&self.capabilities.provider_id, &response));
        }
        Ok(response)
    }

    fn validate_stream_response(
        &self,
        response: Response,
        deadline: Instant,
        cancellation: CancellationToken,
    ) -> Result<LlmEventStream, ProviderError> {
        let provider_id = self.capabilities.provider_id.clone();
        let protocol = self.capabilities.protocol;
        let content_type_ok = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"));
        if !content_type_ok {
            return Err(ProviderError::protocol(
                &provider_id,
                "provider response is not an SSE stream",
            ));
        }
        let mut network = response.bytes_stream();
        let stream = async_stream::stream! {
            let mut decoder = SseDecoder::default();
            let mut parser = ProtocolParser::new(provider_id.clone(), protocol);
            loop {
                let next = tokio::select! {
                    _ = cancellation.cancelled() => {
                        yield LlmEvent::Error { error: ProviderError::cancelled(&provider_id) };
                        return;
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        yield LlmEvent::Error { error: ProviderError::timeout(&provider_id) };
                        return;
                    }
                    next = network.next() => next,
                };
                match next {
                    Some(Ok(bytes)) => {
                        let frames = match decoder.push(&bytes) {
                            Ok(frames) => frames,
                            Err(()) => {
                                yield LlmEvent::Error { error: ProviderError::protocol(&provider_id, "SSE framing is invalid or oversized") };
                                return;
                            }
                        };
                        for frame in frames {
                            match parser.parse(frame) {
                                Ok(events) => {
                                    for event in events {
                                        let terminal = event.terminal();
                                        yield event;
                                        if terminal {
                                            return;
                                        }
                                    }
                                }
                                Err(error) => {
                                    yield LlmEvent::Error { error };
                                    return;
                                }
                            }
                        }
                    }
                    Some(Err(error)) => {
                        yield LlmEvent::Error { error: ProviderError::transport(&provider_id, &error) };
                        return;
                    }
                    None => break,
                }
            }
            let frames = match decoder.finish() {
                Ok(frames) => frames,
                Err(()) => {
                    yield LlmEvent::Error { error: ProviderError::protocol(&provider_id, "SSE stream ended with invalid framing") };
                    return;
                }
            };
            for frame in frames {
                match parser.parse(frame) {
                    Ok(events) => for event in events {
                        let terminal = event.terminal();
                        yield event;
                        if terminal { return; }
                    },
                    Err(error) => {
                        yield LlmEvent::Error { error };
                        return;
                    }
                }
            }
            if !parser.terminal() {
                yield LlmEvent::Error { error: ProviderError::protocol(&provider_id, "provider stream ended before a terminal event") };
            }
        };
        Ok(Box::pin(stream))
    }

    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let provider_id = &self.capabilities.provider_id;
        if !self.capabilities.model_listing {
            return Err(ProviderError::new(
                provider_id,
                ErrorKind::InvalidRequest,
                "provider does not support model listing",
            ));
        }
        let deadline = Instant::now() + self.config.request_timeout;
        let secret = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(provider_id)),
            result = tokio::time::timeout_at(deadline, self.config.secrets.resolve(&self.config.credential)) => match result {
                Ok(Ok(secret)) => secret,
                Ok(Err(_)) => return Err(ProviderError::new(provider_id, ErrorKind::SecretUnavailable, "provider credential is unavailable")),
                Err(_) => return Err(ProviderError::timeout(provider_id)),
            }
        };
        if self.capabilities.protocol == ProviderProtocol::CohereChat {
            let result = self
                .list_cohere_models(&secret, deadline, &cancellation)
                .await;
            drop(secret);
            return result;
        }
        let endpoint = model_endpoint(&self.config.base_url, self.capabilities.protocol)?;
        let headers = auth_headers(self.capabilities.protocol, &secret, provider_id)?;
        let future = self.client.get(endpoint).headers(headers).send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(provider_id)),
            result = tokio::time::timeout_at(deadline, future) => match result {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => return Err(ProviderError::transport(provider_id, &error)),
                Err(_) => return Err(ProviderError::timeout(provider_id)),
            }
        };
        drop(secret);
        if !response.status().is_success() {
            return Err(http_error(provider_id, &response));
        }
        let body = read_limited(response, deadline, &cancellation, provider_id).await?;
        parse_models(self.capabilities.protocol, &body, provider_id)
    }

    async fn list_cohere_models(
        &self,
        secret: &SecretBytes,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let provider_id = &self.capabilities.provider_id;
        let mut models = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..8 {
            let mut endpoint = append_endpoint(&self.config.base_url, "v1/models")?;
            {
                let mut query = endpoint.query_pairs_mut();
                query.append_pair("page_size", "1000");
                query.append_pair("endpoint", "chat");
                if let Some(token) = &page_token {
                    query.append_pair("page_token", token);
                }
            }
            let headers = auth_headers(self.capabilities.protocol, secret, provider_id)?;
            let future = self.client.get(endpoint).headers(headers).send();
            let response = tokio::select! {
                _ = cancellation.cancelled() => return Err(ProviderError::cancelled(provider_id)),
                result = tokio::time::timeout_at(deadline, future) => match result {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => return Err(ProviderError::transport(provider_id, &error)),
                    Err(_) => return Err(ProviderError::timeout(provider_id)),
                }
            };
            if !response.status().is_success() {
                return Err(http_error(provider_id, &response));
            }
            let body = read_limited(response, deadline, cancellation, provider_id).await?;
            let (mut page, next_page_token) = parse_cohere_model_page(&body, provider_id)?;
            models.append(&mut page);
            if models.len() > 4_096 {
                return Err(ProviderError::protocol(
                    provider_id,
                    "model list contains too many entries",
                ));
            }
            match next_page_token {
                Some(token) => page_token = Some(token),
                None => {
                    models.sort_by(|left, right| left.id.cmp(&right.id));
                    models.dedup_by(|left, right| left.id == right.id);
                    return Ok(models);
                }
            }
        }
        Err(ProviderError::protocol(
            provider_id,
            "model list exceeded the pagination limit",
        ))
    }
}

macro_rules! adapter_type {
    ($name:ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(AdapterInner);

        #[async_trait]
        impl HostedLanguageModel for $name {
            fn capabilities(&self) -> &ProviderCapabilities {
                &self.0.capabilities
            }

            async fn stream(
                &self,
                request: LlmRequest,
                cancellation: CancellationToken,
            ) -> Result<LlmEventStream, ProviderError> {
                self.0.stream(request, cancellation).await
            }

            async fn list_models(
                &self,
                cancellation: CancellationToken,
            ) -> Result<Vec<ModelInfo>, ProviderError> {
                self.0.list_models(cancellation).await
            }
        }
    };
}

adapter_type!(OpenAiResponses);
adapter_type!(AnthropicMessages);
adapter_type!(GeminiGenerateContent);
adapter_type!(GroqChatCompletions);
adapter_type!(OpenAiCompatible);
adapter_type!(CohereChat);

#[derive(Clone, Debug)]
pub struct NvidiaNimChat {
    inner: AdapterInner,
    verified_model_capabilities: Arc<BTreeMap<String, ModelCapabilities>>,
    model_cache: Arc<tokio::sync::Mutex<Option<NvidiaModelCache>>>,
    model_cache_ttl: Duration,
}

#[derive(Clone, Debug)]
struct NvidiaModelCache {
    expires_at: Instant,
    models: Vec<ModelInfo>,
}

impl NvidiaNimChat {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        Self::with_verified_model_capabilities(config, BTreeMap::new())
    }

    pub fn hosted(
        credential: SecretReference,
        secrets: Arc<dyn SecretProvider>,
        request_timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let base_url = Url::parse(NVIDIA_NIM_HOSTED_BASE_URL).map_err(|_| {
            ProviderError::new(
                "nvidia-nim",
                ErrorKind::Internal,
                "NVIDIA NIM hosted endpoint is invalid",
            )
        })?;
        Self::new(AdapterConfig {
            base_url,
            credential,
            secrets,
            request_timeout,
            allow_insecure_loopback: false,
        })
    }

    pub fn with_verified_model_capabilities(
        config: AdapterConfig,
        verified_model_capabilities: BTreeMap<String, ModelCapabilities>,
    ) -> Result<Self, ProviderError> {
        Self::with_verified_model_capabilities_and_cache_ttl(
            config,
            verified_model_capabilities,
            NVIDIA_NIM_DEFAULT_MODEL_CACHE_TTL,
        )
    }

    pub fn with_verified_model_capabilities_and_cache_ttl(
        config: AdapterConfig,
        verified_model_capabilities: BTreeMap<String, ModelCapabilities>,
        model_cache_ttl: Duration,
    ) -> Result<Self, ProviderError> {
        if !trusted_nvidia_endpoint(&config)
            || verified_model_capabilities.len() > 4_096
            || verified_model_capabilities
                .keys()
                .any(|model_id| !crate::types::valid_model_id(model_id))
            || !(NVIDIA_NIM_MIN_MODEL_CACHE_TTL..=NVIDIA_NIM_MAX_MODEL_CACHE_TTL)
                .contains(&model_cache_ttl)
        {
            return Err(ProviderError::new(
                "nvidia-nim",
                ErrorKind::InvalidRequest,
                "NVIDIA NIM adapter configuration is invalid",
            ));
        }
        let inner = AdapterInner::new(
            config,
            capabilities(
                "nvidia-nim",
                "NVIDIA NIM API Catalog",
                ProviderProtocol::NvidiaNimChat,
                true,
                true,
                false,
                true,
            ),
            None,
        )?;
        Ok(Self {
            inner,
            verified_model_capabilities: Arc::new(verified_model_capabilities),
            model_cache: Arc::new(tokio::sync::Mutex::new(None)),
            model_cache_ttl,
        })
    }

    fn validate_model_features(&self, request: &LlmRequest) -> Result<(), ProviderError> {
        let evidence = self.verified_model_capabilities.get(&request.model);
        if !request.tools.is_empty()
            && evidence.map(|value| value.tool_calls) != Some(CapabilitySupport::Supported)
        {
            return Err(ProviderError::new(
                "nvidia-nim",
                ErrorKind::InvalidRequest,
                "tool calling is not verified for this exact NVIDIA model ID",
            ));
        }
        if request.response_json_schema.is_some()
            && evidence.map(|value| value.json_schema) != Some(CapabilitySupport::Supported)
        {
            return Err(ProviderError::new(
                "nvidia-nim",
                ErrorKind::InvalidRequest,
                "JSON schema output is not verified for this exact NVIDIA model ID",
            ));
        }
        if evidence.is_some_and(|value| value.streaming == CapabilitySupport::Unsupported) {
            return Err(ProviderError::new(
                "nvidia-nim",
                ErrorKind::InvalidRequest,
                "streaming is explicitly unsupported for this NVIDIA model ID",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl HostedLanguageModel for NvidiaNimChat {
    fn capabilities(&self) -> &ProviderCapabilities {
        &self.inner.capabilities
    }

    async fn stream(
        &self,
        request: LlmRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmEventStream, ProviderError> {
        self.validate_model_features(&request)?;
        self.inner.stream(request, cancellation).await
    }

    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut cache = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled("nvidia-nim")),
            cache = self.model_cache.lock() => cache,
        };
        let now = Instant::now();
        if let Some(cached) = cache.as_ref().filter(|cached| cached.expires_at > now) {
            return Ok(cached.models.clone());
        }
        let mut models = self.inner.list_models(cancellation).await?;
        for model in &mut models {
            model.capabilities = self
                .verified_model_capabilities
                .get(&model.id)
                .cloned()
                .unwrap_or_else(|| ModelCapabilities {
                    streaming: CapabilitySupport::Supported,
                    tool_calls: CapabilitySupport::Unknown,
                    json_schema: CapabilitySupport::Unknown,
                    reasoning: CapabilitySupport::Unknown,
                });
        }
        *cache = Some(NvidiaModelCache {
            expires_at: now + self.model_cache_ttl,
            models: models.clone(),
        });
        Ok(models)
    }
}

impl OpenAiResponses {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        Ok(Self(AdapterInner::new(
            config,
            capabilities(
                "openai",
                "OpenAI",
                ProviderProtocol::OpenAiResponses,
                true,
                true,
                true,
                false,
            ),
            None,
        )?))
    }
}

impl AnthropicMessages {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        Ok(Self(AdapterInner::new(
            config,
            capabilities(
                "anthropic",
                "Anthropic",
                ProviderProtocol::AnthropicMessages,
                true,
                true,
                false,
                true,
            ),
            None,
        )?))
    }
}

impl GeminiGenerateContent {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        Ok(Self(AdapterInner::new(
            config,
            capabilities(
                "gemini",
                "Google Gemini",
                ProviderProtocol::GeminiGenerateContent,
                true,
                true,
                true,
                false,
            ),
            None,
        )?))
    }
}

impl GroqChatCompletions {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        let compatible = CompatibleOptions {
            provider_id: "groq".into(),
            display_name: "Groq".into(),
            supports_model_listing: true,
            supports_json_schema: true,
            send_store_false: false,
            provider_may_retain_data: true,
        };
        Ok(Self(AdapterInner::new(
            config,
            compatible_capabilities(&compatible),
            Some(compatible),
        )?))
    }
}

impl CohereChat {
    pub fn new(config: AdapterConfig) -> Result<Self, ProviderError> {
        Ok(Self(AdapterInner::new(
            config,
            capabilities(
                "cohere",
                "Cohere",
                ProviderProtocol::CohereChat,
                true,
                true,
                false,
                true,
            ),
            None,
        )?))
    }
}

impl OpenAiCompatible {
    pub fn new(config: AdapterConfig, options: CompatibleOptions) -> Result<Self, ProviderError> {
        if !crate::types::valid_stable_id(&options.provider_id)
            || options.display_name.trim().is_empty()
            || options.display_name.len() > 128
        {
            return Err(ProviderError::new(
                "openai-compatible",
                ErrorKind::InvalidRequest,
                "OpenAI-compatible provider options are invalid",
            ));
        }
        Ok(Self(AdapterInner::new(
            config,
            compatible_capabilities(&options),
            Some(options),
        )?))
    }
}

fn capabilities(
    id: &str,
    name: &str,
    protocol: ProviderProtocol,
    model_listing: bool,
    json_schema: bool,
    request_disables_storage: bool,
    provider_may_retain_data: bool,
) -> ProviderCapabilities {
    ProviderCapabilities {
        provider_id: id.into(),
        display_name: name.into(),
        protocol,
        streaming: true,
        cancellation: true,
        tool_calls: true,
        usage_reporting: true,
        model_listing,
        json_schema,
        privacy: PrivacyCapabilities {
            application_stateless: true,
            request_disables_storage,
            provider_may_retain_data,
        },
    }
}

fn compatible_capabilities(options: &CompatibleOptions) -> ProviderCapabilities {
    capabilities(
        &options.provider_id,
        &options.display_name,
        ProviderProtocol::OpenAiChatCompletions,
        options.supports_model_listing,
        options.supports_json_schema,
        options.send_store_false,
        options.provider_may_retain_data,
    )
}

fn generation_endpoint(
    base: &Url,
    protocol: ProviderProtocol,
    model: &str,
) -> Result<Url, ProviderError> {
    let relative = match protocol {
        ProviderProtocol::OpenAiResponses => "responses".to_owned(),
        ProviderProtocol::AnthropicMessages => "messages".to_owned(),
        ProviderProtocol::OpenAiChatCompletions => "chat/completions".to_owned(),
        ProviderProtocol::CohereChat => "v2/chat".to_owned(),
        ProviderProtocol::NvidiaNimChat => "v1/chat/completions".to_owned(),
        ProviderProtocol::GeminiGenerateContent => {
            let model = model.strip_prefix("models/").unwrap_or(model);
            format!("models/{model}:streamGenerateContent?alt=sse")
        }
    };
    append_endpoint(base, &relative)
}

fn model_endpoint(base: &Url, protocol: ProviderProtocol) -> Result<Url, ProviderError> {
    let relative = match protocol {
        ProviderProtocol::GeminiGenerateContent => "models?pageSize=1000",
        ProviderProtocol::CohereChat => "v1/models?page_size=1000&endpoint=chat",
        ProviderProtocol::NvidiaNimChat => "v1/models",
        _ => "models",
    };
    append_endpoint(base, relative)
}

fn append_endpoint(base: &Url, relative: &str) -> Result<Url, ProviderError> {
    let mut url = base.clone();
    let (path_part, query) = relative.split_once('?').unwrap_or((relative, ""));
    let path = format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        path_part.trim_start_matches('/')
    );
    url.set_path(&path);
    url.set_query((!query.is_empty()).then_some(query));
    Ok(url)
}

fn auth_headers(
    protocol: ProviderProtocol,
    secret: &SecretBytes,
    provider_id: &str,
) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    match protocol {
        ProviderProtocol::AnthropicMessages => {
            headers.insert(
                "x-api-key",
                HeaderValue::from_bytes(secret.expose())
                    .map_err(|_| invalid_credential(provider_id))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        ProviderProtocol::GeminiGenerateContent => {
            headers.insert(
                "x-goog-api-key",
                HeaderValue::from_bytes(secret.expose())
                    .map_err(|_| invalid_credential(provider_id))?,
            );
        }
        ProviderProtocol::OpenAiResponses
        | ProviderProtocol::OpenAiChatCompletions
        | ProviderProtocol::CohereChat
        | ProviderProtocol::NvidiaNimChat => {
            let mut bearer = Zeroizing::new(Vec::with_capacity(7 + secret.expose().len()));
            bearer.extend_from_slice(b"Bearer ");
            bearer.extend_from_slice(secret.expose());
            let value =
                HeaderValue::from_bytes(&bearer).map_err(|_| invalid_credential(provider_id))?;
            headers.insert(AUTHORIZATION, value);
            bearer.zeroize();
            if protocol == ProviderProtocol::CohereChat {
                headers.insert(
                    "x-client-name",
                    HeaderValue::from_static("interactive-npcs"),
                );
            }
        }
    }
    Ok(headers)
}

fn invalid_credential(provider_id: &str) -> ProviderError {
    ProviderError::new(
        provider_id,
        ErrorKind::Authentication,
        "provider credential has an invalid header representation",
    )
}

fn trusted_nvidia_endpoint(config: &AdapterConfig) -> bool {
    let url = &config.base_url;
    let hosted = url.scheme() == "https"
        && url.host_str() == Some("integrate.api.nvidia.com")
        && url.port().is_none()
        && url.path() == "/";
    let fixture = config.allow_insecure_loopback
        && url.scheme() == "http"
        && matches!(
            url.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("::1")
        );
    hosted || fixture
}

fn request_body(
    protocol: ProviderProtocol,
    request: &LlmRequest,
    compatible: Option<&CompatibleOptions>,
) -> Result<Value, ProviderError> {
    match protocol {
        ProviderProtocol::OpenAiResponses => openai_responses_body(request),
        ProviderProtocol::AnthropicMessages => anthropic_body(request),
        ProviderProtocol::GeminiGenerateContent => gemini_body(request),
        ProviderProtocol::OpenAiChatCompletions => compatible_body(request, compatible),
        ProviderProtocol::CohereChat => cohere_body(request),
        ProviderProtocol::NvidiaNimChat => nvidia_nim_body(request),
    }
}

fn openai_responses_body(request: &LlmRequest) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("store".into(), Value::Bool(false));
    body.insert(
        "max_output_tokens".into(),
        json!(request.maximum_output_tokens),
    );
    body.insert(
        "input".into(),
        Value::Array(
            request
                .messages
                .iter()
                .map(openai_response_message)
                .collect::<Result<_, _>>()?,
        ),
    );
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(request.tools.iter().map(openai_response_tool).collect()),
        );
    }
    if let Some(schema) = &request.response_json_schema {
        body.insert("text".into(), json!({"format":{"type":"json_schema","name":"npc_response","strict":true,"schema":schema}}));
    }
    Ok(Value::Object(body))
}

fn openai_response_message(message: &ChatMessage) -> Result<Value, ProviderError> {
    if message.role == ChatRole::Tool {
        return Ok(json!({
            "type":"function_call_output",
            "call_id":message.tool_call_id,
            "output":message.content,
        }));
    }
    Ok(json!({
        "role": match message.role { ChatRole::System => "system", ChatRole::User => "user", ChatRole::Assistant => "assistant", ChatRole::Tool => unreachable!() },
        "content": message.content,
    }))
}

fn openai_response_tool(tool: &ToolDefinition) -> Value {
    json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.input_schema,"strict":true})
}

fn anthropic_body(request: &LlmRequest) -> Result<Value, ProviderError> {
    let system = request
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::System)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages = request.messages.iter().filter(|message| message.role != ChatRole::System).map(|message| {
        match message.role {
            ChatRole::User => Ok(json!({"role":"user","content":message.content})),
            ChatRole::Assistant => Ok(json!({"role":"assistant","content":message.content})),
            ChatRole::Tool => {
                let content: Value = serde_json::from_str(&message.content).unwrap_or_else(|_| Value::String(message.content.clone()));
                Ok(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":message.tool_call_id,"content":content}]}))
            }
            ChatRole::System => unreachable!(),
        }
    }).collect::<Result<Vec<_>, ProviderError>>()?;
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("max_tokens".into(), json!(request.maximum_output_tokens));
    body.insert("messages".into(), Value::Array(messages));
    if !system.is_empty() {
        body.insert("system".into(), Value::String(system));
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert("tools".into(), Value::Array(request.tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"input_schema":tool.input_schema})).collect()));
    }
    if let Some(schema) = &request.response_json_schema {
        body.insert(
            "output_config".into(),
            json!({"format":{"type":"json_schema","schema":schema}}),
        );
    }
    Ok(Value::Object(body))
}

fn gemini_body(request: &LlmRequest) -> Result<Value, ProviderError> {
    let system = request
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::System)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let contents = request.messages.iter().filter(|message| message.role != ChatRole::System).map(|message| {
        match message.role {
            ChatRole::User => Ok(json!({"role":"user","parts":[{"text":message.content}]})),
            ChatRole::Assistant => Ok(json!({"role":"model","parts":[{"text":message.content}]})),
            ChatRole::Tool => {
                let response: Value = serde_json::from_str(&message.content).map_err(|_| ProviderError::new("gemini", ErrorKind::InvalidRequest, "Gemini tool results must be JSON"))?;
                Ok(json!({"role":"user","parts":[{"functionResponse":{"name":message.name,"response":response}}]}))
            }
            ChatRole::System => unreachable!(),
        }
    }).collect::<Result<Vec<_>, ProviderError>>()?;
    let mut generation = Map::new();
    generation.insert(
        "maxOutputTokens".into(),
        json!(request.maximum_output_tokens),
    );
    if let Some(temperature) = request.temperature {
        generation.insert("temperature".into(), json!(temperature));
    }
    if let Some(schema) = &request.response_json_schema {
        generation.insert(
            "responseMimeType".into(),
            Value::String("application/json".into()),
        );
        generation.insert(
            "responseJsonSchema".into(),
            gemini_portable_json_schema(schema.clone()),
        );
    }
    let mut body = Map::new();
    body.insert("contents".into(), Value::Array(contents));
    body.insert("generationConfig".into(), Value::Object(generation));
    // Gemini's current REST surface supports request-level storage control.
    body.insert("store".into(), Value::Bool(false));
    if !system.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({"parts":[{"text":system}]}),
        );
    }
    if !request.tools.is_empty() {
        body.insert("tools".into(), json!([{"functionDeclarations": request.tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"parametersJsonSchema":tool.input_schema})).collect::<Vec<_>>() }]));
    }
    Ok(Value::Object(body))
}

fn compatible_body(
    request: &LlmRequest,
    options: Option<&CompatibleOptions>,
) -> Result<Value, ProviderError> {
    let options = options.ok_or_else(|| {
        ProviderError::new(
            "openai-compatible",
            ErrorKind::Internal,
            "compatible options are missing",
        )
    })?;
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("stream_options".into(), json!({"include_usage":true}));
    body.insert("max_tokens".into(), json!(request.maximum_output_tokens));
    body.insert("messages".into(), Value::Array(request.messages.iter().map(|message| {
        let mut value = json!({"role":match message.role { ChatRole::System=>"system", ChatRole::User=>"user", ChatRole::Assistant=>"assistant", ChatRole::Tool=>"tool" },"content":message.content});
        if let Some(name) = &message.name { value["name"] = json!(name); }
        if let Some(id) = &message.tool_call_id { value["tool_call_id"] = json!(id); }
        value
    }).collect()));
    if options.send_store_false {
        body.insert("store".into(), Value::Bool(false));
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert("tools".into(), Value::Array(request.tools.iter().map(|tool| json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.input_schema,"strict":true}})).collect()));
    }
    if let Some(schema) = &request.response_json_schema {
        body.insert("response_format".into(), json!({"type":"json_schema","json_schema":{"name":"npc_response","strict":true,"schema":schema}}));
    }
    apply_qualified_model_parameters(&mut body, options, &request.model);
    Ok(Value::Object(body))
}

fn apply_qualified_model_parameters(
    body: &mut Map<String, Value>,
    options: &CompatibleOptions,
    model: &str,
) {
    // Groq's Qwen 3.6 route otherwise spends a bounded NPC response budget on
    // reasoning tokens and can finish without any dialogue. These parameters
    // are deliberately exact-provider/exact-model: other Groq models retain
    // their documented defaults, and generic compatible endpoints never
    // inherit provider-specific request fields.
    if options.provider_id == "groq" && model == "qwen/qwen3.6-27b" {
        body.insert("reasoning_effort".into(), json!("none"));
        body.insert("reasoning_format".into(), json!("hidden"));
    }
    if options.provider_id == "openrouter" && model == "liquid/lfm-2.5-2.6b:free" {
        body.insert(
            "provider".into(),
            json!({ "allow_fallbacks": false, "require_parameters": true }),
        );
    }
}

fn gemini_portable_json_schema(mut schema: Value) -> Value {
    match &mut schema {
        Value::Object(object) => {
            if let Some(constant) = object.remove("const") {
                object
                    .entry("enum".to_owned())
                    .or_insert_with(|| Value::Array(vec![constant]));
            }
            for value in object.values_mut() {
                *value = gemini_portable_json_schema(value.take());
            }
        }
        Value::Array(values) => {
            for value in values {
                *value = gemini_portable_json_schema(value.take());
            }
        }
        _ => {}
    }
    schema
}

fn cohere_body(request: &LlmRequest) -> Result<Value, ProviderError> {
    let messages = request
        .messages
        .iter()
        .map(|message| match message.role {
            ChatRole::System | ChatRole::User | ChatRole::Assistant => Ok(json!({
                "role": match message.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => unreachable!(),
                },
                "content": message.content,
            })),
            ChatRole::Tool => Ok(json!({
                "role": "tool",
                "tool_call_id": message.tool_call_id,
                "content": [{"type":"document","document":{"data":message.content}}],
            })),
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    let mut body = Map::new();
    body.insert("model".into(), json!(request.model));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("max_tokens".into(), json!(request.maximum_output_tokens));
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type":"function",
                            "function":{
                                "name":tool.name,
                                "description":tool.description,
                                "parameters":tool.input_schema,
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(schema) = &request.response_json_schema {
        body.insert(
            "response_format".into(),
            json!({"type":"json_object","schema":schema}),
        );
    }
    if request.model == "command-a-plus-05-2026" {
        body.insert("thinking".into(), json!({ "type": "disabled" }));
    }
    Ok(Value::Object(body))
}

fn nvidia_nim_body(request: &LlmRequest) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("max_tokens".into(), json!(request.maximum_output_tokens));
    // Nemotron 3 enables reasoning by default. Interactive NPC turns need the
    // directly spoken answer, not a long visible chain-of-thought preamble.
    // NVIDIA's hosted API exposes this model-scoped chat-template control.
    if request.model.starts_with("nvidia/nemotron-3") {
        body.insert(
            "chat_template_kwargs".into(),
            json!({ "enable_thinking": false }),
        );
    }
    body.insert(
        "messages".into(),
        Value::Array(
            request
                .messages
                .iter()
                .map(|message| {
                    let mut value = json!({
                        "role": match message.role {
                            ChatRole::System => "system",
                            ChatRole::User => "user",
                            ChatRole::Assistant => "assistant",
                            ChatRole::Tool => "tool",
                        },
                        "content": message.content,
                    });
                    if let Some(name) = &message.name {
                        value["name"] = json!(name);
                    }
                    if let Some(id) = &message.tool_call_id {
                        value["tool_call_id"] = json!(id);
                    }
                    value
                })
                .collect(),
        ),
    );
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type":"function",
                            "function":{
                                "name":tool.name,
                                "description":tool.description,
                                "parameters":tool.input_schema,
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(schema) = &request.response_json_schema {
        body.insert(
            "response_format".into(),
            json!({
                "type":"json_schema",
                "json_schema":{
                    "name":"npc_response",
                    "strict":true,
                    "schema":schema,
                }
            }),
        );
    }
    Ok(Value::Object(body))
}

fn http_error(provider_id: &str, response: &Response) -> ProviderError {
    let mut error = ProviderError::from_http(provider_id, response.status());
    error.request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 256 && !value.chars().any(char::is_control))
        .map(str::to_owned);
    error.retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    error
}

async fn accepted_response_error(
    response: Response,
    deadline: Instant,
    cancellation: &CancellationToken,
    provider_id: &str,
    secret: &SecretBytes,
) -> ProviderError {
    let request_id = match read_limited(response, deadline, cancellation, provider_id).await {
        Ok(body) => serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("requestId")
                    .or_else(|| value.get("request_id"))
                    .or_else(|| value.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|value| {
                value.len() <= 512
                    && !value.chars().any(char::is_control)
                    && std::str::from_utf8(secret.expose())
                        .map_or(true, |secret| !value.contains(secret))
            }),
        Err(_) => None,
    };
    let mut error = ProviderError::new(
        provider_id,
        ErrorKind::PollingRequired,
        "provider accepted asynchronous work; polling is not supported by this adapter",
    );
    error.http_status = Some(202);
    error.request_id = request_id;
    error
}

async fn read_limited(
    response: Response,
    deadline: Instant,
    cancellation: &CancellationToken,
    provider_id: &str,
) -> Result<Vec<u8>, ProviderError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    loop {
        let next = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled(provider_id)),
            _ = tokio::time::sleep_until(deadline) => return Err(ProviderError::timeout(provider_id)),
            next = stream.next() => next,
        };
        match next {
            Some(Ok(bytes)) => {
                if body.len().saturating_add(bytes.len()) > MAX_MODEL_LIST_BYTES {
                    return Err(ProviderError::protocol(
                        provider_id,
                        "model list is oversized",
                    ));
                }
                body.extend_from_slice(&bytes);
            }
            Some(Err(error)) => return Err(ProviderError::transport(provider_id, &error)),
            None => return Ok(body),
        }
    }
}

fn parse_models(
    protocol: ProviderProtocol,
    body: &[u8],
    provider_id: &str,
) -> Result<Vec<ModelInfo>, ProviderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| {
        ProviderError::protocol(provider_id, "provider returned a malformed model list")
    })?;
    let values = match protocol {
        ProviderProtocol::GeminiGenerateContent | ProviderProtocol::CohereChat => {
            value.get("models")
        }
        _ => value.get("data"),
    }
    .and_then(Value::as_array)
    .ok_or_else(|| {
        ProviderError::protocol(provider_id, "provider model list has an invalid shape")
    })?;
    let mut models = Vec::with_capacity(values.len().min(4_096));
    for value in values.iter().take(4_096) {
        let raw_id = value
            .get("id")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::protocol(provider_id, "provider model entry is missing an id")
            })?;
        let id = raw_id.strip_prefix("models/").unwrap_or(raw_id);
        if !crate::types::valid_model_id(id) {
            continue;
        }
        let supports_generation = if protocol == ProviderProtocol::CohereChat {
            !value
                .get("is_deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && value
                    .get("endpoints")
                    .and_then(Value::as_array)
                    .is_some_and(|endpoints| {
                        endpoints
                            .iter()
                            .any(|endpoint| endpoint.as_str() == Some("chat"))
                    })
        } else {
            let methods = value
                .get("supportedGenerationMethods")
                .and_then(Value::as_array);
            methods.is_none_or(|methods| {
                methods.iter().any(|method| {
                    method
                        .as_str()
                        .is_some_and(|name| name.contains("generateContent"))
                })
            })
        };
        models.push(ModelInfo {
            id: id.to_owned(),
            display_name: value
                .get("display_name")
                .or_else(|| value.get("displayName"))
                .and_then(Value::as_str)
                .map(|value| value.chars().take(256).collect()),
            owned_by: value
                .get("owned_by")
                .and_then(Value::as_str)
                .map(|value| value.chars().take(256).collect()),
            supports_generation,
            capabilities: crate::ModelCapabilities::default(),
        });
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

fn parse_cohere_model_page(
    body: &[u8],
    provider_id: &str,
) -> Result<(Vec<ModelInfo>, Option<String>), ProviderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| {
        ProviderError::protocol(provider_id, "provider returned a malformed model list")
    })?;
    let models = parse_models(ProviderProtocol::CohereChat, body, provider_id)?;
    let next_page_token = value
        .get("next_page_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    if next_page_token
        .as_ref()
        .is_some_and(|token| token.len() > 2_048 || token.chars().any(char::is_control))
    {
        return Err(ProviderError::protocol(
            provider_id,
            "provider model pagination token is invalid",
        ));
    }
    Ok((models, next_page_token))
}
