use std::{collections::BTreeMap, fmt, sync::Arc};

use async_trait::async_trait;
use futures_util::StreamExt;
use npc_runtime_core::{
    DataClass, GenerationRequest, LanguageModelProvider, LlmDelta, LlmStream, ProviderDescriptor,
    ProviderError as RuntimeProviderError, ProviderErrorKind, ProviderLocation, ProviderModality,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    ChatMessage, ChatRole, ErrorKind, HostedLanguageModel, LlmEvent, LlmRequest, ProviderError,
};

#[derive(Clone, Debug)]
pub struct RuntimeBridgeConfig {
    pub default_model: String,
    pub maximum_output_tokens: u32,
    pub temperature: Option<f32>,
    /// Game-profile prompt material belongs here. The bridge adds turn context as
    /// structured JSON rather than interpolating it into executable templates.
    pub system_prompt: Option<String>,
}

impl RuntimeBridgeConfig {
    fn validate(&self, provider_id: &str) -> Result<(), ProviderError> {
        if !crate::types::valid_model_id(&self.default_model)
            || self.maximum_output_tokens == 0
            || self.maximum_output_tokens > 1_000_000
            || self
                .temperature
                .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
            || self
                .system_prompt
                .as_ref()
                .is_some_and(|prompt| prompt.len() > crate::types::MAX_TEXT_BYTES)
        {
            return Err(ProviderError::new(
                provider_id,
                ErrorKind::InvalidRequest,
                "runtime bridge configuration is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct RuntimeBridge {
    provider: Arc<dyn HostedLanguageModel>,
    config: RuntimeBridgeConfig,
    descriptor: ProviderDescriptor,
}

impl fmt::Debug for RuntimeBridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeBridge")
            .field("provider", &self.provider.capabilities().provider_id)
            .field("config", &self.config)
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

impl RuntimeBridge {
    pub fn new(
        provider: Arc<dyn HostedLanguageModel>,
        config: RuntimeBridgeConfig,
    ) -> Result<Self, ProviderError> {
        let capabilities = provider.capabilities();
        config.validate(&capabilities.provider_id)?;
        let mut descriptor_capabilities = BTreeMap::new();
        descriptor_capabilities.insert("streaming".into(), capabilities.streaming.to_string());
        descriptor_capabilities
            .insert("cancellation".into(), capabilities.cancellation.to_string());
        descriptor_capabilities.insert("tool_calls".into(), capabilities.tool_calls.to_string());
        descriptor_capabilities.insert(
            "usage_reporting".into(),
            capabilities.usage_reporting.to_string(),
        );
        descriptor_capabilities.insert(
            "model_listing".into(),
            capabilities.model_listing.to_string(),
        );
        descriptor_capabilities.insert("json_schema".into(), capabilities.json_schema.to_string());
        descriptor_capabilities.insert(
            "application_stateless".into(),
            capabilities.privacy.application_stateless.to_string(),
        );
        descriptor_capabilities.insert(
            "request_disables_storage".into(),
            capabilities.privacy.request_disables_storage.to_string(),
        );
        let descriptor = ProviderDescriptor {
            id: capabilities.provider_id.clone(),
            display_name: capabilities.display_name.clone(),
            modality: ProviderModality::LanguageModel,
            location: ProviderLocation::Cloud {
                service: capabilities.provider_id.clone(),
            },
            may_retain_data: capabilities.privacy.provider_may_retain_data,
            transmitted_data: vec![DataClass::Transcript, DataClass::PromptContext],
            capabilities: descriptor_capabilities,
        };
        Ok(Self {
            provider,
            config,
            descriptor,
        })
    }

    fn normalize_request(&self, request: GenerationRequest) -> LlmRequest {
        let structured_response =
            request
                .metadata
                .get("npc_response_format")
                .is_some_and(|format| {
                    matches!(
                        format.as_str(),
                        "structured_v1" | "structured_speech_first_v1"
                    )
                });
        let mut messages = Vec::with_capacity(2);
        if let Some(system_prompt) = &self.config.system_prompt {
            messages.push(ChatMessage {
                role: ChatRole::System,
                content: system_prompt.clone(),
                name: None,
                tool_call_id: None,
            });
        }
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: json!({
                "transcript": request.transcript,
                "character": request.character,
                "memory": request.memory,
                "locale": request.locale,
                "metadata": request.metadata,
            })
            .to_string(),
            name: None,
            tool_call_id: None,
        });
        LlmRequest {
            model: self.config.default_model.clone(),
            messages,
            maximum_output_tokens: self.config.maximum_output_tokens,
            temperature: self.config.temperature,
            tools: Vec::new(),
            response_json_schema: structured_response.then(portable_npc_response_schema),
        }
    }
}

/// Small common schema accepted by every qualified hosted route. Optional
/// effect fields stay outside this contract so providers with narrower JSON
/// Schema dialects can still guarantee the versioned spoken response.
fn portable_npc_response_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "schema_version": {
                "type": "string",
                "enum": ["npc_response.v1"]
            },
            "spoken_response": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            }
        },
        "required": ["schema_version", "spoken_response"]
    })
}

#[async_trait]
impl LanguageModelProvider for RuntimeBridge {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    async fn stream_response(
        &self,
        request: GenerationRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmStream, RuntimeProviderError> {
        let request = self.normalize_request(request);
        let mut provider_stream = self
            .provider
            .stream(request, cancellation)
            .await
            .map_err(runtime_error)?;
        let stream = async_stream::stream! {
            let mut sequence = 0_u64;
            while let Some(event) = provider_stream.next().await {
                match event {
                    LlmEvent::TextDelta { text, .. } if !text.is_empty() => {
                        sequence = sequence.saturating_add(1);
                        yield Ok(LlmDelta { text, sequence });
                    }
                    LlmEvent::Error { error } => {
                        yield Err(runtime_error(error));
                        return;
                    }
                    LlmEvent::Finished { .. } => return,
                    _ => {}
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

fn runtime_error(error: ProviderError) -> RuntimeProviderError {
    RuntimeProviderError {
        provider_id: error.provider_id,
        kind: match error.kind {
            ErrorKind::Cancelled => ProviderErrorKind::Cancelled,
            ErrorKind::Timeout => ProviderErrorKind::Timeout,
            ErrorKind::Authentication | ErrorKind::SecretUnavailable => {
                ProviderErrorKind::Authentication
            }
            ErrorKind::RateLimited => ProviderErrorKind::RateLimited,
            ErrorKind::InvalidRequest => ProviderErrorKind::InvalidRequest,
            ErrorKind::Unavailable => ProviderErrorKind::Unavailable,
            ErrorKind::PollingRequired => ProviderErrorKind::Unavailable,
            ErrorKind::Protocol => ProviderErrorKind::Protocol,
            ErrorKind::Internal => ProviderErrorKind::Internal,
        },
        message: error.message,
        retryable: error.retryable,
        retry_after: error.retry_after,
    }
}
