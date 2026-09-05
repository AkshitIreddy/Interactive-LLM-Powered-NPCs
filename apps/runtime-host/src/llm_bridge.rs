//! Trusted construction of one user-selected hosted LLM route.
//!
//! The request may name a provider/model and repeat an opaque credential
//! reference, but it cannot redirect the vault read or configure an endpoint.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use interactive_npcs_credential_vault::{CredentialVault, VaultError};
use npc_providers_llm::{
    AdapterConfig, AnthropicMessages, CohereChat, CompatibleOptions, GeminiGenerateContent,
    GroqChatCompletions, HostedLanguageModel, NvidiaNimChat, OpenAiCompatible, OpenAiResponses,
    RuntimeBridge, RuntimeBridgeConfig, SecretBytes, SecretError, SecretProvider, SecretReference,
};
use npc_runtime_core::LanguageModelProvider;
use url::Url;

use crate::runtime_timing::{ObservedHostedLanguageModel, RuntimeTurnTimingLedger};
use crate::{RouteExecution, SelectedProviderRoute};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub fn selected_hosted_llm(
    route: &SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
    system_prompt: String,
) -> Result<Arc<dyn LanguageModelProvider>, SelectedLlmError> {
    selected_hosted_llm_with_timing(route, vault, system_prompt, None)
}

