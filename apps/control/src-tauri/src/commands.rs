use crate::catalog::{
    credential_provider_name, credential_reference_for, is_known_provider, model_summaries,
    provider_summaries, CredentialMutationError, CredentialPresence, ResourceCatalog,
    SystemCredentialPresence,
};
use crate::credential_prompt::{
    CredentialPrompt, NativePromptOutcome, NativeWindowOwner, SystemCredentialPrompt,
};
use crate::domain::{
    BootstrapSnapshot, CatalogState, CheckStatus, CredentialPromptOutcome,
    CredentialPromptSaveResult, DiagnosticCheck, DiagnosticOverall, DiagnosticSummary,
    MeasurementStatus, MediaBrokerDiagnostics, MediaBrokerHealthSnapshot, ModelSummary,
    OnboardingPersistence, OnboardingSnapshot, PersistenceHealth, ProviderConnectionOutcome,
    ProviderConnectionTestResult, ProviderCredentialSummary, RuntimeConnectionState,
    RuntimeHealthSnapshot, SafetyBoundary, SaveOnboardingResult, SimulationEvent,
    StartSimulationRequest, StartSimulationResult, CONTROL_CONTRACT_VERSION,
};
use crate::media_broker::{MediaBrokerLaunchConfig, MediaBrokerSupervisor};
use crate::persistence::OnboardingStore;
use crate::provider_loadouts::ProviderLoadoutManager;
use crate::runtime_router::RuntimeRouter;
use crate::sidecar_protocol::{NativeDoctorReport, NativeProfile};
use crate::sidecar_supervisor::{RuntimeLaunchConfig, RuntimeSupervisor};
use interactive_npcs_credential_vault::SecretValue;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::State;

#[derive(Debug)]
struct OnboardingCache {
    snapshot: OnboardingSnapshot,
    persistence: OnboardingPersistence,
}

#[derive(Debug)]
pub struct AppState {
    onboarding: Mutex<OnboardingCache>,
    onboarding_store: OnboardingStore,
    pub runtime: RuntimeRouter,
    pub media_broker: MediaBrokerSupervisor,
    credentials: Arc<dyn CredentialPresence>,
    credential_prompt: Arc<dyn CredentialPrompt>,
    resources: ResourceCatalog,
    pub(crate) provider_loadouts: ProviderLoadoutManager,
}

impl AppState {
    pub fn new(
        config_directory: PathBuf,
        resource_directory: Option<PathBuf>,
    ) -> Result<Self, String> {
        let config_directory =
            crate::private_directory::ensure_private_directory(&config_directory)
                .map_err(|error| error.to_string())?;
        let onboarding_store = OnboardingStore::new(config_directory.clone());
        let (snapshot, persistence) = onboarding_store.load();
        let credentials: Arc<dyn CredentialPresence> = match SystemCredentialPresence::new() {
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
            resource_root,
            config_directory.join("runtime-host-data"),
            cfg!(dev) && cfg!(debug_assertions),
        )
        .map_err(|error| error.to_string())?;
        let runtime = RuntimeRouter::new(
            RuntimeSupervisor::try_new(launch).map_err(|error| error.to_string())?,
        );
        let media_launch = MediaBrokerLaunchConfig::from_application(
            cfg!(dev) && cfg!(debug_assertions),
            &config_directory,
        )
        .map_err(|error| error.to_string())?;
        let media_broker = MediaBrokerSupervisor::new(media_launch, runtime.supervisor().clone());
        Ok(Self {
            onboarding: Mutex::new(OnboardingCache {
                snapshot,
                persistence,
            }),
            onboarding_store,
            runtime,
            media_broker,
            credentials,
            credential_prompt,
            resources: ResourceCatalog::new(resource_directory),
            provider_loadouts: ProviderLoadoutManager::new(config_directory),
        })
    }

