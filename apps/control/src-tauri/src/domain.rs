use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTROL_CONTRACT_VERSION: u32 = 1;
pub const ONBOARDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    Cloud,
    #[default]
    Hybrid,
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PerformanceMode {
    Competitive,
    Fast,
    #[default]
    Balanced,
    Immersive,
    Maximum,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferenceSnapshot {
    pub execution: ExecutionMode,
    pub performance: PerformanceMode,
    pub subtitles: bool,
    pub ptt: bool,
    pub local_only: bool,
    pub screen_presence: bool,
    pub diagnostics: bool,
}

impl Default for PreferenceSnapshot {
    fn default() -> Self {
        Self {
            execution: ExecutionMode::Hybrid,
            performance: PerformanceMode::Balanced,
            subtitles: true,
            ptt: true,
            local_only: false,
            screen_presence: false,
            diagnostics: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnboardingSnapshot {
    pub schema_version: u32,
    pub completed: bool,
    pub current_step: OnboardingStep,
    pub selected_game_id: Option<String>,
    pub preferences: PreferenceSnapshot,
    pub updated_at_epoch_ms: u64,
}

impl Default for OnboardingSnapshot {
    fn default() -> Self {
        Self {
            schema_version: ONBOARDING_SCHEMA_VERSION,
            completed: false,
            current_step: OnboardingStep::Welcome,
            selected_game_id: None,
            preferences: PreferenceSnapshot::default(),
            updated_at_epoch_ms: 0,
        }
    }
}

impl OnboardingSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != ONBOARDING_SCHEMA_VERSION {
            return Err(format!(
                "unsupported onboarding schema {}; expected {ONBOARDING_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if let Some(game_id) = &self.selected_game_id {
            validate_identifier("selected game", game_id)?;
        }
        if self.completed && self.current_step != OnboardingStep::Ready {
            return Err("completed onboarding must end at the ready step".into());
        }
        if self.preferences.local_only && self.preferences.execution != ExecutionMode::Local {
            return Err("localOnly requires the local execution mode".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum OnboardingStep {
    #[default]
    Welcome,
    Scan,
    Execution,
    Game,
    Providers,
    Microphone,
    Presence,
    Performance,
    Simulation,
    Ready,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PersistenceHealth {
    Healthy,
    FirstRun,
    RecoveredFromInvalidFile,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingPersistence {
    pub health: PersistenceHealth,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveOnboardingResult {
    pub onboarding: OnboardingSnapshot,
    pub persistence: OnboardingPersistence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SimulationStatus {
    Idle,
    Running,
    Cancelling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SimulationSnapshot {
    pub status: SimulationStatus,
    pub simulation_id: Option<String>,
    pub generation: u64,
    pub active_stage: Option<ResponseStage>,
    pub backend: RuntimeBackend,
}

impl Default for SimulationSnapshot {
    fn default() -> Self {
        Self {
            status: SimulationStatus::Idle,
            simulation_id: None,
            generation: 0,
            active_stage: None,
            backend: RuntimeBackend::DeterministicFixture,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeBackend {
    DeterministicFixture,
    NativeRuntime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeConnectionState {
    Cold,
    Starting,
    Ready,
    RestartBackoff,
    Quarantined,
    DevelopmentFixture,
    Unavailable,
    ShuttingDown,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthSnapshot {
    pub state: RuntimeConnectionState,
    pub connected: bool,
    pub backend: RuntimeBackend,
    pub process_id: Option<u32>,
    pub restart_count: u32,
    pub recent_failure_count: u32,
    pub protocol_version: Option<String>,
    pub fixture_only: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrokerHealthSnapshot {
    pub state: RuntimeConnectionState,
    pub connected: bool,
    pub process_id: Option<u32>,
    pub restart_count: u32,
    pub recent_failure_count: u32,
    pub protocol_version: Option<u32>,
    pub fixture_only: bool,
    pub broker_state: Option<String>,
    pub capture_available: bool,
    pub overlay_available: bool,
    pub capture_audio_available: bool,
    pub render_audio_available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrokerDiagnostics {
    pub state: String,
    pub capture_backend: String,
    pub overlay_backend: String,
    pub capture_audio: String,
    pub render_audio: String,
    pub target_state: String,
    pub device_generation: u64,
    pub audio_device_generation: u64,
    pub cancellation_generation: u64,
    pub frames_received: u64,
    pub frames_presented: u64,
    pub frames_dropped: u64,
    pub overlays_suppressed: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStage {
    Listening,
    Transcribing,
    Identifying,
    Remembering,
    Responding,
    Voicing,
    Animating,
}

pub const DEV_LIVE_TTS_PROVIDER_ID: &str = "elevenlabs";
pub const DEV_LIVE_TTS_MODEL_ID: &str = "eleven_flash_v2_5";
pub const DEV_LIVE_TTS_STOCK_VOICE_IDS: &[&str] = &["EXAVITQu4vr4xnSDxMaL"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevLiveTtsRequest {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: String,
    pub explicit_user_authorization: bool,
}

impl DevLiveTtsRequest {
    fn validate(&self, execution_mode: ExecutionMode) -> Result<(), String> {
        if !cfg!(debug_assertions) {
            return Err("devLiveTts is unavailable in release builds".into());
        }
        if !self.explicit_user_authorization {
            return Err("devLiveTts requires explicit user authorization".into());
        }
        if execution_mode == ExecutionMode::Local {
            return Err("devLiveTts cannot be used in Fully Local execution mode".into());
        }
        if self.provider_id != DEV_LIVE_TTS_PROVIDER_ID {
            return Err("devLiveTts provider is not allowlisted".into());
        }
        if self.model_id != DEV_LIVE_TTS_MODEL_ID {
            return Err("devLiveTts model is not allowlisted".into());
        }
        if !DEV_LIVE_TTS_STOCK_VOICE_IDS.contains(&self.voice_id.as_str()) {
            return Err("devLiveTts voice is not an allowlisted stock voice".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSimulationRequest {
    pub game_profile_id: Option<String>,
    pub character_name: Option<String>,
    #[serde(default)]
    pub character_id: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
    pub execution_mode: ExecutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_live_tts: Option<DevLiveTtsRequest>,
}

impl Default for StartSimulationRequest {
    fn default() -> Self {
        Self {
            game_profile_id: Some("eclipse-harbor".into()),
            character_name: Some("Mara Venn".into()),
            character_id: None,
            transcript: None,
            execution_mode: ExecutionMode::Hybrid,
            dev_live_tts: None,
        }
    }
}

impl StartSimulationRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(id) = &self.game_profile_id {
            validate_identifier("game profile", id)?;
        }
        if let Some(name) = &self.character_name {
            let trimmed = name.trim();
            if trimmed.is_empty()
                || trimmed.chars().count() > 96
                || trimmed.chars().any(char::is_control)
            {
                return Err("character name must contain 1-96 printable characters".into());
            }
        }
        if let Some(id) = &self.character_id {
            validate_identifier("character", id)?;
        }
        if let Some(transcript) = &self.transcript {
            let trimmed = transcript.trim();
            if trimmed.is_empty()
                || transcript.len() > 64 * 1024
                || transcript.chars().any(|character| character == '\0')
            {
                return Err("simulation transcript must contain 1-65536 bytes and no NUL".into());
            }
        }
        if let Some(route) = &self.dev_live_tts {
            route.validate(self.execution_mode)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartSimulationResult {
    pub simulation_id: String,
    pub generation: u64,
    pub backend: RuntimeBackend,
    pub measurement_basis: MeasurementBasis,
    pub runtime_fixture_only: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementBasis {
    DeterministicFixture,
    TrustedRuntimeFixture,
    ControlledBenchmark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SimulationEvent {
    Started {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        measurement_basis: MeasurementBasis,
    },
    StageStarted {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        stage: ResponseStage,
        estimated_duration_ms: u64,
    },
    StageCompleted {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        stage: ResponseStage,
        fixture_elapsed_ms: u64,
    },
    SentenceReady {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        text: String,
    },
    Completed {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        fixture_first_audio_ms: u64,
        runtime_fixture_only: bool,
        delivered_text: String,
    },
    Cancelled {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelSimulationResult {
    pub simulation_id: Option<String>,
    pub generation: u64,
    pub outcome: CancelOutcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CancelOutcome {
    CancellationRequested,
    AlreadyIdle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialSummary {
    pub provider_id: String,
    pub display_name: String,
    pub credential_reference: Option<String>,
    pub status: CredentialReferenceStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CredentialReferenceStatus {
    Present,
    Missing,
    NotRequired,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CredentialPromptOutcome {
    Saved,
    Cancelled,
    DevelopmentFixtureOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialPromptSaveResult {
    pub provider_id: String,
    pub outcome: CredentialPromptOutcome,
    pub credential_status: CredentialReferenceStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderConnectionOutcome {
    ReadyForConnection,
    NeedsCredential,
    ProviderContractUnavailable,
    RuntimeUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionTestResult {
    pub provider_id: String,
    pub outcome: ProviderConnectionOutcome,
    pub credential_status: CredentialReferenceStatus,
    pub runtime_connected: bool,
    pub provider_contract_available: bool,
    pub network_request_performed: bool,
    pub response_body_returned: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameProfileSummary {
    pub id: String,
    pub display_name: String,
    pub wave: String,
    pub safety: ProfileSafety,
    pub catalog_state: CatalogState,
    pub default_fallback: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProfileSafety {
    SinglePlayerOnly,
    OfflineOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CatalogState {
    Bundled,
    MissingFromBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    pub display_name: String,
    pub purpose: String,
    pub execution: String,
    pub lifecycle: String,
    pub installation: ModelInstallation,
    pub qualification_note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelInstallation {
    NotInspected,
    UserImportRequired,
    CatalogOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSummary {
    pub overall: DiagnosticOverall,
    pub generated_at_epoch_ms: u64,
    pub measurements: MeasurementStatus,
    pub checks: Vec<DiagnosticCheck>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticOverall {
    ReadyForSimulation,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementStatus {
    pub state: String,
    pub reason: String,
    pub current_results_are_release_evidence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub id: String,
    pub status: CheckStatus,
    pub title: String,
    pub detail: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Passed,
    Informational,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafetyBoundary {
    pub single_player_only: bool,
    pub blocks_online_modes: bool,
    pub blocks_detected_anti_cheat: bool,
    pub silent_egress_changes_allowed: bool,
    pub credential_values_exposed_to_webview: bool,
}

impl Default for SafetyBoundary {
    fn default() -> Self {
        Self {
            single_player_only: true,
            blocks_online_modes: true,
            blocks_detected_anti_cheat: true,
            silent_egress_changes_allowed: false,
            credential_values_exposed_to_webview: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub contract_version: u32,
    pub app_version: String,
    pub onboarding: OnboardingSnapshot,
    pub onboarding_persistence: OnboardingPersistence,
    pub simulation: SimulationSnapshot,
    pub runtime: RuntimeHealthSnapshot,
    pub media_broker: MediaBrokerHealthSnapshot,
    pub providers: Vec<ProviderCredentialSummary>,
    pub game_profiles: Vec<GameProfileSummary>,
    pub models: Vec<ModelSummary>,
    pub diagnostics: DiagnosticSummary,
    pub safety: SafetyBoundary,
    pub capabilities: BTreeMap<String, bool>,
}

pub fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 96 {
        return Err(format!("{label} identifier must contain 1-96 bytes"));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
    }) {
        return Err(format!(
            "{label} identifier contains unsupported characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only_requires_fully_local_execution() {
        let state = OnboardingSnapshot {
            preferences: PreferenceSnapshot {
                local_only: true,
                execution: ExecutionMode::Hybrid,
                ..PreferenceSnapshot::default()
            },
            ..OnboardingSnapshot::default()
        };
        assert!(state.validate().is_err());
    }

    #[test]
    fn completed_onboarding_requires_ready_step() {
        let state = OnboardingSnapshot {
            completed: true,
            current_step: OnboardingStep::Simulation,
            ..OnboardingSnapshot::default()
        };
        assert!(state.validate().is_err());
    }

    #[test]
    fn simulation_event_wire_fields_are_camel_case() {
        let event = SimulationEvent::Completed {
            simulation_id: "simulation-fixture".into(),
            generation: 2,
            sequence: 9,
            fixture_first_audio_ms: 418,
            runtime_fixture_only: true,
            delivered_text: "The harbor remembers.".into(),
        };
        let wire = serde_json::to_value(event).expect("serialize simulation event");
        assert_eq!(wire["type"], "completed");
        assert_eq!(wire["simulationId"], "simulation-fixture");
        assert_eq!(wire["fixtureFirstAudioMs"], 418);
        assert_eq!(wire["runtimeFixtureOnly"], true);
        assert_eq!(wire["deliveredText"], "The harbor remembers.");
        for legacy_key in [
            "simulation_id",
            "fixture_first_audio_ms",
            "runtime_fixture_only",
            "delivered_text",
        ] {
            assert!(wire.get(legacy_key).is_none(), "legacy key {legacy_key}");
        }
    }

    #[test]
    fn simulation_request_omits_dev_live_tts_by_default() {
        let wire = serde_json::to_value(StartSimulationRequest::default())
            .expect("serialize default simulation request");
        assert!(wire.get("devLiveTts").is_none());
    }

    #[test]
    fn dev_live_tts_requires_the_exact_authorized_hosted_route() {
        let valid = DevLiveTtsRequest {
            provider_id: DEV_LIVE_TTS_PROVIDER_ID.into(),
            model_id: DEV_LIVE_TTS_MODEL_ID.into(),
            voice_id: DEV_LIVE_TTS_STOCK_VOICE_IDS[0].into(),
            explicit_user_authorization: true,
        };
        let request = StartSimulationRequest {
            dev_live_tts: Some(valid.clone()),
            ..StartSimulationRequest::default()
        };
        assert!(request.validate().is_ok());

        for invalid in [
            DevLiveTtsRequest {
                explicit_user_authorization: false,
                ..valid.clone()
            },
            DevLiveTtsRequest {
                provider_id: "unknown-provider".into(),
                ..valid.clone()
            },
            DevLiveTtsRequest {
                model_id: "unknown-model".into(),
                ..valid.clone()
            },
            DevLiveTtsRequest {
                voice_id: "custom-or-cloned-voice".into(),
                ..valid.clone()
            },
        ] {
            assert!(StartSimulationRequest {
                dev_live_tts: Some(invalid),
                ..StartSimulationRequest::default()
            }
            .validate()
            .is_err());
        }

        assert!(StartSimulationRequest {
            execution_mode: ExecutionMode::Local,
            dev_live_tts: Some(valid),
            ..StartSimulationRequest::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn identifiers_are_path_independent() {
        assert!(validate_identifier("game", "skyrim-special-edition").is_ok());
        assert!(validate_identifier("game", "../../secret").is_err());
        assert!(validate_identifier("game", "Game Name").is_err());
    }
}
