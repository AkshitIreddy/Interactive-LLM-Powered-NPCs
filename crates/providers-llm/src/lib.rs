//! Hosted language-model adapters for the 2.0 runtime.
//!
//! The crate deliberately does not read environment variables or Windows
//! Credential Manager. A trusted host supplies a [`SecretProvider`] and an opaque
//! [`SecretReference`]. Secrets are resolved only while an HTTP request is being
//! constructed; configurations, diagnostics, and errors contain references, never
//! credential bytes.

mod adapter;
mod error;
mod parser;
mod runtime_bridge;
mod secret;
mod sse;
mod types;

pub use adapter::{
    AnthropicMessages, CohereChat, GeminiGenerateContent, GroqChatCompletions, HostedLanguageModel,
    NvidiaNimChat, OpenAiCompatible, OpenAiResponses, NVIDIA_NIM_DEFAULT_MODEL_CACHE_TTL,
    NVIDIA_NIM_HOSTED_BASE_URL,
};
pub use error::{ErrorKind, ProviderError, SecretError};
pub use runtime_bridge::{RuntimeBridge, RuntimeBridgeConfig};
pub use secret::{MemorySecretProvider, SecretBytes, SecretProvider, SecretReference};
pub use types::{
    AdapterConfig, CapabilitySupport, ChatMessage, ChatRole, CompatibleOptions, FinishReason,
    LlmEvent, LlmEventStream, LlmRequest, ModelCapabilities, ModelInfo, PrivacyCapabilities,
    ProviderCapabilities, ProviderProtocol, TokenUsage, ToolDefinition,
};
