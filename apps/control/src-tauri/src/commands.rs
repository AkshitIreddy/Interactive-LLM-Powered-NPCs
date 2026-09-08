use crate::catalog::{
    credential_provider_name, credential_reference_for, is_known_provider, model_summaries,
    provider_summaries, CredentialMutationError, CredentialPresence, ResourceCatalog,
    SystemCredentialPresence,
};
use crate::character_content_overrides::{
    CharacterContentOverrideManagerV1, CharacterContentOverrideMutationV1,
    CharacterContentOverrideScopeV1, CharacterContentOverrideSnapshotV1,
    SaveCharacterContentOverrideRequestV1,
};
use crate::character_mouth_packs::{
    CharacterMouthPackFilesV1, CharacterMouthPackManagerV1, CharacterMouthPackPreviewV1,
    CharacterMouthPackStateV1, CurrentCharacterMouthEnableAuthorityV1,
    DisableCharacterMouthPackRequestV1, DisabledCharacterMouthPackV1,
    EnableCharacterMouthPackRequestV1, EnabledCharacterMouthPackV1,
    ImportCharacterMouthPackRequestV1, InstalledCharacterMouthPackV1,
    SelectedCharacterMouthAuthorityV1,
};
use crate::character_workspace::{
    CharacterCatalogSnapshot, CharacterInspection, CharacterInspectionRequest,
    CharacterMemoryBackupDeleteRequest, CharacterMemoryBackupDeleteResult,
    CharacterMemoryBackupEntry, CharacterMemoryBackupRequest, CharacterMemoryBackupResult,
    CharacterMemoryEraseRequest, CharacterMemoryEraseResult, CharacterMemoryRestoreRequest,
    CharacterMemoryRestoreResult, CharacterMemoryStatus, CharacterMemoryStatusRequest,
    CharacterWorkspace, RemoveAllLocalMemoryRequest, RemoveAllLocalMemoryResult,
    SelectedCharacterResult,
};
use crate::content_packs::{
    ActivateContentPackRequestV1, ActiveContentPackV1, ContentPackManagerV1, ContentPackPreviewV1,
    ContentPackStateV1, InspectContentPackRequestV1,
};
use crate::credential_prompt::{
    CredentialPrompt, NativePromptOutcome, NativeWindowOwner, SystemCredentialPrompt,
};
use crate::diagnostics_v2::{
    DiagnosticsExportRequest, DiagnosticsExportResult, DiagnosticsSettingsV1, DiagnosticsV2Manager,
    DiagnosticsV2Snapshot,
};
use crate::domain::{
    BootstrapSnapshot, CatalogState, CheckStatus, CredentialPromptOutcome,
    CredentialPromptSaveResult, CredentialReferenceStatus, DiagnosticCheck, DiagnosticOverall,
    DiagnosticSummary, ExecutionMode, MeasurementStatus, MediaBrokerHealthSnapshot,
    OnboardingPersistence, OnboardingSnapshot, PersistenceHealth, ProviderConnectionOutcome,
    ProviderConnectionTestResult, ProviderCredentialSummary, RuntimeConnectionState,
    RuntimeHealthSnapshot, SafetyBoundary, SaveOnboardingResult, SimulationEvent, SimulationStatus,
    StartSimulationRequest, StartSimulationResult, CONTROL_CONTRACT_VERSION,
};
use crate::game_targets::{
    GameCaptureVerification, GameTargetCandidate, GameTargetManager, GameTargetSelection,
    SelectGameTargetRequest,
};
use crate::identity_runtime::{
    IdentityReferenceEnrollmentCommandRequest, IdentityReferenceEnrollmentReceipt,
    NativeActorLockBusV1, NativeActorLockStateV1, QualifiedIdentityRuntimeServiceV1,
};
use crate::local_resources::{
    ExperimentalPackMutationRequest, ExperimentalPackState, LocalResourceManager,
    LocalResourceSettings, ResourceTelemetryResult, SelectedLoadoutAdmissionResult,
    SelectedLoadoutPlannerResult, TrustedLocalPackCatalogResult,
    TrustedOptionalPackActivationRequestV1, TrustedOptionalPackActivationResultV1,
    TrustedOptionalPackLifecycleResultV1, TrustedOptionalPackMutationRequestV1,
};
use crate::media_broker::{
    AudioInputSelection, AudioInputSnapshot, AudioOutputSelection, AudioOutputSnapshot,
    MediaBrokerLaunchConfig, MediaBrokerSupervisor, NativeManualActorPickerReceiptV1,
    NativeManualActorPickerRequestV1, NativeManualActorPickerStatusV1, SelectedAudioInput,
    SelectedAudioOutput,
};
use crate::persistence::OnboardingStore;
use crate::product_benchmark::ControlBenchmarkProbe;
use crate::product_preferences::{
    ProductPreferenceManager, ProductPreferenceScopeV1, ProductPreferenceSnapshotV1,
    ResetProductPreferencesRequestV1, ResourceAuthorityReferenceV1, RouteAuthorityReferenceV1,
    SaveProductPreferencesRequestV1,
};
use crate::provider_loadouts::ProviderLoadoutManager;
use crate::runtime_router::RuntimeRouter;
use crate::selected_stt::{
    SelectedSttCancelResultV1, SelectedSttCapturingV1, SelectedSttController, SelectedSttError,
    SelectedSttPushToTalkEventV1, SelectedSttStatusV1, StartSelectedSttPushToTalkRequestV1,
};
use crate::sidecar_protocol::{
    NativeProfileSafetyPolicy, NativeSafetyEvidenceState, NativeSimulationSafetyContext,
    NativeTtsVoiceDiscoveryRequest, NativeTtsVoiceDiscoveryResult,
};
use crate::sidecar_supervisor::{RuntimeLaunchConfig, RuntimeSupervisor};
#[cfg(debug_assertions)]
use crate::synthetic_review_target::SyntheticReviewTargetLauncher;
use crate::visual_runtime::{MouthWorkerLaunchConfig, MouthWorkerSupervisor, VisualCoordinator};
use interactive_npcs_credential_vault::SecretValue;
use interactive_npcs_diagnostics::{
    CheckCategory, CheckResult, CredentialPresenceState, DiagnosticMatrixBuilder,
    DiagnosticMatrixResult, DiagnosticStatus, ObservationProvenance, ProviderCredentialPresence,
    Severity, SuggestedAction, SuggestedActionKind,
};
use npc_character_db::{CharacterDatabase, EncounterLifecycleEventV1};
use npc_model_manager::SelectedLoadoutSelectionV1;
use npc_product_benchmark::{
    BenchmarkManager, BenchmarkReportStore, BenchmarkReportV1, BenchmarkRunRequestV1,
    BenchmarkStatusV1, EvidenceExecutionMode, NativeTelemetryCollector, ProductBindingV1,
    ProviderRole as BenchmarkProviderRole, ProviderRouteBindingV1, BENCHMARK_REQUEST_SCHEMA_V1,
};
use npc_provider_loadouts::LoadoutContextV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::State;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug)]
struct OnboardingCache {
    snapshot: OnboardingSnapshot,
    persistence: OnboardingPersistence,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ManualActorPickerPresentationStateV1 {
    Waiting,
    Selected,
    Cancelled,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ManualActorPickerUnavailableReasonV1 {
    NoAdmittedNativeCandidateSet,
    StaleNativeFrame,
    NativeOverlayUnavailable,
    NoActiveSelection,
}

/// The only command-31 shape permitted across the WebView boundary. Native
/// request IDs, handles, pixels, candidates, rectangles, clicks, nonces, and
/// receipt provenance are deliberately absent.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualActorPickerPresentationV1 {
    pub schema_version: u32,
    pub state: ManualActorPickerPresentationStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<ManualActorPickerUnavailableReasonV1>,
    pub detail: String,
}

#[derive(Clone)]
struct ActiveManualActorPickerV1 {
    request: NativeManualActorPickerRequestV1,
    receipt_nonce_high: u64,
    receipt_nonce_low: u64,
}

#[derive(Default)]
struct ManualActorPickerStateV1 {
    /// Published only by an admitted native visual detector. There is no Tauri
    /// command or serde DTO capable of filling this slot.
    candidate_set: Option<NativeManualActorPickerRequestV1>,
    active: Option<ActiveManualActorPickerV1>,
}

struct ManualActorPickerControllerV1 {
    state: tokio::sync::Mutex<ManualActorPickerStateV1>,
    next_request_id: AtomicU64,
}

impl ManualActorPickerControllerV1 {
    fn new() -> Self {
        Self {
            state: tokio::sync::Mutex::new(ManualActorPickerStateV1::default()),
            next_request_id: AtomicU64::new(1),
        }
    }

    async fn publish_native_candidate_set(&self, mut request: NativeManualActorPickerRequestV1) {
        // Request identity is always minted at start; visual producers cannot
        // choose it and the WebView can never observe it.
        request.request_id.clear();
        self.state.lock().await.candidate_set = Some(request);
    }

    async fn invalidate(&self) -> Option<ActiveManualActorPickerV1> {
        let mut state = self.state.lock().await;
        state.candidate_set = None;
        state.active.take()
    }
}

pub struct AppState {
    onboarding: Mutex<OnboardingCache>,
    onboarding_store: OnboardingStore,
    pub runtime: RuntimeRouter,
    pub media_broker: MediaBrokerSupervisor,
    pub visual_runtime: MouthWorkerSupervisor,
    visual_coordinator: VisualCoordinator,
    pub(crate) identity_actor_locks: Arc<NativeActorLockBusV1>,
    pub(crate) identity_runtime: Arc<QualifiedIdentityRuntimeServiceV1>,
    pub(crate) credentials: Arc<dyn CredentialPresence>,
    credential_prompt: Arc<dyn CredentialPrompt>,
    resources: ResourceCatalog,
    content_packs: ContentPackManagerV1,
    character_content_overrides: CharacterContentOverrideManagerV1,
    character_mouth_packs: Arc<CharacterMouthPackManagerV1>,
    pub(crate) provider_loadouts: ProviderLoadoutManager,
    character_workspace: CharacterWorkspace,
    game_targets: GameTargetManager,
    #[cfg(debug_assertions)]
    synthetic_review_target: SyntheticReviewTargetLauncher,
    pub(crate) local_resources: Arc<LocalResourceManager>,
    diagnostics_v2: Arc<DiagnosticsV2Manager>,
    product_benchmark: BenchmarkManager,
    product_preferences: ProductPreferenceManager,
    subtitle_preferences: npc_subtitle_engine::SubtitlePreferenceManager,
    selected_stt: SelectedSttController,
    manual_actor_picker: ManualActorPickerControllerV1,
    runtime_storage_gate: tokio::sync::Mutex<()>,
}

impl AppState {
    #[cfg(test)]
    pub fn new(
        config_directory: PathBuf,
        resource_directory: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::new_for_distribution(
            config_directory,
            resource_directory,
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE.into(),
        )
    }

    pub(crate) fn new_for_distribution(
        config_directory: PathBuf,
        resource_directory: Option<PathBuf>,
        application_namespace: String,
    ) -> Result<Self, String> {
        let config_directory =
            crate::private_directory::ensure_private_directory(&config_directory)
                .map_err(|error| error.to_string())?;
        let onboarding_store = OnboardingStore::new(config_directory.clone());
        let (snapshot, persistence) = onboarding_store.load();
        let credentials: Arc<dyn CredentialPresence> =
            match SystemCredentialPresence::new_for_application(&application_namespace) {
                Ok(presence) => Arc::new(presence),
                Err(_) => Arc::new(UnavailableCredentialPresence),
            };
        let credential_prompt: Arc<dyn CredentialPrompt> = Arc::new(SystemCredentialPrompt::new(
            cfg!(dev) && cfg!(debug_assertions),
        ));
        let resource_root = resource_directory
            .clone()
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."));
        let launch = RuntimeLaunchConfig::from_application_paths(
            resource_root.clone(),
            config_directory.join("runtime-host-data"),
            cfg!(dev) && cfg!(debug_assertions),
            application_namespace.clone(),
        )
        .map_err(|error| error.to_string())?;
        let runtime_supervisor =
            RuntimeSupervisor::try_new(launch).map_err(|error| error.to_string())?;
        let diagnostics_v2 = Arc::new(
            DiagnosticsV2Manager::new(&config_directory).map_err(|error| error.to_string())?,
        );
        let media_launch = MediaBrokerLaunchConfig::from_application(
            cfg!(dev) && cfg!(debug_assertions),
            &config_directory,
        )
        .map_err(|error| error.to_string())?;
        #[cfg(debug_assertions)]
        let synthetic_review_target = SyntheticReviewTargetLauncher::new(
            std::env::current_exe()
                .map_err(|error| format!("control executable path is unavailable: {error}"))?,
            resource_root.clone(),
            media_launch.debug_synthetic_metadata_path.clone(),
            runtime_supervisor.clone(),
        );
        let media_broker = MediaBrokerSupervisor::new(media_launch, runtime_supervisor.clone());
        let character_mouth_packs = Arc::new(
            CharacterMouthPackManagerV1::new(&config_directory)
                .map_err(|error| error.to_string())?,
        );
        let visual_runtime = MouthWorkerSupervisor::new(
            MouthWorkerLaunchConfig::from_application(
                cfg!(dev) && cfg!(debug_assertions),
                &config_directory,
            )
            .map_err(|error| error.to_string())?,
            runtime_supervisor.clone(),
            media_broker.clone(),
        )
        .with_character_mouth_packs(Arc::clone(&character_mouth_packs));
        let identity_actor_locks = NativeActorLockBusV1::new_unqualified();
        let local_resources = Arc::new(
            LocalResourceManager::new(&config_directory, Some(&resource_root))
                .map_err(|error| error.to_string())?,
        );
        let identity_runtime = QualifiedIdentityRuntimeServiceV1::new(
            Arc::clone(&local_resources),
            media_broker.clone(),
            runtime_supervisor.clone(),
            Arc::clone(&identity_actor_locks),
        );
        let visual_coordinator = VisualCoordinator::new(
            Arc::clone(&identity_actor_locks),
            visual_runtime.clone(),
            Arc::clone(&local_resources),
        );
        let runtime = RuntimeRouter::new(
            runtime_supervisor,
            media_broker.clone(),
            visual_coordinator.clone(),
            config_directory.join("encounter-registry-v1.json"),
            Arc::clone(&diagnostics_v2),
        )
        .map_err(|error| error.to_string())?;
        let character_workspace =
            CharacterWorkspace::new(&config_directory).map_err(|error| error.to_string())?;
        let benchmark_store = BenchmarkReportStore::new(config_directory.join("benchmark-reports"))
            .map_err(|error| error.to_string())?;
        let product_benchmark = BenchmarkManager::new(
            benchmark_store,
            Arc::new(ControlBenchmarkProbe::new(
                runtime.supervisor().clone(),
                media_broker.clone(),
            )),
            Arc::new(NativeTelemetryCollector),
        );
        let product_preferences =
            ProductPreferenceManager::new(&config_directory, &snapshot.preferences)
                .map_err(|error| error.to_string())?;
        let subtitle_preferences =
            npc_subtitle_engine::SubtitlePreferenceManager::new(&config_directory)
                .map_err(|error| error.to_string())?;
        let selected_stt = SelectedSttController::new(
            runtime.supervisor().clone(),
            media_broker.clone(),
            Arc::clone(&diagnostics_v2),
        );
        let content_packs = ContentPackManagerV1::new(&config_directory);
        let character_content_overrides = CharacterContentOverrideManagerV1::new(&config_directory);
        let resources = ResourceCatalog::with_user_content_layers(
            resource_directory,
            content_packs.active_profile_root(),
            character_content_overrides.path(),
        );
        let provider_loadouts = ProviderLoadoutManager::new_for_distribution(
            config_directory,
            application_namespace,
            Arc::clone(&credentials),
        );
        Ok(Self {
            onboarding: Mutex::new(OnboardingCache {
                snapshot,
                persistence,
            }),
            onboarding_store,
            runtime,
            media_broker,
            visual_runtime,
            visual_coordinator,
            identity_actor_locks,
            identity_runtime,
            credentials,
            credential_prompt,
            resources,
            content_packs,
            character_content_overrides,
            character_mouth_packs,
            provider_loadouts,
            character_workspace,
            game_targets: GameTargetManager::default(),
            #[cfg(debug_assertions)]
            synthetic_review_target,
            local_resources,
            diagnostics_v2,
            product_benchmark,
            product_preferences,
            subtitle_preferences,
            selected_stt,
            manual_actor_picker: ManualActorPickerControllerV1::new(),
            runtime_storage_gate: tokio::sync::Mutex::new(()),
        })
    }

