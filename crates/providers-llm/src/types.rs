use std::{fmt, pin::Pin, sync::Arc, time::Duration};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{ProviderError, SecretProvider, SecretReference};

pub const MAX_MESSAGES: usize = 4_096;
pub const MAX_TEXT_BYTES: usize = 1_048_576;
pub const MAX_REQUEST_BYTES: usize = 8 * 1_048_576;
pub const MAX_TOOLS: usize = 128;
pub const MAX_SCHEMA_BYTES: usize = 256 * 1_024;

pub type LlmEventStream = Pin<Box<dyn Stream<Item = LlmEvent> + Send + 'static>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    OpenAiResponses,
    AnthropicMessages,
    GeminiGenerateContent,
    OpenAiChatCompletions,
    CohereChat,
    NvidiaNimChat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyCapabilities {
    /// The adapter does not use provider-side conversation/session state.
    pub application_stateless: bool,
    /// The request carries a provider-supported no-store flag.
    pub request_disables_storage: bool,
    /// Provider policy may still retain request data for safety/operations.
    pub provider_may_retain_data: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub provider_id: String,
    pub display_name: String,
    pub protocol: ProviderProtocol,
    pub streaming: bool,
    pub cancellation: bool,
    pub tool_calls: bool,
    pub usage_reporting: bool,
    pub model_listing: bool,
    pub json_schema: bool,
    pub privacy: PrivacyCapabilities,
}

impl ProviderCapabilities {
    pub(crate) fn validate(&self) -> Result<(), ProviderError> {
        if !valid_stable_id(&self.provider_id) || self.display_name.trim().is_empty() {
            return Err(ProviderError::protocol(
                &self.provider_id,
                "provider capability descriptor is invalid",
            ));
        }
        if !self.streaming || !self.cancellation {
            return Err(ProviderError::protocol(
                &self.provider_id,
                "hosted adapter must support streaming and cancellation",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AdapterConfig {
    pub base_url: Url,
    pub credential: SecretReference,
    pub secrets: Arc<dyn SecretProvider>,
    pub request_timeout: Duration,
    /// Plain HTTP is allowed only for loopback fixture or user-managed local
    /// servers. Public hosted endpoints must always use HTTPS.
    pub allow_insecure_loopback: bool,
}

impl fmt::Debug for AdapterConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdapterConfig")
            .field("base_url", &self.base_url)
            .field("credential", &self.credential)
            .field("secrets", &"<SECRET_PROVIDER>")
            .field("request_timeout", &self.request_timeout)
            .field("allow_insecure_loopback", &self.allow_insecure_loopback)
            .finish()
    }
}

impl AdapterConfig {
    pub fn validate(&self, provider_id: &str) -> Result<(), ProviderError> {
        let url = &self.base_url;
        let loopback = matches!(
            url.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("::1")
        );
        let secure = url.scheme() == "https";
        if !(secure || self.allow_insecure_loopback && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.request_timeout < Duration::from_millis(100)
            || self.request_timeout > Duration::from_secs(15 * 60)
        {
            return Err(ProviderError::new(
                provider_id,
                crate::ErrorKind::InvalidRequest,
                "adapter configuration is invalid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub maximum_output_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<Value>,
}

impl LlmRequest {
    pub fn validate(&self, provider_id: &str) -> Result<(), ProviderError> {
        if !valid_model_id(&self.model)
            || self.messages.is_empty()
            || self.messages.len() > MAX_MESSAGES
            || self.maximum_output_tokens == 0
            || self.maximum_output_tokens > 1_000_000
            || self
                .temperature
                .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
            || self.tools.len() > MAX_TOOLS
        {
            return Err(invalid_request(provider_id));
        }

        let mut approximate_size = self.model.len();
        for message in &self.messages {
            approximate_size = approximate_size.saturating_add(message.content.len());
            if message.content.len() > MAX_TEXT_BYTES
                || message
                    .name
                    .as_ref()
                    .is_some_and(|name| !valid_tool_name(name))
                || message
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| !valid_opaque_id(id))
                || (message.role == ChatRole::Tool
                    && (message.tool_call_id.is_none() || message.name.is_none()))
            {
                return Err(invalid_request(provider_id));
            }
        }

        for tool in &self.tools {
            let schema_size = serde_json::to_vec(&tool.input_schema)
                .map_err(|_| invalid_request(provider_id))?
                .len();
            approximate_size = approximate_size
                .saturating_add(tool.name.len())
                .saturating_add(tool.description.len())
                .saturating_add(schema_size);
            if !valid_tool_name(&tool.name)
                || tool.description.len() > 4_096
                || schema_size > MAX_SCHEMA_BYTES
                || !tool.input_schema.is_object()
            {
                return Err(invalid_request(provider_id));
            }
        }

        if let Some(schema) = &self.response_json_schema {
            let serialized =
                serde_json::to_vec(schema).map_err(|_| invalid_request(provider_id))?;
            if serialized.len() > MAX_SCHEMA_BYTES || !schema.is_object() {
                return Err(invalid_request(provider_id));
            }
        }

        if approximate_size > MAX_REQUEST_BYTES {
            return Err(invalid_request(provider_id));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub supports_generation: bool,
    /// Exact-ID capability evidence. Unknown is intentional because NVIDIA API
    /// Catalog models expose different feature sets.
    #[serde(default)]
    pub capabilities: ModelCapabilities,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub streaming: CapabilitySupport,
    pub tool_calls: CapabilitySupport,
    pub json_schema: CapabilitySupport,
    pub reasoning: CapabilitySupport,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    EndTurn,
    MaximumTokens,
    ToolUse,
    StopSequence,
    Safety,
    Cancelled,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmEvent {
    Started {
        request_id: Option<String>,
    },
    TextDelta {
        text: String,
        content_index: usize,
    },
    ToolCallStarted {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments_fragment: String,
    },
    ToolCallCompleted {
        index: usize,
        id: String,
        name: String,
        arguments: Value,
    },
    Usage {
        usage: TokenUsage,
    },
    Finished {
        reason: FinishReason,
        provider_reason: String,
    },
    Error {
        error: ProviderError,
    },
}

impl LlmEvent {
    pub(crate) fn terminal(&self) -> bool {
        matches!(self, Self::Finished { .. } | Self::Error { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibleOptions {
    pub provider_id: String,
    pub display_name: String,
    pub supports_model_listing: bool,
    pub supports_json_schema: bool,
    /// Send `store: false` only when the endpoint documents compatibility.
    pub send_store_false: bool,
    pub provider_may_retain_data: bool,
}

fn invalid_request(provider_id: &str) -> ProviderError {
    ProviderError::new(
        provider_id,
        crate::ErrorKind::InvalidRequest,
        "language model request failed local validation",
    )
}

pub(crate) fn valid_stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

pub(crate) fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains("..")
        && !value.starts_with('/')
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
        })
}

fn valid_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_opaque_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}