pub(crate) fn selected_hosted_llm_with_timing(
    route: &SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
    system_prompt: String,
    timing: Option<RuntimeTurnTimingLedger>,
) -> Result<Arc<dyn LanguageModelProvider>, SelectedLlmError> {
    if route.execution != RouteExecution::Cloud {
        return Err(SelectedLlmError::UnsupportedRoute);
    }
    let credential_target =
        credential_target(&route.provider_id).ok_or(SelectedLlmError::UnsupportedRoute)?;
    if route.credential_reference.as_deref() != Some(credential_target) {
        return Err(SelectedLlmError::CredentialReferenceMismatch);
    }
    let reference = SecretReference::new(credential_target)
        .map_err(|_| SelectedLlmError::CredentialReferenceMismatch)?;
    let secrets: Arc<dyn SecretProvider> = Arc::new(VaultLlmSecrets {
        vault,
        allowed_reference: reference.clone(),
        credential_target,
    });
    let provider: Arc<dyn HostedLanguageModel> = match route.provider_id.as_str() {
        "openai" => Arc::new(
            OpenAiResponses::new(adapter("https://api.openai.com/v1/", reference, secrets)?)
                .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "anthropic" => Arc::new(
            AnthropicMessages::new(adapter(
                "https://api.anthropic.com/v1/",
                reference,
                secrets,
            )?)
            .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "gemini" => Arc::new(
            GeminiGenerateContent::new(adapter(
                "https://generativelanguage.googleapis.com/v1beta/",
                reference,
                secrets,
            )?)
            .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "groq" => Arc::new(
            GroqChatCompletions::new(adapter(
                "https://api.groq.com/openai/v1/",
                reference,
                secrets,
            )?)
            .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "mistral" => Arc::new(
            OpenAiCompatible::new(
                adapter("https://api.mistral.ai/v1/", reference, secrets)?,
                CompatibleOptions {
                    provider_id: "mistral".into(),
                    display_name: "Mistral AI".into(),
                    supports_model_listing: true,
                    supports_json_schema: true,
                    send_store_false: false,
                    provider_may_retain_data: true,
                },
            )
            .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "openrouter" => Arc::new(
            OpenAiCompatible::new(
                adapter("https://openrouter.ai/api/v1/", reference, secrets)?,
                CompatibleOptions {
                    provider_id: "openrouter".into(),
                    display_name: "OpenRouter".into(),
                    supports_model_listing: true,
                    supports_json_schema: true,
                    send_store_false: false,
                    provider_may_retain_data: true,
                },
            )
            .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "cohere" => Arc::new(
            CohereChat::new(adapter("https://api.cohere.com/", reference, secrets)?)
                .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        "nvidia-nim" => Arc::new(
            NvidiaNimChat::hosted(reference, secrets, REQUEST_TIMEOUT)
                .map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        ),
        _ => return Err(SelectedLlmError::UnsupportedRoute),
    };
    let provider: Arc<dyn HostedLanguageModel> = match timing {
        Some(timing) => Arc::new(ObservedHostedLanguageModel::new(provider, timing)),
        None => provider,
    };
    let bridge = RuntimeBridge::new(
        provider,
        RuntimeBridgeConfig {
            default_model: route.model_id.clone(),
            maximum_output_tokens: 512,
            temperature: Some(0.7),
            system_prompt: Some(system_prompt),
        },
    )
    .map_err(|_| SelectedLlmError::InvalidConfiguration)?;
    Ok(Arc::new(bridge))
}

fn adapter(
    endpoint: &'static str,
    credential: SecretReference,
    secrets: Arc<dyn SecretProvider>,
) -> Result<AdapterConfig, SelectedLlmError> {
    Ok(AdapterConfig {
        base_url: Url::parse(endpoint).map_err(|_| SelectedLlmError::InvalidConfiguration)?,
        credential,
        secrets,
        request_timeout: REQUEST_TIMEOUT,
        allow_insecure_loopback: false,
    })
}

fn credential_target(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openai" => Some("providers/openai"),
        "anthropic" => Some("providers/anthropic"),
        "gemini" => Some("providers/gemini"),
        "groq" => Some("providers/groq"),
        "mistral" => Some("providers/mistral"),
        "openrouter" => Some("providers/openrouter"),
        "cohere" => Some("providers/cohere"),
        "nvidia-nim" => Some("providers/nvidia-nim"),
        _ => None,
    }
}

struct VaultLlmSecrets {
    vault: Arc<dyn CredentialVault>,
    allowed_reference: SecretReference,
    credential_target: &'static str,
}

#[async_trait]
impl SecretProvider for VaultLlmSecrets {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretBytes, SecretError> {
        if reference != &self.allowed_reference {
            return Err(SecretError::NotFound);
        }
        let secret = self
            .vault
            .get(self.credential_target)
            .map_err(map_vault_error)?;
        SecretBytes::new(secret.expose().to_vec()).map_err(|_| SecretError::Unavailable)
    }
}

fn map_vault_error(error: VaultError) -> SecretError {
    match error {
        VaultError::NotFound => SecretError::NotFound,
        VaultError::EmptyTarget
        | VaultError::TargetTooLong
        | VaultError::InvalidTarget
        | VaultError::EmptySecret
        | VaultError::SecretTooLarge(_)
        | VaultError::System(_)
        | VaultError::Poisoned => SecretError::Unavailable,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectedLlmError {
    #[error("selected LLM route is unsupported")]
    UnsupportedRoute,
    #[error("selected LLM credential reference does not match the fixed provider target")]
    CredentialReferenceMismatch,
    #[error("selected LLM adapter configuration is invalid")]
    InvalidConfiguration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use interactive_npcs_credential_vault::MemoryCredentialVault;

    #[test]
    fn request_cannot_redirect_the_fixed_provider_credential() {
        let route = SelectedProviderRoute {
            provider_id: "openai".into(),
            model_id: "gpt-4.1-mini".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "conversation_text".into(),
            credential_reference: Some("providers/anthropic".into()),
        };
        assert!(matches!(
            selected_hosted_llm(
                &route,
                Arc::new(MemoryCredentialVault::default()),
                "Remain in character.".into()
            ),
            Err(SelectedLlmError::CredentialReferenceMismatch)
        ));
    }

    #[test]
    fn named_mistral_route_builds_with_its_fixed_vault_target() {
        let route = SelectedProviderRoute {
            provider_id: "mistral".into(),
            model_id: "ministral-8b-2512".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "conversation_text".into(),
            credential_reference: Some("providers/mistral".into()),
        };
        let provider = selected_hosted_llm(
            &route,
            Arc::new(MemoryCredentialVault::default()),
            "Remain in character.".into(),
        )
        .expect("fixed Mistral route");
        assert_eq!(provider.descriptor().id, "mistral");
        assert_eq!(provider.descriptor().display_name, "Mistral AI");
        assert_eq!(
            provider
                .descriptor()
                .capabilities
                .get("json_schema")
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn generic_compatible_route_cannot_supply_an_endpoint_or_vault_target() {
        let route = SelectedProviderRoute {
            provider_id: "openai-compatible".into(),
            model_id: "user-model".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "conversation_text".into(),
            credential_reference: Some("providers/openai-compatible".into()),
        };
        assert!(matches!(
            selected_hosted_llm(
                &route,
                Arc::new(MemoryCredentialVault::default()),
                "Remain in character.".into()
            ),
            Err(SelectedLlmError::UnsupportedRoute)
        ));
    }

    #[test]
    fn named_openrouter_route_builds_with_its_fixed_vault_target() {
        let route = SelectedProviderRoute {
            provider_id: "openrouter".into(),
            model_id: "liquid/lfm-2.5-2.6b:free".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "conversation_text".into(),
            credential_reference: Some("providers/openrouter".into()),
        };
        let provider = selected_hosted_llm(
            &route,
            Arc::new(MemoryCredentialVault::default()),
            "Remain in character.".into(),
        )
        .expect("fixed OpenRouter route");
        assert_eq!(provider.descriptor().id, "openrouter");
        assert_eq!(provider.descriptor().display_name, "OpenRouter");
        assert!(matches!(
            selected_hosted_llm(
                &SelectedProviderRoute {
                    credential_reference: Some("providers/openai-compatible".into()),
                    ..route
                },
                Arc::new(MemoryCredentialVault::default()),
                "Remain in character.".into()
            ),
            Err(SelectedLlmError::CredentialReferenceMismatch)
        ));
    }
}