    pub async fn shutdown_services(&self) {
        self.media_broker.shutdown().await;
        self.runtime.shutdown().await;
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
    request: StartSimulationRequest,
    events: Channel<SimulationEvent>,
    state: State<'_, AppState>,
) -> Result<StartSimulationResult, CommandError> {
    state
        .runtime
        .start(request, events)
        .await
        .map_err(|error| CommandError::Simulation {
            message: error.to_string(),
        })
}

#[tauri::command]
pub async fn cancel_simulation(
    state: State<'_, AppState>,
) -> Result<crate::domain::CancelSimulationResult, CommandError> {
    Ok(state.runtime.cancel().await)
}

#[tauri::command]
pub async fn runtime_health(
    state: State<'_, AppState>,
) -> Result<RuntimeHealthSnapshot, CommandError> {
    match state.runtime.supervisor().ping().await {
        Ok(health) => Ok(health),
        Err(_)
            if state.runtime.supervisor().health().state
                == RuntimeConnectionState::DevelopmentFixture =>
        {
            Ok(state.runtime.supervisor().health())
        }
        Err(error) => Err(CommandError::Runtime {
            message: error.to_string(),
        }),
    }
}

#[tauri::command]
pub async fn runtime_doctor(
    state: State<'_, AppState>,
) -> Result<NativeDoctorReport, CommandError> {
    state
        .runtime
        .supervisor()
        .doctor()
        .await
        .map_err(|error| CommandError::Runtime {
            message: error.to_string(),
        })
}

#[tauri::command]
pub async fn runtime_profile_summaries(
    state: State<'_, AppState>,
) -> Result<Vec<NativeProfile>, CommandError> {
    state
        .runtime
        .supervisor()
        .profiles()
        .await
        .map_err(|error| CommandError::Runtime {
            message: error.to_string(),
        })
}

#[tauri::command]
pub async fn media_broker_health(
    state: State<'_, AppState>,
) -> Result<MediaBrokerHealthSnapshot, CommandError> {
    match state.media_broker.refresh_health().await {
        Ok(health) => Ok(health),
        Err(_)
            if state.media_broker.health().state == RuntimeConnectionState::DevelopmentFixture =>
        {
            Ok(state.media_broker.health())
        }
        Err(error) => Err(CommandError::Runtime {
            message: error.to_string(),
        }),
    }
}

#[tauri::command]
pub async fn media_broker_diagnostics(
    state: State<'_, AppState>,
) -> Result<MediaBrokerDiagnostics, CommandError> {
    state
        .media_broker
        .diagnostics()
        .await
        .map_err(|error| CommandError::Runtime {
            message: error.to_string(),
        })
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
    state
        .media_broker
        .debug_select_synthetic_replay_capture_target()
        .await
        .map_err(map_debug_synthetic_capture_error)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn debug_clear_synthetic_replay_capture_target(
    state: State<'_, AppState>,
) -> Result<crate::media_broker::DebugSyntheticReplayCaptureSnapshot, CommandError> {
    state
        .media_broker
        .debug_clear_synthetic_replay_capture_target()
        .await
        .map_err(map_debug_synthetic_capture_error)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn debug_synthetic_replay_capture_diagnostics(
    state: State<'_, AppState>,
) -> Result<crate::media_broker::DebugSyntheticReplayCaptureSnapshot, CommandError> {
    state
        .media_broker
        .debug_synthetic_replay_capture_diagnostics()
        .await
        .map_err(map_debug_synthetic_capture_error)
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
        .delete(reference)
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
    let credential_status = state.credentials.status(reference);
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
            "Credential presence and the runtime provider contract are ready. No content-bearing network request was made.".into()
        } else {
            "The runtime does not currently advertise this provider contract. No network request was made.".into()
        },
    })
}

#[tauri::command]
pub fn game_profile_summaries(
    state: State<'_, AppState>,
) -> Vec<crate::domain::GameProfileSummary> {
    state.resources.game_summaries()
}

#[tauri::command]
pub fn model_pack_summaries() -> Vec<ModelSummary> {
    model_summaries()
}

fn hosted_credential_reference(provider_id: &str) -> Result<&'static str, CommandError> {
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
        .prompt(owner, provider_id, provider_name)
        .map_err(|error| CommandError::Credential {
            message: error.to_string(),
        })?
    {
        NativePromptOutcome::Submitted(secret) => {
            credentials
                .save(reference, &secret)
                .map_err(|error| CommandError::Credential {
                    message: error.to_string(),
                })?;
            drop(secret);
            Ok(CredentialPromptSaveResult {
                provider_id: provider_id.into(),
                outcome: CredentialPromptOutcome::Saved,
                credential_status: credentials.status(reference),
                detail: "Credential saved directly to Windows Credential Manager.".into(),
            })
        }
        NativePromptOutcome::Cancelled => Ok(CredentialPromptSaveResult {
            provider_id: provider_id.into(),
            outcome: CredentialPromptOutcome::Cancelled,
            credential_status: credentials.status(reference),
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
        assert!(!snapshot.safety.credential_values_exposed_to_webview);
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
}