    pub async fn shutdown_services(&self) {
        let _ = self.selected_stt.cancel().await;
        if let Some(active) = self.manual_actor_picker.invalidate().await {
            let _ = self
                .media_broker
                .cancel_manual_actor_picker(&active.request)
                .await;
        }
        self.visual_coordinator.stop_turn(None).await;
        self.identity_runtime.shutdown().await;
        self.visual_runtime.shutdown().await;
        self.media_broker.shutdown().await;
        self.runtime.shutdown().await;
        let _ = self.diagnostics_v2.mark_clean_exit();
    }

    pub fn start_packaged_privacy_receipt_probe(&self) {
        crate::packaged_privacy_receipt::start_probe(
            &self.provider_loadouts,
            &self.local_resources,
        );
    }

    #[cfg(debug_assertions)]
    pub fn start_installer_smoke_probe(&self, config_directory: PathBuf) {
        if std::env::var_os("NPC2_INSTALLER_SMOKE").as_deref() != Some(std::ffi::OsStr::new("1")) {
            return;
        }
        let runtime = self.runtime.supervisor().clone();
        let media = self.media_broker.clone();
        tauri::async_runtime::spawn(async move {
            let runtime_result = runtime.ping().await;
            let media_result = media.refresh_health().await;
            let report = InstallerSmokeHealth {
                schema_version: 1,
                runtime_authenticated: runtime_result.is_ok(),
                runtime: runtime.health(),
                media_broker_authenticated: media_result.is_ok(),
                media_broker: media.health(),
            };
            let _ = write_installer_smoke_health(&config_directory, &report);
        });
    }

    fn onboarding_snapshot(&self) -> (OnboardingSnapshot, OnboardingPersistence) {
        self.onboarding
            .lock()
            .map(|cache| (cache.snapshot.clone(), cache.persistence.clone()))
            .unwrap_or_else(|_| {
                (
                    OnboardingSnapshot::default(),
                    OnboardingPersistence {
                        health: PersistenceHealth::Unavailable,
                        detail: "Onboarding state is temporarily unavailable.".into(),
                    },
                )
            })
    }
}

fn preference_authority_references(
    scope: &ProductPreferenceScopeV1,
    state: &AppState,
) -> (
    Option<RouteAuthorityReferenceV1>,
    ResourceAuthorityReferenceV1,
) {
    let context = match scope {
        ProductPreferenceScopeV1::Global => LoadoutContextV1::global(),
        ProductPreferenceScopeV1::Game { game_profile_id } => {
            LoadoutContextV1::game(game_profile_id)
        }
        ProductPreferenceScopeV1::Character {
            game_profile_id,
            character_id,
        } => LoadoutContextV1::character(game_profile_id, character_id),
    };
    let route_snapshot = state
        .provider_loadouts
        .resolve_for_turn(context, false, &state.local_resources)
        .ok()
        .and_then(|resolved| {
            serde_json::to_vec(&resolved)
                .ok()
                .map(|canonical| RouteAuthorityReferenceV1 {
                    source_loadout_id: resolved.leaf_loadout_id.to_string(),
                    generation: None,
                    sha256: format!("{:x}", Sha256::digest(canonical)),
                    activation_performed: false,
                })
        });
    let selection_id = state
        .local_resources
        .selected_loadout_planner()
        .ok()
        .and_then(|planner| planner.selected.map(|selection| selection.selection_id));
    (
        route_snapshot,
        ResourceAuthorityReferenceV1 {
            selection_id,
            admission_status: None,
            admission_receipt_present: false,
            exact_target_pid: None,
            activation_performed: false,
        },
    )
}

#[tauri::command]
pub fn product_preferences_snapshot(
    scope: ProductPreferenceScopeV1,
    state: State<'_, AppState>,
) -> Result<ProductPreferenceSnapshotV1, CommandError> {
    let (route, resource) = preference_authority_references(&scope, &state);
    state
        .product_preferences
        .snapshot(scope, route, resource)
        .map_err(product_error)
}

#[tauri::command]
pub fn effective_configuration_snapshot(
    request: crate::effective_configuration::EffectiveConfigurationRequestV1,
    state: State<'_, AppState>,
) -> Result<crate::effective_configuration::EffectiveConfigurationSnapshotV1, CommandError> {
    crate::effective_configuration::build_effective_configuration_snapshot(
        request,
        &state.product_preferences,
        &state.provider_loadouts,
        &state.local_resources,
    )
    .map_err(|error| CommandError::Product {
        message: error.to_string(),
    })
}

#[tauri::command]
pub fn read_subtitle_preferences(
    request: npc_subtitle_engine::ReadSubtitlePreferencesRequestV1,
    state: State<'_, AppState>,
) -> Result<npc_subtitle_engine::SubtitlePreferenceSnapshotV1, CommandError> {
    state
        .subtitle_preferences
        .read(request)
        .map_err(product_error)
}

#[tauri::command]
pub fn save_subtitle_preferences(
    request: npc_subtitle_engine::SaveSubtitlePreferencesRequestV1,
    state: State<'_, AppState>,
) -> Result<npc_subtitle_engine::SubtitlePreferenceSnapshotV1, CommandError> {
    state
        .subtitle_preferences
        .save(request)
        .map_err(product_error)
}

#[tauri::command]
pub fn reset_subtitle_preferences(
    request: npc_subtitle_engine::ResetSubtitlePreferencesRequestV1,
    state: State<'_, AppState>,
) -> Result<npc_subtitle_engine::SubtitlePreferenceSnapshotV1, CommandError> {
    state
        .subtitle_preferences
        .reset(request)
        .map_err(product_error)
}

#[tauri::command]
pub fn save_product_preferences(
    request: SaveProductPreferencesRequestV1,
    state: State<'_, AppState>,
) -> Result<ProductPreferenceSnapshotV1, CommandError> {
    let (route, resource) = preference_authority_references(&request.entry.scope, &state);
    state
        .product_preferences
        .save(request, route, resource)
        .map_err(product_error)
}

#[tauri::command]
pub fn reset_product_preferences(
    request: ResetProductPreferencesRequestV1,
    state: State<'_, AppState>,
) -> Result<ProductPreferenceSnapshotV1, CommandError> {
    let (route, resource) = preference_authority_references(&request.scope, &state);
    state
        .product_preferences
        .reset(request, route, resource)
        .map_err(product_error)
}

#[cfg(debug_assertions)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallerSmokeHealth {
    schema_version: u32,
    runtime_authenticated: bool,
    runtime: RuntimeHealthSnapshot,
    media_broker_authenticated: bool,
    media_broker: MediaBrokerHealthSnapshot,
}

#[cfg(debug_assertions)]
fn write_installer_smoke_health(
    directory: &std::path::Path,
    report: &InstallerSmokeHealth,
) -> Result<(), std::io::Error> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if bytes.len() > 32 * 1024 {
        return Err(std::io::Error::other("smoke health exceeds bound"));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(directory.join("installer-smoke-health-v1.json"))
        .map_err(|error| error.error)?;
    Ok(())
}

#[derive(Debug)]
struct UnavailableCredentialPresence;

impl CredentialPresence for UnavailableCredentialPresence {
    fn status(&self, _reference: &str) -> crate::domain::CredentialReferenceStatus {
        crate::domain::CredentialReferenceStatus::Unavailable
    }

    fn save(&self, _reference: &str, _secret: &SecretValue) -> Result<(), CredentialMutationError> {
        Err(CredentialMutationError::Unavailable)
    }

    fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
        Err(CredentialMutationError::Unavailable)
    }

    fn availability_detail(&self) -> &'static str {
        "Windows Credential Manager could not be initialized."
    }
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum CommandError {
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
    #[error("state is temporarily unavailable")]
    StateUnavailable,
    #[error("settings could not be persisted: {message}")]
    Persistence { message: String },
    #[error("simulation could not start: {message}")]
    Simulation { message: String },
    #[error("runtime operation failed: {message}")]
    Runtime { message: String },
    #[error("credential operation failed: {message}")]
    Credential { message: String },
    #[error("product operation failed: {message}")]
    Product { message: String },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IdentityReferenceEnrollmentAvailabilityV1 {
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityReferenceEnrollmentStatusV1 {
    pub schema_version: u32,
    pub status: IdentityReferenceEnrollmentAvailabilityV1,
    pub reason_code: String,
    pub detail: String,
    pub signed_identity_pack_admitted: bool,
    pub native_picker_available_after_admission: bool,
    pub raw_pixels_exposed_to_webview: bool,
    pub worker_capability_exposed_to_webview: bool,
}

#[tauri::command]
pub fn bootstrap_snapshot(state: State<'_, AppState>) -> BootstrapSnapshot {
    build_bootstrap(&state)
}

#[tauri::command]
pub fn save_onboarding(
    mut onboarding: OnboardingSnapshot,
    state: State<'_, AppState>,
) -> Result<SaveOnboardingResult, CommandError> {
    onboarding.updated_at_epoch_ms = epoch_ms();
    onboarding
        .validate()
        .map_err(|message| CommandError::InvalidRequest { message })?;
    state
        .onboarding_store
        .save(&onboarding)
        .map_err(|error| CommandError::Persistence {
            message: error.to_string(),
        })?;
    let persistence = OnboardingPersistence {
        health: PersistenceHealth::Healthy,
        detail: "Onboarding settings were saved to the current user's app-data directory.".into(),
    };
    let mut cache = state
        .onboarding
        .lock()
        .map_err(|_| CommandError::StateUnavailable)?;
    cache.snapshot = onboarding.clone();
    cache.persistence = persistence.clone();
    Ok(SaveOnboardingResult {
        onboarding,
        persistence,
    })
}

#[tauri::command]
pub async fn start_simulation(
    mut request: StartSimulationRequest,
    events: Channel<SimulationEvent>,
    state: State<'_, AppState>,
) -> Result<StartSimulationResult, CommandError> {
    // Serialize runtime launch with native memory restore/removal. The guard is
    // released once the runtime has accepted the turn; a later maintenance
    // request may then stop that child and wait for its SQLite writer to exit.
    let _runtime_storage_guard = state.runtime_storage_gate.lock().await;
    request
        .validate()
        .map_err(|message| CommandError::InvalidRequest { message })?;
    if state.selected_stt.status().await.status
        != crate::selected_stt::SelectedSttCaptureStatusV1::Idle
    {
        return Err(CommandError::Simulation {
            message: "selected_stt_capture_active: finish or cancel push-to-talk capture before starting a response".into(),
        });
    }
    let synthetic_capture_verified = if request.game_profile_id.as_deref() == Some("eclipse-harbor")
    {
        state
            .game_targets
            .verify_capture(&state.media_broker)
            .await
            .map_err(product_error)?;
        true
    } else {
        false
    };
    if let Some(game_profile_id) = request
        .game_profile_id
        .as_deref()
        .filter(|id| *id != "eclipse-harbor")
    {
        let character_id = state
            .character_workspace
            .resolve_for_turn(
                &state.resources,
                game_profile_id,
                request.character_id.as_deref(),
            )
            .map_err(product_error)?;
        request.character_id = Some(character_id);
        // Authored profiles use the canonical character ID. A display name is
        // generic/manual-selection data and must not override profile identity.
        request.character_name = None;
        request.effective_game_profile = Some(
            state
                .resources
                .load_game_profile(game_profile_id)
                .map_err(product_error)?,
        );
    }
    let context = LoadoutContextV1 {
        game_id: request.game_profile_id.clone(),
        character_id: request.character_id.clone(),
    };
    let routes = state.provider_loadouts.resolve_for_turn(
        context,
        request.execution_mode == ExecutionMode::Local,
        &state.local_resources,
    )?;
    // Eclipse Harbor is a real external capture target. It must traverse the
    // same selected provider, WASAPI playback, subtitle, and visual pipelines
    // as any other game; a profile ID must never silently replace those routes
    // with a deterministic no-audio fixture.
    let private_evaluation_acknowledgements = state
        .provider_loadouts
        .private_evaluation_authority_for_routes(&routes)?
        .into_iter()
        .map(|authority| authority.acknowledgement)
        .collect();
    let selected_stt = if let Some(reference) = request.selected_stt_receipt.take() {
        let consumed = state
            .selected_stt
            .consume(
                &reference.receipt_id,
                reference.generation,
                request.game_profile_id.as_deref(),
                request.character_id.as_deref(),
                &routes.leaf_loadout_id.to_string(),
            )
            .await
            .map_err(selected_stt_error)?;
        request.transcript = Some(consumed.transcript.clone());
        Some(consumed)
    } else {
        None
    };
    let application_namespace = state.provider_loadouts.application_namespace().to_owned();
    let safety_context =
        trusted_turn_safety_context(&request, &state.resources, synthetic_capture_verified);
    let product_preference_scope = match (
        request.game_profile_id.as_ref(),
        request.character_id.as_ref(),
    ) {
        (Some(game_profile_id), Some(character_id)) => ProductPreferenceScopeV1::Character {
            game_profile_id: game_profile_id.clone(),
            character_id: character_id.clone(),
        },
        (Some(game_profile_id), None) => ProductPreferenceScopeV1::Game {
            game_profile_id: game_profile_id.clone(),
        },
        (None, _) => ProductPreferenceScopeV1::Global,
    };
    let (route_authority, resource_authority) =
        preference_authority_references(&product_preference_scope, &state);
    let product_preferences = state
        .product_preferences
        .snapshot(
            product_preference_scope,
            route_authority,
            resource_authority,
        )
        .map_err(product_error)?;
    // Pin the effective subtitle renderer state atomically after canonical
    // character selection and before dispatch. The WebView never supplies this
    // authority, and save/reset races can only affect a later turn.
    let subtitle_scope = match (
        request.game_profile_id.as_ref(),
        request.character_id.as_ref(),
    ) {
        (Some(game_profile_id), Some(character_id)) => {
            npc_subtitle_engine::SubtitlePreferenceScopeV1::Character {
                game_profile_id: game_profile_id.clone(),
                character_id: character_id.clone(),
            }
        }
        (Some(game_profile_id), None) => npc_subtitle_engine::SubtitlePreferenceScopeV1::Game {
            game_profile_id: game_profile_id.clone(),
        },
        (None, _) => npc_subtitle_engine::SubtitlePreferenceScopeV1::Global,
    };
    let subtitle_renderer_authority = state
        .subtitle_preferences
        .resolve_renderer_authority(subtitle_scope)
        .map_err(product_error)?;
    state
        .runtime
        .start(
            request,
            events,
            routes,
            private_evaluation_acknowledgements,
            application_namespace,
            safety_context,
            selected_stt,
            subtitle_renderer_authority,
            product_preferences.effective.subtitles.value,
            product_preferences.effective.overlay.value,
        )
        .await
        .map_err(|error| CommandError::Simulation {
            message: error.to_string(),
        })
}

fn trusted_turn_safety_context(
    request: &StartSimulationRequest,
    resources: &ResourceCatalog,
    synthetic_capture_verified: bool,
) -> NativeSimulationSafetyContext {
    if request.game_profile_id.as_deref() == Some("eclipse-harbor") && synthetic_capture_verified {
        return NativeSimulationSafetyContext {
            evidence_state: NativeSafetyEvidenceState::VerifiedSafe,
            profile_policy: NativeProfileSafetyPolicy::SyntheticFixture,
            visuals_allowed: true,
            protected_online_detected: false,
            anti_cheat_detected: false,
        };
    }
    let known_profile = request.game_profile_id.as_deref().and_then(|id| {
        resources
            .game_summaries()
            .into_iter()
            .find(|profile| profile.id == id)
    });
    // A known authored profile can run as an isolated control-console
    // conversation: no capture, overlay, executable action, native identity,
    // or live-game-awareness claim. Runtime validation admits only this exact
    // evidence/policy pair and independently rejects vision/lip-sync routes.
    if known_profile.is_some() {
        return NativeSimulationSafetyContext {
            evidence_state: NativeSafetyEvidenceState::ConsoleIsolated,
            profile_policy: NativeProfileSafetyPolicy::ConsoleIsolatedNoGameInteraction,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: false,
        };
    }
    NativeSimulationSafetyContext {
        evidence_state: NativeSafetyEvidenceState::Unknown,
        profile_policy: NativeProfileSafetyPolicy::Unknown,
        visuals_allowed: false,
        protected_online_detected: false,
        anti_cheat_detected: false,
    }
}

#[tauri::command]
pub async fn cancel_simulation(
    state: State<'_, AppState>,
) -> Result<crate::domain::CancelSimulationResult, CommandError> {
    Ok(state.runtime.cancel().await)
}

#[tauri::command]
pub async fn discover_tts_stock_voices(
    force_refresh: bool,
    state: State<'_, AppState>,
) -> Result<NativeTtsVoiceDiscoveryResult, CommandError> {
    if !state.provider_loadouts.private_evaluation_enabled() {
        return Err(CommandError::InvalidRequest {
            message: "NVIDIA Magpie voice discovery is available only in the native private-evaluation review namespace".into(),
        });
    }
    let result = state
        .runtime
        .supervisor()
        .discover_tts_voices(NativeTtsVoiceDiscoveryRequest {
            schema_version: 1,
            provider_id: "nvidia-nim-magpie".into(),
            model_id: "magpie-tts-multilingual".into(),
            force_refresh,
        })
        .await
        .map_err(|error| CommandError::Runtime {
            message: error.to_string(),
        })?;
    state
        .provider_loadouts
        .record_stock_voice_discovery(&result)?;
    let available = matches!(
        result.status,
        crate::sidecar_protocol::NativeTtsVoiceDiscoveryStatus::Available
    );
    let _ = state.diagnostics_v2.record_native_event(
        "tts",
        if available {
            "stock_voice.discovery_available"
        } else {
            "stock_voice.discovery_unavailable"
        },
        if available {
            Severity::Info
        } else {
            Severity::Warn
        },
        if available {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Failed
        },
        (!available).then_some("stock_voice_discovery_unavailable"),
    );
    Ok(result)
}

#[tauri::command]
pub async fn enumerate_audio_outputs(
    state: State<'_, AppState>,
) -> Result<AudioOutputSnapshot, CommandError> {
    let result = state
        .media_broker
        .enumerate_audio_outputs()
        .await
        .map_err(product_error);
    let _ = state.diagnostics_v2.record_native_event(
        "audio-broker",
        if result.is_ok() {
            "output.enumeration_completed"
        } else {
            "output.enumeration_failed"
        },
        if result.is_ok() {
            Severity::Info
        } else {
            Severity::Error
        },
        if result.is_ok() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Failed
        },
        result.is_err().then_some("audio_output_enumeration_failed"),
    );
    result
}

#[tauri::command]
pub async fn selected_audio_output(
    state: State<'_, AppState>,
) -> Result<Option<SelectedAudioOutput>, CommandError> {
    state
        .media_broker
        .selected_audio_output()
        .await
        .map_err(product_error)
}

#[tauri::command]
pub async fn select_audio_output(
    selection: AudioOutputSelection,
    state: State<'_, AppState>,
) -> Result<SelectedAudioOutput, CommandError> {
    let result = state
        .media_broker
        .select_audio_output(selection)
        .await
        .map_err(product_error);
    let _ = state.diagnostics_v2.record_native_event(
        "audio-broker",
        if result.is_ok() {
            "output.selection_persisted"
        } else {
            "output.selection_failed"
        },
        if result.is_ok() {
            Severity::Info
        } else {
            Severity::Error
        },
        if result.is_ok() {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Failed
        },
        result.is_err().then_some("audio_output_selection_failed"),
    );
    result
}

#[tauri::command]
pub async fn enumerate_audio_inputs(
    state: State<'_, AppState>,
) -> Result<AudioInputSnapshot, CommandError> {
    let result = state
        .media_broker
        .enumerate_audio_inputs()
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "audio-broker",
        result.is_ok(),
        "input.enumeration_completed",
        "input.enumeration_failed",
        "audio_input_enumeration_failed",
    );
    result
}

#[tauri::command]
pub async fn selected_audio_input(
    state: State<'_, AppState>,
) -> Result<Option<SelectedAudioInput>, CommandError> {
    state
        .media_broker
        .selected_audio_input()
        .await
        .map_err(product_error)
}

#[tauri::command]
pub async fn select_audio_input(
    selection: AudioInputSelection,
    state: State<'_, AppState>,
) -> Result<SelectedAudioInput, CommandError> {
    let result = state
        .media_broker
        .select_audio_input(selection)
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "audio-broker",
        result.is_ok(),
        "input.selection_persisted",
        "input.selection_failed",
        "audio_input_selection_failed",
    );
    result
}

#[tauri::command]
pub async fn start_selected_stt_push_to_talk(
    mut request: StartSelectedSttPushToTalkRequestV1,
    events: Channel<SelectedSttPushToTalkEventV1>,
    state: State<'_, AppState>,
) -> Result<SelectedSttCapturingV1, CommandError> {
    let _runtime_storage_guard = state.runtime_storage_gate.lock().await;
    if state.runtime.snapshot().status != SimulationStatus::Idle {
        return Err(CommandError::Runtime {
            message: "selected_stt_simulation_active: finish or cancel the active response before starting push-to-talk capture".into(),
        });
    }
    if let Some(game_profile_id) = request.game_profile_id.as_deref() {
        let character_id = state
            .character_workspace
            .resolve_for_turn(
                &state.resources,
                game_profile_id,
                request.character_id.as_deref(),
            )
            .map_err(product_error)?;
        request.character_id = Some(character_id);
    }
    let context = LoadoutContextV1 {
        game_id: request.game_profile_id.clone(),
        character_id: request.character_id.clone(),
    };
    let resolved =
        state
            .provider_loadouts
            .resolve_for_turn(context, false, &state.local_resources)?;
    let assemblyai_reference = credential_reference_for("assemblyai").ok_or_else(|| {
        CommandError::Credential {
            message: "selected_stt_credential_unavailable: AssemblyAI is not in the native credential allowlist".into(),
        }
    })?;
    if state.credentials.status(&assemblyai_reference) != CredentialReferenceStatus::Present {
        return Err(CommandError::Credential {
            message: "selected_stt_credential_missing: save the AssemblyAI credential in the native vault before starting push-to-talk capture".into(),
        });
    }
    let result = state
        .selected_stt
        .start(request, &resolved, events)
        .await
        .map_err(selected_stt_error);
    record_native_product_outcome(
        &state,
        "stt",
        result.is_ok(),
        "selected_stt.capture_started",
        "selected_stt.capture_start_failed",
        "selected_stt_capture_start_failed",
    );
    result
}

#[tauri::command]
pub async fn selected_stt_push_to_talk_status(
    state: State<'_, AppState>,
) -> Result<SelectedSttStatusV1, CommandError> {
    Ok(state.selected_stt.status().await)
}

#[tauri::command]
pub async fn cancel_selected_stt_push_to_talk(
    state: State<'_, AppState>,
) -> Result<SelectedSttCancelResultV1, CommandError> {
    state
        .selected_stt
        .cancel()
        .await
        .map_err(selected_stt_error)
}

fn selected_stt_error(error: SelectedSttError) -> CommandError {
    CommandError::Runtime {
        message: error.to_string(),
    }
}

#[tauri::command]
pub fn identity_reference_enrollment_status() -> IdentityReferenceEnrollmentStatusV1 {
    IdentityReferenceEnrollmentStatusV1 {
        schema_version: 1,
        status: IdentityReferenceEnrollmentAvailabilityV1::Unavailable,
        reason_code: "identity_pack_not_admitted".into(),
        detail: "Reference enrollment is unavailable until Model Manager proves an exact signed, verified, measured, and admitted identity pack for this device. No picker or worker was started.".into(),
        signed_identity_pack_admitted: false,
        native_picker_available_after_admission: true,
        raw_pixels_exposed_to_webview: false,
        worker_capability_exposed_to_webview: false,
    }
}

#[tauri::command]
pub fn enroll_identity_reference(
    request: IdentityReferenceEnrollmentCommandRequest,
) -> Result<IdentityReferenceEnrollmentReceipt, CommandError> {
    // Shape, rights, consent, and bounded identifiers are validated even while
    // activation is unavailable. Crucially this occurs before any native file
    // picker, broker mapping, or worker launch can be reached.
    let _validated = request.into_native().map_err(product_error)?;
    Err(CommandError::Product {
        message: "identity_pack_not_admitted: reference enrollment requires an exact signed, verified, measured, and admitted identity pack; no picker or worker was started".into(),
    })
}

fn product_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::Product {
        message: error.to_string(),
    }
}

fn record_native_product_event(
    state: &AppState,
    component: &'static str,
    event_name: &'static str,
) {
    // Product actions remain authoritative if the bounded local diagnostics
    // store is temporarily unavailable (for example, a full disk). Logging is
    // observability, not a transaction prerequisite.
    let _ = state.diagnostics_v2.record_native_event(
        component,
        event_name,
        Severity::Info,
        DiagnosticStatus::Ok,
        None,
    );
}

fn record_native_product_outcome(
    state: &AppState,
    component: &'static str,
    succeeded: bool,
    completed_event: &'static str,
    failed_event: &'static str,
    failure_code: &'static str,
) {
    let _ = state.diagnostics_v2.record_native_event(
        component,
        if succeeded {
            completed_event
        } else {
            failed_event
        },
        if succeeded {
            Severity::Info
        } else {
            Severity::Error
        },
        if succeeded {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Failed
        },
        (!succeeded).then_some(failure_code),
    );
}

