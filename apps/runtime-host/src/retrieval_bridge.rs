//! Trusted construction of one user-selected hosted embedding route.
//!
//! The selected snapshot may name only a curated provider/model and opaque credential reference.
//! It cannot redirect the adapter endpoint, pass a credential value, or enable an automatic
//! fallback. Manual retry is a new caller-owned operation with a new cancellation generation.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use interactive_npcs_credential_vault::{CredentialVault, VaultError};
use npc_providers_retrieval::{
    EmbeddingProvider,
    NvidiaEmbeddingModel,
    NvidiaNimAdapter,
    NvidiaNimConfig,
    ReqwestTransport,
    RetrievalError,
    SecretReference,
    SecretResolver,
    SecretString,
    // Public catalog model identifier example; never credential material.
    NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID,
};

use crate::{RouteExecution, SelectedProviderRoute};

const CREDENTIAL_TARGET: &str = "providers/nvidia-nim";
const EXPECTED_DIMENSIONS: usize = 2_048;

pub fn selected_hosted_embedding(
    route: &SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
) -> Result<Arc<dyn EmbeddingProvider>, SelectedEmbeddingError> {
    if route.execution != RouteExecution::Cloud
        || route.provider_id != "nvidia-nim"
        || route.model_id != NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID
    {
        return Err(SelectedEmbeddingError::UnsupportedRoute);
    }
    if route.credential_reference.as_deref() != Some(CREDENTIAL_TARGET) {
        return Err(SelectedEmbeddingError::CredentialReferenceMismatch);
    }
    let reference = SecretReference::new(CREDENTIAL_TARGET)
        .map_err(|_| SelectedEmbeddingError::InvalidConfiguration)?;
    let resolver: Arc<dyn SecretResolver> = Arc::new(VaultRetrievalSecrets {
        vault,
        allowed_reference: reference.clone(),
    });
    let model = NvidiaEmbeddingModel::new(
        NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID,
        Some(EXPECTED_DIMENSIONS),
    )
    .map_err(|_| SelectedEmbeddingError::InvalidConfiguration)?;
    let transport = ReqwestTransport::new(Duration::from_secs(15))
        .map_err(|_| SelectedEmbeddingError::InvalidConfiguration)?;
    let adapter = NvidiaNimAdapter::new(
        NvidiaNimConfig {
            credential: reference,
            secrets: resolver,
            embedding_models: vec![model],
            rerank_models: Vec::new(),
        },
        Arc::new(transport),
    )
    .map_err(|_| SelectedEmbeddingError::InvalidConfiguration)?;
    Ok(Arc::new(adapter))
}

struct VaultRetrievalSecrets {
    vault: Arc<dyn CredentialVault>,
    allowed_reference: SecretReference,
}

#[async_trait]
impl SecretResolver for VaultRetrievalSecrets {
    async fn resolve(&self, reference: &SecretReference) -> Result<SecretString, RetrievalError> {
        if reference != &self.allowed_reference {
            return Err(credential_unavailable());
        }
        let secret = self.vault.get(CREDENTIAL_TARGET).map_err(map_vault_error)?;
        let value =
            String::from_utf8(secret.expose().to_vec()).map_err(|_| credential_unavailable())?;
        SecretString::new(value)
    }
}

fn map_vault_error(_error: VaultError) -> RetrievalError {
    credential_unavailable()
}

fn credential_unavailable() -> RetrievalError {
    RetrievalError::credential_unavailable()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectedEmbeddingError {
    #[error("selected embedding route is unsupported")]
    UnsupportedRoute,
    #[error("selected embedding credential reference does not match the fixed provider target")]
    CredentialReferenceMismatch,
    #[error("selected embedding adapter configuration is invalid")]
    InvalidConfiguration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use interactive_npcs_credential_vault::MemoryCredentialVault;

    fn route(reference: &str) -> SelectedProviderRoute {
        SelectedProviderRoute {
            provider_id: "nvidia-nim".into(),
            model_id: NVIDIA_NEMOTRON_3_EMBED_1B_MODEL_ID.into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "selected_memory_and_lore_text".into(),
            credential_reference: Some(reference.into()),
        }
    }

    #[test]
    fn selected_route_cannot_redirect_endpoint_model_or_credential() {
        let vault = Arc::new(MemoryCredentialVault::default());
        assert!(matches!(
            selected_hosted_embedding(&route("providers/cohere"), vault.clone()),
            Err(SelectedEmbeddingError::CredentialReferenceMismatch)
        ));
        let mut wrong_model = route(CREDENTIAL_TARGET);
        wrong_model.model_id = "nvidia/other-model".into();
        assert!(matches!(
            selected_hosted_embedding(&wrong_model, vault),
            Err(SelectedEmbeddingError::UnsupportedRoute)
        ));
    }
}
