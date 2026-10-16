//! Bounded per-role "Test" check for the Loadout page.
//!
//! The check answers one question for the provider and model currently
//! selected for a role: could a turn use this route right now? It combines the
//! facts the app already owns, without sending content to any provider:
//!
//! 1. the route exists in the bundled provider catalog and is selectable;
//! 2. a hosted route has a saved credential (the vault reports presence only;
//!    the value never leaves native code);
//! 3. the authenticated runtime sidecar currently advertises the provider's
//!    role contract (the same doctor report `test_provider_connection` uses).
//!
//! `latency_ms` is the measured duration of this check, including the runtime
//! round trip when one is needed. It is not model or voice latency, and the
//! result says so: `network_request_performed` is always `false`.

use crate::catalog::{bundled_provider_catalog, credential_reference_for, CredentialPresence};
use crate::commands::{runtime_advertises_provider, AppState, CommandError};
use crate::domain::CredentialReferenceStatus;
use npc_provider_catalog::{CatalogDocument, ExecutionLocation, Lifecycle, Modality};
use npc_provider_loadouts::credential_target_provider_id;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::State;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LoadoutTestRole {
    Llm,
    Stt,
    Tts,
    Embeddings,
    Vision,
}

impl LoadoutTestRole {
    fn modality(self) -> Modality {
        match self {
            Self::Llm => Modality::Llm,
            Self::Stt => Modality::Stt,
            Self::Tts => Modality::Tts,
            Self::Embeddings => Modality::Embedding,
            // Vision is carried by a catalog-qualified multimodal LLM route.
            Self::Vision => Modality::Llm,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutRoleTestRequest {
    pub role: LoadoutTestRole,
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LoadoutRoleTestOutcome {
    Ok,
    UnknownRoute,
    NotSelectable,
    NeedsCredential,
    CredentialStoreUnavailable,
    RuntimeUnavailable,
    ProviderContractUnavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutRoleTestResult {
    pub role: LoadoutTestRole,
    pub provider_id: String,
    pub model_id: String,
    pub outcome: LoadoutRoleTestOutcome,
    /// Duration of this readiness check in milliseconds.
    pub latency_ms: f64,
    pub credential_status: CredentialReferenceStatus,
    pub runtime_checked: bool,
    pub network_request_performed: bool,
    pub response_body_returned: bool,
    pub detail: String,
}

/// What the offline part of the check decided.
#[derive(Debug, PartialEq, Eq)]
enum RolePlan {
    Done(LoadoutRoleTestOutcome, CredentialReferenceStatus, String),
    /// Hosted route with a saved credential: the runtime must advertise this
    /// catalog provider id.
    NeedsRuntimeContract(String),
}

fn plan_role_test(
    request: &LoadoutRoleTestRequest,
    catalog: &CatalogDocument,
    credentials: &dyn CredentialPresence,
) -> RolePlan {
    use CredentialReferenceStatus::NotRequired;
    use LoadoutRoleTestOutcome::{
        CredentialStoreUnavailable, NeedsCredential, NotSelectable, Ok, UnknownRoute,
    };
    // Built-in local routes that are not catalog providers.
    if request.role == LoadoutTestRole::Embeddings
        && request.provider_id == "fts-only"
        && request.model_id == "sqlite-fts5"
    {
        return RolePlan::Done(
            Ok,
            NotRequired,
            "Built-in SQLite full-text search needs no credential or download. No request was made."
                .into(),
        );
    }
    let catalog_provider_id = credential_target_provider_id(&request.provider_id);
    let Some(provider) = catalog
        .content
        .providers
        .iter()
        .find(|provider| provider.id == catalog_provider_id)
    else {
        return RolePlan::Done(
            UnknownRoute,
            NotRequired,
            "This provider is not in the bundled provider catalog.".into(),
        );
    };
    let modality = request.role.modality();
    let Some(model) = catalog.content.models.iter().find(|model| {
        model.provider_id == provider.id
            && model.modality == modality
            && (model.upstream_id == request.model_id
                || model.id == request.model_id
                || model.upstream_id == "$provider_default")
    }) else {
        return RolePlan::Done(
            UnknownRoute,
            NotRequired,
            "This model is not in the bundled catalog for this provider and role.".into(),
        );
    };
    if model.lifecycle == Lifecycle::Deprecated {
        return RolePlan::Done(
            NotSelectable,
            NotRequired,
            "This model is retired in the catalog. Choose its catalog successor.".into(),
        );
    }
    if !model.is_selectable() {
        return RolePlan::Done(
            NotSelectable,
            NotRequired,
            format!(
                "The catalog does not allow selecting this route yet: {}",
                model.availability_note
            ),
        );
    }
    let hosted = provider.execution == ExecutionLocation::Hosted;
    if !hosted {
        return RolePlan::Done(
            Ok,
            NotRequired,
            "Local route is in the catalog and selectable. Installed model files are checked by the model manager, not by this test. No request was made."
                .into(),
        );
    }
    if !provider.credential.required {
        return RolePlan::NeedsRuntimeContract(provider.id.clone());
    }
    let Some(reference) = credential_reference_for(&provider.id) else {
        return RolePlan::Done(
            UnknownRoute,
            NotRequired,
            "No credential reference exists for this hosted provider.".into(),
        );
    };
    match credentials.status(&reference) {
        CredentialReferenceStatus::Present => RolePlan::NeedsRuntimeContract(provider.id.clone()),
        CredentialReferenceStatus::Unavailable => RolePlan::Done(
            CredentialStoreUnavailable,
            CredentialReferenceStatus::Unavailable,
            "The native credential store is unavailable, so the saved key could not be checked."
                .into(),
        ),
        status => RolePlan::Done(
            NeedsCredential,
            status,
            "No saved key for this provider. Connect the account first.".into(),
        ),
    }
}

fn result_for(
    request: &LoadoutRoleTestRequest,
    started: Instant,
    outcome: LoadoutRoleTestOutcome,
    credential_status: CredentialReferenceStatus,
    runtime_checked: bool,
    detail: String,
) -> LoadoutRoleTestResult {
    LoadoutRoleTestResult {
        role: request.role,
        provider_id: request.provider_id.clone(),
        model_id: request.model_id.clone(),
        outcome,
        latency_ms: started.elapsed().as_secs_f64() * 1_000.0,
        credential_status,
        runtime_checked,
        network_request_performed: false,
        response_body_returned: false,
        detail,
    }
}

#[tauri::command]
pub async fn test_loadout_role(
    request: LoadoutRoleTestRequest,
    state: State<'_, AppState>,
) -> Result<LoadoutRoleTestResult, CommandError> {
    if request.provider_id.len() > 128 || request.model_id.len() > 256 {
        return Err(CommandError::InvalidRequest {
            message: "provider or model identifier is too long".into(),
        });
    }
    let started = Instant::now();
    let catalog = bundled_provider_catalog();
    let plan = plan_role_test(&request, &catalog, state.credentials.as_ref());
    let catalog_provider_id = match plan {
        RolePlan::Done(outcome, credential_status, detail) => {
            return Ok(result_for(
                &request,
                started,
                outcome,
                credential_status,
                false,
                detail,
            ));
        }
        RolePlan::NeedsRuntimeContract(provider_id) => provider_id,
    };
    let credential_status = credential_reference_for(&catalog_provider_id)
        .map(|reference| state.credentials.status(&reference))
        .unwrap_or(CredentialReferenceStatus::NotRequired);
    let doctor = match state.runtime.supervisor().doctor().await {
        Ok(report) => report,
        Err(_) => {
            return Ok(result_for(
                &request,
                started,
                LoadoutRoleTestOutcome::RuntimeUnavailable,
                credential_status,
                false,
                "The trusted runtime is unavailable, so this route could not be checked.".into(),
            ));
        }
    };
    if runtime_advertises_provider(&doctor, &catalog_provider_id) {
        Ok(result_for(
            &request,
            started,
            LoadoutRoleTestOutcome::Ok,
            credential_status,
            true,
            "Route is in the catalog, the saved key is present, and the runtime advertises this provider. This is a readiness check: no request was sent to the provider, so the model or voice itself is not exercised."
                .into(),
        ))
    } else {
        Ok(result_for(
            &request,
            started,
            LoadoutRoleTestOutcome::ProviderContractUnavailable,
            credential_status,
            true,
            "The runtime does not currently advertise this provider. No request was made.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CredentialMutationError;
    use interactive_npcs_credential_vault::SecretValue;

    #[derive(Debug)]
    struct Presence(CredentialReferenceStatus);

    impl CredentialPresence for Presence {
        fn status(&self, _reference: &str) -> CredentialReferenceStatus {
            self.0
        }
        fn save(&self, _: &str, _: &SecretValue) -> Result<(), CredentialMutationError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), CredentialMutationError> {
            Ok(())
        }
        fn availability_detail(&self) -> &'static str {
            "fixed"
        }
    }

    fn request(role: LoadoutTestRole, provider: &str, model: &str) -> LoadoutRoleTestRequest {
        LoadoutRoleTestRequest {
            role,
            provider_id: provider.into(),
            model_id: model.into(),
        }
    }

    fn plan(request: &LoadoutRoleTestRequest, status: CredentialReferenceStatus) -> RolePlan {
        plan_role_test(request, &bundled_provider_catalog(), &Presence(status))
    }

    #[test]
    fn hosted_route_with_a_saved_key_proceeds_to_the_runtime_contract_check() {
        let plan = plan(
            &request(LoadoutTestRole::Llm, "groq", "qwen/qwen3.8-27b"),
            CredentialReferenceStatus::Present,
        );
        assert_eq!(plan, RolePlan::NeedsRuntimeContract("groq".into()));
        // Sibling NVIDIA routes use the shared account credential.
        let magpie = plan_role_test(
            &request(
                LoadoutTestRole::Tts,
                "nvidia-nim-magpie",
                "magpie-tts-multilingual",
            ),
            &bundled_provider_catalog(),
            &Presence(CredentialReferenceStatus::Present),
        );
        assert!(matches!(
            magpie,
            RolePlan::NeedsRuntimeContract(ref id) if id == "nvidia-nim"
        ));
    }

    #[test]
    fn missing_or_unreadable_key_is_reported_without_asking_the_runtime() {
        let missing = plan(
            &request(LoadoutTestRole::Stt, "assemblyai", "u3-rt-pro"),
            CredentialReferenceStatus::Missing,
        );
        assert!(matches!(
            missing,
            RolePlan::Done(
                LoadoutRoleTestOutcome::NeedsCredential,
                CredentialReferenceStatus::Missing,
                _
            )
        ));
        let unavailable = plan(
            &request(LoadoutTestRole::Stt, "assemblyai", "u3-rt-pro"),
            CredentialReferenceStatus::Unavailable,
        );
        assert!(matches!(
            unavailable,
            RolePlan::Done(LoadoutRoleTestOutcome::CredentialStoreUnavailable, _, _)
        ));
    }

    #[test]
    fn local_and_builtin_routes_pass_offline_without_a_credential() {
        for (role, provider, model) in [
            (LoadoutTestRole::Llm, "testing", "testing-echo-v1"),
            (LoadoutTestRole::Tts, "testing", "testing-synth-v1"),
            (LoadoutTestRole::Embeddings, "fts-only", "sqlite-fts5"),
        ] {
            let result = plan(
                &request(role, provider, model),
                CredentialReferenceStatus::Missing,
            );
            assert!(
                matches!(
                    result,
                    RolePlan::Done(
                        LoadoutRoleTestOutcome::Ok,
                        CredentialReferenceStatus::NotRequired,
                        _
                    )
                ),
                "{provider}/{model}: {result:?}"
            );
        }
    }

    #[test]
    fn unknown_providers_and_models_fail_before_any_credential_lookup() {
        for request in [
            request(LoadoutTestRole::Llm, "not-a-provider", "x"),
            request(LoadoutTestRole::Llm, "groq", "not-a-model"),
            // Right provider and model id, wrong role.
            request(LoadoutTestRole::Tts, "groq", "qwen/qwen3.8-27b"),
        ] {
            let result = plan(&request, CredentialReferenceStatus::Present);
            assert!(
                matches!(
                    result,
                    RolePlan::Done(LoadoutRoleTestOutcome::UnknownRoute, _, _)
                ),
                "{request:?}: {result:?}"
            );
        }
    }

    #[test]
    fn result_serializes_without_secrets_and_with_camel_case_outcomes() {
        let request = request(LoadoutTestRole::Llm, "groq", "qwen/qwen3.8-27b");
        let result = result_for(
            &request,
            Instant::now(),
            LoadoutRoleTestOutcome::ProviderContractUnavailable,
            CredentialReferenceStatus::Present,
            true,
            "detail".into(),
        );
        let json = serde_json::to_value(&result).expect("result json");
        assert_eq!(json["outcome"], "providerContractUnavailable");
        assert_eq!(json["role"], "llm");
        assert_eq!(json["networkRequestPerformed"], false);
        assert!(json["latencyMs"].as_f64().is_some_and(|ms| ms >= 0.0));
    }
}