#[tauri::command]
pub fn discover_game_targets(
    game_profile_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<GameTargetCandidate>, CommandError> {
    state
        .game_targets
        .discover(&state.resources, &game_profile_id)
        .map_err(product_error)
}

#[tauri::command]
pub async fn select_game_target(
    request: SelectGameTargetRequest,
    state: State<'_, AppState>,
) -> Result<GameTargetSelection, CommandError> {
    record_native_product_event(&state, "game-target", "selection.requested");
    state
        .game_targets
        .select(&state.resources, request)
        .map_err(product_error)?;
    let revocation = state.local_resources.revoke_active_loadout_admission();
    state.identity_runtime.invalidate_and_reconcile_background();
    revocation.map_err(product_error)?;
    state
        .game_targets
        .bind_selected_capture(&state.resources, &state.media_broker)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub fn selected_game_target(
    state: State<'_, AppState>,
) -> Result<Option<GameTargetSelection>, CommandError> {
    state.game_targets.snapshot().map_err(product_error)
}

#[tauri::command]
pub async fn clear_game_target(state: State<'_, AppState>) -> Result<(), CommandError> {
    record_native_product_event(&state, "game-target", "selection.clear_requested");
    state
        .game_targets
        .clear(&state.media_broker)
        .await
        .map_err(product_error)?;
    let _ = state.manual_actor_picker.invalidate().await;
    let revocation = state.local_resources.revoke_active_loadout_admission();
    state.identity_runtime.invalidate_and_reconcile_background();
    revocation.map_err(product_error)?;
    Ok(())
}

#[tauri::command]
pub async fn verify_selected_game_capture(
    state: State<'_, AppState>,
) -> Result<GameCaptureVerification, CommandError> {
    record_native_product_event(&state, "capture", "verification.requested");
    let result = state
        .game_targets
        .verify_capture(&state.media_broker)
        .await
        .map_err(product_error);
    match &result {
        Ok(_) => {
            let _ = state.diagnostics_v2.record_native_event(
                "capture",
                "verification.completed",
                Severity::Info,
                DiagnosticStatus::Ok,
                None,
            );
        }
        Err(_) => {
            let _ = state.diagnostics_v2.record_native_event(
                "capture",
                "verification.failed",
                Severity::Warn,
                DiagnosticStatus::Failed,
                Some("capture_verification_failed"),
            );
        }
    }
    result
}

fn manual_actor_presentation(
    state: ManualActorPickerPresentationStateV1,
    detail: &'static str,
) -> ManualActorPickerPresentationV1 {
    ManualActorPickerPresentationV1 {
        schema_version: 1,
        unavailable_reason: (state == ManualActorPickerPresentationStateV1::Unavailable)
            .then_some(ManualActorPickerUnavailableReasonV1::NativeOverlayUnavailable),
        state,
        detail: detail.into(),
    }
}

fn manual_actor_unavailable(
    reason: ManualActorPickerUnavailableReasonV1,
    detail: &'static str,
) -> ManualActorPickerPresentationV1 {
    ManualActorPickerPresentationV1 {
        schema_version: 1,
        state: ManualActorPickerPresentationStateV1::Unavailable,
        unavailable_reason: Some(reason),
        detail: detail.into(),
    }
}

fn manual_actor_no_candidate_set() -> ManualActorPickerPresentationV1 {
    manual_actor_unavailable(
        ManualActorPickerUnavailableReasonV1::NoAdmittedNativeCandidateSet,
        "No current admitted native visual candidate set is available. Audio and subtitles remain available.",
    )
}

async fn settle_manual_actor_picker_receipt(
    state: &AppState,
    active: ActiveManualActorPickerV1,
    receipt: NativeManualActorPickerReceiptV1,
) -> ManualActorPickerPresentationV1 {
    if receipt.receipt_nonce_high != active.receipt_nonce_high
        || receipt.receipt_nonce_low != active.receipt_nonce_low
    {
        let _ = state.manual_actor_picker.invalidate().await;
        state
            .identity_actor_locks
            .revoke(active.request.cancellation_generation);
        return manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Unavailable,
            "Native actor selection became stale. Start a new selection from the current game frame.",
        );
    }
    match receipt.status {
        NativeManualActorPickerStatusV1::Pending => manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Waiting,
            "Choose one detected character in the native game overlay. The app never receives the frame or click location.",
        ),
        NativeManualActorPickerStatusV1::Selected => {
            let _ = state.manual_actor_picker.invalidate().await;
            let visual_admission_current = state
                .local_resources
                .resolve_admitted_openseeface_launch()
                .is_ok_and(|admitted| {
                    active.request.visual_pack_id == admitted.identity.pack_id.as_str()
                        && active.request.visual_pack_admission_sha256
                            == admitted.measured_envelope_sha256.as_str()
                        && active.request.selected_process_id == admitted.exact_target_pid
                });
            if !visual_admission_current {
                state
                    .identity_actor_locks
                    .revoke(active.request.cancellation_generation);
                return manual_actor_unavailable(
                    ManualActorPickerUnavailableReasonV1::StaleNativeFrame,
                    "The native visual-pack admission changed before the click could be consumed.",
                );
            }
            match state.identity_actor_locks.publish_from_sealed_native_click(
                &active.request,
                &receipt,
                epoch_ms(),
                active.request.cancellation_generation,
            ) {
                Ok(_) => manual_actor_presentation(
                    ManualActorPickerPresentationStateV1::Selected,
                    "Character selected from one sealed native click on the current captured frame.",
                ),
                Err(_) => {
                    state
                        .identity_actor_locks
                        .revoke(active.request.cancellation_generation);
                    manual_actor_presentation(
                        ManualActorPickerPresentationStateV1::Unavailable,
                        "Native actor selection could not be admitted because its frame binding was no longer current.",
                    )
                }
            }
        }
        NativeManualActorPickerStatusV1::Cancelled
        | NativeManualActorPickerStatusV1::TimedOut
        | NativeManualActorPickerStatusV1::ClickOutsideDetectedRoi
        | NativeManualActorPickerStatusV1::AmbiguousDetectedRoi
        | NativeManualActorPickerStatusV1::UntrustedPointerInput => {
            let _ = state.manual_actor_picker.invalidate().await;
            state
                .identity_actor_locks
                .revoke(active.request.cancellation_generation);
            manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Cancelled,
                "No character was selected. Start again and click inside exactly one detected character.",
            )
        }
        NativeManualActorPickerStatusV1::TargetLost
        | NativeManualActorPickerStatusV1::TargetResized
        | NativeManualActorPickerStatusV1::DpiChanged
        | NativeManualActorPickerStatusV1::DeviceChanged
        | NativeManualActorPickerStatusV1::CaptureChanged
        | NativeManualActorPickerStatusV1::OverlayUnavailable
        | NativeManualActorPickerStatusV1::InternalError => {
            let _ = state.manual_actor_picker.invalidate().await;
            state
                .identity_actor_locks
                .revoke(active.request.cancellation_generation);
            manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Unavailable,
                "Native actor selection is unavailable because the game frame or display context changed.",
            )
        }
    }
}

#[tauri::command]
pub async fn start_manual_actor_picker(
    state: State<'_, AppState>,
) -> Result<ManualActorPickerPresentationV1, CommandError> {
    let needs_candidates = {
        let picker = state.manual_actor_picker.state.lock().await;
        picker.active.is_none() && picker.candidate_set.is_none()
    };
    if needs_candidates {
        let game_profile_id = match state.game_targets.snapshot().map_err(product_error)? {
            Some(selected) => selected.game_profile_id,
            None => return Ok(manual_actor_no_candidate_set()),
        };
        let mut capture_verified = false;
        for attempt in 0..15_u8 {
            if state
                .game_targets
                .verify_capture(&state.media_broker)
                .await
                .is_ok()
            {
                capture_verified = true;
                break;
            }
            if attempt < 14 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        if !capture_verified {
            return Ok(manual_actor_no_candidate_set());
        }
        let discovered = match state
            .visual_runtime
            .discover_native_actor_candidates(&state.local_resources, &game_profile_id)
            .await
        {
            Ok(discovered) => discovered,
            Err(_) => return Ok(manual_actor_no_candidate_set()),
        };
        state
            .manual_actor_picker
            .publish_native_candidate_set(discovered)
            .await;
    }
    let request = {
        let mut picker = state.manual_actor_picker.state.lock().await;
        if picker.active.is_some() {
            return Ok(manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Waiting,
                "Native actor selection is already waiting for one click in the game overlay.",
            ));
        }
        let Some(mut request) = picker.candidate_set.take() else {
            return Ok(manual_actor_no_candidate_set());
        };
        let request_number = state
            .manual_actor_picker
            .next_request_id
            .fetch_add(1, Ordering::AcqRel);
        if request_number == 0 || request_number == u64::MAX {
            return Ok(manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Unavailable,
                "Native actor selection request identity is unavailable.",
            ));
        }
        request.request_id = format!("native-actor-click-{request_number}");
        request
    };
    if epoch_ms() >= request.expires_at_unix_ms {
        return Ok(manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Unavailable,
            "The detected character frame expired. Wait for a fresh native frame and start again.",
        ));
    }
    let admitted_visual = match state.local_resources.resolve_admitted_openseeface_launch() {
        Ok(admitted) => admitted,
        Err(_) => return Ok(manual_actor_no_candidate_set()),
    };
    if request.visual_pack_id != admitted_visual.identity.pack_id.as_str()
        || request.visual_pack_admission_sha256 != admitted_visual.measured_envelope_sha256.as_str()
        || request.selected_process_id != admitted_visual.exact_target_pid
    {
        return Ok(manual_actor_unavailable(
            ManualActorPickerUnavailableReasonV1::StaleNativeFrame,
            "The native visual candidate set no longer matches the exact admitted pack and game process.",
        ));
    }
    let receipt =
        match state.media_broker.begin_manual_actor_picker(&request).await {
            Ok(receipt) => receipt,
            Err(_) => return Ok(manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Unavailable,
                "The native actor overlay could not start. Audio and subtitles remain available.",
            )),
        };
    let active = ActiveManualActorPickerV1 {
        request,
        receipt_nonce_high: receipt.receipt_nonce_high,
        receipt_nonce_low: receipt.receipt_nonce_low,
    };
    state.manual_actor_picker.state.lock().await.active = Some(active.clone());
    Ok(settle_manual_actor_picker_receipt(&state, active, receipt).await)
}

#[tauri::command]
pub async fn manual_actor_picker_status(
    state: State<'_, AppState>,
) -> Result<ManualActorPickerPresentationV1, CommandError> {
    let active = state.manual_actor_picker.state.lock().await.active.clone();
    let Some(active) = active else {
        return Ok(manual_actor_unavailable(
            ManualActorPickerUnavailableReasonV1::NoActiveSelection,
            "No native actor selection is active.",
        ));
    };
    match state
        .media_broker
        .query_manual_actor_picker(&active.request)
        .await
    {
        Ok(receipt) => Ok(settle_manual_actor_picker_receipt(&state, active, receipt).await),
        Err(_) => {
            let _ = state.manual_actor_picker.invalidate().await;
            state
                .identity_actor_locks
                .revoke(active.request.cancellation_generation);
            Ok(manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Unavailable,
                "Native actor selection became stale or unavailable.",
            ))
        }
    }
}

#[tauri::command]
pub async fn cancel_manual_actor_picker(
    state: State<'_, AppState>,
) -> Result<ManualActorPickerPresentationV1, CommandError> {
    let active = state.manual_actor_picker.state.lock().await.active.clone();
    let Some(active) = active else {
        return Ok(manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Cancelled,
            "No native actor selection was active.",
        ));
    };
    let receipt = state
        .media_broker
        .cancel_manual_actor_picker(&active.request)
        .await;
    let _ = state.manual_actor_picker.invalidate().await;
    state
        .identity_actor_locks
        .revoke(active.request.cancellation_generation);
    Ok(match receipt {
        Ok(receipt)
            if receipt.receipt_nonce_high == active.receipt_nonce_high
                && receipt.receipt_nonce_low == active.receipt_nonce_low =>
        {
            manual_actor_presentation(
                ManualActorPickerPresentationStateV1::Cancelled,
                "Native actor selection was cancelled without choosing a character.",
            )
        }
        _ => manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Unavailable,
            "Native actor selection ended without retaining any actor authority.",
        ),
    })
}

#[tauri::command]
pub async fn character_database_inspection(
    request: CharacterInspectionRequest,
    state: State<'_, AppState>,
) -> Result<CharacterInspection, CommandError> {
    state
        .character_workspace
        .inspect(&state.resources, request)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub fn character_database_catalog(
    game_profile_id: String,
    state: State<'_, AppState>,
) -> Result<CharacterCatalogSnapshot, CommandError> {
    state
        .character_workspace
        .catalog(&state.resources, &game_profile_id)
        .map_err(product_error)
}

#[tauri::command]
pub fn select_game_character(
    game_profile_id: String,
    character_id: String,
    state: State<'_, AppState>,
) -> Result<SelectedCharacterResult, CommandError> {
    record_native_product_event(&state, "character-db", "character.selection_requested");
    state
        .character_workspace
        .select(&state.resources, &game_profile_id, &character_id)
        .map_err(product_error)
}

#[tauri::command]
pub fn character_content_override(
    request: CharacterContentOverrideScopeV1,
    state: State<'_, AppState>,
) -> Result<CharacterContentOverrideSnapshotV1, CommandError> {
    let profile = state
        .resources
        .load_game_profile_without_character_overrides(&request.game_profile_id)
        .map_err(product_error)?;
    CharacterDatabase::new(profile)
        .map_err(product_error)?
        .require_character(&request.character_id)
        .map_err(product_error)?;
    state
        .character_content_overrides
        .snapshot(request)
        .map_err(product_error)
}

#[tauri::command]
pub fn save_character_content_override(
    request: SaveCharacterContentOverrideRequestV1,
    state: State<'_, AppState>,
) -> Result<CharacterContentOverrideMutationV1, CommandError> {
    record_native_product_event(
        &state,
        "character-db",
        "character.content_override_save_requested",
    );
    let profile = state
        .resources
        .load_game_profile_without_character_overrides(&request.game_profile_id)
        .map_err(product_error)?;
    state
        .character_content_overrides
        .save(request, &profile)
        .map_err(product_error)
}

#[tauri::command]
pub fn reset_character_content_override(
    request: CharacterContentOverrideScopeV1,
    state: State<'_, AppState>,
) -> Result<CharacterContentOverrideMutationV1, CommandError> {
    record_native_product_event(
        &state,
        "character-db",
        "character.content_override_reset_requested",
    );
    let profile = state
        .resources
        .load_game_profile_without_character_overrides(&request.game_profile_id)
        .map_err(product_error)?;
    CharacterDatabase::new(profile)
        .map_err(product_error)?
        .require_character(&request.character_id)
        .map_err(product_error)?;
    state
        .character_content_overrides
        .reset(request)
        .map_err(product_error)
}

fn selected_character_mouth_authority(
    state: &AppState,
    game_profile_id: &str,
) -> Result<SelectedCharacterMouthAuthorityV1, CommandError> {
    let profile = state
        .resources
        .load_game_profile(game_profile_id)
        .map_err(product_error)?;
    let selected_character_id = state.character_workspace.selected_for_game(game_profile_id);
    SelectedCharacterMouthAuthorityV1::from_persisted_selection(
        &profile,
        selected_character_id.as_deref(),
    )
    .map_err(product_error)
}

#[tauri::command]
pub fn inspect_character_mouth_pack(
    files: CharacterMouthPackFilesV1,
    state: State<'_, AppState>,
) -> Result<CharacterMouthPackPreviewV1, CommandError> {
    let authority = selected_character_mouth_authority(&state, &files.game_profile_id)?;
    state
        .character_mouth_packs
        .inspect(&authority, &files)
        .map_err(product_error)
}

#[tauri::command]
pub fn import_character_mouth_pack(
    request: ImportCharacterMouthPackRequestV1,
    state: State<'_, AppState>,
) -> Result<InstalledCharacterMouthPackV1, CommandError> {
    record_native_product_event(
        &state,
        "character-mouth-pack",
        "mouth-pack.import_requested",
    );
    let authority = selected_character_mouth_authority(&state, &request.files.game_profile_id)?;
    state
        .character_mouth_packs
        .import(&authority, request, epoch_ms())
        .map_err(product_error)
}

#[tauri::command]
pub fn enable_character_mouth_pack(
    request: EnableCharacterMouthPackRequestV1,
    state: State<'_, AppState>,
) -> Result<EnabledCharacterMouthPackV1, CommandError> {
    record_native_product_event(
        &state,
        "character-mouth-pack",
        "mouth-pack.enable_requested",
    );
    let selected = selected_character_mouth_authority(&state, &request.game_profile_id)?;
    let lock = match state.identity_actor_locks.snapshot() {
        NativeActorLockStateV1::Selected(lock) => lock,
        NativeActorLockStateV1::IdentityPackUnqualified
        | NativeActorLockStateV1::QualifiedNoSelection { .. } => {
            return Err(product_error(
                "enabling a mouth pack requires a current qualified or sealed native actor selection",
            ));
        }
    };
    let authority = CurrentCharacterMouthEnableAuthorityV1::from_current_actor(
        selected,
        lock.as_ref(),
        epoch_ms(),
    )
    .map_err(product_error)?;
    state
        .character_mouth_packs
        .enable(&authority, request, epoch_ms())
        .map_err(product_error)
}

#[tauri::command]
pub fn disable_character_mouth_pack(
    request: DisableCharacterMouthPackRequestV1,
    state: State<'_, AppState>,
) -> Result<DisabledCharacterMouthPackV1, CommandError> {
    record_native_product_event(
        &state,
        "character-mouth-pack",
        "mouth-pack.disable_requested",
    );
    let authority = selected_character_mouth_authority(&state, &request.game_profile_id)?;
    state
        .character_mouth_packs
        .disable(&authority, request)
        .map_err(product_error)
}

#[tauri::command]
pub fn character_mouth_pack_state(
    state: State<'_, AppState>,
) -> Result<CharacterMouthPackStateV1, CommandError> {
    state.character_mouth_packs.state().map_err(product_error)
}

#[tauri::command]
pub fn inspect_content_pack(
    request: InspectContentPackRequestV1,
    state: State<'_, AppState>,
) -> Result<ContentPackPreviewV1, CommandError> {
    state
        .content_packs
        .inspect(request, &state.resources)
        .map_err(product_error)
}

#[tauri::command]
pub fn activate_content_pack(
    request: ActivateContentPackRequestV1,
    state: State<'_, AppState>,
) -> Result<ActiveContentPackV1, CommandError> {
    record_native_product_event(&state, "content-packs", "content-pack.activation_requested");
    state
        .content_packs
        .activate(request, &state.resources)
        .map_err(product_error)
}

#[tauri::command]
pub fn content_pack_state(state: State<'_, AppState>) -> Result<ContentPackStateV1, CommandError> {
    state.content_packs.state().map_err(product_error)
}

#[tauri::command]
pub async fn character_memory_status(
    request: CharacterMemoryStatusRequest,
    state: State<'_, AppState>,
) -> Result<CharacterMemoryStatus, CommandError> {
    state
        .character_workspace
        .memory_status(&state.resources, request)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub async fn backup_all_local_memory(
    request: CharacterMemoryBackupRequest,
    state: State<'_, AppState>,
) -> Result<CharacterMemoryBackupResult, CommandError> {
    let result = state
        .character_workspace
        .backup_all_local_memory(request)
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "character-memory",
        result.is_ok(),
        "backup.completed",
        "backup.failed",
        "memory_backup_failed",
    );
    result
}

#[tauri::command]
pub fn list_local_memory_backups(
    state: State<'_, AppState>,
) -> Result<Vec<CharacterMemoryBackupEntry>, CommandError> {
    state
        .character_workspace
        .list_memory_backups()
        .map_err(product_error)
}

#[tauri::command]
pub fn delete_local_memory_backup(
    request: CharacterMemoryBackupDeleteRequest,
    state: State<'_, AppState>,
) -> Result<CharacterMemoryBackupDeleteResult, CommandError> {
    let result = state
        .character_workspace
        .delete_memory_backup(request)
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "character-memory",
        result.is_ok(),
        "backup.delete_completed",
        "backup.delete_failed",
        "memory_backup_delete_failed",
    );
    result
}

