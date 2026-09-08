use crate::catalog::{credential_reference_for, CredentialPresence};
use crate::commands::{AppState, CommandError};
use crate::domain::CredentialReferenceStatus;
use crate::local_resources::LocalResourceManager;
use crate::sidecar_protocol::{
    NativeTtsVoiceDiscoveryResult, NativeTtsVoiceDiscoveryStatus, NativeTtsVoiceProvenance,
};
use npc_provider_catalog::{
    CatalogDocument, EgressClass as CatalogEgressClass,
    ExecutionLocation as CatalogExecutionLocation, Modality, Model, Provider, RestrictedData,
};
use npc_provider_loadouts::{
    credential_target_provider_id, CatalogDisclosureV1, CredentialReferenceV1, EgressClassV1,
    ExecutionLocationV1, LoadoutContextV1, LoadoutError, LoadoutId, LoadoutScopeV1,
    ProviderLoadoutDocumentV1, ProviderLoadoutV1, ProviderModelRouteV1, ProviderRole,
    ResolvedProviderLoadoutV1, RoleOverrideV1, RoleRouteV1, TransmittedDataV1, ValidationContextV1,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;
use tempfile::NamedTempFile;
use thiserror::Error;

const LOADOUT_FILE_NAME: &str = "provider-loadouts-v1.json";
const RECOVERY_FILE_NAME: &str = "provider-loadouts-v1.last-good.json";
const MAX_LOADOUT_BYTES: u64 = 512 * 1024;
const PRIVATE_EVALUATION_FILE_NAME: &str = "provider-private-evaluation-v1.json";
const PRIVATE_EVALUATION_PROVIDER_ID: &str = "nvidia-nim";
const MAGPIE_ROUTE_PROVIDER_ID: &str = "nvidia-nim-magpie";
const CARTESIA_QUALIFIED_MODEL_ID: &str = "sonic-3.6";
const CARTESIA_QUALIFIED_STOCK_VOICE_ID: &str = "a0e99841-438c-4a64-b679-ae501e7d6091";
const INWORLD_QUALIFIED_MODEL_ID: &str = "inworld-tts-2-flash";
const INWORLD_QUALIFIED_STOCK_VOICE_ID: &str = "Dennis";
const DEEPGRAM_QUALIFIED_MODEL_ID: &str = "aura-2-arcas-en";
const DEEPGRAM_QUALIFIED_STOCK_VOICE_ID: &str = "Arcas";
pub const PRIVATE_EVALUATION_TERMS_REVISION: &str =
    "nvidia-api-trial-terms-2025-09-19-private-evaluation-v1";
pub const PRIVATE_EVALUATION_TERMS_URL: &str = "https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf";
#[cfg(test)]
pub(crate) const PRODUCTION_APPLICATION_NAMESPACE: &str = "io.github.akshitireddy.interactive-npcs";
pub(crate) const REVIEW_APPLICATION_NAMESPACE: &str =
    "io.github.akshitireddy.interactive-npcs.review";
pub(crate) const DEBUG_APPLICATION_NAMESPACE: &str =
    "io.github.akshitireddy.interactive-npcs.debug";
const BUNDLED_CATALOG: &[u8] = include_bytes!("../../../../catalog/v1/catalog.json");

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderEntitlementModeV1 {
    PrivateEvaluationOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPrivateEvaluationAcknowledgementV1 {
    pub schema_version: u32,
    pub provider_id: String,
    pub mode: ProviderEntitlementModeV1,
    pub terms_revision: String,
    pub catalog_revision: u64,
    pub application_namespace: String,
    pub acknowledged_at_epoch_ms: u64,
    pub promotion_supported: bool,
    pub publication_supported: bool,
}

/// Native-owned pre-consent policy. Unlike the persisted acknowledgement,
/// this is always available so an untrusted WebView can show the exact current
/// terms and identifier-isolated namespace before asking for confirmation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPrivateEvaluationPolicyV1 {
    pub schema_version: u32,
    pub provider_id: String,
    pub mode: ProviderEntitlementModeV1,
    pub terms_revision: String,
    pub terms_url: String,
    pub catalog_revision: u64,
    pub application_namespace: String,
    pub namespace_eligible: bool,
    pub trial_purposes_only: bool,
    pub production_use_supported: bool,
    pub promotion_supported: bool,
    pub publication_supported: bool,
    pub confidential_sensitive_or_personal_data_supported: bool,
    pub limits_apply: bool,
    pub separate_subscription_required_for_production: bool,
    pub access_scope: String,
    pub rate_limit_note: String,
    pub prohibited_data: BTreeSet<RestrictedData>,
    pub security_abuse_logging: bool,
    pub product_improvement_collection_disclosed: bool,
    pub service_specific_disclosures_apply: bool,
    pub exact_model_terms_apply: bool,
    pub acknowledgement: Option<ProviderPrivateEvaluationAcknowledgementV1>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcknowledgeProviderPrivateEvaluationRequestV1 {
    pub provider_id: String,
    pub terms_revision: String,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPrivateEvaluationAuthorityV1 {
    pub acknowledgement: ProviderPrivateEvaluationAcknowledgementV1,
    pub acknowledgement_sha256: String,
    pub credential_present: bool,
    pub exact_stock_voice_discovered: bool,
    pub authorized_roles: Vec<ProviderRole>,
    pub application_namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderPrivateEvaluationStoreV1 {
    schema_version: u32,
    acknowledgement: ProviderPrivateEvaluationAcknowledgementV1,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderLoadoutPersistenceHealth {
    Healthy,
    FirstRun,
    RecoveredLastGood,
    RecoveredSeed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadoutSnapshot {
    pub document: ProviderLoadoutDocumentV1,
    pub catalog_revision: u64,
    pub persistence_health: ProviderLoadoutPersistenceHealth,
    pub detail: String,
    pub credentials_checked: bool,
    pub network_request_performed: bool,
}

impl ProviderLoadoutSnapshot {
    fn new(
        document: ProviderLoadoutDocumentV1,
        persistence_health: ProviderLoadoutPersistenceHealth,
    ) -> Self {
        let detail = match persistence_health {
            ProviderLoadoutPersistenceHealth::Healthy => {
                "Provider loadouts were loaded from private application storage."
            }
            ProviderLoadoutPersistenceHealth::FirstRun => {
                "An API-first starter loadout is ready; provider credentials have not been checked."
            }
            ProviderLoadoutPersistenceHealth::RecoveredLastGood => {
                "The primary loadout file was invalid, so the last known-good document was used."
            }
            ProviderLoadoutPersistenceHealth::RecoveredSeed => {
                "Saved loadouts were invalid, so a safe starter loadout was restored in memory."
            }
        };
        Self {
            document,
            catalog_revision: bundled_catalog().catalog_revision,
            persistence_health,
            detail: detail.into(),
            credentials_checked: false,
            network_request_performed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadoutReview {
    pub resolved: ResolvedProviderLoadoutV1,
    pub offline: bool,
    pub credentials_checked: bool,
    pub network_request_performed: bool,
    pub detail: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
enum ProviderLoadoutStoreError {
    #[error("provider loadout directory is unavailable")]
    Directory,
    #[error("provider loadout file could not be inspected")]
    Inspect,
    #[error("provider loadout file exceeds its safety limit")]
    TooLarge,
    #[error("provider loadout file could not be read")]
    Read,
    #[error("provider loadout JSON is invalid")]
    Json,
    #[error("provider loadout validation failed")]
    Validation,
    #[error("provider loadout does not match the bundled provider catalog")]
    Catalog,
    #[error("provider loadout contains secret-like material")]
    SecretMaterial,
    #[error("provider loadout file could not be staged")]
    Stage,
    #[error("provider loadout file could not be committed")]
    Commit,
}

#[derive(Debug, Clone)]
struct ProviderLoadoutStore {
    directory: PathBuf,
    application_namespace: String,
}

impl ProviderLoadoutStore {
    #[cfg(test)]
    fn new(directory: impl Into<PathBuf>) -> Self {
        Self::new_for_distribution(directory, PRODUCTION_APPLICATION_NAMESPACE.into())
    }

    fn new_for_distribution(directory: impl Into<PathBuf>, application_namespace: String) -> Self {
        Self {
            directory: directory.into(),
            application_namespace,
        }
    }

    fn load(&self) -> ProviderLoadoutSnapshot {
        match self.load_path(&self.primary_path()) {
            Ok(Some(document)) => {
                ProviderLoadoutSnapshot::new(document, ProviderLoadoutPersistenceHealth::Healthy)
            }
            Ok(None) => ProviderLoadoutSnapshot::new(
                starter_document(),
                ProviderLoadoutPersistenceHealth::FirstRun,
            ),
            Err(_) => match self.load_path(&self.recovery_path()) {
                Ok(Some(document)) => ProviderLoadoutSnapshot::new(
                    document,
                    ProviderLoadoutPersistenceHealth::RecoveredLastGood,
                ),
                Ok(None) | Err(_) => ProviderLoadoutSnapshot::new(
                    starter_document(),
                    ProviderLoadoutPersistenceHealth::RecoveredSeed,
                ),
            },
        }
    }

    fn save(&self, document: &ProviderLoadoutDocumentV1) -> Result<(), ProviderLoadoutStoreError> {
        document
            .validate()
            .map_err(|_| ProviderLoadoutStoreError::Validation)?;
        validate_document_against_catalog_for_distribution(document, &self.application_namespace)
            .map_err(|_| ProviderLoadoutStoreError::Catalog)?;
        reject_secret_material(document)?;
        fs::create_dir_all(&self.directory).map_err(|_| ProviderLoadoutStoreError::Directory)?;
        let bytes =
            serde_json::to_vec_pretty(document).map_err(|_| ProviderLoadoutStoreError::Json)?;
        if bytes.len() as u64 > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }

        // Write the recovery copy first. If the process stops between commits,
        // the visible primary remains the previous complete document.
        self.atomic_write(&self.recovery_path(), &bytes)?;
        self.atomic_write(&self.primary_path(), &bytes)?;
        if let Ok(directory) = File::open(&self.directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn load_path(
        &self,
        path: &Path,
    ) -> Result<Option<ProviderLoadoutDocumentV1>, ProviderLoadoutStoreError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProviderLoadoutStoreError::Inspect),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ProviderLoadoutStoreError::Inspect);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(ProviderLoadoutStoreError::Inspect);
            }
        }
        if metadata.len() > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(path)
            .map_err(|_| ProviderLoadoutStoreError::Read)?
            .take(MAX_LOADOUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ProviderLoadoutStoreError::Read)?;
        if bytes.len() as u64 > MAX_LOADOUT_BYTES {
            return Err(ProviderLoadoutStoreError::TooLarge);
        }
        let document: ProviderLoadoutDocumentV1 =
            serde_json::from_slice(&bytes).map_err(|_| ProviderLoadoutStoreError::Json)?;
        let document = migrate_document(document);
        document
            .validate()
            .map_err(|_| ProviderLoadoutStoreError::Validation)?;
        validate_document_against_catalog_for_distribution(&document, &self.application_namespace)
            .map_err(|_| ProviderLoadoutStoreError::Catalog)?;
        reject_secret_material(&document)?;
        Ok(Some(document))
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), ProviderLoadoutStoreError> {
        let mut temporary =
            NamedTempFile::new_in(&self.directory).map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .write_all(bytes)
            .map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| ProviderLoadoutStoreError::Stage)?;
        temporary
            .persist(path)
            .map_err(|_| ProviderLoadoutStoreError::Commit)?;
        Ok(())
    }

    fn primary_path(&self) -> PathBuf {
        self.directory.join(LOADOUT_FILE_NAME)
    }

    fn recovery_path(&self) -> PathBuf {
        self.directory.join(RECOVERY_FILE_NAME)
    }
}

fn migrate_document(mut document: ProviderLoadoutDocumentV1) -> ProviderLoadoutDocumentV1 {
    let catalog_revision = bundled_catalog().catalog_revision;
    for loadout in document.loadouts.values_mut() {
        for role in loadout.roles.values_mut() {
            let RoleOverrideV1::Route(route) = role else {
                continue;
            };
            route.primary.disclosure.catalog_revision = route
                .primary
                .disclosure
                .catalog_revision
                .max(catalog_revision);
            for fallback in &mut route.fallbacks {
                fallback.route.disclosure.catalog_revision = fallback
                    .route
                    .disclosure
                    .catalog_revision
                    .max(catalog_revision);
            }
        }
    }
    let Ok(starter_id) = LoadoutId::new("api-first-starter") else {
        return document;
    };
    let Some(starter) = document.loadouts.get_mut(&starter_id) else {
        return document;
    };
    if let Some(RoleOverrideV1::Route(llm)) = starter.roles.get_mut(&ProviderRole::Llm) {
        if llm.primary.provider_id == "nvidia-nim"
            && llm.primary.model_id == "nvidia/nemotron-3-nano-30b-a3b"
        {
            llm.primary.model_id = "nvidia/nemotron-3.5-lightning-30b-a3b".into();
        }
    }
    if let Some(RoleOverrideV1::Route(stt)) = starter.roles.get_mut(&ProviderRole::Stt) {
        if stt.primary.provider_id == "assemblyai" && stt.primary.model_id == "universal-streaming"
        {
            stt.primary.model_id = "u3-rt-pro".into();
        }
    }
    if let Some(RoleOverrideV1::Route(tts)) = starter.roles.get_mut(&ProviderRole::Tts) {
        if tts.primary.provider_id == "elevenlabs"
            && tts.primary.model_id == "eleven-flash-v2.5"
            && tts.primary.voice_id.is_none()
            && !tts.primary.explicit_user_selection
        {
            tts.primary.model_id = "eleven_flash_v2_5".into();
            tts.primary.voice_id = Some("EXAVITQu4vr4xnSDxMaL".into());
            tts.primary.explicit_user_selection = true;
        }
    }
    document
}

fn bundled_catalog() -> CatalogDocument {
    CatalogDocument::parse(BUNDLED_CATALOG).expect("bundled provider catalog must stay valid")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogRouteError {
    Revision,
    Provider,
    Model,
    Capability,
    Credential,
    Disclosure,
}

#[cfg(test)]
fn validate_document_against_catalog(
    document: &ProviderLoadoutDocumentV1,
) -> Result<(), CatalogRouteError> {
    validate_document_against_catalog_for_distribution(document, PRODUCTION_APPLICATION_NAMESPACE)
}

fn validate_document_against_catalog_for_distribution(
    document: &ProviderLoadoutDocumentV1,
    application_namespace: &str,
) -> Result<(), CatalogRouteError> {
    let private_evaluation_enabled = matches!(
        application_namespace,
        REVIEW_APPLICATION_NAMESPACE | DEBUG_APPLICATION_NAMESPACE
    );
    let catalog = bundled_catalog();
    for loadout in document.loadouts.values() {
        for (role, route_override) in &loadout.roles {
            let RoleOverrideV1::Route(route) = route_override else {
                continue;
            };
            validate_route_against_catalog_for_distribution(
                *role,
                &route.primary,
                &catalog,
                private_evaluation_enabled,
            )?;
            for fallback in &route.fallbacks {
                validate_route_against_catalog_for_distribution(
                    *role,
                    &fallback.route,
                    &catalog,
                    private_evaluation_enabled,
                )?;
            }
        }
    }
    Ok(())
}

fn validate_route_against_catalog_for_distribution(
    role: ProviderRole,
    route: &ProviderModelRouteV1,
    catalog: &CatalogDocument,
    private_evaluation_enabled: bool,
) -> Result<(), CatalogRouteError> {
    if route.disclosure.catalog_revision != catalog.catalog_revision {
        return Err(CatalogRouteError::Revision);
    }
    if validate_builtin_local_route(role, route) {
        return Ok(());
    }

    let catalog_provider_id = credential_target_provider_id(&route.provider_id);
    let provider = catalog
        .content
        .providers
        .iter()
        .find(|provider| provider.id == catalog_provider_id)
        .ok_or(CatalogRouteError::Provider)?;
    validate_provider_route_metadata(route, provider)?;
    if role == ProviderRole::Tts
        && (!hosted_tts_route_supported(&route.provider_id, private_evaluation_enabled)
            || !qualified_hosted_tts_route(route))
    {
        // Catalog metadata may describe future adapters, but native
        // persistence can authorize only transports implemented by the current
        // runtime host. A crafted or stale WebView document must not turn a
        // catalog-only TTS declaration into an executable route.
        return Err(CatalogRouteError::Capability);
    }

    let modality = match role {
        ProviderRole::Llm => Modality::Llm,
        ProviderRole::Stt => Modality::Stt,
        ProviderRole::Tts => Modality::Tts,
        ProviderRole::Embeddings => Modality::Embedding,
        // Vision is currently transported by a catalog-qualified multimodal
        // LLM route. The explicit Image disclosure below keeps it opt-in.
        ProviderRole::Vision => Modality::Llm,
        ProviderRole::Lipsync => return Err(CatalogRouteError::Capability),
    };
    let model = catalog
        .content
        .models
        .iter()
        .filter(|model| model.provider_id == catalog_provider_id && model.modality == modality)
        .find(|model| model_matches_route(model, route))
        .ok_or(CatalogRouteError::Model)?;
    if !model.is_selectable() {
        return Err(CatalogRouteError::Capability);
    }
    if role == ProviderRole::Vision
        && !route
            .disclosure
            .transmitted_data
            .contains(&TransmittedDataV1::Image)
    {
        return Err(CatalogRouteError::Disclosure);
    }
    validate_voice_capability(role, route, model)
}

fn hosted_tts_route_supported(provider_id: &str, private_evaluation_enabled: bool) -> bool {
    match provider_id {
        "cartesia" | "deepgram" | "elevenlabs" | "inworld" => true,
        // Magpie is executable only in an isolated private-evaluation namespace.
        // Activation separately requires the persisted current provider-wide
        // NVIDIA trial acknowledgement, exact credential, and stock-voice gates;
        // compiling a Release review binary does not grant production authority.
        "nvidia-nim-magpie" => private_evaluation_enabled,
        _ => false,
    }
}

fn qualified_hosted_tts_route(route: &ProviderModelRouteV1) -> bool {
    match route.provider_id.as_str() {
        "cartesia" => {
            route.model_id == CARTESIA_QUALIFIED_MODEL_ID
                && route.voice_id.as_deref() == Some(CARTESIA_QUALIFIED_STOCK_VOICE_ID)
                && route.explicit_user_selection
        }
        "inworld" => {
            route.model_id == INWORLD_QUALIFIED_MODEL_ID
                && route.voice_id.as_deref() == Some(INWORLD_QUALIFIED_STOCK_VOICE_ID)
                && route.explicit_user_selection
        }
        "deepgram" => {
            route.model_id == DEEPGRAM_QUALIFIED_MODEL_ID
                && route.voice_id.as_deref() == Some(DEEPGRAM_QUALIFIED_STOCK_VOICE_ID)
                && route.explicit_user_selection
        }
        _ => true,
    }
}

fn model_matches_route(model: &Model, route: &ProviderModelRouteV1) -> bool {
    if model.upstream_id == route.model_id {
        return true;
    }
    match model.upstream_id.as_str() {
        "$provider_default" => true,
        "$user_selected"
        | "$user_selected_ggml"
        | "$user_imported_gguf"
        | "$user_imported_onnx" => {
            model.eligibility.default_candidate || route.explicit_user_selection
        }
        _ => false,
    }
}

fn validate_provider_route_metadata(
    route: &ProviderModelRouteV1,
    provider: &Provider,
) -> Result<(), CatalogRouteError> {
    let execution_matches = matches!(
        (route.disclosure.execution, provider.execution),
        (
            ExecutionLocationV1::Hosted,
            CatalogExecutionLocation::Hosted
        ) | (ExecutionLocationV1::Local, CatalogExecutionLocation::Local)
            | (
                ExecutionLocationV1::ExternalLocal,
                CatalogExecutionLocation::ExternalLocal
            )
    );
    let egress_matches = matches!(
        (route.disclosure.egress, provider.egress),
        (EgressClassV1::None, CatalogEgressClass::Offline)
            | (
                EgressClassV1::ProviderCloud,
                CatalogEgressClass::ProviderCloud
            )
            | (
                EgressClassV1::UserConfiguredEndpoint,
                CatalogEgressClass::UserConfiguredEndpoint
            )
    );
    if !execution_matches || !egress_matches {
        return Err(CatalogRouteError::Disclosure);
    }
    let expected_credential = provider.credential.required.then_some(provider.id.as_str());
    if route
        .credential
        .as_ref()
        .map(|value| value.provider_id.as_str())
        != expected_credential
    {
        return Err(CatalogRouteError::Credential);
    }
    Ok(())
}

fn validate_voice_capability(
    role: ProviderRole,
    route: &ProviderModelRouteV1,
    model: &Model,
) -> Result<(), CatalogRouteError> {
    if role != ProviderRole::Tts {
        return route
            .voice_id
            .is_none()
            .then_some(())
            .ok_or(CatalogRouteError::Capability);
    }
    if model.capabilities.stock_voice_only {
        if route.provider_id == MAGPIE_ROUTE_PROVIDER_ID {
            let voice = route
                .voice_id
                .as_deref()
                .ok_or(CatalogRouteError::Capability)?;
            if !voice.starts_with("Magpie-")
                || model.capabilities.eligible_stock_voice_count == Some(0)
            {
                return Err(CatalogRouteError::Capability);
            }
        } else if !qualified_hosted_tts_route(route) {
            return Err(CatalogRouteError::Capability);
        }
    }
    Ok(())
}

fn validate_builtin_local_route(role: ProviderRole, route: &ProviderModelRouteV1) -> bool {
    let known_route = matches!(
        (role, route.provider_id.as_str(), route.model_id.as_str()),
        (ProviderRole::Embeddings, "fts-only", "sqlite-fts5")
            | (
                ProviderRole::Lipsync,
                "local-visual-worker",
                "a2f3d-regression" | "nvidia-ar-lipsync" | "musetalk"
            )
    );
    known_route
        && route.disclosure.execution == ExecutionLocationV1::Local
        && route.disclosure.egress == EgressClassV1::None
        && route.disclosure.transmitted_data.is_empty()
        && route.credential.is_none()
        && route.voice_id.is_none()
        && (role != ProviderRole::Lipsync || route.explicit_user_selection)
}

#[derive(Debug)]
pub(crate) struct ProviderLoadoutManager {
    store: ProviderLoadoutStore,
    snapshot: Mutex<ProviderLoadoutSnapshot>,
    discovered_stock_voices: Mutex<BTreeMap<(String, String), DiscoveredStockVoiceMembership>>,
    private_evaluation_enabled: bool,
    application_namespace: String,
    credentials: Option<Arc<dyn CredentialPresence>>,
    private_evaluation_path: PathBuf,
    private_evaluation_acknowledgement: Mutex<Option<ProviderPrivateEvaluationAcknowledgementV1>>,
}

#[derive(Debug, Clone)]
struct DiscoveredStockVoiceMembership {
    voice_ids: BTreeSet<String>,
    expires_at_epoch_ms: u64,
}

impl ProviderLoadoutManager {
    #[cfg(test)]
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self::new_internal(directory, PRODUCTION_APPLICATION_NAMESPACE.into(), None)
    }

    pub(crate) fn new_for_distribution(
        directory: PathBuf,
        application_namespace: String,
        credentials: Arc<dyn CredentialPresence>,
    ) -> Self {
        Self::new_internal(directory, application_namespace, Some(credentials))
    }

    fn new_internal(
        directory: PathBuf,
        application_namespace: String,
        credentials: Option<Arc<dyn CredentialPresence>>,
    ) -> Self {
        let private_evaluation_enabled = matches!(
            application_namespace.as_str(),
            REVIEW_APPLICATION_NAMESPACE | DEBUG_APPLICATION_NAMESPACE
        );
        let private_evaluation_path = directory.join(PRIVATE_EVALUATION_FILE_NAME);
        let private_evaluation_acknowledgement = private_evaluation_enabled
            .then(|| {
                load_private_evaluation_acknowledgement(
                    &private_evaluation_path,
                    &application_namespace,
                )
            })
            .flatten();
        let store =
            ProviderLoadoutStore::new_for_distribution(directory, application_namespace.clone());
        let snapshot = store.load();
        Self {
            store,
            snapshot: Mutex::new(snapshot),
            discovered_stock_voices: Mutex::new(BTreeMap::new()),
            private_evaluation_enabled,
            application_namespace,
            credentials,
            private_evaluation_path,
            private_evaluation_acknowledgement: Mutex::new(private_evaluation_acknowledgement),
        }
    }

    pub(crate) const fn private_evaluation_enabled(&self) -> bool {
        self.private_evaluation_enabled
    }

    pub(crate) fn application_namespace(&self) -> &str {
        &self.application_namespace
    }

    pub(crate) fn record_stock_voice_discovery(
        &self,
        result: &NativeTtsVoiceDiscoveryResult,
    ) -> Result<(), CommandError> {
        if result.provider_id != MAGPIE_ROUTE_PROVIDER_ID
            || result.model_id != "magpie-tts-multilingual"
            || result.provenance != NativeTtsVoiceProvenance::ProviderStockDiscovery
        {
            return Err(CommandError::InvalidRequest {
                message: "stock_voice_discovery_invalid: the result is not the authenticated NVIDIA Magpie stock-voice catalog".into(),
            });
        }
        let mut catalogs = self
            .discovered_stock_voices
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let key = (result.provider_id.clone(), result.model_id.clone());
        if result.status != NativeTtsVoiceDiscoveryStatus::Available || result.error.is_some() {
            catalogs.remove(&key);
            return Ok(());
        }
        let expires_at_epoch_ms = result
            .refresh
            .expires_at_epoch_ms
            .filter(|expires| *expires > current_epoch_ms())
            .ok_or_else(|| CommandError::InvalidRequest {
                message: "stock_voice_discovery_expired: refresh the provider stock-voice catalog"
                    .into(),
            })?;
        let voice_ids = result
            .voices
            .iter()
            .map(|voice| voice.voice_id.clone())
            .collect::<BTreeSet<_>>();
        if voice_ids.is_empty() || voice_ids.len() != result.voices.len() {
            return Err(CommandError::InvalidRequest {
                message:
                    "stock_voice_discovery_invalid: the provider returned no unique eligible voices"
                        .into(),
            });
        }
        catalogs.insert(
            key,
            DiscoveredStockVoiceMembership {
                voice_ids,
                expires_at_epoch_ms,
            },
        );
        Ok(())
    }

    pub(crate) fn private_evaluation_acknowledgement(
        &self,
    ) -> Result<Option<ProviderPrivateEvaluationAcknowledgementV1>, CommandError> {
        self.private_evaluation_acknowledgement
            .lock()
            .map(|value| value.clone())
            .map_err(|_| CommandError::StateUnavailable)
    }

    pub(crate) fn private_evaluation_policy(
        &self,
    ) -> Result<ProviderPrivateEvaluationPolicyV1, CommandError> {
        let catalog = bundled_catalog();
        let terms = catalog
            .content
            .providers
            .iter()
            .find(|provider| provider.id == "nvidia-nim")
            .and_then(|provider| provider.trial_terms.as_ref())
            .filter(|terms| {
                terms.terms_revision == PRIVATE_EVALUATION_TERMS_REVISION
                    && terms.terms_url == PRIVATE_EVALUATION_TERMS_URL
            })
            .ok_or_else(|| CommandError::InvalidRequest {
                message: "private_evaluation_policy_unavailable: the bundled provider catalog does not match the native acknowledgement policy".into(),
            })?;
        Ok(ProviderPrivateEvaluationPolicyV1 {
            schema_version: 1,
            provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
            mode: ProviderEntitlementModeV1::PrivateEvaluationOnly,
            terms_revision: terms.terms_revision.clone(),
            terms_url: terms.terms_url.clone(),
            catalog_revision: catalog.catalog_revision,
            application_namespace: self.application_namespace.clone(),
            namespace_eligible: self.private_evaluation_enabled,
            trial_purposes_only: true,
            production_use_supported: false,
            promotion_supported: false,
            publication_supported: false,
            confidential_sensitive_or_personal_data_supported: false,
            limits_apply: true,
            separate_subscription_required_for_production: true,
            access_scope: terms.access_scope.clone(),
            rate_limit_note: terms.rate_limit_note.clone(),
            prohibited_data: terms.prohibited_data.clone(),
            security_abuse_logging: terms.security_abuse_logging,
            product_improvement_collection_disclosed: terms
                .product_improvement_collection_disclosed,
            service_specific_disclosures_apply: terms.service_specific_disclosures_apply,
            exact_model_terms_apply: terms.exact_model_terms_apply,
            acknowledgement: self
                .private_evaluation_acknowledgement()?
                .filter(|acknowledgement| {
                    acknowledgement.application_namespace == self.application_namespace
                        && valid_private_evaluation_acknowledgement(acknowledgement)
                }),
        })
    }

    pub(crate) fn acknowledge_private_evaluation(
        &self,
        request: AcknowledgeProviderPrivateEvaluationRequestV1,
    ) -> Result<ProviderPrivateEvaluationAcknowledgementV1, CommandError> {
        if !self.private_evaluation_enabled {
            return Err(CommandError::InvalidRequest {
                message: "private_evaluation_unavailable: this public production namespace cannot authorize experimental provider terms".into(),
            });
        }
        if request.provider_id != PRIVATE_EVALUATION_PROVIDER_ID
            || request.terms_revision != PRIVATE_EVALUATION_TERMS_REVISION
        {
            return Err(CommandError::InvalidRequest {
                message: "private_evaluation_terms_mismatch: refresh the exact current provider terms before acknowledging".into(),
            });
        }
        if !request.explicit_user_confirmation {
            return Err(CommandError::InvalidRequest {
                message: "private_evaluation_confirmation_required: explicit user confirmation is required".into(),
            });
        }
        let acknowledgement = ProviderPrivateEvaluationAcknowledgementV1 {
            schema_version: 1,
            provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
            mode: ProviderEntitlementModeV1::PrivateEvaluationOnly,
            terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
            catalog_revision: bundled_catalog().catalog_revision,
            application_namespace: self.application_namespace.clone(),
            acknowledged_at_epoch_ms: current_epoch_ms(),
            promotion_supported: false,
            publication_supported: false,
        };
        persist_private_evaluation_acknowledgement(
            &self.private_evaluation_path,
            &acknowledgement,
        )?;
        *self
            .private_evaluation_acknowledgement
            .lock()
            .map_err(|_| CommandError::StateUnavailable)? = Some(acknowledgement.clone());
        Ok(acknowledgement)
    }

    fn validate_discovered_stock_voices(
        &self,
        document: &ProviderLoadoutDocumentV1,
    ) -> Result<(), CommandError> {
        let catalogs = self
            .discovered_stock_voices
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let now = current_epoch_ms();
        for loadout in document.loadouts.values() {
            let Some(RoleOverrideV1::Route(tts)) = loadout.roles.get(&ProviderRole::Tts) else {
                continue;
            };
            for route in std::iter::once(&tts.primary)
                .chain(tts.fallbacks.iter().map(|fallback| &fallback.route))
            {
                if route.provider_id != MAGPIE_ROUTE_PROVIDER_ID {
                    continue;
                }
                let membership = catalogs
                    .get(&(route.provider_id.clone(), route.model_id.clone()))
                    .filter(|catalog| catalog.expires_at_epoch_ms > now)
                    .ok_or_else(|| CommandError::InvalidRequest {
                        message: "stock_voice_discovery_required: refresh NVIDIA Magpie voices before saving or activating this loadout".into(),
                    })?;
                let voice_id =
                    route
                        .voice_id
                        .as_ref()
                        .ok_or_else(|| {
                            CommandError::InvalidRequest {
                        message:
                            "stock_voice_required: select a discovered NVIDIA Magpie stock voice"
                                .into(),
                    }
                        })?;
                if !membership.voice_ids.contains(voice_id) {
                    return Err(CommandError::InvalidRequest {
                        message: "stock_voice_not_discovered: select an exact eligible voice from the authenticated NVIDIA Magpie discovery result".into(),
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Result<ProviderLoadoutSnapshot, CommandError> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| CommandError::StateUnavailable)
    }

    fn mutate(
        &self,
        operation: impl FnOnce(&mut ProviderLoadoutDocumentV1) -> Result<(), ()>,
    ) -> Result<ProviderLoadoutSnapshot, CommandError> {
        let mut guard = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let mut candidate = guard.document.clone();
        operation(&mut candidate).map_err(|()| CommandError::InvalidRequest {
            message: "provider loadout request failed validation".into(),
        })?;
        self.validate_discovered_stock_voices(&candidate)?;
        self.store
            .save(&candidate)
            .map_err(|_| CommandError::Persistence {
                message: "provider loadouts could not be persisted".into(),
            })?;
        *guard = ProviderLoadoutSnapshot::new(candidate, ProviderLoadoutPersistenceHealth::Healthy);
        Ok(guard.clone())
    }

    pub(crate) fn review(
        &self,
        context: LoadoutContextV1,
        offline: bool,
        local_resources: &LocalResourceManager,
    ) -> Result<ProviderLoadoutReview, CommandError> {
        self.validate_current_stock_voices()?;
        let resolved = self.resolve_raw(&context, offline)?;
        reject_unadmitted_lipsync(&resolved, local_resources)?;
        let private_evaluation = self.private_evaluation_authority_for_routes(&resolved)?;
        Ok(ProviderLoadoutReview {
            resolved,
            offline,
            credentials_checked: private_evaluation.is_some(),
            network_request_performed: false,
            detail: if private_evaluation.is_some() {
                "Routes were resolved from saved configuration. The exact provider-wide NVIDIA trial acknowledgement, credential presence, and route-specific readiness (including stock-voice membership when applicable) were checked natively; no provider network request was made."
            } else {
                "Routes were resolved from saved configuration only; credentials and providers were not contacted."
            }
            .into(),
        })
    }

    pub(crate) fn resolve_for_turn(
        &self,
        context: LoadoutContextV1,
        offline: bool,
        local_resources: &LocalResourceManager,
    ) -> Result<ResolvedProviderLoadoutV1, CommandError> {
        self.validate_current_stock_voices()?;
        let mut resolved = self.resolve_raw(&context, offline)?;
        if let Some(route) = resolved.roles.get(&ProviderRole::Lipsync) {
            if !local_resources
                .admits_complete_lipsync_route(&route.primary.provider_id, &route.primary.model_id)
                .map_err(|error| CommandError::Product {
                    message: error.to_string(),
                })?
            {
                // Runtime receives no local lip-sync route unless native model
                // state proves an exact complete and admitted pack. A crafted
                // or stale persisted route therefore pins as Disabled.
                resolved.roles.remove(&ProviderRole::Lipsync);
            }
        }
        self.private_evaluation_authority_for_routes(&resolved)?;
        Ok(resolved)
    }

    fn validate_current_stock_voices(&self) -> Result<(), CommandError> {
        let document = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?
            .document
            .clone();
        self.validate_discovered_stock_voices(&document)
    }

    fn resolve_raw(
        &self,
        context: &LoadoutContextV1,
        offline: bool,
    ) -> Result<ResolvedProviderLoadoutV1, CommandError> {
        let guard = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let validation = if offline {
            ValidationContextV1::offline()
        } else {
            ValidationContextV1::online()
        };
        guard
            .document
            .resolve(context, &validation)
            .map_err(loadout_resolution_error)
    }

    fn activate(
        &self,
        id: &LoadoutId,
        local_resources: &LocalResourceManager,
    ) -> Result<ProviderLoadoutSnapshot, CommandError> {
        let mut guard = self
            .snapshot
            .lock()
            .map_err(|_| CommandError::StateUnavailable)?;
        let mut candidate = guard.document.clone();
        candidate.activate(id).map_err(loadout_resolution_error)?;
        let scope = candidate
            .loadouts
            .get(id)
            .map(|loadout| loadout.scope.clone())
            .ok_or_else(|| CommandError::InvalidRequest {
                message:
                    "provider_loadout_invalid: the selected provider profile could not be resolved"
                        .into(),
            })?;
        let context = context_for_scope(&scope);
        let resolved = candidate
            .resolve(&context, &ValidationContextV1::online())
            .map_err(loadout_resolution_error)?;
        self.validate_discovered_stock_voices(&candidate)?;
        reject_unadmitted_lipsync(&resolved, local_resources)?;
        self.private_evaluation_authority_for_routes(&resolved)?;
        self.store
            .save(&candidate)
            .map_err(|_| CommandError::Persistence {
                message: "provider loadouts could not be persisted".into(),
            })?;
        *guard = ProviderLoadoutSnapshot::new(candidate, ProviderLoadoutPersistenceHealth::Healthy);
        Ok(guard.clone())
    }

    pub(crate) fn private_evaluation_authority_for_routes(
        &self,
        resolved: &ResolvedProviderLoadoutV1,
    ) -> Result<Option<ProviderPrivateEvaluationAuthorityV1>, CommandError> {
        // A fallback is dormant policy, not the activated turn route. The
        // runtime snapshot executes only the pinned primary and forbids
        // automatic fallback, so provider-wide trial authority must not be
        // minted or required merely because a manual-only NVIDIA route exists.
        let trial_routes = resolved
            .roles
            .iter()
            .filter_map(|(role, routes)| {
                is_nvidia_trial_route(&routes.primary).then_some((*role, &routes.primary))
            })
            .collect::<Vec<_>>();
        if trial_routes.is_empty() {
            return Ok(None);
        }
        if !self.private_evaluation_enabled {
            return Err(CommandError::InvalidRequest {
                message: "private_evaluation_unavailable: NVIDIA hosted Developer API trial routes cannot activate in the public production namespace".into(),
            });
        }
        let acknowledgement = self
            .private_evaluation_acknowledgement()?
            .filter(|acknowledgement| {
                acknowledgement.application_namespace == self.application_namespace
                    && valid_private_evaluation_acknowledgement(acknowledgement)
            })
            .ok_or_else(|| CommandError::InvalidRequest {
                message: "private_evaluation_acknowledgement_required: acknowledge the exact current provider-wide NVIDIA API trial terms before activating any hosted NVIDIA trial route".into(),
            })?;
        for (_, route) in &trial_routes {
            let credential = route.credential.as_ref().ok_or_else(|| {
                CommandError::InvalidRequest {
                    message: "private_evaluation_credential_required: select the native NVIDIA NIM credential reference for every active NVIDIA trial route".into(),
                }
            })?;
            if credential.provider_id != PRIVATE_EVALUATION_PROVIDER_ID
                || credential.reference_id != "personal"
            {
                return Err(CommandError::InvalidRequest {
                    message: "private_evaluation_credential_mismatch: NVIDIA trial routes require the exact native NVIDIA NIM personal credential reference".into(),
                });
            }
        }
        let credential_reference = credential_reference_for("nvidia-nim").ok_or_else(|| {
            CommandError::InvalidRequest {
                message: "private_evaluation_credential_unavailable: NVIDIA NIM is not in the native credential allowlist".into(),
            }
        })?;
        let credential_present = self.credentials.as_ref().is_some_and(|credentials| {
            credentials.status(&credential_reference) == CredentialReferenceStatus::Present
        });
        if !credential_present {
            return Err(CommandError::InvalidRequest {
                message: "private_evaluation_credential_missing: save the NVIDIA NIM credential in the native vault before activation".into(),
            });
        }
        let magpie_route = trial_routes.iter().find_map(|(_, route)| {
            (route.provider_id == MAGPIE_ROUTE_PROVIDER_ID).then_some(*route)
        });
        let exact_stock_voice_discovered = if let Some(route) = magpie_route {
            self.discovered_stock_voices
                .lock()
                .map_err(|_| CommandError::StateUnavailable)?
                .get(&(route.provider_id.clone(), route.model_id.clone()))
                .is_some_and(|membership| {
                    membership.expires_at_epoch_ms > current_epoch_ms()
                        && route
                            .voice_id
                            .as_ref()
                            .is_some_and(|voice| membership.voice_ids.contains(voice))
                })
        } else {
            true
        };
        if !exact_stock_voice_discovered {
            return Err(CommandError::InvalidRequest {
                message: "stock_voice_discovery_required: refresh and select an exact authenticated NVIDIA Magpie stock voice".into(),
            });
        }
        Ok(Some(ProviderPrivateEvaluationAuthorityV1 {
            acknowledgement_sha256: private_evaluation_acknowledgement_sha256(&acknowledgement)?,
            acknowledgement,
            credential_present,
            exact_stock_voice_discovered,
            authorized_roles: trial_routes.iter().map(|(role, _)| *role).collect(),
            application_namespace: self.application_namespace.clone(),
        }))
    }
}

fn is_nvidia_trial_route(route: &ProviderModelRouteV1) -> bool {
    route.disclosure.execution == ExecutionLocationV1::Hosted
        && matches!(
            route.provider_id.as_str(),
            PRIVATE_EVALUATION_PROVIDER_ID | MAGPIE_ROUTE_PROVIDER_ID
        )
}

fn valid_private_evaluation_acknowledgement(
    acknowledgement: &ProviderPrivateEvaluationAcknowledgementV1,
) -> bool {
    acknowledgement.schema_version == 1
        && acknowledgement.provider_id == PRIVATE_EVALUATION_PROVIDER_ID
        && acknowledgement.mode == ProviderEntitlementModeV1::PrivateEvaluationOnly
        && acknowledgement.terms_revision == PRIVATE_EVALUATION_TERMS_REVISION
        && acknowledgement.catalog_revision == bundled_catalog().catalog_revision
        && matches!(
            acknowledgement.application_namespace.as_str(),
            REVIEW_APPLICATION_NAMESPACE | DEBUG_APPLICATION_NAMESPACE
        )
        && acknowledgement.acknowledged_at_epoch_ms > 0
        && !acknowledgement.promotion_supported
        && !acknowledgement.publication_supported
}

fn private_evaluation_acknowledgement_sha256(
    acknowledgement: &ProviderPrivateEvaluationAcknowledgementV1,
) -> Result<String, CommandError> {
    let bytes = serde_json::to_vec(acknowledgement).map_err(|_| CommandError::Persistence {
        message: "private evaluation acknowledgement could not be serialized".into(),
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn load_private_evaluation_acknowledgement(
    path: &Path,
    expected_application_namespace: &str,
) -> Option<ProviderPrivateEvaluationAcknowledgementV1> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let stored: ProviderPrivateEvaluationStoreV1 = serde_json::from_slice(&bytes).ok()?;
    (stored.schema_version == 1
        && valid_private_evaluation_acknowledgement(&stored.acknowledgement)
        && stored.acknowledgement.application_namespace == expected_application_namespace)
        .then_some(stored.acknowledgement)
}

fn persist_private_evaluation_acknowledgement(
    path: &Path,
    acknowledgement: &ProviderPrivateEvaluationAcknowledgementV1,
) -> Result<(), CommandError> {
    if !valid_private_evaluation_acknowledgement(acknowledgement) {
        return Err(CommandError::InvalidRequest {
            message: "private evaluation acknowledgement is not current".into(),
        });
    }
    let directory = path.parent().ok_or_else(|| CommandError::Persistence {
        message: "private evaluation acknowledgement directory is unavailable".into(),
    })?;
    fs::create_dir_all(directory).map_err(|_| CommandError::Persistence {
        message: "private evaluation acknowledgement directory could not be created".into(),
    })?;
    let bytes = serde_json::to_vec_pretty(&ProviderPrivateEvaluationStoreV1 {
        schema_version: 1,
        acknowledgement: acknowledgement.clone(),
    })
    .map_err(|_| CommandError::Persistence {
        message: "private evaluation acknowledgement could not be serialized".into(),
    })?;
    let mut staged = NamedTempFile::new_in(directory).map_err(|_| CommandError::Persistence {
        message: "private evaluation acknowledgement could not be staged".into(),
    })?;
    staged
        .write_all(&bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|_| CommandError::Persistence {
            message: "private evaluation acknowledgement could not be staged".into(),
        })?;
    staged
        .persist(path)
        .map_err(|_| CommandError::Persistence {
            message: "private evaluation acknowledgement could not be committed".into(),
        })?;
    Ok(())
}

fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn context_for_scope(scope: &LoadoutScopeV1) -> LoadoutContextV1 {
    match scope {
        LoadoutScopeV1::Global => LoadoutContextV1::global(),
        LoadoutScopeV1::Game { game_id } => LoadoutContextV1::game(game_id),
        LoadoutScopeV1::Character {
            game_id,
            character_id,
        } => LoadoutContextV1::character(game_id, character_id),
    }
}

fn reject_unadmitted_lipsync(
    resolved: &ResolvedProviderLoadoutV1,
    local_resources: &LocalResourceManager,
) -> Result<(), CommandError> {
    let Some(route) = resolved.roles.get(&ProviderRole::Lipsync) else {
        return Ok(());
    };
    let admitted = local_resources
        .admits_complete_lipsync_route(&route.primary.provider_id, &route.primary.model_id)
        .map_err(|error| CommandError::Product {
            message: error.to_string(),
        })?;
    if admitted {
        Ok(())
    } else {
        Err(CommandError::InvalidRequest {
            message: "local_lipsync_unavailable: the exact route is not backed by an installed, hash-verified, device-admitted complete lip-sync pack".into(),
        })
    }
}

fn loadout_resolution_error(error: LoadoutError) -> CommandError {
    let message = match error {
        LoadoutError::CloudRouteForbiddenOffline { .. } => {
            "provider_loadout_policy_blocked: a selected hosted route is unavailable in fully local mode"
        }
        LoadoutError::RequiredRoleMissing(_) => {
            "provider_loadout_incomplete: a required provider role is not configured"
        }
        _ => "provider_loadout_invalid: the selected provider profile could not be resolved",
    };
    CommandError::InvalidRequest {
        message: message.into(),
    }
}

#[tauri::command]
pub fn provider_loadout_snapshot(
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.snapshot()
}

#[tauri::command]
pub fn provider_private_evaluation_acknowledgement(
    state: State<'_, AppState>,
) -> Result<Option<ProviderPrivateEvaluationAcknowledgementV1>, CommandError> {
    state.provider_loadouts.private_evaluation_acknowledgement()
}

#[tauri::command]
pub fn provider_private_evaluation_policy(
    state: State<'_, AppState>,
) -> Result<ProviderPrivateEvaluationPolicyV1, CommandError> {
    state.provider_loadouts.private_evaluation_policy()
}

#[tauri::command]
pub fn acknowledge_provider_private_evaluation(
    request: AcknowledgeProviderPrivateEvaluationRequestV1,
    state: State<'_, AppState>,
) -> Result<ProviderPrivateEvaluationAcknowledgementV1, CommandError> {
    state
        .provider_loadouts
        .acknowledge_private_evaluation(request)
}

#[tauri::command]
pub fn create_provider_loadout(
    loadout: ProviderLoadoutV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.insert(loadout).map_err(|_| ()))
}

#[tauri::command]
pub fn clone_provider_loadout(
    source_id: LoadoutId,
    new_id: LoadoutId,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.mutate(move |document| {
        document
            .clone_loadout(&source_id, new_id, new_name)
            .map_err(|_| ())
    })
}

#[tauri::command]
pub fn rename_provider_loadout(
    id: LoadoutId,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.rename(&id, new_name).map_err(|_| ()))
}

#[tauri::command]
pub fn update_provider_loadout(
    id: LoadoutId,
    loadout: ProviderLoadoutV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| update_document_entry(document, &id, loadout))
}

#[tauri::command]
pub fn delete_provider_loadout(
    id: LoadoutId,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .mutate(move |document| document.delete(&id).map(|_| ()).map_err(|_| ()))
}

fn update_document_entry(
    document: &mut ProviderLoadoutDocumentV1,
    id: &LoadoutId,
    loadout: ProviderLoadoutV1,
) -> Result<(), ()> {
    if &loadout.id != id {
        return Err(());
    }
    let previous = document.loadouts.get(id).cloned().ok_or(())?;
    if previous.scope != loadout.scope {
        return Err(());
    }
    document.loadouts.insert(id.clone(), loadout);
    if document.validate().is_err() {
        document.loadouts.insert(id.clone(), previous);
        return Err(());
    }
    Ok(())
}

#[tauri::command]
pub fn activate_provider_loadout(
    id: LoadoutId,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state
        .provider_loadouts
        .activate(&id, &state.local_resources)
}

#[tauri::command]
pub fn deactivate_provider_loadout_scope(
    scope: LoadoutScopeV1,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutSnapshot, CommandError> {
    state.provider_loadouts.mutate(move |document| {
        document
            .deactivate_scope(&scope)
            .map(|_| ())
            .map_err(|_| ())
    })
}

#[tauri::command]
pub fn review_provider_loadout(
    context: LoadoutContextV1,
    offline: bool,
    state: State<'_, AppState>,
) -> Result<ProviderLoadoutReview, CommandError> {
    state
        .provider_loadouts
        .review(context, offline, &state.local_resources)
}

pub(crate) fn starter_document() -> ProviderLoadoutDocumentV1 {
    let catalog_revision = bundled_catalog().catalog_revision;
    let hosted = |provider_id: &str,
                  model_id: &str,
                  voice_id: Option<&str>,
                  privacy_summary: &str,
                  cost_summary: &str,
                  transmitted_data: BTreeSet<TransmittedDataV1>| {
        RoleOverrideV1::Route(Box::new(RoleRouteV1 {
            primary: ProviderModelRouteV1 {
                provider_id: provider_id.into(),
                model_id: model_id.into(),
                voice_id: voice_id.map(str::to_owned),
                credential: Some(CredentialReferenceV1 {
                    provider_id: provider_id.into(),
                    reference_id: "personal".into(),
                }),
                disclosure: CatalogDisclosureV1 {
                    catalog_revision,
                    execution: ExecutionLocationV1::Hosted,
                    egress: EgressClassV1::ProviderCloud,
                    privacy_summary: privacy_summary.into(),
                    cost_summary: cost_summary.into(),
                    transmitted_data,
                },
                // The starter names concrete upstream models; catalog routes
                // that rely on discovery therefore remain an explicit,
                // reviewable selection rather than an implicit provider default.
                explicit_user_selection: true,
            },
            fallbacks: Vec::new(),
        }))
    };
    let default = ProviderLoadoutV1 {
        id: LoadoutId::new("api-first-starter").expect("static starter id"),
        name: "API-first starter".into(),
        scope: LoadoutScopeV1::Global,
        parent: None,
        roles: BTreeMap::from([
            (
                ProviderRole::Llm,
                hosted(
                    "groq",
                    "qwen/qwen3.6-27b",
                    None,
                    "Transcript, selected game context, and memory context are sent to Groq when this route is used.",
                    "Uses the user's Groq account and its current free or paid limits.",
                    BTreeSet::from([
                        TransmittedDataV1::Transcript,
                        TransmittedDataV1::GameContext,
                        TransmittedDataV1::MemoryContext,
                    ]),
                ),
            ),
            (
                ProviderRole::Stt,
                hosted(
                    "assemblyai",
                    "u3-rt-pro",
                    None,
                    "Microphone audio is sent to AssemblyAI only while this speech route is used.",
                    "Uses the user's AssemblyAI account and its current trial or paid limits.",
                    BTreeSet::from([TransmittedDataV1::MicrophoneAudio]),
                ),
            ),
            (
                ProviderRole::Tts,
                hosted(
                    "cartesia",
                    CARTESIA_QUALIFIED_MODEL_ID,
                    Some(CARTESIA_QUALIFIED_STOCK_VOICE_ID),
                    "Response text is sent to Cartesia only while this voice route is used.",
                    "Uses the user's Cartesia account and its current free or paid limits.",
                    BTreeSet::from([TransmittedDataV1::ResponseText]),
                ),
            ),
            (
                ProviderRole::Embeddings,
                RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                    primary: ProviderModelRouteV1 {
                        provider_id: "fts-only".into(),
                        model_id: "sqlite-fts5".into(),
                        voice_id: None,
                        credential: None,
                        disclosure: CatalogDisclosureV1 {
                            catalog_revision,
                            execution: ExecutionLocationV1::Local,
                            egress: EgressClassV1::None,
                            privacy_summary: "Scoped keyword memory search stays on this PC."
                                .into(),
                            cost_summary: "Uses the bundled SQLite FTS index without a model download."
                                .into(),
                            transmitted_data: BTreeSet::new(),
                        },
                        explicit_user_selection: true,
                    },
                    fallbacks: Vec::new(),
                })),
            ),
            (ProviderRole::Vision, RoleOverrideV1::Disabled),
            (ProviderRole::Lipsync, RoleOverrideV1::Disabled),
        ]),
    };
    ProviderLoadoutDocumentV1::new(default).expect("static starter loadout must validate")
}

fn reject_secret_material(
    document: &ProviderLoadoutDocumentV1,
) -> Result<(), ProviderLoadoutStoreError> {
    let value = serde_json::to_value(document).map_err(|_| ProviderLoadoutStoreError::Json)?;
    if contains_secret_material(&value) {
        Err(ProviderLoadoutStoreError::SecretMaterial)
    } else {
        Ok(())
    }
}

fn contains_secret_material(value: &Value) -> bool {
    match value {
        Value::Object(entries) => entries.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
            matches!(
                normalized.as_str(),
                "apikey"
                    | "token"
                    | "secret"
                    | "password"
                    | "authorization"
                    | "credentialvalue"
                    | "privatekey"
                    | "accesstoken"
            ) || contains_secret_material(value)
        }),
        Value::Array(values) => values.iter().any(contains_secret_material),
        Value::String(text) => looks_like_secret(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn looks_like_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("bearer ") || lower.contains("-----begin private key-----") {
        return true;
    }
    const PREFIXES: [&str; 8] = [
        "sk-", "sk_", "nvapi-", "gsk_", "pplx-", "xai-", "aiza", "akia",
    ];
    if PREFIXES
        .iter()
        .any(|prefix| contains_token_prefix(&lower, prefix))
    {
        return true;
    }
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            let has_digit = token.bytes().any(|byte| byte.is_ascii_digit());
            let mixed_case = token.bytes().any(|byte| byte.is_ascii_lowercase())
                && token.bytes().any(|byte| byte.is_ascii_uppercase());
            token.len() >= 40 || (token.len() >= 32 && (has_digit || mixed_case))
        })
}

fn contains_token_prefix(text: &str, prefix: &str) -> bool {
    text.match_indices(prefix).any(|(index, _)| {
        let at_boundary = index == 0
            || text[..index]
                .chars()
                .next_back()
                .map_or(true, |character| !character.is_ascii_alphanumeric());
        let tail = text[index + prefix.len()..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
            .count();
        at_boundary && tail >= 12
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CredentialMutationError;
    use interactive_npcs_credential_vault::SecretValue;
    use npc_provider_loadouts::{CredentialReferenceV1, ExplicitFallbackV1, FallbackActivationV1};
    use pretty_assertions::assert_eq;

    fn id(value: &str) -> LoadoutId {
        LoadoutId::new(value).expect("valid fixture id")
    }

    fn local_resources() -> (tempfile::TempDir, LocalResourceManager) {
        let directory = tempfile::tempdir().expect("local resource directory");
        let manager = LocalResourceManager::new(directory.path(), None).expect("local resources");
        (directory, manager)
    }

    #[derive(Debug)]
    struct PresentCredential;

    impl CredentialPresence for PresentCredential {
        fn status(&self, _reference: &str) -> CredentialReferenceStatus {
            CredentialReferenceStatus::Present
        }

        fn save(
            &self,
            _reference: &str,
            _secret: &SecretValue,
        ) -> Result<(), CredentialMutationError> {
            Ok(())
        }

        fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
            Ok(())
        }

        fn availability_detail(&self) -> &'static str {
            "test presence only"
        }
    }

    fn local_lipsync_route(model_id: &str) -> RoleOverrideV1 {
        RoleOverrideV1::Route(Box::new(RoleRouteV1 {
            primary: ProviderModelRouteV1 {
                provider_id: "local-visual-worker".into(),
                model_id: model_id.into(),
                voice_id: None,
                credential: None,
                disclosure: CatalogDisclosureV1 {
                    catalog_revision: bundled_catalog().catalog_revision,
                    execution: ExecutionLocationV1::Local,
                    egress: EgressClassV1::None,
                    privacy_summary: "Local visual processing only; no content leaves this PC."
                        .into(),
                    cost_summary: "Optional user-installed local resource.".into(),
                    transmitted_data: BTreeSet::new(),
                },
                explicit_user_selection: true,
            },
            fallbacks: Vec::new(),
        }))
    }

    #[test]
    fn first_run_seed_is_api_first_but_claims_no_credentials_or_network() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let snapshot = ProviderLoadoutStore::new(temp.path()).load();
        assert_eq!(
            snapshot.persistence_health,
            ProviderLoadoutPersistenceHealth::FirstRun
        );
        assert!(!snapshot.credentials_checked);
        assert!(!snapshot.network_request_performed);
        assert_eq!(snapshot.document.loadouts.len(), 1);
        validate_document_against_catalog(&snapshot.document).expect("starter matches catalog");
        let serialized = serde_json::to_string(&snapshot).expect("serialize seed");
        assert!(!serialized.contains("api_key"));
        assert!(!serialized.contains("credential_value"));
        let resolved = snapshot
            .document
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("starter route resolution");
        let pinned = resolved.pin_turn_routes(1);
        assert_eq!(
            pinned
                .route(ProviderRole::Llm)
                .map(|route| (route.provider_id.as_str(), route.model_id.as_str())),
            Some(("groq", "qwen/qwen3.6-27b"))
        );
        assert_eq!(
            pinned.route(ProviderRole::Tts).map(|route| (
                route.provider_id.as_str(),
                route.model_id.as_str(),
                route.voice_id.as_deref()
            )),
            Some((
                "cartesia",
                CARTESIA_QUALIFIED_MODEL_ID,
                Some(CARTESIA_QUALIFIED_STOCK_VOICE_ID)
            ))
        );
        let embeddings = pinned
            .route(ProviderRole::Embeddings)
            .expect("embeddings route");
        assert_eq!(
            (
                embeddings.provider_id.as_str(),
                embeddings.model_id.as_str()
            ),
            ("fts-only", "sqlite-fts5")
        );
        assert!(embeddings.credential.is_none());
        assert_eq!(embeddings.disclosure.execution, ExecutionLocationV1::Local);
        assert!(pinned.route(ProviderRole::Vision).is_none());
        assert!(pinned.route(ProviderRole::Lipsync).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn private_review_script_adds_valid_cyberpunk_presets_without_replacing_global_state() {
        let temp = tempfile::tempdir().expect("temporary parent");
        let directory = temp.path().join(REVIEW_APPLICATION_NAMESPACE);
        std::fs::create_dir(&directory).expect("review directory");
        let store = ProviderLoadoutStore::new_for_distribution(
            &directory,
            REVIEW_APPLICATION_NAMESPACE.into(),
        );
        store.save(&starter_document()).expect("starter document");
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/windows/configure-private-review-loadouts.ps1");
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .arg("-File")
            .arg(script)
            .arg("-ConfigDirectory")
            .arg(&directory)
            .output()
            .expect("run private review configurator");
        assert!(
            output.status.success(),
            "configurator failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("redacted configurator receipt");
        assert_eq!(receipt["credential_values_recorded"], false);
        assert_eq!(receipt["production_state_changed"], false);

        let snapshot = store.load();
        assert_eq!(snapshot.document.activation.global, id("api-first-starter"));
        assert_eq!(snapshot.document.loadouts.len(), 4);
        let resolved = snapshot
            .document
            .resolve(
                &LoadoutContextV1::game("cyberpunk-2077"),
                &ValidationContextV1::online(),
            )
            .expect("active Cyberpunk preset");
        assert_eq!(resolved.leaf_loadout_id, id("cyberpunk-private-fast-groq"));
        assert_eq!(
            resolved.roles[&ProviderRole::Llm].primary.provider_id,
            "groq"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Stt].primary.provider_id,
            "assemblyai"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Tts].primary.provider_id,
            "cartesia"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Embeddings]
                .primary
                .provider_id,
            "fts-only"
        );
    }

    #[cfg(windows)]
    #[test]
    fn private_review_script_seeds_a_native_valid_document_without_launching_the_app() {
        let temp = tempfile::tempdir().expect("temporary parent");
        let directory = temp.path().join(REVIEW_APPLICATION_NAMESPACE);
        let store = ProviderLoadoutStore::new_for_distribution(
            &directory,
            REVIEW_APPLICATION_NAMESPACE.into(),
        );
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/windows/configure-private-review-loadouts.ps1");
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .arg("-File")
            .arg(script)
            .arg("-ConfigDirectory")
            .arg(&directory)
            .output()
            .expect("run headless private review configurator");
        assert!(
            output.status.success(),
            "configurator failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("redacted configurator receipt");
        assert_eq!(receipt["seeded_review_config"], true);
        assert_eq!(receipt["credential_values_recorded"], false);
        assert_eq!(receipt["production_state_changed"], false);

        let snapshot = store.load();
        assert_eq!(
            snapshot.persistence_health,
            ProviderLoadoutPersistenceHealth::Healthy
        );
        assert_eq!(snapshot.document.activation.global, id("api-first-starter"));
        assert_eq!(snapshot.document.loadouts.len(), 4);
        let resolved = snapshot
            .document
            .resolve(
                &LoadoutContextV1::game("cyberpunk-2077"),
                &ValidationContextV1::online(),
            )
            .expect("active Cyberpunk preset");
        assert_eq!(resolved.leaf_loadout_id, id("cyberpunk-private-fast-groq"));
        assert_eq!(
            resolved.roles[&ProviderRole::Llm].primary.provider_id,
            "groq"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Stt].primary.provider_id,
            "assemblyai"
        );
        assert_eq!(
            resolved.roles[&ProviderRole::Tts].primary.provider_id,
            "cartesia"
        );

        let inspect_script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/windows/inspect-private-review-provider-setup.ps1");
        let inspect_output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .arg("-File")
            .arg(inspect_script)
            .arg("-ConfigDirectory")
            .arg(&directory)
            .output()
            .expect("inspect private review setup headlessly");
        assert!(
            inspect_output.status.success(),
            "inspector failed: {}",
            String::from_utf8_lossy(&inspect_output.stderr)
        );
        let evidence: serde_json::Value =
            serde_json::from_slice(&inspect_output.stdout).expect("redacted inspector evidence");
        assert_eq!(
            evidence["classification"],
            "recorded_native_config_and_vault_presence"
        );
        assert_eq!(evidence["production_namespace_consulted"], false);
        assert_eq!(evidence["credential_values_exposed"], false);
        assert_eq!(evidence["provider_network_requests_made"], false);
        assert_eq!(
            evidence["game"]["active_loadout_id"],
            "cyberpunk-private-fast-groq"
        );
    }

    #[test]
    fn corrupt_primary_recovers_last_good_then_can_be_replaced_atomically() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let document = starter_document();
        store.save(&document).expect("save seed");
        fs::write(store.primary_path(), b"{not-json").expect("corrupt primary fixture");

        let recovered = store.load();
        assert_eq!(
            recovered.persistence_health,
            ProviderLoadoutPersistenceHealth::RecoveredLastGood
        );
        assert_eq!(recovered.document, document);
        store
            .save(&recovered.document)
            .expect("atomically replace corrupt primary");
        assert_eq!(
            store.load().persistence_health,
            ProviderLoadoutPersistenceHealth::Healthy
        );
    }

    #[test]
    fn legacy_starter_migrates_model_voice_and_embeddings_without_touching_custom_routes() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        fs::create_dir_all(temp.path()).expect("create fixture directory");
        let mut legacy = starter_document();
        let starter = legacy
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(tts) = starter.roles.get_mut(&ProviderRole::Tts).expect("tts")
        else {
            panic!("expected TTS route")
        };
        tts.primary.provider_id = "elevenlabs".into();
        tts.primary.model_id = "eleven-flash-v2.5".into();
        tts.primary.voice_id = None;
        tts.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "elevenlabs".into(),
            reference_id: "personal".into(),
        });
        tts.primary.explicit_user_selection = false;
        let legacy_json = serde_json::to_string_pretty(&legacy)
            .expect("serialize legacy fixture")
            .replacen("\"embeddings\"", "\"retrieval\"", 1);
        fs::write(store.primary_path(), legacy_json).expect("write legacy fixture");

        let loaded = store.load();
        assert_eq!(
            loaded.persistence_health,
            ProviderLoadoutPersistenceHealth::Healthy
        );
        let starter = &loaded.document.loadouts[&id("api-first-starter")];
        assert!(starter.roles.contains_key(&ProviderRole::Embeddings));
        let RoleOverrideV1::Route(tts) = &starter.roles[&ProviderRole::Tts] else {
            panic!("expected migrated TTS route")
        };
        assert_eq!(tts.primary.model_id, "eleven_flash_v2_5");
        assert_eq!(
            tts.primary.voice_id.as_deref(),
            Some("EXAVITQu4vr4xnSDxMaL")
        );

        let mut custom = tts.primary.clone();
        custom.model_id = "eleven-flash-v2.5".into();
        custom.voice_id = None;
        custom.explicit_user_selection = true;
        let migrated_custom = migrate_document(
            ProviderLoadoutDocumentV1::new(ProviderLoadoutV1 {
                id: id("custom-global"),
                name: "Custom global".into(),
                scope: LoadoutScopeV1::Global,
                parent: None,
                roles: BTreeMap::from([
                    (ProviderRole::Llm, starter.roles[&ProviderRole::Llm].clone()),
                    (ProviderRole::Stt, starter.roles[&ProviderRole::Stt].clone()),
                    (
                        ProviderRole::Tts,
                        RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                            primary: custom,
                            fallbacks: Vec::new(),
                        })),
                    ),
                    (
                        ProviderRole::Embeddings,
                        starter.roles[&ProviderRole::Embeddings].clone(),
                    ),
                ]),
            })
            .expect("custom document"),
        );
        let RoleOverrideV1::Route(custom_tts) =
            &migrated_custom.loadouts[&id("custom-global")].roles[&ProviderRole::Tts]
        else {
            panic!("expected custom TTS")
        };
        assert_eq!(custom_tts.primary.model_id, "eleven-flash-v2.5");
        assert!(custom_tts.primary.voice_id.is_none());
    }

    #[test]
    fn scope_inheritance_is_preserved_through_native_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        let game = ProviderLoadoutV1 {
            id: id("cyberpunk-streaming"),
            name: "Cyberpunk streaming".into(),
            scope: LoadoutScopeV1::Game {
                game_id: "cyberpunk-2077".into(),
            },
            parent: Some(id("api-first-starter")),
            roles: BTreeMap::from([(ProviderRole::Tts, RoleOverrideV1::Inherit)]),
        };
        document.insert(game).expect("insert game loadout");
        let character = ProviderLoadoutV1 {
            id: id("judy-dialogue"),
            name: "Judy dialogue".into(),
            scope: LoadoutScopeV1::Character {
                game_id: "cyberpunk-2077".into(),
                character_id: "judy-alvarez".into(),
            },
            parent: Some(id("cyberpunk-streaming")),
            roles: BTreeMap::new(),
        };
        document
            .insert(character)
            .expect("insert character loadout");
        document
            .activate(&id("cyberpunk-streaming"))
            .expect("activate game");
        document
            .activate(&id("judy-dialogue"))
            .expect("activate character");
        store.save(&document).expect("save hierarchy");

        let loaded = store.load().document;
        let resolved = loaded
            .resolve(
                &LoadoutContextV1::character("cyberpunk-2077", "judy-alvarez"),
                &ValidationContextV1::online(),
            )
            .expect("resolve persisted hierarchy");
        assert_eq!(
            resolved.inheritance_chain,
            vec![
                id("api-first-starter"),
                id("cyberpunk-streaming"),
                id("judy-dialogue")
            ]
        );
    }

    #[test]
    fn fallback_must_remain_manual_and_explicit_before_native_save() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        {
            let loadout = document
                .loadouts
                .get_mut(&id("api-first-starter"))
                .expect("starter loadout");
            let RoleOverrideV1::Route(route) = loadout
                .roles
                .get_mut(&ProviderRole::Llm)
                .expect("llm route")
            else {
                panic!("expected route")
            };
            route.fallbacks.push(ExplicitFallbackV1 {
                route: ProviderModelRouteV1 {
                    provider_id: "anthropic".into(),
                    model_id: "claude-sonnet-4".into(),
                    voice_id: None,
                    credential: Some(CredentialReferenceV1 {
                        provider_id: "anthropic".into(),
                        reference_id: "personal".into(),
                    }),
                    disclosure: route.primary.disclosure.clone(),
                    explicit_user_selection: true,
                },
                activation: FallbackActivationV1::ManualOnly,
                user_authorized: false,
            });
        }
        assert!(store.save(&document).is_err());
        let loadout = document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.fallbacks[0].user_authorized = true;
        store.save(&document).expect("save explicit fallback");
        let resolved = store
            .load()
            .document
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("resolve fallback loadout");
        assert!(resolved
            .pin_turn_routes(1)
            .automatic_fallback(ProviderRole::Llm)
            .is_err());
        assert_eq!(
            resolved
                .manual_fallback_candidate(ProviderRole::Llm, 0)
                .expect("manual candidate")
                .provider_id,
            "anthropic"
        );
    }

    #[test]
    fn api_first_seed_is_denied_in_offline_review() {
        let manager =
            ProviderLoadoutManager::new(tempfile::tempdir().expect("temporary directory").keep());
        let (_resources_directory, resources) = local_resources();
        let error = manager
            .review(LoadoutContextV1::global(), true, &resources)
            .expect_err("cloud routes cannot resolve offline");
        assert!(matches!(error, CommandError::InvalidRequest { .. }));
        assert!(!error.to_string().contains("nvidia"));
    }

    #[test]
    fn secret_like_values_and_fields_never_enter_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        let loadout = document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "groq".into(),
            reference_id: "personal".into(),
        });
        route.primary.disclosure.cost_summary = "sk-canary-never-persist-this".into();
        assert!(matches!(
            store.save(&document),
            Err(ProviderLoadoutStoreError::SecretMaterial)
        ));
        assert!(!store.primary_path().exists());

        let valid_json = serde_json::to_string(&starter_document()).expect("serialize fixture");
        let malicious = valid_json.replacen(
            "\"reference_id\":\"personal\"",
            "\"reference_id\":\"personal\",\"api_key\":\"nvapi-canary\"",
            1,
        );
        fs::write(store.primary_path(), malicious).expect("write malicious fixture");
        assert_eq!(
            store.load().persistence_health,
            ProviderLoadoutPersistenceHealth::RecoveredSeed
        );

        let mut opaque_secret = starter_document();
        let loadout = opaque_secret
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter loadout");
        let RoleOverrideV1::Route(route) = loadout
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm route")
        else {
            panic!("expected route")
        };
        route.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "groq".into(),
            reference_id: "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b".into(),
        });
        assert!(matches!(
            store.save(&opaque_secret),
            Err(ProviderLoadoutStoreError::SecretMaterial)
        ));
        assert!(!looks_like_secret(
            "Risk-based provider routing remains user controlled."
        ));
    }

    #[test]
    fn update_is_transactional_and_preserves_id_and_scope() {
        let mut document = starter_document();
        let before = document.clone();
        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.id = id("different-id");
        assert!(
            update_document_entry(&mut document, &id("api-first-starter"), replacement).is_err()
        );
        assert_eq!(document, before);

        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.scope = LoadoutScopeV1::Game {
            game_id: "cyberpunk-2077".into(),
        };
        assert!(
            update_document_entry(&mut document, &id("api-first-starter"), replacement).is_err()
        );
        assert_eq!(document, before);

        let mut replacement = document.loadouts[&id("api-first-starter")].clone();
        replacement.name = "My API mix".into();
        update_document_entry(&mut document, &id("api-first-starter"), replacement)
            .expect("valid update");
        assert_eq!(
            document.loadouts[&id("api-first-starter")].name,
            "My API mix"
        );
    }

    #[test]
    fn scoped_activation_can_be_removed_before_delete() {
        let mut document = starter_document();
        let game = ProviderLoadoutV1 {
            id: id("skyrim-api"),
            name: "Skyrim API".into(),
            scope: LoadoutScopeV1::Game {
                game_id: "skyrim-special-edition".into(),
            },
            parent: Some(id("api-first-starter")),
            roles: BTreeMap::new(),
        };
        document.insert(game).expect("insert game loadout");
        document.activate(&id("skyrim-api")).expect("activate game");
        assert!(document.delete(&id("skyrim-api")).is_err());
        document
            .deactivate_scope(&LoadoutScopeV1::Game {
                game_id: "skyrim-special-edition".into(),
            })
            .expect("deactivate game scope");
        document
            .delete(&id("skyrim-api"))
            .expect("delete inactive loadout");
        assert!(document.deactivate_scope(&LoadoutScopeV1::Global).is_err());
    }

    #[test]
    fn manager_reports_stable_nonsecret_resolution_failures() {
        let manager =
            ProviderLoadoutManager::new(tempfile::tempdir().expect("temporary directory").keep());
        let (_resources_directory, resources) = local_resources();
        let error = manager
            .resolve_for_turn(LoadoutContextV1::global(), true, &resources)
            .expect_err("hosted starter must not resolve offline");
        let CommandError::InvalidRequest { message } = error else {
            panic!("expected stable invalid-request failure")
        };
        assert_eq!(
            message,
            "provider_loadout_policy_blocked: a selected hosted route is unavailable in fully local mode"
        );
        for forbidden in ["nvidia", "assemblyai", "elevenlabs", "personal"] {
            assert!(!message.contains(forbidden));
        }
    }

    #[test]
    fn crafted_or_stale_local_lipsync_never_becomes_runtime_ready() {
        let directory = tempfile::tempdir().expect("loadout directory");
        let store = ProviderLoadoutStore::new(directory.path());
        let mut document = starter_document();
        let roles = &mut document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter")
            .roles;
        // This fixture exercises only the local lip-sync admission boundary.
        // Remove unrelated NVIDIA trial primaries so the provider-wide
        // private-evaluation gate cannot mask the assertion under test.
        let RoleOverrideV1::Route(llm) = roles.get_mut(&ProviderRole::Llm).expect("llm") else {
            panic!("starter LLM route")
        };
        llm.primary.provider_id = "cohere".into();
        llm.primary.model_id = "command-a-plus-05-2026".into();
        llm.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "cohere".into(),
            reference_id: "personal".into(),
        });
        roles.insert(
            ProviderRole::Embeddings,
            RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                primary: ProviderModelRouteV1 {
                    provider_id: "fts-only".into(),
                    model_id: "sqlite-fts5".into(),
                    voice_id: None,
                    credential: None,
                    disclosure: CatalogDisclosureV1 {
                        catalog_revision: bundled_catalog().catalog_revision,
                        execution: ExecutionLocationV1::Local,
                        egress: EgressClassV1::None,
                        privacy_summary: "Exact scoped text search stays on this PC.".into(),
                        cost_summary: "Uses the bundled local SQLite FTS index.".into(),
                        transmitted_data: BTreeSet::new(),
                    },
                    explicit_user_selection: true,
                },
                fallbacks: Vec::new(),
            })),
        );
        roles.insert(ProviderRole::Lipsync, local_lipsync_route("musetalk"));
        store.save(&document).expect("persist crafted stale route");

        let manager = ProviderLoadoutManager::new(directory.path().to_path_buf());
        let (_resources_directory, resources) = local_resources();
        let resolved = manager
            .resolve_for_turn(LoadoutContextV1::global(), false, &resources)
            .expect("audio/text routes remain usable");
        let pinned = resolved.pin_turn_routes(1);
        assert_eq!(
            pinned.roles[&ProviderRole::Lipsync].state,
            npc_provider_loadouts::TurnRouteStateV1::Disabled
        );
        assert!(pinned.route(ProviderRole::Lipsync).is_none());

        let review_error = manager
            .review(LoadoutContextV1::global(), false, &resources)
            .expect_err("review must reject false-ready local lip-sync");
        assert!(review_error
            .to_string()
            .contains("local_lipsync_unavailable"));
        let activation_error = manager
            .activate(&id("api-first-starter"), &resources)
            .expect_err("activation must reject false-ready local lip-sync");
        assert!(activation_error
            .to_string()
            .contains("local_lipsync_unavailable"));
    }

    #[test]
    fn bundled_catalog_revision_and_capabilities_gate_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let catalog_revision = bundled_catalog().catalog_revision;
        let document = starter_document();
        let loadout = &document.loadouts[&id("api-first-starter")];
        for route in loadout.roles.values() {
            if let RoleOverrideV1::Route(route) = route {
                assert_eq!(route.primary.disclosure.catalog_revision, catalog_revision);
            }
        }

        let mut unknown_model = document.clone();
        let RoleOverrideV1::Route(llm) = unknown_model
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter")
            .roles
            .get_mut(&ProviderRole::Llm)
            .expect("llm")
        else {
            panic!("expected route")
        };
        llm.primary.model_id = "not-a-catalog-model".into();
        llm.primary.explicit_user_selection = false;
        assert_eq!(
            store.save(&unknown_model).expect_err("unknown model"),
            ProviderLoadoutStoreError::Catalog
        );

        let mut stale = document;
        let RoleOverrideV1::Route(tts) = stale
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter")
            .roles
            .get_mut(&ProviderRole::Tts)
            .expect("tts")
        else {
            panic!("expected route")
        };
        tts.primary.disclosure.catalog_revision = catalog_revision - 1;
        assert_eq!(
            store.save(&stale).expect_err("stale revision"),
            ProviderLoadoutStoreError::Catalog
        );
    }

    #[test]
    fn nvidia_adapter_credential_alias_is_exact_and_catalog_backed() {
        let catalog = bundled_catalog();
        let revision = catalog.catalog_revision;
        let mut route = ProviderModelRouteV1 {
            provider_id: "nvidia-nim-magpie".into(),
            model_id: "magpie-tts-multilingual".into(),
            voice_id: Some("Magpie-Multilingual.EN-US.Aria".into()),
            credential: Some(CredentialReferenceV1 {
                provider_id: "nvidia-nim".into(),
                reference_id: "personal".into(),
            }),
            disclosure: CatalogDisclosureV1 {
                catalog_revision: revision,
                execution: ExecutionLocationV1::Hosted,
                egress: EgressClassV1::ProviderCloud,
                privacy_summary: "Response text is sent to NVIDIA NIM.".into(),
                cost_summary: "Uses the configured NVIDIA account.".into(),
                transmitted_data: BTreeSet::from([TransmittedDataV1::ResponseText]),
            },
            explicit_user_selection: true,
        };
        validate_route_against_catalog_for_distribution(ProviderRole::Tts, &route, &catalog, true)
            .expect("declared adapter alias");
        route.credential.as_mut().expect("credential").provider_id = "openai".into();
        assert_eq!(
            validate_route_against_catalog_for_distribution(
                ProviderRole::Tts,
                &route,
                &catalog,
                true,
            ),
            Err(CatalogRouteError::Credential)
        );
        route.credential.as_mut().expect("credential").provider_id = "nvidia-nim".into();
        route.voice_id = Some("custom-clone".into());
        assert_eq!(
            validate_route_against_catalog_for_distribution(
                ProviderRole::Tts,
                &route,
                &catalog,
                true,
            ),
            Err(CatalogRouteError::Capability)
        );
    }

    #[test]
    fn hosted_tts_routes_accept_only_the_exact_qualified_model_and_stock_voice() {
        let exact = [
            (
                "cartesia",
                CARTESIA_QUALIFIED_MODEL_ID,
                CARTESIA_QUALIFIED_STOCK_VOICE_ID,
            ),
            (
                "deepgram",
                DEEPGRAM_QUALIFIED_MODEL_ID,
                DEEPGRAM_QUALIFIED_STOCK_VOICE_ID,
            ),
            (
                "inworld",
                INWORLD_QUALIFIED_MODEL_ID,
                INWORLD_QUALIFIED_STOCK_VOICE_ID,
            ),
        ];
        for (provider_id, model_id, voice_id) in exact {
            let mut route = ProviderModelRouteV1 {
                provider_id: provider_id.into(),
                model_id: model_id.into(),
                voice_id: Some(voice_id.into()),
                credential: None,
                disclosure: CatalogDisclosureV1 {
                    catalog_revision: 9,
                    execution: ExecutionLocationV1::Hosted,
                    egress: EgressClassV1::ProviderCloud,
                    privacy_summary: String::new(),
                    cost_summary: String::new(),
                    transmitted_data: BTreeSet::new(),
                },
                explicit_user_selection: true,
            };
            assert!(qualified_hosted_tts_route(&route), "{provider_id}");
            route.voice_id = Some("crafted-stock-voice".into());
            assert!(!qualified_hosted_tts_route(&route), "{provider_id}");
            route.voice_id = Some(voice_id.into());
            route.model_id = "crafted-model".into();
            assert!(!qualified_hosted_tts_route(&route), "{provider_id}");
        }
    }

    #[test]
    fn magpie_without_stock_voice_cannot_mutate_native_persistence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let manager = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        let before = manager.snapshot().expect("initial snapshot");
        let result = manager.mutate(|document| {
            let loadout = document
                .loadouts
                .get_mut(&id("api-first-starter"))
                .ok_or(())?;
            let RoleOverrideV1::Route(tts) = loadout.roles.get_mut(&ProviderRole::Tts).ok_or(())?
            else {
                return Err(());
            };
            tts.primary.provider_id = "nvidia-nim-magpie".into();
            tts.primary.model_id = "magpie-tts-multilingual".into();
            tts.primary.voice_id = None;
            tts.primary.credential = Some(CredentialReferenceV1 {
                provider_id: "nvidia-nim".into(),
                reference_id: "personal".into(),
            });
            tts.primary.explicit_user_selection = true;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(manager.snapshot().expect("unchanged snapshot"), before);
        assert!(!temp.path().join(LOADOUT_FILE_NAME).exists());
    }

    #[test]
    fn magpie_route_authority_is_private_evaluation_namespace_only() {
        assert!(hosted_tts_route_supported("elevenlabs", false));
        assert!(hosted_tts_route_supported("cartesia", false));
        assert!(hosted_tts_route_supported("deepgram", false));
        assert!(hosted_tts_route_supported("inworld", false));
        assert!(!hosted_tts_route_supported("nvidia-nim-magpie", false));
        assert!(hosted_tts_route_supported("nvidia-nim-magpie", true));
    }

    #[test]
    fn dormant_manual_magpie_fallback_does_not_require_or_mint_turn_authority() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let manager = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        let mut resolved = manager
            .resolve_raw(&LoadoutContextV1::global(), false)
            .expect("starter resolution");
        // The bundled starter intentionally selects NVIDIA trial primaries.
        // They are irrelevant to this dormant-fallback test and would
        // correctly require provider-wide acknowledgement on their own.
        resolved.roles.remove(&ProviderRole::Llm);
        resolved.roles.remove(&ProviderRole::Embeddings);
        let tts = resolved
            .roles
            .get_mut(&ProviderRole::Tts)
            .expect("starter TTS route");
        let mut fallback_route = tts.primary.clone();
        fallback_route.provider_id = MAGPIE_ROUTE_PROVIDER_ID.into();
        fallback_route.model_id = "magpie-tts-multilingual".into();
        fallback_route.voice_id = Some("Magpie-Multilingual.EN-US.Aria".into());
        fallback_route.credential = Some(CredentialReferenceV1 {
            provider_id: "nvidia-nim".into(),
            reference_id: "personal".into(),
        });
        tts.fallbacks = vec![npc_provider_loadouts::ExplicitFallbackV1 {
            activation: npc_provider_loadouts::FallbackActivationV1::ManualOnly,
            user_authorized: true,
            route: fallback_route,
        }];
        assert!(manager
            .private_evaluation_authority_for_routes(&resolved)
            .expect("dormant fallback evaluation")
            .is_none());
    }

    #[test]
    fn provider_wide_nvidia_trial_authority_covers_selected_llm_and_embeddings() {
        fn select_nvidia_trial_routes(routes: &mut ResolvedProviderLoadoutV1) {
            for role in [ProviderRole::Llm, ProviderRole::Embeddings] {
                let selected = routes.roles.get_mut(&role).expect("starter route");
                selected.primary.provider_id = PRIVATE_EVALUATION_PROVIDER_ID.into();
                selected.primary.model_id = match role {
                    ProviderRole::Llm => "nvidia/nemotron-3.5-lightning-30b-a3b",
                    ProviderRole::Embeddings => "nvidia/nemotron-3-embed-1b",
                    _ => unreachable!("test selects only NVIDIA trial roles"),
                }
                .into();
                selected.primary.credential = Some(CredentialReferenceV1 {
                    provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
                    reference_id: "personal".into(),
                });
                selected.primary.disclosure.execution = ExecutionLocationV1::Hosted;
                selected.primary.disclosure.egress = EgressClassV1::ProviderCloud;
            }
        }

        let temp = tempfile::tempdir().expect("temporary directory");
        let public = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            PRODUCTION_APPLICATION_NAMESPACE.into(),
            Some(Arc::new(PresentCredential)),
        );
        let mut public_routes = public
            .resolve_raw(&LoadoutContextV1::global(), false)
            .expect("starter resolution");
        select_nvidia_trial_routes(&mut public_routes);
        assert!(public
            .private_evaluation_authority_for_routes(&public_routes)
            .expect_err("production namespace must block NVIDIA trial routes")
            .to_string()
            .contains("public production namespace"));

        let review = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            Some(Arc::new(PresentCredential)),
        );
        let mut review_routes = review
            .resolve_raw(&LoadoutContextV1::global(), false)
            .expect("review starter resolution");
        select_nvidia_trial_routes(&mut review_routes);
        assert!(review
            .private_evaluation_authority_for_routes(&review_routes)
            .expect_err("current acknowledgement is required")
            .to_string()
            .contains("acknowledgement_required"));
        review
            .acknowledge_private_evaluation(AcknowledgeProviderPrivateEvaluationRequestV1 {
                provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
                terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
                explicit_user_confirmation: true,
            })
            .expect("provider-wide acknowledgement");
        let authority = review
            .private_evaluation_authority_for_routes(&review_routes)
            .expect("provider-wide route authority")
            .expect("NVIDIA trial authority");
        assert_eq!(
            authority.authorized_roles,
            vec![ProviderRole::Llm, ProviderRole::Embeddings]
        );
        assert!(authority.credential_present);
        assert!(authority.exact_stock_voice_discovered);
        assert_eq!(
            authority.acknowledgement.provider_id,
            PRIVATE_EVALUATION_PROVIDER_ID
        );
    }

    #[test]
    fn private_evaluation_acknowledgement_is_native_stamped_persisted_and_namespace_bound() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let public = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
        );
        let request = AcknowledgeProviderPrivateEvaluationRequestV1 {
            provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
            terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
            explicit_user_confirmation: true,
        };
        assert!(public
            .acknowledge_private_evaluation(request.clone())
            .is_err());
        assert!(!temp.path().join(PRIVATE_EVALUATION_FILE_NAME).exists());
        let public_policy = public
            .private_evaluation_policy()
            .expect("public pre-consent policy");
        assert_eq!(
            public_policy.terms_revision,
            PRIVATE_EVALUATION_TERMS_REVISION
        );
        assert_eq!(public_policy.terms_url, PRIVATE_EVALUATION_TERMS_URL);
        assert_eq!(
            public_policy.application_namespace,
            PRODUCTION_APPLICATION_NAMESPACE
        );
        assert!(!public_policy.namespace_eligible);
        assert!(public_policy.acknowledgement.is_none());
        assert!(!public_policy.production_use_supported);
        assert!(!public_policy.promotion_supported);
        assert!(!public_policy.publication_supported);

        let review = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        let acknowledgement = review
            .acknowledge_private_evaluation(request)
            .expect("review acknowledgement");
        assert!(valid_private_evaluation_acknowledgement(&acknowledgement));
        assert!(!acknowledgement.promotion_supported);
        assert!(!acknowledgement.publication_supported);
        assert_eq!(
            private_evaluation_acknowledgement_sha256(&acknowledgement)
                .expect("acknowledgement digest")
                .len(),
            64
        );

        let reopened = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        assert_eq!(
            reopened
                .private_evaluation_acknowledgement()
                .expect("reopened acknowledgement"),
            Some(acknowledgement.clone())
        );
        let production_reopen = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            PRODUCTION_APPLICATION_NAMESPACE.into(),
            None,
        );
        assert_eq!(
            production_reopen
                .private_evaluation_acknowledgement()
                .expect("production acknowledgement state"),
            None,
            "production namespace must not read a review acknowledgement even if pointed at the same test directory"
        );
        let debug_reopen = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            DEBUG_APPLICATION_NAMESPACE.into(),
            None,
        );
        assert_eq!(
            debug_reopen
                .private_evaluation_acknowledgement()
                .expect("debug acknowledgement state"),
            None,
            "debug namespace must not replay a review acknowledgement"
        );
        let review_policy = reopened
            .private_evaluation_policy()
            .expect("review pre-consent policy");
        assert!(review_policy.namespace_eligible);
        assert_eq!(review_policy.acknowledgement, Some(acknowledgement));
    }

    #[test]
    fn stale_private_evaluation_terms_or_catalog_revision_are_never_loaded() {
        for (provider_id, terms_revision, catalog_revision) in [
            (
                MAGPIE_ROUTE_PROVIDER_ID,
                "nvidia-api-trial-terms-2025-09-19-magpie-private-evaluation-v1",
                bundled_catalog().catalog_revision,
            ),
            (
                PRIVATE_EVALUATION_PROVIDER_ID,
                PRIVATE_EVALUATION_TERMS_REVISION,
                bundled_catalog().catalog_revision.saturating_sub(1),
            ),
        ] {
            let temp = tempfile::tempdir().expect("temporary directory");
            let stale = ProviderPrivateEvaluationAcknowledgementV1 {
                schema_version: 1,
                provider_id: provider_id.into(),
                mode: ProviderEntitlementModeV1::PrivateEvaluationOnly,
                terms_revision: terms_revision.into(),
                catalog_revision,
                application_namespace: REVIEW_APPLICATION_NAMESPACE.into(),
                acknowledged_at_epoch_ms: 1,
                promotion_supported: false,
                publication_supported: false,
            };
            fs::write(
                temp.path().join(PRIVATE_EVALUATION_FILE_NAME),
                serde_json::to_vec_pretty(&ProviderPrivateEvaluationStoreV1 {
                    schema_version: 1,
                    acknowledgement: stale,
                })
                .expect("serialize stale acknowledgement"),
            )
            .expect("write stale acknowledgement fixture");
            let reopened = ProviderLoadoutManager::new_internal(
                temp.path().to_path_buf(),
                REVIEW_APPLICATION_NAMESPACE.into(),
                None,
            );
            assert_eq!(
                reopened
                    .private_evaluation_acknowledgement()
                    .expect("stale acknowledgement state"),
                None
            );
            assert!(reopened
                .private_evaluation_policy()
                .expect("current policy remains readable")
                .acknowledgement
                .is_none());
        }
    }

    #[test]
    fn private_evaluation_authority_rejects_an_in_memory_cross_namespace_replay() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let manager = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        let replay = ProviderPrivateEvaluationAcknowledgementV1 {
            schema_version: 1,
            provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
            mode: ProviderEntitlementModeV1::PrivateEvaluationOnly,
            terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
            catalog_revision: bundled_catalog().catalog_revision,
            application_namespace: DEBUG_APPLICATION_NAMESPACE.into(),
            acknowledged_at_epoch_ms: 1,
            promotion_supported: false,
            publication_supported: false,
        };
        *manager
            .private_evaluation_acknowledgement
            .lock()
            .expect("acknowledgement state") = Some(replay);
        let mut resolved = manager
            .resolve_raw(&LoadoutContextV1::global(), false)
            .expect("starter resolution");
        let tts = resolved
            .roles
            .get_mut(&ProviderRole::Tts)
            .expect("starter TTS route");
        tts.primary.provider_id = PRIVATE_EVALUATION_PROVIDER_ID.into();
        tts.primary.model_id = "magpie-tts-multilingual".into();
        tts.primary.voice_id = Some("Magpie-Multilingual.EN-US.Aria".into());
        tts.primary.credential = Some(CredentialReferenceV1 {
            provider_id: "nvidia-nim".into(),
            reference_id: "personal".into(),
        });
        tts.primary.explicit_user_selection = true;
        let error = manager
            .private_evaluation_authority_for_routes(&resolved)
            .expect_err("cross-namespace replay must fail");
        assert!(format!("{error:?}").contains("acknowledgement_required"));
    }

    #[test]
    fn magpie_turn_authority_requires_ack_credential_and_exact_discovered_voice() {
        use crate::sidecar_protocol::{NativeDiscoveredStockVoice, NativeTtsVoiceRefreshEvidence};

        let temp = tempfile::tempdir().expect("temporary directory");
        let manager = ProviderLoadoutManager::new_for_distribution(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            Arc::new(PresentCredential),
        );
        manager
            .record_stock_voice_discovery(&NativeTtsVoiceDiscoveryResult {
                schema_version: 1,
                provider_id: MAGPIE_ROUTE_PROVIDER_ID.into(),
                model_id: "magpie-tts-multilingual".into(),
                status: NativeTtsVoiceDiscoveryStatus::Available,
                voices: vec![NativeDiscoveredStockVoice {
                    voice_id: "Magpie-Multilingual.EN-US.Aria".into(),
                    display_name: "Aria".into(),
                    language: "en-US".into(),
                    styles: vec!["neutral".into()],
                    provenance: NativeTtsVoiceProvenance::ProviderStockDiscovery,
                }],
                provenance: NativeTtsVoiceProvenance::ProviderStockDiscovery,
                refresh: NativeTtsVoiceRefreshEvidence {
                    requested: true,
                    performed: true,
                    cache_hit: false,
                    refreshed_at_epoch_ms: Some(current_epoch_ms()),
                    expires_at_epoch_ms: Some(current_epoch_ms() + 300_000),
                },
                error: None,
            })
            .expect("authenticated stock voices");
        manager
            .mutate(|document| {
                let loadout = document
                    .loadouts
                    .get_mut(&id("api-first-starter"))
                    .ok_or(())?;
                let RoleOverrideV1::Route(tts) =
                    loadout.roles.get_mut(&ProviderRole::Tts).ok_or(())?
                else {
                    return Err(());
                };
                tts.primary.provider_id = MAGPIE_ROUTE_PROVIDER_ID.into();
                tts.primary.model_id = "magpie-tts-multilingual".into();
                tts.primary.voice_id = Some("Magpie-Multilingual.EN-US.Aria".into());
                tts.primary.credential = Some(CredentialReferenceV1 {
                    provider_id: "nvidia-nim".into(),
                    reference_id: "personal".into(),
                });
                tts.primary.explicit_user_selection = true;
                Ok(())
            })
            .expect("save inactive private-evaluation draft");
        let resolved = manager
            .resolve_raw(&LoadoutContextV1::global(), false)
            .expect("resolve draft");
        assert!(manager
            .private_evaluation_authority_for_routes(&resolved)
            .is_err());

        manager
            .acknowledge_private_evaluation(AcknowledgeProviderPrivateEvaluationRequestV1 {
                provider_id: PRIVATE_EVALUATION_PROVIDER_ID.into(),
                terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
                explicit_user_confirmation: true,
            })
            .expect("acknowledge exact terms");
        let authority = manager
            .private_evaluation_authority_for_routes(&resolved)
            .expect("native authority")
            .expect("Magpie authority");
        assert!(authority.credential_present);
        assert!(authority.exact_stock_voice_discovered);
        assert_eq!(
            authority.application_namespace,
            REVIEW_APPLICATION_NAMESPACE
        );
        assert_eq!(authority.acknowledgement_sha256.len(), 64);
    }

    #[test]
    fn magpie_requires_exact_current_authenticated_discovery_membership() {
        use crate::sidecar_protocol::{NativeDiscoveredStockVoice, NativeTtsVoiceRefreshEvidence};

        let temp = tempfile::tempdir().expect("temporary directory");
        let manager = ProviderLoadoutManager::new_internal(
            temp.path().to_path_buf(),
            REVIEW_APPLICATION_NAMESPACE.into(),
            None,
        );
        manager
            .record_stock_voice_discovery(&NativeTtsVoiceDiscoveryResult {
                schema_version: 1,
                provider_id: "nvidia-nim-magpie".into(),
                model_id: "magpie-tts-multilingual".into(),
                status: NativeTtsVoiceDiscoveryStatus::Available,
                voices: vec![NativeDiscoveredStockVoice {
                    voice_id: "Magpie-Multilingual.EN-US.Aria".into(),
                    display_name: "Aria".into(),
                    language: "en-US".into(),
                    styles: vec!["neutral".into()],
                    provenance: NativeTtsVoiceProvenance::ProviderStockDiscovery,
                }],
                provenance: NativeTtsVoiceProvenance::ProviderStockDiscovery,
                refresh: NativeTtsVoiceRefreshEvidence {
                    requested: true,
                    performed: true,
                    cache_hit: false,
                    refreshed_at_epoch_ms: Some(current_epoch_ms()),
                    expires_at_epoch_ms: Some(current_epoch_ms() + 300_000),
                },
                error: None,
            })
            .expect("record authenticated membership");

        let mutation = |voice_id: &str| {
            manager.mutate(|document| {
                let loadout = document
                    .loadouts
                    .get_mut(&id("api-first-starter"))
                    .ok_or(())?;
                let RoleOverrideV1::Route(tts) =
                    loadout.roles.get_mut(&ProviderRole::Tts).ok_or(())?
                else {
                    return Err(());
                };
                tts.primary.provider_id = "nvidia-nim-magpie".into();
                tts.primary.model_id = "magpie-tts-multilingual".into();
                tts.primary.voice_id = Some(voice_id.into());
                tts.primary.credential = Some(CredentialReferenceV1 {
                    provider_id: "nvidia-nim".into(),
                    reference_id: "personal".into(),
                });
                tts.primary.explicit_user_selection = true;
                Ok(())
            })
        };
        let error = mutation("Magpie-Multilingual.EN-US.NotReal")
            .expect_err("prefix-only voice must be rejected");
        assert!(error.to_string().contains("stock_voice_not_discovered"));
        mutation("Magpie-Multilingual.EN-US.Aria").expect("exact discovered voice");
    }

    #[test]
    fn enabled_local_lipsync_accepts_only_local_private_input_metadata() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let store = ProviderLoadoutStore::new(temp.path());
        let mut document = starter_document();
        document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter")
            .roles
            .insert(
                ProviderRole::Lipsync,
                RoleOverrideV1::Route(Box::new(RoleRouteV1 {
                    primary: ProviderModelRouteV1 {
                        provider_id: "local-visual-worker".into(),
                        model_id: "musetalk".into(),
                        voice_id: None,
                        credential: None,
                        disclosure: CatalogDisclosureV1 {
                            catalog_revision: bundled_catalog().catalog_revision,
                            execution: ExecutionLocationV1::Local,
                            egress: EgressClassV1::None,
                            privacy_summary:
                                "Mouth-region frames and delivered audio stay on this PC.".into(),
                            cost_summary: "Uses local GPU and VRAM resources.".into(),
                            transmitted_data: BTreeSet::new(),
                        },
                        explicit_user_selection: true,
                    },
                    fallbacks: Vec::new(),
                })),
            );
        store.save(&document).expect("valid local lip-sync route");

        let RoleOverrideV1::Route(lipsync) = document
            .loadouts
            .get_mut(&id("api-first-starter"))
            .expect("starter")
            .roles
            .get_mut(&ProviderRole::Lipsync)
            .expect("lipsync")
        else {
            panic!("expected route")
        };
        lipsync
            .primary
            .disclosure
            .transmitted_data
            .insert(TransmittedDataV1::Image);
        assert!(store.save(&document).is_err());
    }
}