#[tauri::command]
pub async fn erase_character_memory(
    request: CharacterMemoryEraseRequest,
    state: State<'_, AppState>,
) -> Result<CharacterMemoryEraseResult, CommandError> {
    let result = state
        .character_workspace
        .erase_character_memory(&state.resources, request)
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "character-memory",
        result.is_ok(),
        "character.erase_completed",
        "character.erase_failed",
        "character_memory_erase_failed",
    );
    result
}

#[tauri::command]
pub async fn restore_local_memory_backup(
    request: CharacterMemoryRestoreRequest,
    state: State<'_, AppState>,
) -> Result<CharacterMemoryRestoreResult, CommandError> {
    if !request.explicit_user_confirmation {
        return Err(product_error(
            "memory restore requires explicit user confirmation",
        ));
    }
    let parsed = uuid::Uuid::parse_str(&request.backup_id)
        .map_err(|_| product_error("memory backup ID is invalid"))?;
    if parsed.to_string() != request.backup_id {
        return Err(product_error(
            "memory backup ID must use canonical lowercase UUID form",
        ));
    }
    let _runtime_storage_guard = state.runtime_storage_gate.lock().await;
    // The runtime host owns the only live writer to this database. Stop it
    // before the workspace performs its validated, atomic replacement; the
    // supervisor will start a fresh child on the next turn.
    state.runtime.shutdown().await;
    let result = state
        .character_workspace
        .restore_local_memory_backup(request)
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "character-memory",
        result.is_ok(),
        "restore.completed",
        "restore.failed",
        "memory_restore_failed",
    );
    result
}

#[tauri::command]
pub async fn remove_all_local_memory(
    request: RemoveAllLocalMemoryRequest,
    state: State<'_, AppState>,
) -> Result<RemoveAllLocalMemoryResult, CommandError> {
    if !request.explicit_user_confirmation {
        return Err(product_error(
            "complete local-memory removal requires explicit user confirmation",
        ));
    }
    let _runtime_storage_guard = state.runtime_storage_gate.lock().await;
    // Complete erasure has the same exclusive-writer requirement as restore.
    // No browser-supplied path or principal participates in the operation.
    state.runtime.shutdown().await;
    let result = state
        .character_workspace
        .remove_all_local_memory(request)
        .await
        .map_err(product_error);
    record_native_product_outcome(
        &state,
        "character-memory",
        result.is_ok(),
        "remove_all.completed",
        "remove_all.failed",
        "memory_remove_all_failed",
    );
    result
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorrectEncounterRequest {
    pub game_profile_id: String,
    pub encounter_id: uuid::Uuid,
    pub character_id: String,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeEncountersRequest {
    pub game_profile_id: String,
    pub source_encounter_id: uuid::Uuid,
    pub destination_encounter_id: uuid::Uuid,
    pub explicit_user_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncounterMutationResult {
    pub schema_version: u32,
    pub game_profile_id: String,
    pub event: EncounterLifecycleEventV1,
    pub memory_migration_required: bool,
    pub memory_migration_performed: bool,
    pub detail: String,
}

#[tauri::command]
pub fn correct_encounter_to_authored_character(
    request: CorrectEncounterRequest,
    state: State<'_, AppState>,
) -> Result<EncounterMutationResult, CommandError> {
    record_native_product_event(&state, "character-db", "encounter.correction_requested");
    let profile = state
        .resources
        .load_game_profile(&request.game_profile_id)
        .map_err(product_error)?;
    let database = CharacterDatabase::new(profile).map_err(product_error)?;
    let event = state
        .runtime
        .correct_encounter(
            &database,
            request.encounter_id,
            &request.character_id,
            request.explicit_user_confirmation,
        )
        .map_err(product_error)?;
    encounter_mutation_result(request.game_profile_id, event)
}

#[tauri::command]
pub fn merge_unknown_encounters(
    request: MergeEncountersRequest,
    state: State<'_, AppState>,
) -> Result<EncounterMutationResult, CommandError> {
    record_native_product_event(&state, "character-db", "encounter.merge_requested");
    let profile = state
        .resources
        .load_game_profile(&request.game_profile_id)
        .map_err(product_error)?;
    CharacterDatabase::new(profile).map_err(product_error)?;
    let event = state
        .runtime
        .merge_encounters(
            &request.game_profile_id,
            request.source_encounter_id,
            request.destination_encounter_id,
            request.explicit_user_confirmation,
        )
        .map_err(product_error)?;
    encounter_mutation_result(request.game_profile_id, event)
}

fn encounter_mutation_result(
    game_profile_id: String,
    event: EncounterLifecycleEventV1,
) -> Result<EncounterMutationResult, CommandError> {
    let memory_migration_required = match &event {
        EncounterLifecycleEventV1::CorrectedToAuthoredCharacter {
            memory_migration_required,
            ..
        }
        | EncounterLifecycleEventV1::MergedIntoEncounter {
            memory_migration_required,
            ..
        } => *memory_migration_required,
        _ => false,
    };
    if !memory_migration_required {
        return Err(CommandError::Product {
            message: "encounter mutation did not require an explicit memory migration".into(),
        });
    }
    Ok(EncounterMutationResult {
        schema_version: 1,
        game_profile_id,
        event,
        memory_migration_required: true,
        memory_migration_performed: false,
        detail: "The trusted native encounter lifecycle was updated. Memory remains in its original authority scope until a separate receipt-backed migration is implemented and explicitly confirmed.".into(),
    })
}

#[tauri::command]
pub fn local_resource_settings(
    state: State<'_, AppState>,
) -> Result<LocalResourceSettings, CommandError> {
    state.local_resources.settings().map_err(product_error)
}

#[tauri::command]
pub fn save_local_resource_settings(
    settings: LocalResourceSettings,
    state: State<'_, AppState>,
) -> Result<LocalResourceSettings, CommandError> {
    let settings = state
        .local_resources
        .save_settings(settings)
        .map_err(product_error)?;
    let revocation = state.local_resources.revoke_active_loadout_admission();
    state.identity_runtime.invalidate_and_reconcile_background();
    revocation.map_err(product_error)?;
    Ok(settings)
}

#[tauri::command]
pub fn local_resource_telemetry(
    state: State<'_, AppState>,
) -> Result<ResourceTelemetryResult, CommandError> {
    record_native_product_event(&state, "resource-governor", "telemetry.requested");
    state
        .local_resources
        .telemetry(state.game_targets.selected_process_id())
        .map_err(product_error)
}

#[tauri::command]
pub fn selected_local_loadout_planner(
    state: State<'_, AppState>,
) -> Result<SelectedLoadoutPlannerResult, CommandError> {
    state
        .local_resources
        .selected_loadout_planner()
        .map_err(product_error)
}

#[tauri::command]
pub fn trusted_local_pack_catalog(
    state: State<'_, AppState>,
) -> Result<TrustedLocalPackCatalogResult, CommandError> {
    state
        .local_resources
        .trusted_pack_catalog()
        .map_err(product_error)
}

#[tauri::command]
pub async fn trusted_optional_pack_lifecycle(
    state: State<'_, AppState>,
) -> Result<TrustedOptionalPackLifecycleResultV1, CommandError> {
    state
        .local_resources
        .trusted_optional_pack_lifecycle()
        .await
        .map_err(product_error)
}

#[tauri::command]
pub async fn install_trusted_optional_pack(
    request: TrustedOptionalPackMutationRequestV1,
    state: State<'_, AppState>,
) -> Result<TrustedOptionalPackLifecycleResultV1, CommandError> {
    record_native_product_event(&state, "model-pack", "optional.install_requested");
    let result = state
        .local_resources
        .install_trusted_optional_pack(request)
        .await
        .map_err(product_error)?;
    state.identity_runtime.invalidate_and_reconcile_background();
    Ok(result)
}

#[tauri::command]
pub async fn activate_trusted_optional_pack(
    request: TrustedOptionalPackActivationRequestV1,
    state: State<'_, AppState>,
) -> Result<TrustedOptionalPackActivationResultV1, CommandError> {
    record_native_product_event(&state, "model-pack", "optional.activation_requested");
    let result = state
        .local_resources
        .activate_trusted_optional_pack(request, &state.visual_runtime)
        .await;
    state.identity_runtime.invalidate_and_reconcile_background();
    result.map_err(product_error)
}

#[tauri::command]
pub fn cancel_trusted_optional_pack_download(
    request: TrustedOptionalPackMutationRequestV1,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .local_resources
        .cancel_trusted_optional_pack_download(request)
        .map_err(product_error)
}

#[tauri::command]
pub async fn repair_trusted_optional_pack(
    request: TrustedOptionalPackMutationRequestV1,
    state: State<'_, AppState>,
) -> Result<TrustedOptionalPackLifecycleResultV1, CommandError> {
    record_native_product_event(&state, "model-pack", "optional.repair_requested");
    let result = state
        .local_resources
        .repair_trusted_optional_pack(request)
        .await
        .map_err(product_error)?;
    state.identity_runtime.invalidate_and_reconcile_background();
    Ok(result)
}

#[tauri::command]
pub async fn remove_trusted_optional_pack(
    request: TrustedOptionalPackMutationRequestV1,
    state: State<'_, AppState>,
) -> Result<TrustedOptionalPackLifecycleResultV1, CommandError> {
    record_native_product_event(&state, "model-pack", "optional.remove_requested");
    let result = state
        .local_resources
        .remove_trusted_optional_pack(request)
        .await
        .map_err(product_error)?;
    state.identity_runtime.invalidate_and_reconcile_background();
    Ok(result)
}

#[tauri::command]
pub fn admit_selected_local_loadout(
    selection: SelectedLoadoutSelectionV1,
    state: State<'_, AppState>,
) -> Result<SelectedLoadoutAdmissionResult, CommandError> {
    record_native_product_event(&state, "resource-governor", "loadout.admission_requested");
    let exact_target_pid = state
        .game_targets
        .revalidated_snapshot(&state.resources)
        .map_err(product_error)?
        .map(|selection| selection.target.process_id);
    let result = state
        .local_resources
        .admit_selected_loadout(selection, exact_target_pid)
        .map_err(product_error)?;
    state.identity_runtime.invalidate_and_reconcile_background();
    Ok(result)
}

#[tauri::command]
pub fn experimental_visual_pack_status(
    state: State<'_, AppState>,
) -> Result<ExperimentalPackState, CommandError> {
    state.local_resources.pack_state().map_err(product_error)
}

#[tauri::command]
pub async fn install_experimental_model_pack(
    request: ExperimentalPackMutationRequest,
    state: State<'_, AppState>,
) -> Result<ExperimentalPackState, CommandError> {
    record_native_product_event(&state, "model-pack", "install.requested");
    state
        .local_resources
        .install(request)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub fn activate_experimental_model_pack(
    request: ExperimentalPackMutationRequest,
    state: State<'_, AppState>,
) -> Result<ExperimentalPackState, CommandError> {
    record_native_product_event(&state, "model-pack", "activation.requested");
    state
        .local_resources
        .activate(request, state.game_targets.selected_process_id())
        .map_err(product_error)
}

#[tauri::command]
pub async fn repair_model_pack(
    request: ExperimentalPackMutationRequest,
    state: State<'_, AppState>,
) -> Result<ExperimentalPackState, CommandError> {
    record_native_product_event(&state, "model-pack", "repair.requested");
    state
        .local_resources
        .repair(request)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub async fn remove_model_pack(
    request: ExperimentalPackMutationRequest,
    state: State<'_, AppState>,
) -> Result<ExperimentalPackState, CommandError> {
    record_native_product_event(&state, "model-pack", "removal.requested");
    state
        .local_resources
        .remove(request)
        .await
        .map_err(product_error)
}

#[tauri::command]
pub fn diagnostics_v2_snapshot(
    max_events: usize,
    state: State<'_, AppState>,
) -> Result<DiagnosticsV2Snapshot, CommandError> {
    state
        .diagnostics_v2
        .snapshot(max_events)
        .map_err(product_error)
}

#[tauri::command]
pub fn diagnostics_v2_settings(
    state: State<'_, AppState>,
) -> Result<DiagnosticsSettingsV1, CommandError> {
    state.diagnostics_v2.settings().map_err(product_error)
}

#[tauri::command]
pub fn save_diagnostics_v2_settings(
    settings: DiagnosticsSettingsV1,
    state: State<'_, AppState>,
) -> Result<DiagnosticsSettingsV1, CommandError> {
    state
        .diagnostics_v2
        .save_settings(settings)
        .map_err(product_error)
}

#[tauri::command]
pub async fn diagnostics_v2_matrix(
    state: State<'_, AppState>,
) -> Result<DiagnosticMatrixResult, CommandError> {
    let mut builder = DiagnosticMatrixBuilder::new();
    // Correlation is native-owned. A WebView caller cannot attach measured
    // diagnostics to an arbitrary or previously completed turn identifier.
    if let Some(turn_id) = state.runtime.snapshot().simulation_id {
        builder = builder.correlated_turn_id(turn_id);
    }
    let observed_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(product_error)?;
    let credential_rows = provider_summaries(state.credentials.as_ref())
        .into_iter()
        .filter(|provider| provider.credential_reference.is_some())
        .map(|provider| ProviderCredentialPresence {
            provider_id: provider.provider_id,
            state: match provider.status {
                CredentialReferenceStatus::Present => CredentialPresenceState::Present,
                CredentialReferenceStatus::Missing => CredentialPresenceState::Absent,
                CredentialReferenceStatus::Unavailable => CredentialPresenceState::Unavailable,
                CredentialReferenceStatus::NotRequired => CredentialPresenceState::Unknown,
            },
            provenance: ObservationProvenance::Measured,
            observed_at_utc: Some(observed_at.clone()),
        })
        .collect::<Vec<_>>();
    let credential_status = if credential_rows
        .iter()
        .any(|row| row.state == CredentialPresenceState::Unavailable)
    {
        DiagnosticStatus::Failed
    } else if credential_rows
        .iter()
        .any(|row| row.state == CredentialPresenceState::Absent)
    {
        DiagnosticStatus::Degraded
    } else {
        DiagnosticStatus::Ok
    };
    for row in credential_rows {
        builder
            .push_credential_presence(row)
            .map_err(product_error)?;
    }
    builder
        .push_check(measured_matrix_check(
            "credentials.providers",
            CheckCategory::Credential,
            credential_status,
            "credentials.providers.presence_checked",
            "Credential values were not read or exported; native storage returned presence state only.",
            "providers",
        ))
        .map_err(product_error)?;

    let selected_output = state.media_broker.selected_audio_output().await;
    let (speaker_status, speaker_code, speaker_summary) = match selected_output {
        Ok(Some(_)) => (
            DiagnosticStatus::Ok,
            "audio.speaker.selection_resolved",
            "The persisted render-output policy resolved to a current native endpoint; this is not a playback or audibility test.",
        ),
        Ok(None) => (
            DiagnosticStatus::Degraded,
            "audio.speaker.selection_required",
            "No render-output policy is selected. Choose System default or an exact endpoint before hosted playback.",
        ),
        Err(_) => (
            DiagnosticStatus::Failed,
            "audio.speaker.enumeration_failed",
            "The native audio broker could not resolve the selected render output.",
        ),
    };
    builder
        .push_check(measured_matrix_check(
            "audio.speaker",
            CheckCategory::Speaker,
            speaker_status,
            speaker_code,
            speaker_summary,
            "speaker",
        ))
        .map_err(product_error)?;

    let selected_input = state.media_broker.selected_audio_input().await;
    let (microphone_status, microphone_code, microphone_summary) = match selected_input {
        Ok(Some(_)) => (
            DiagnosticStatus::Degraded,
            "audio.microphone.selection_resolved_capture_unmeasured",
            "The persisted capture-input policy resolved to a current native endpoint; permission, audible input, and transcription remain unmeasured until a broker receipt completes.",
        ),
        Ok(None) => (
            DiagnosticStatus::Degraded,
            "audio.microphone.selection_required",
            "No capture-input policy is selected. Choose System default or an exact endpoint before push-to-talk.",
        ),
        Err(_) => (
            DiagnosticStatus::Failed,
            "audio.microphone.enumeration_failed",
            "The native audio broker could not resolve the selected capture input.",
        ),
    };
    builder
        .push_check(measured_matrix_check(
            "audio.microphone",
            CheckCategory::Microphone,
            microphone_status,
            microphone_code,
            microphone_summary,
            "microphone",
        ))
        .map_err(product_error)?;

    let game_selected = state
        .game_targets
        .revalidated_snapshot(&state.resources)
        .map_err(product_error)?
        .is_some();
    builder
        .push_check(measured_matrix_check(
            "game.target",
            CheckCategory::Game,
            if game_selected {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Degraded
            },
            if game_selected {
                "game.target.selection_present"
            } else {
                "game.target.selection_required"
            },
            if game_selected {
                "A native game-target selection exists; capture and safety qualification remain separate checks."
            } else {
                "No exact game process/window target is selected."
            },
            "game",
        ))
        .map_err(product_error)?;

    if let Ok(broker) = state.media_broker.diagnostics().await {
        builder
            .push_check(measured_matrix_check(
                "media.capture",
                CheckCategory::Capture,
                if matches!(
                    broker.capture_backend.as_str(),
                    "windowsGraphicsCapture" | "desktopDuplication"
                ) {
                    DiagnosticStatus::Ok
                } else {
                    DiagnosticStatus::Degraded
                },
                "media.capture.broker_observed",
                "The native broker reported its current capture backend; frame freshness is not inferred when no advancing receipt exists.",
                "capture",
            ))
            .map_err(product_error)?;
        builder
            .push_check(measured_matrix_check(
                "media.overlay",
                CheckCategory::Overlay,
                if broker.overlay_backend == "d3d11DirectComposition" {
                    DiagnosticStatus::Ok
                } else {
                    DiagnosticStatus::Degraded
                },
                "media.overlay.broker_observed",
                "The native broker reported its current overlay backend; no in-game presentation claim is inferred.",
                "overlay",
            ))
            .map_err(product_error)?;
    }

    let packs = state
        .local_resources
        .trusted_pack_catalog()
        .map_err(product_error)?;
    builder
        .push_check(measured_matrix_check(
            "models.pack_integrity",
            CheckCategory::ModelPack,
            if packs.ready {
                DiagnosticStatus::Ok
            } else {
                DiagnosticStatus::Degraded
            },
            if packs.ready {
                "models.pack_integrity.signed_catalog_verified"
            } else {
                "models.pack_integrity.catalog_unavailable"
            },
            &packs.detail,
            "models",
        ))
        .map_err(product_error)?;
    builder.fill_unmeasured();
    let result = builder.build().map_err(product_error)?;
    state
        .diagnostics_v2
        .record_matrix(&result)
        .map_err(product_error)?;
    Ok(result)
}

fn measured_matrix_check(
    check_id: &str,
    category: CheckCategory,
    status: DiagnosticStatus,
    summary_code: &str,
    summary: &str,
    settings_target: &str,
) -> CheckResult {
    CheckResult {
        check_id: check_id.into(),
        category,
        status,
        provenance: ObservationProvenance::Measured,
        observed_at_utc: Some(
            OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into()),
        ),
        duration_ms: None,
        timing: None,
        summary_code: summary_code.into(),
        summary: summary.into(),
        error_code: None,
        provider_id: None,
        model_id: None,
        metrics: BTreeMap::new(),
        suggested_actions: vec![SuggestedAction {
            action_id: format!("remediate.{check_id}"),
            kind: SuggestedActionKind::OpenSettingsSection,
            label: "Review this diagnostic in Settings".into(),
            target_id: Some(settings_target.into()),
            requires_confirmation: false,
        }],
    }
}

#[tauri::command]
pub fn export_diagnostics_v2(
    request: DiagnosticsExportRequest,
    state: State<'_, AppState>,
) -> Result<DiagnosticsExportResult, CommandError> {
    state.diagnostics_v2.export(request).map_err(product_error)
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartThisPcBenchmarkRequest {
    pub requested_iterations: u16,
    pub timeout_millis: u64,
    pub baseline_window_millis: u64,
}

#[tauri::command]
pub fn start_this_pc_benchmark(
    request: StartThisPcBenchmarkRequest,
    state: State<'_, AppState>,
) -> Result<BenchmarkStatusV1, CommandError> {
    record_native_product_event(&state, "benchmark", "run.requested");
    let selected = state
        .game_targets
        .revalidated_snapshot(&state.resources)
        .map_err(product_error)?
        .ok_or_else(|| CommandError::Product {
            message: "This-PC benchmark requires an exact selected running game target".into(),
        })?;
    let context = LoadoutContextV1::game(&selected.game_profile_id);
    let resolved =
        state
            .provider_loadouts
            .resolve_for_turn(context, false, &state.local_resources)?;
    let resolved_bytes = serde_json::to_vec(&resolved).map_err(product_error)?;
    let loadout_revision = format!("sha256-{:x}", Sha256::digest(&resolved_bytes));
    let mut provider_routes = resolved
        .roles
        .iter()
        .map(|(role, route)| {
            let role = match role {
                npc_provider_loadouts::ProviderRole::Stt => BenchmarkProviderRole::Stt,
                npc_provider_loadouts::ProviderRole::Llm => BenchmarkProviderRole::Llm,
                npc_provider_loadouts::ProviderRole::Tts => BenchmarkProviderRole::Tts,
                npc_provider_loadouts::ProviderRole::Embeddings => BenchmarkProviderRole::Embedding,
                npc_provider_loadouts::ProviderRole::Vision => BenchmarkProviderRole::VisualSignal,
                npc_provider_loadouts::ProviderRole::Lipsync => {
                    BenchmarkProviderRole::MouthAnimation
                }
            };
            let primary = &route.primary;
            ProviderRouteBindingV1 {
                role,
                provider_id: primary.provider_id.clone(),
                model_id: primary.model_id.clone(),
                voice_id: primary.voice_id.clone(),
                route_revision: format!("catalog-{}", primary.disclosure.catalog_revision),
                egress: canonical_benchmark_egress(&primary.disclosure),
                execution_mode: if primary.provider_id.starts_with("mock-") {
                    EvidenceExecutionMode::Mocked
                } else {
                    EvidenceExecutionMode::Live
                },
            }
        })
        .collect::<Vec<_>>();
    provider_routes.sort_by_key(|route| route.role);
    let benchmark_request = BenchmarkRunRequestV1 {
        schema_version: BENCHMARK_REQUEST_SCHEMA_V1.into(),
        requested_iterations: request.requested_iterations,
        timeout_millis: request.timeout_millis,
        baseline_window_millis: request.baseline_window_millis,
        binding: ProductBindingV1 {
            selected_game_pid: Some(selected.target.process_id),
            game_profile_id: selected.game_profile_id,
            executable_sha256: selected.target.executable_path_sha256,
            target_instance_recorded: false,
            loadout_revision,
            provider_routes,
        },
    };
    state
        .product_benchmark
        .start(benchmark_request)
        .map_err(product_error)
}

fn canonical_benchmark_egress(disclosure: &npc_provider_loadouts::CatalogDisclosureV1) -> String {
    let class = serde_json::to_string(&disclosure.egress)
        .unwrap_or_else(|_| "\"invalid\"".into())
        .trim_matches('"')
        .to_owned();
    let transmitted = if disclosure.transmitted_data.is_empty() {
        "none".into()
    } else {
        disclosure
            .transmitted_data
            .iter()
            .map(|value| {
                serde_json::to_string(value)
                    .unwrap_or_else(|_| "\"invalid\"".into())
                    .trim_matches('"')
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join(".")
    };
    format!("{class}:{transmitted}")
}

#[tauri::command]
pub fn cancel_this_pc_benchmark(
    state: State<'_, AppState>,
) -> Result<BenchmarkStatusV1, CommandError> {
    state.product_benchmark.cancel().map_err(product_error)
}

#[tauri::command]
pub fn this_pc_benchmark_status(
    state: State<'_, AppState>,
) -> Result<BenchmarkStatusV1, CommandError> {
    state.product_benchmark.status().map_err(product_error)
}

#[tauri::command]
pub fn this_pc_benchmark_report(
    report_id: String,
    state: State<'_, AppState>,
) -> Result<BenchmarkReportV1, CommandError> {
    state
        .product_benchmark
        .report(&report_id)
        .map_err(product_error)
}

#[cfg(debug_assertions)]
fn map_debug_synthetic_capture_error(error: crate::media_broker::MediaBrokerError) -> CommandError {
    match error {
        crate::media_broker::MediaBrokerError::DebugSyntheticTargetInvalid
        | crate::media_broker::MediaBrokerError::DebugSyntheticTargetMismatch
        | crate::media_broker::MediaBrokerError::DebugSyntheticMetadataInvalid => {
            CommandError::InvalidRequest {
                message: error.to_string(),
            }
        }
        _ => CommandError::Runtime {
            message: error.to_string(),
        },
    }
}

/// Debug-only control surface used by the task-owned synthetic replay player.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn debug_select_synthetic_replay_capture_target(
    state: State<'_, AppState>,
) -> Result<crate::media_broker::DebugSyntheticReplayCaptureSnapshot, CommandError> {
    select_synthetic_replay_capture_target(&state).await
}

#[cfg(debug_assertions)]
async fn select_synthetic_replay_capture_target(
    state: &AppState,
) -> Result<crate::media_broker::DebugSyntheticReplayCaptureSnapshot, CommandError> {
    let snapshot = state
        .media_broker
        .debug_select_synthetic_replay_capture_target()
        .await
        .map_err(map_debug_synthetic_capture_error)?;
    let selected = state.game_targets.select(
        &state.resources,
        SelectGameTargetRequest {
            game_profile_id: "eclipse-harbor".into(),
            native_window_hint: Some(snapshot.target_window_handle),
            explicit_user_confirmed_offline_single_player: true,
        },
    );
    let selected = match selected {
        Ok(selected) => selected,
        Err(error) => {
            let _ = state
                .media_broker
                .debug_clear_synthetic_replay_capture_target()
                .await;
            return Err(product_error(error));
        }
    };
    let revocation = state.local_resources.revoke_active_loadout_admission();
    state.identity_runtime.invalidate_and_reconcile_background();
    revocation.map_err(product_error)?;
    if selected.target.process_id != snapshot.target_process_id
        || selected.target.native_window != snapshot.target_window_handle
        || !selected
            .target
            .executable_name
            .eq_ignore_ascii_case(&snapshot.target_executable_basename)
    {
        let _ = state
            .media_broker
            .debug_clear_synthetic_replay_capture_target()
            .await;
        return Err(product_error(
            "synthetic game selection did not match the broker-bound target",
        ));
    }
    state
        .game_targets
        .mark_debug_synthetic_broker_bound(
            snapshot.target_process_id,
            snapshot.target_window_handle,
            &snapshot.target_executable_basename,
        )
        .map_err(product_error)?;
    Ok(snapshot)
}

#[cfg(debug_assertions)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSyntheticReviewTarget {
    pub schema_version: u32,
    pub launched: bool,
    pub executable_path: String,
    pub target_process_id: u32,
    pub target_window_handle: u64,
    pub target_executable_basename: String,
    pub fixture_motion_mode: String,
}

/// Resolves and starts only the manifest-attested, co-located local-review
/// target, then binds the exact process/window through the native broker.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn prepare_synthetic_review_target(
    state: State<'_, AppState>,
) -> Result<PreparedSyntheticReviewTarget, CommandError> {
    let executable_path = state
        .synthetic_review_target
        .executable_path()
        .map_err(product_error)?;
    if let Ok(snapshot) = select_synthetic_replay_capture_target(&state).await {
        return Ok(PreparedSyntheticReviewTarget {
            schema_version: 1,
            launched: false,
            executable_path: executable_path.to_string_lossy().into_owned(),
            target_process_id: snapshot.target_process_id,
            target_window_handle: snapshot.target_window_handle,
            target_executable_basename: snapshot.target_executable_basename,
            fixture_motion_mode: snapshot.fixture_motion_mode,
        });
    }

    let mut launched = state
        .synthetic_review_target
        .launch()
        .map_err(product_error)?;
    let launched_process_id = launched.child.id();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match select_synthetic_replay_capture_target(&state).await {
            Ok(snapshot) if snapshot.target_process_id == launched_process_id => {
                return Ok(PreparedSyntheticReviewTarget {
                    schema_version: 1,
                    launched: true,
                    executable_path: launched.executable_path.to_string_lossy().into_owned(),
                    target_process_id: snapshot.target_process_id,
                    target_window_handle: snapshot.target_window_handle,
                    target_executable_basename: snapshot.target_executable_basename,
                    fixture_motion_mode: snapshot.fixture_motion_mode,
                });
            }
            Ok(_) | Err(_) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = launched.child.kill();
            let _ = launched.child.wait();
            return Err(CommandError::Runtime {
                message: "the manifest-attested synthetic target did not publish metadata for its own capturable PID/HWND within 10 seconds".into(),
            });
        }
        if launched.child.try_wait().map_err(product_error)?.is_some() {
            return Err(CommandError::Runtime {
                message: "the synthetic target exited before its window became capturable".into(),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Debug-only receipt probe for qualifying the project-owned review path.
/// It exposes no handles, pixels, model paths, audio, or credentials.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn debug_latest_visual_presentation_receipt(
    state: State<'_, AppState>,
) -> Option<crate::visual_runtime::VisualPresentationReceipt> {
    state.visual_coordinator.latest_receipt()
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn debug_visual_presentation_receipt_history(
    state: State<'_, AppState>,
) -> Vec<crate::visual_runtime::VisualPresentationReceipt> {
    state.visual_coordinator.receipt_history()
}

#[tauri::command]
pub fn provider_credential_status(
    provider_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderCredentialSummary>, CommandError> {
    if let Some(id) = provider_id.as_deref() {
        if !is_known_provider(id) {
            return Err(CommandError::InvalidRequest {
                message: format!("unknown provider identifier `{id}`"),
            });
        }
    }
    let mut providers = provider_summaries(state.credentials.as_ref());
    if let Some(id) = provider_id {
        providers.retain(|provider| provider.provider_id == id);
    }
    Ok(providers)
}

#[tauri::command]
pub async fn prompt_and_save_provider_credential(
    provider_id: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<CredentialPromptSaveResult, CommandError> {
    let owner = native_window_owner(&window)?;
    let credentials = Arc::clone(&state.credentials);
    let prompt = Arc::clone(&state.credential_prompt);
    tauri::async_runtime::spawn_blocking(move || {
        prompt_and_save_impl(&provider_id, owner, credentials.as_ref(), prompt.as_ref())
    })
    .await
    .map_err(|_| CommandError::Credential {
        message: "native credential prompt task failed".into(),
    })?
}

#[tauri::command]
pub fn delete_provider_credential(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<ProviderCredentialSummary, CommandError> {
    let reference = hosted_credential_reference(&provider_id)?;
    state
        .credentials
        .delete(&reference)
        .map_err(|error| CommandError::Credential {
            message: error.to_string(),
        })?;
    provider_summary(state.credentials.as_ref(), &provider_id)
}

#[tauri::command]
pub async fn test_provider_connection(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<ProviderConnectionTestResult, CommandError> {
    let reference = hosted_credential_reference(&provider_id)?;
    let credential_status = state.credentials.status(&reference);
    if credential_status != crate::domain::CredentialReferenceStatus::Present {
        return Ok(ProviderConnectionTestResult {
            provider_id,
            outcome: ProviderConnectionOutcome::NeedsCredential,
            credential_status,
            runtime_connected: state.runtime.supervisor().health().connected,
            provider_contract_available: false,
            network_request_performed: false,
            response_body_returned: false,
            detail:
                "A saved credential reference is required before provider readiness can be checked."
                    .into(),
        });
    }
    let doctor =
        match state.runtime.supervisor().doctor().await {
            Ok(report) => report,
            Err(_) => return Ok(ProviderConnectionTestResult {
                provider_id,
                outcome: ProviderConnectionOutcome::RuntimeUnavailable,
                credential_status,
                runtime_connected: false,
                provider_contract_available: false,
                network_request_performed: false,
                response_body_returned: false,
                detail:
                    "The trusted runtime is unavailable, so provider readiness was not attempted."
                        .into(),
            }),
        };
    let provider_contract_available = doctor
        .hosted_provider_contracts
        .iter()
        .any(|contract| contract.split([':', ',']).any(|id| id == provider_id));
    Ok(ProviderConnectionTestResult {
        provider_id,
        outcome: if provider_contract_available {
            ProviderConnectionOutcome::ReadyForConnection
        } else {
            ProviderConnectionOutcome::ProviderContractUnavailable
        },
        credential_status,
        runtime_connected: true,
        provider_contract_available,
        network_request_performed: false,
        response_body_returned: false,
        detail: if provider_contract_available {
            "Credential presence and at least one runtime role contract for this provider are ready. This does not qualify every role, model, or voice; exact loadout review remains authoritative. No content-bearing network request was made.".into()
        } else {
            "The runtime does not currently advertise this provider contract. No network request was made.".into()
        },
    })
}

fn hosted_credential_reference(provider_id: &str) -> Result<String, CommandError> {
    if !is_known_provider(provider_id) {
        return Err(CommandError::InvalidRequest {
            message: format!("unknown provider identifier `{provider_id}`"),
        });
    }
    credential_reference_for(provider_id).ok_or_else(|| CommandError::InvalidRequest {
        message: "credentials are only accepted for allowlisted hosted providers".into(),
    })
}

fn native_window_owner(window: &tauri::Window) -> Result<NativeWindowOwner, CommandError> {
    #[cfg(windows)]
    {
        window
            .hwnd()
            .map(|handle| NativeWindowOwner(handle.0 as usize))
            .map_err(|_| CommandError::Credential {
                message: "native credential prompt could not attach to the application window"
                    .into(),
            })
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(NativeWindowOwner(0))
    }
}

fn prompt_and_save_impl(
    provider_id: &str,
    owner: NativeWindowOwner,
    credentials: &dyn CredentialPresence,
    prompt: &dyn CredentialPrompt,
) -> Result<CredentialPromptSaveResult, CommandError> {
    let reference = hosted_credential_reference(provider_id)?;
    let provider_name =
        credential_provider_name(provider_id).ok_or_else(|| CommandError::InvalidRequest {
            message: "credentials are only accepted for allowlisted hosted providers".into(),
        })?;
    match prompt
        .prompt(owner, provider_id, &provider_name)
        .map_err(|error| CommandError::Credential {
            message: error.to_string(),
        })?
    {
        NativePromptOutcome::Submitted(secret) => {
            credentials
                .save(&reference, &secret)
                .map_err(|error| CommandError::Credential {
                    message: error.to_string(),
                })?;
            drop(secret);
            Ok(CredentialPromptSaveResult {
                provider_id: provider_id.into(),
                outcome: CredentialPromptOutcome::Saved,
                credential_status: credentials.status(&reference),
                detail: "Credential saved directly to Windows Credential Manager.".into(),
            })
        }
        NativePromptOutcome::Cancelled => Ok(CredentialPromptSaveResult {
            provider_id: provider_id.into(),
            outcome: CredentialPromptOutcome::Cancelled,
            credential_status: credentials.status(&reference),
            detail: "Native credential entry was cancelled; the existing credential was unchanged."
                .into(),
        }),
        NativePromptOutcome::DevelopmentFixtureOnly => Ok(CredentialPromptSaveResult {
            provider_id: provider_id.into(),
            outcome: CredentialPromptOutcome::DevelopmentFixtureOnly,
            credential_status: crate::domain::CredentialReferenceStatus::Unavailable,
            detail: "Native credential entry is fixture-only on this non-Windows development build; no value was requested or stored.".into(),
        }),
    }
}

fn provider_summary(
    presence: &dyn CredentialPresence,
    provider_id: &str,
) -> Result<ProviderCredentialSummary, CommandError> {
    provider_summaries(presence)
        .into_iter()
        .find(|provider| provider.provider_id == provider_id)
        .ok_or_else(|| CommandError::InvalidRequest {
            message: "provider is not available".into(),
        })
}

#[tauri::command]
pub fn diagnostic_summary(state: State<'_, AppState>) -> DiagnosticSummary {
    build_diagnostics(&state)
}

fn build_bootstrap(state: &AppState) -> BootstrapSnapshot {
    let (onboarding, onboarding_persistence) = state.onboarding_snapshot();
    let runtime = state.runtime.supervisor().health();
    let media_broker = state.media_broker.health();
    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "deterministicSimulation".into(),
        runtime.state == RuntimeConnectionState::DevelopmentFixture,
    );
    capabilities.insert("nativeRuntimeConnected".into(), runtime.connected);
    capabilities.insert("mediaBrokerConnected".into(), media_broker.connected);
    capabilities.insert("providerCredentialReferences".into(), cfg!(windows));
    capabilities.insert("generalFilesystemAccess".into(), false);
    capabilities.insert("shellExecution".into(), false);
    capabilities.insert("screenCapture".into(), false);
    capabilities.insert("microphoneCapture".into(), false);
    capabilities.insert("debugSyntheticReplayCapture".into(), cfg!(debug_assertions));
    capabilities.insert(
        "debugSyntheticReviewTargetLaunch".into(),
        cfg!(debug_assertions),
    );

    BootstrapSnapshot {
        contract_version: CONTROL_CONTRACT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        onboarding,
        onboarding_persistence,
        simulation: state.runtime.snapshot(),
        runtime,
        media_broker,
        providers: provider_summaries(state.credentials.as_ref()),
        game_profiles: state.resources.game_summaries(),
        models: model_summaries(),
        diagnostics: build_diagnostics(state),
        safety: SafetyBoundary::default(),
        capabilities,
    }
}

fn build_diagnostics(state: &AppState) -> DiagnosticSummary {
    let (_, persistence) = state.onboarding_snapshot();
    let runtime = state.runtime.supervisor().health();
    let media_broker = state.media_broker.health();
    let profiles = state.resources.game_summaries();
    let bundled_profiles = profiles
        .iter()
        .filter(|profile| profile.catalog_state == CatalogState::Bundled)
        .count();
    let persistence_status = match persistence.health {
        PersistenceHealth::Healthy | PersistenceHealth::FirstRun => CheckStatus::Passed,
        PersistenceHealth::RecoveredFromInvalidFile => CheckStatus::Warning,
        PersistenceHealth::Unavailable => CheckStatus::Warning,
    };
    let credential_status = if cfg!(windows) {
        CheckStatus::Passed
    } else {
        CheckStatus::Informational
    };
    let profile_status = if bundled_profiles == 20 {
        CheckStatus::Passed
    } else {
        CheckStatus::Warning
    };
    let overall =
        if matches!(persistence.health, PersistenceHealth::Unavailable) || bundled_profiles == 0 {
            DiagnosticOverall::Degraded
        } else {
            DiagnosticOverall::ReadyForSimulation
        };

    DiagnosticSummary {
        overall,
        generated_at_epoch_ms: epoch_ms(),
        measurements: MeasurementStatus {
            state: "notCaptured".into(),
            reason: "No controlled benchmark has run. Deterministic fixture timings are illustrative estimates, not measurements from this PC.".into(),
            current_results_are_release_evidence: false,
        },
        checks: vec![
            DiagnosticCheck {
                id: "control-boundary".into(),
                status: CheckStatus::Passed,
                title: "Restricted WebView boundary".into(),
                detail: "The Response Console has no shell command or general filesystem capability.".into(),
                remediation: None,
            },
            DiagnosticCheck {
                id: "onboarding-persistence".into(),
                status: persistence_status,
                title: "Onboarding persistence".into(),
                detail: persistence.detail,
                remediation: matches!(persistence_status, CheckStatus::Warning)
                    .then(|| "Review app-data permissions, then save onboarding settings again.".into()),
            },
            DiagnosticCheck {
                id: "profile-bundle".into(),
                status: profile_status,
                title: "Game profile bundle".into(),
                detail: format!("{bundled_profiles} of 20 locked game profiles are present in this build."),
                remediation: (bundled_profiles != 20).then(|| "Complete and validate missing profile files before an RC build.".into()),
            },
            DiagnosticCheck {
                id: "credential-vault".into(),
                status: credential_status,
                title: "Credential reference boundary".into(),
                detail: state.credentials.availability_detail().into(),
                remediation: (!cfg!(windows)).then(|| "Run the packaged application on supported Windows 10 or Windows 11.".into()),
            },
            DiagnosticCheck {
                id: "runtime-bridge".into(),
                status: match runtime.state {
                    RuntimeConnectionState::Ready => CheckStatus::Passed,
                    RuntimeConnectionState::DevelopmentFixture => CheckStatus::Informational,
                    _ => CheckStatus::Warning,
                },
                title: "Trusted runtime bridge".into(),
                detail: runtime.detail.clone(),
                remediation: (runtime.state != RuntimeConnectionState::Ready).then(|| {
                    if runtime.state == RuntimeConnectionState::DevelopmentFixture {
                        "The fixture is only for unbundled UI development; package npc-runtime before release testing.".into()
                    } else {
                        "Review runtime health and restart the application after resolving a missing or quarantined sidecar.".into()
                    }
                }),
            },
            DiagnosticCheck {
                id: "media-broker".into(),
                status: match media_broker.state {
                    RuntimeConnectionState::Ready => CheckStatus::Passed,
                    RuntimeConnectionState::DevelopmentFixture => CheckStatus::Informational,
                    _ => CheckStatus::Warning,
                },
                title: "Native media broker".into(),
                detail: media_broker.detail.clone(),
                remediation: (media_broker.state != RuntimeConnectionState::Ready).then(|| {
                    "Conversation control remains available; use audio/subtitles while media capabilities recover or remain unavailable.".into()
                }),
            },
            DiagnosticCheck {
                id: "performance-evidence".into(),
                status: CheckStatus::Informational,
                title: "Performance evidence deferred".into(),
                detail: "Silent mode, disabled CPU boost, and concurrent project agents are valid development conditions but not a controlled benchmark environment.".into(),
                remediation: Some("Capture release metrics later under a recorded power profile and controlled background load; CPU boost need not remain enabled outside that test.".into()),
            },
        ],
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential_prompt::PromptError;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Debug, Default)]
    struct RedactedCredentialFixture {
        present: AtomicBool,
    }

    impl CredentialPresence for RedactedCredentialFixture {
        fn status(&self, _reference: &str) -> crate::domain::CredentialReferenceStatus {
            if self.present.load(Ordering::Acquire) {
                crate::domain::CredentialReferenceStatus::Present
            } else {
                crate::domain::CredentialReferenceStatus::Missing
            }
        }

        fn save(
            &self,
            _reference: &str,
            _secret: &SecretValue,
        ) -> Result<(), CredentialMutationError> {
            self.present.store(true, Ordering::Release);
            Ok(())
        }

        fn delete(&self, _reference: &str) -> Result<(), CredentialMutationError> {
            self.present.store(false, Ordering::Release);
            Ok(())
        }

        fn availability_detail(&self) -> &'static str {
            "fixture"
        }
    }

    struct CanaryPrompt;

    impl std::fmt::Debug for CanaryPrompt {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("CanaryPrompt(<REDACTED>)")
        }
    }

    impl CredentialPrompt for CanaryPrompt {
        fn prompt(
            &self,
            _owner: NativeWindowOwner,
            _provider_id: &str,
            _provider_name: &str,
        ) -> Result<NativePromptOutcome, PromptError> {
            Ok(NativePromptOutcome::Submitted(
                SecretValue::new(b"sk-canary-never-return-this".to_vec()).expect("secret"),
            ))
        }
    }

    #[derive(Debug)]
    struct CancelPrompt;

    impl CredentialPrompt for CancelPrompt {
        fn prompt(
            &self,
            _owner: NativeWindowOwner,
            _provider_id: &str,
            _provider_name: &str,
        ) -> Result<NativePromptOutcome, PromptError> {
            Ok(NativePromptOutcome::Cancelled)
        }
    }

    #[derive(Debug)]
    struct MustNotPrompt;

    impl CredentialPrompt for MustNotPrompt {
        fn prompt(
            &self,
            _owner: NativeWindowOwner,
            _provider_id: &str,
            _provider_name: &str,
        ) -> Result<NativePromptOutcome, PromptError> {
            panic!("unknown providers must be rejected before prompting")
        }
    }

    #[test]
    fn bootstrap_declares_absent_privileges() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let state = AppState::new(temp.path().to_path_buf(), None).expect("app state");
        let snapshot = build_bootstrap(&state);
        assert_eq!(snapshot.capabilities.get("shellExecution"), Some(&false));
        assert_eq!(
            snapshot.capabilities.get("generalFilesystemAccess"),
            Some(&false)
        );
        assert_eq!(
            snapshot.capabilities.get("debugSyntheticReplayCapture"),
            Some(&cfg!(debug_assertions))
        );
        assert_eq!(
            snapshot
                .capabilities
                .get("debugSyntheticReviewTargetLaunch"),
            Some(&cfg!(debug_assertions))
        );
        assert!(!snapshot.safety.credential_values_exposed_to_webview);
        assert_eq!(
            state.identity_actor_locks.snapshot(),
            crate::identity_runtime::NativeActorLockStateV1::IdentityPackUnqualified
        );
        let wire = serde_json::to_string(&snapshot).expect("serialize bootstrap");
        assert!(!wire.contains("identityActorLock"));
        assert!(!wire.contains("activateQualified"));
    }

    #[test]
    fn diagnostics_never_treat_fixture_estimates_as_evidence() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let state = AppState::new(temp.path().to_path_buf(), None).expect("app state");
        let diagnostics = build_diagnostics(&state);
        assert!(
            !diagnostics
                .measurements
                .current_results_are_release_evidence
        );
        assert_eq!(diagnostics.measurements.state, "notCaptured");
    }

    #[test]
    fn authored_profile_console_turn_is_isolated_without_game_awareness() {
        let resources = ResourceCatalog::new(None);
        let synthetic =
            trusted_turn_safety_context(&StartSimulationRequest::default(), &resources, true);
        assert_eq!(
            synthetic.evidence_state,
            NativeSafetyEvidenceState::VerifiedSafe
        );
        assert_eq!(
            synthetic.profile_policy,
            NativeProfileSafetyPolicy::SyntheticFixture
        );

        let real = trusted_turn_safety_context(
            &StartSimulationRequest {
                game_profile_id: Some("skyrim-special-edition".into()),
                ..StartSimulationRequest::default()
            },
            &resources,
            false,
        );
        assert_eq!(
            real.evidence_state,
            NativeSafetyEvidenceState::ConsoleIsolated
        );
        assert_eq!(
            real.profile_policy,
            NativeProfileSafetyPolicy::ConsoleIsolatedNoGameInteraction
        );
        assert!(!real.visuals_allowed);
        assert!(!real.protected_online_detected);
        assert!(!real.anti_cheat_detected);

        let unknown = trusted_turn_safety_context(
            &StartSimulationRequest {
                game_profile_id: Some("not-a-profile".into()),
                ..StartSimulationRequest::default()
            },
            &resources,
            false,
        );
        assert_eq!(unknown.evidence_state, NativeSafetyEvidenceState::Unknown);
        assert_eq!(unknown.profile_policy, NativeProfileSafetyPolicy::Unknown);

        let unverified_synthetic =
            trusted_turn_safety_context(&StartSimulationRequest::default(), &resources, false);
        assert_eq!(
            unverified_synthetic.evidence_state,
            NativeSafetyEvidenceState::Unknown
        );
        assert!(!unverified_synthetic.visuals_allowed);
    }

    #[test]
    fn credential_state_and_responses_never_serialize_canary() {
        const CANARY: &str = "sk-canary-never-return-this";
        let manager = RedactedCredentialFixture::default();
        let response =
            prompt_and_save_impl("openai", NativeWindowOwner(0), &manager, &CanaryPrompt)
                .expect("native prompt result");
        let connection = ProviderConnectionTestResult {
            provider_id: "openai".into(),
            outcome: ProviderConnectionOutcome::ReadyForConnection,
            credential_status: crate::domain::CredentialReferenceStatus::Present,
            runtime_connected: true,
            provider_contract_available: true,
            network_request_performed: false,
            response_body_returned: false,
            detail: "Ready without returning a body.".into(),
        };
        let serialized = format!(
            "{} {} {:?}",
            serde_json::to_string(&response).expect("serialize prompt status"),
            serde_json::to_string(&connection).expect("serialize connection"),
            manager
        );
        assert!(!serialized.contains(CANARY));
        assert!(!serialized.to_ascii_lowercase().contains("credentialvalue"));
    }

    #[test]
    fn webview_benchmark_request_cannot_mint_target_or_route_binding() {
        let wire = serde_json::json!({
            "requestedIterations": 3,
            "timeoutMillis": 10_000,
            "baselineWindowMillis": 1_000,
            "binding": {
                "selectedGamePid": 42,
                "gameProfileId": "forged",
                "providerRoutes": []
            }
        });
        assert!(serde_json::from_value::<StartThisPcBenchmarkRequest>(wire).is_err());
    }

    #[test]
    fn webview_encounter_requests_cannot_mint_identity_or_migration_evidence() {
        let correction = serde_json::json!({
            "gameProfileId": "skyrim-special-edition",
            "encounterId": uuid::Uuid::nil(),
            "characterId": "lydia",
            "explicitUserConfirmation": true,
            "nativeIdentityDecision": {"state":"matched"}
        });
        assert!(serde_json::from_value::<CorrectEncounterRequest>(correction).is_err());
        let merge = serde_json::json!({
            "gameProfileId": "skyrim-special-edition",
            "sourceEncounterId": uuid::Uuid::nil(),
            "destinationEncounterId": uuid::Uuid::max(),
            "explicitUserConfirmation": true,
            "memoryMigrationPerformed": true
        });
        assert!(serde_json::from_value::<MergeEncountersRequest>(merge).is_err());
    }

    #[test]
    fn identity_reference_enrollment_webview_shape_cannot_carry_pixels_paths_or_capabilities() {
        let safe = serde_json::json!({
            "gameProfileId": "skyrim-special-edition",
            "characterId": "lydia",
            "referenceId": "lydia-reference-1",
            "subjectDisplayName": "Lydia",
            "sourceClass": "user_private",
            "ownerUserId": "local-user",
            "originalWorkLicense": null,
            "explicitUserConsent": true
        });
        serde_json::from_value::<IdentityReferenceEnrollmentCommandRequest>(safe)
            .expect("bounded safe enrollment request");
        for forbidden in [
            "filePath",
            "pixels",
            "mappingName",
            "oneTimeToken",
            "embedding",
        ] {
            let mut forged = serde_json::json!({
                "gameProfileId": "skyrim-special-edition",
                "characterId": "lydia",
                "referenceId": "lydia-reference-1",
                "subjectDisplayName": "Lydia",
                "sourceClass": "user_private",
                "ownerUserId": "local-user",
                "originalWorkLicense": null,
                "explicitUserConsent": true
            });
            forged[forbidden] = serde_json::json!("forged");
            assert!(
                serde_json::from_value::<IdentityReferenceEnrollmentCommandRequest>(forged)
                    .is_err()
            );
        }
        let status = identity_reference_enrollment_status();
        assert_eq!(
            status.status,
            IdentityReferenceEnrollmentAvailabilityV1::Unavailable
        );
        assert!(!status.signed_identity_pack_admitted);
        assert!(!status.raw_pixels_exposed_to_webview);
        assert!(!status.worker_capability_exposed_to_webview);
    }

    #[test]
    fn native_prompt_cancellation_is_typed_and_preserves_status() {
        let manager = RedactedCredentialFixture::default();
        let result = prompt_and_save_impl("openai", NativeWindowOwner(0), &manager, &CancelPrompt)
            .expect("cancel result");
        assert_eq!(result.outcome, CredentialPromptOutcome::Cancelled);
        assert_eq!(
            result.credential_status,
            crate::domain::CredentialReferenceStatus::Missing
        );
    }

    #[test]
    fn nvidia_nim_is_accepted_by_native_prompt_allowlist() {
        let manager = RedactedCredentialFixture::default();
        let result =
            prompt_and_save_impl("nvidia-nim", NativeWindowOwner(0), &manager, &CanaryPrompt)
                .expect("NVIDIA NIM prompt result");
        assert_eq!(result.provider_id, "nvidia-nim");
        assert_eq!(result.outcome, CredentialPromptOutcome::Saved);
        assert_eq!(
            result.credential_status,
            crate::domain::CredentialReferenceStatus::Present
        );
        assert!(!serde_json::to_string(&result)
            .expect("serialize result")
            .contains("sk-canary"));
    }

    #[test]
    fn unknown_provider_is_rejected_before_native_prompt() {
        let manager = RedactedCredentialFixture::default();
        let result = prompt_and_save_impl(
            "unknown-provider",
            NativeWindowOwner(0),
            &manager,
            &MustNotPrompt,
        );
        assert!(matches!(result, Err(CommandError::InvalidRequest { .. })));
    }

    #[test]
    fn production_invoke_registry_has_no_secret_valued_command() {
        let registry = include_str!("lib.rs");
        let commands = include_str!("commands.rs");
        let forbidden_command = ["save", "provider", "credential"].join("_");
        let forbidden_parameter = ["credential", ": String"].concat();
        assert!(registry.contains("commands::prompt_and_save_provider_credential"));
        assert!(!registry.contains(&format!("commands::{forbidden_command}")));
        assert!(!commands.contains(&format!("pub fn {forbidden_command}(")));
        assert!(!commands.contains(&forbidden_parameter));
    }

    #[test]
    fn manual_actor_picker_webview_contract_is_coarse_redacted_and_reachable() {
        let waiting = manual_actor_presentation(
            ManualActorPickerPresentationStateV1::Waiting,
            "Waiting for one native click.",
        );
        let encoded = serde_json::to_value(waiting).expect("safe presentation DTO");
        assert_eq!(encoded["schemaVersion"], 1);
        assert_eq!(encoded["state"], "waiting");
        let text = encoded.to_string();
        for forbidden in [
            "requestId",
            "candidate",
            "pixel",
            "coordinate",
            "windowHandle",
            "processId",
            "trackId",
            "actorId",
            "nonce",
            "sha256",
        ] {
            assert!(!text.contains(forbidden), "leaked {forbidden}");
        }
        let registry = include_str!("lib.rs");
        for command in [
            "commands::start_manual_actor_picker",
            "commands::manual_actor_picker_status",
            "commands::cancel_manual_actor_picker",
        ] {
            assert!(registry.contains(command), "missing {command}");
        }
        let unavailable = manual_actor_no_candidate_set();
        let encoded = serde_json::to_value(unavailable).expect("typed unavailable DTO");
        assert_eq!(encoded["state"], "unavailable");
        assert_eq!(encoded["unavailableReason"], "noAdmittedNativeCandidateSet");
    }
}
