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
            // Keep first run API-first. Optional local roles are admitted only
            // after their exact pack revisions have measured resource envelopes.
            execution: ExecutionMode::Cloud,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSimulationRequest {
    pub game_profile_id: Option<String>,
    pub character_name: Option<String>,
    #[serde(default)]
    pub character_id: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_stt_receipt: Option<SelectedSttReceiptReferenceV1>,
    #[serde(default)]
    pub enabled_spoiler_tiers: Vec<String>,
    pub execution_mode: ExecutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_live_tts: Option<DevLiveTtsRequest>,
    /// Trusted native-only authored profile snapshot. Serde skips this field so
    /// the WebView cannot supply or observe the runtime profile authority.
    #[serde(skip)]
    pub effective_game_profile: Option<npc_game_profile::GameProfileV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedSttReceiptReferenceV1 {
    pub receipt_id: String,
    pub generation: u64,
}

impl Default for StartSimulationRequest {
    fn default() -> Self {
        Self {
            game_profile_id: Some("eclipse-harbor".into()),
            character_name: Some("Mara Venn".into()),
            character_id: Some("mara-venn".into()),
            transcript: None,
            selected_stt_receipt: None,
            enabled_spoiler_tiers: Vec::new(),
            execution_mode: ExecutionMode::Hybrid,
            dev_live_tts: None,
            effective_game_profile: None,
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
        if let Some(receipt) = &self.selected_stt_receipt {
            if self.transcript.is_some()
                || receipt.generation == 0
                || uuid::Uuid::parse_str(&receipt.receipt_id).is_err()
            {
                return Err(
                    "selectedSttReceipt must be a native opaque UUID and nonzero generation, and cannot be combined with WebView transcript text"
                        .into(),
                );
            }
        }
        if self.enabled_spoiler_tiers.len() > 32
            || self.enabled_spoiler_tiers.iter().any(|tier| {
                tier.trim().is_empty()
                    || tier.len() > 64
                    || !tier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        {
            return Err("enabled spoiler tiers must contain at most 32 bounded identifiers".into());
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
    PendingProviderEvidence,
    ControlledBenchmark,
}

/// Bounded, WebView-safe proof of the final native visual presentation attempt.
///
/// Worker handles, actor coordinates, model paths, and broker credentials stay
/// native-only. The UI receives only enough evidence to distinguish a mouth
/// overlay that was actually presented from an armed or unavailable path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeVisualPresentationEvidence {
    pub schema_version: u32,
    pub source_frame_sequence: u64,
    pub residual_proposed: bool,
    pub presented: bool,
    pub degraded: bool,
    pub pixel_source: Option<String>,
    pub pixel_scope: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        fixture_first_audio_ms: Option<u64>,
        runtime_fixture_only: bool,
        delivered_text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_execution: Option<Box<NativeTurnExecutionEvidence>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        character_context: Option<crate::sidecar_protocol::NativeCharacterContextEvidence>,
        #[serde(skip_serializing_if = "Option::is_none")]
        visual_presentation: Option<NativeVisualPresentationEvidence>,
    },
    Failed {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_execution: Option<Box<NativeTurnExecutionEvidence>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        character_context: Option<crate::sidecar_protocol::NativeCharacterContextEvidence>,
    },
    Cancelled {
        simulation_id: String,
        generation: u64,
        sequence: u64,
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn_execution: Option<Box<NativeTurnExecutionEvidence>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        character_context: Option<crate::sidecar_protocol::NativeCharacterContextEvidence>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnExecutionEvidence {
    pub consumed_route: NativeConsumedRouteSnapshot,
    pub input: NativeTurnInputEvidence,
    pub delivery_state: NativeTurnDeliveryState,
    pub commit_state: NativeDeliveryCommitState,
    pub subtitles: Vec<NativeSubtitleCue>,
    #[serde(default)]
    pub subtitle_presentation_receipts: Vec<NativeSubtitlePresentationReceipt>,
    pub audio_receipts: Vec<NativeAudioReceiptSummary>,
    pub degradations: Vec<NativeTurnExecutionDegradation>,
    pub success: NativeTurnExecutionSuccess,
}

impl NativeTurnExecutionEvidence {
    /// The runtime response cannot claim speech recognition success with only
    /// booleans. Push-to-talk completion must preserve the exact native
    /// one-time receipt; typed and unavailable capture states must not carry
    /// one. This is checked at the authenticated sidecar boundary before the
    /// result can reach product state.
    pub fn input_evidence_valid(&self) -> bool {
        match (
            self.input.mode,
            self.input.push_to_talk_state,
            self.input.selected_stt_receipt.as_ref(),
        ) {
            (NativeTurnInputMode::Typed, NativePushToTalkState::NotRequested, None) => {
                self.success.stt_skipped
            }
            (NativeTurnInputMode::PushToTalk, NativePushToTalkState::CaptureUnavailable, None) => {
                self.success.stt_skipped
            }
            (
                NativeTurnInputMode::PushToTalk,
                NativePushToTalkState::TranscriptReady,
                Some(receipt),
            ) => !self.success.stt_skipped && self.selected_stt_receipt_valid(receipt),
            _ => false,
        }
    }

    fn selected_stt_receipt_valid(
        &self,
        receipt: &crate::sidecar_protocol::NativeSelectedSttTurnEvidenceV1,
    ) -> bool {
        let route = &receipt.route;
        receipt.schema_version == 1
            && uuid::Uuid::parse_str(&receipt.receipt_id).is_ok()
            && receipt.receipt_sha256.len() == 64
            && receipt
                .receipt_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && valid_stt_identity(&receipt.capture_session_id, 128)
            && valid_stt_identity(&receipt.capture_turn_id, 128)
            && receipt.capture_generation > 0
            && valid_stt_identity(&receipt.game_id, 128)
            && receipt
                .character_id
                .as_deref()
                .map_or(true, |value| valid_stt_identity(value, 128))
            && valid_stt_identity(&receipt.source_loadout_id, 128)
            && receipt.source_loadout_id == self.consumed_route.source_loadout_id
            && route.provider_id == "assemblyai"
            && route.model_id == "u3-rt-pro"
            && route.credential_reference == "providers/assemblyai"
            && route.egress == "microphone_audio_and_optional_non_secret_context"
            && route.generation == receipt.capture_generation
            && valid_stt_identity(&route.input_endpoint_id, 1024)
            && route.input_endpoint_generation > 0
            && !route.automatic_fallback
            && route.captured_frames > 0
            && route.ptt_virtual_key == 0x77
            && route.ptt_press_transition_sequence > 0
            && route.ptt_pressed_qpc > 0
            && route.ptt_release_transition_sequence > route.ptt_press_transition_sequence
            && route.ptt_released_qpc > route.ptt_pressed_qpc
            && receipt.chunks_sent > 0
            && receipt.pcm_bytes_sent > 0
    }

    /// Semantic guard above serde shape validation. A cue is not presentation
    /// evidence: the delivered bit and count must be backed one-for-one by
    /// committed, non-duplicate native receipt records.
    pub fn subtitle_receipts_valid(&self) -> bool {
        let receipts = &self.subtitle_presentation_receipts;
        if receipts.len() != self.success.subtitle_receipt_count
            || self.success.subtitle_delivered == receipts.is_empty()
        {
            return false;
        }
        let mut sentences = std::collections::BTreeSet::new();
        let first_authority = receipts.first().map(|receipt| {
            (
                receipt.renderer_authority_revision,
                receipt.renderer_authority_sha256.as_str(),
                &receipt.renderer_authority_sources,
                receipt.renderer_style_id.as_str(),
                receipt.renderer_safe_area_dp,
                receipt.renderer_text_scale,
                receipt.renderer_backplate_enabled,
                receipt.renderer_opacity,
            )
        });
        receipts.iter().all(|receipt| {
            let identity_valid = match receipt.provenance {
                NativeSubtitlePresentationProvenance::TrustedNativeCapture => {
                    receipt.target_geometry_epoch > 0
                        && receipt.capture_sequence > 0
                        && receipt.graphics_generation > 0
                }
                NativeSubtitlePresentationProvenance::ConsoleBottomCenterUnavailable => {
                    receipt.target_geometry_epoch == 0
                        && receipt.capture_sequence == 0
                        && receipt.graphics_generation == 0
                        && receipt.used_bottom_center_fallback
                        && receipt.color_treatment
                            == NativeSubtitleColorTreatment::WindowsCompositorSdrWhiteMapping
                }
                NativeSubtitlePresentationProvenance::DeterministicFixture => true,
            };
            receipt.committed
                && !receipt.receipt_id.is_empty()
                && receipt.sentence_id > 0
                && receipt.presentation_id > 0
                && receipt.width_px > 0
                && receipt.height_px > 0
                && (48..=960).contains(&receipt.dpi_x)
                && (48..=960).contains(&receipt.dpi_y)
                && receipt.layer_hash_hex.len() == 16
                && receipt
                    .layer_hash_hex
                    .bytes()
                    .all(|value| value.is_ascii_hexdigit())
                && receipt
                    .presented_qpc_ticks
                    .parse::<u64>()
                    .is_ok_and(|value| value > 0)
                && receipt.bidi_shaping_applied
                && receipt.grapheme_clusters_preserved
                && receipt.renderer_authority_sha256.len() == 64
                && receipt
                    .renderer_authority_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && !receipt.renderer_style_id.is_empty()
                && receipt.renderer_style_id.len() <= 128
                && !receipt.renderer_style_id.chars().any(char::is_control)
                && receipt.renderer_safe_area_dp.is_finite()
                && (0.0..=256.0).contains(&receipt.renderer_safe_area_dp)
                && receipt.renderer_text_scale.is_finite()
                && (0.75..=2.0).contains(&receipt.renderer_text_scale)
                && receipt.renderer_opacity.is_finite()
                && (0.25..=1.0).contains(&receipt.renderer_opacity)
                && first_authority.is_some_and(|expected| {
                    expected
                        == (
                            receipt.renderer_authority_revision,
                            receipt.renderer_authority_sha256.as_str(),
                            &receipt.renderer_authority_sources,
                            receipt.renderer_style_id.as_str(),
                            receipt.renderer_safe_area_dp,
                            receipt.renderer_text_scale,
                            receipt.renderer_backplate_enabled,
                            receipt.renderer_opacity,
                        )
                })
                && identity_valid
                && sentences.insert(receipt.sentence_id)
                && self
                    .subtitles
                    .iter()
                    .any(|cue| cue.sentence_id == receipt.sentence_id)
        })
    }

    /// A live audio claim is valid only when every receipt is a completed,
    /// non-cancelled v2 native-broker receipt bound to this turn generation.
    /// Serde shape alone is never delivery evidence.
    pub fn audio_receipts_valid(&self) -> bool {
        let receipts = &self.audio_receipts;
        if receipts.is_empty() {
            return self.success.audio_receipt_count == 0
                && !self.success.audio_submitted
                && !self.success.audio_drained;
        }
        if receipts.len() != self.success.audio_receipt_count
            || !self.success.audio_submitted
            || !self.success.audio_drained
        {
            return false;
        }
        let mut receipt_ids = std::collections::BTreeSet::new();
        let mut stream_ids = std::collections::BTreeSet::new();
        let mut sentence_ids = std::collections::BTreeSet::new();
        let mut turn_identity: Option<(&str, &str)> = None;
        receipts.iter().all(|receipt| {
            let Some(stream_id) = receipt.stream_id.as_deref() else {
                return false;
            };
            let Some(session_id) = receipt.session_id.as_deref() else {
                return false;
            };
            let Some(turn_id) = receipt.turn_id.as_deref() else {
                return false;
            };
            let Some(output_endpoint_id) = receipt.output_endpoint_id.as_deref() else {
                return false;
            };
            let identity = (session_id, turn_id);
            if turn_identity.is_some_and(|expected| expected != identity) {
                return false;
            }
            turn_identity = Some(identity);
            valid_audio_identifier(&receipt.receipt_id)
                && valid_audio_identifier(stream_id)
                && valid_audio_identifier(session_id)
                && valid_audio_identifier(turn_id)
                && receipt.transport_schema_version == Some(2)
                && receipt.generation == Some(self.consumed_route.generation)
                && receipt.sentence_id > 0
                && receipt.source_frames > 0
                && receipt.device_frames == receipt.source_frames
                && receipt.duration_ms > 0
                && receipt
                    .peak
                    .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 1.0)
                && receipt
                    .rms
                    .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 1.0)
                && receipt.sink == "nativeBrokerSubmission"
                && receipt.submitted
                && receipt.drained
                && receipt.completed
                && receipt.cancelled == Some(false)
                && receipt.output_selection_mode.is_some()
                && !output_endpoint_id.is_empty()
                && output_endpoint_id.len() <= 1024
                && !output_endpoint_id.contains('\0')
                && receipt
                    .output_endpoint_generation
                    .is_some_and(|value| value > 0)
                && receipt_ids.insert(receipt.receipt_id.as_str())
                && stream_ids.insert(stream_id)
                && sentence_ids.insert(receipt.sentence_id)
        })
    }
}

fn valid_audio_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_stt_identity(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && !value.contains("..")
        && !value.chars().any(char::is_control)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeConsumedRouteSnapshot {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub generation: u64,
    pub sha256: String,
    pub llm: Option<NativeConsumedProviderRoute>,
    pub tts: Option<NativeConsumedProviderRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_evaluation_acknowledgement:
        Option<NativeConsumedPrivateEvaluationAcknowledgementV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeConsumedPrivateEvaluationAcknowledgementV1 {
    pub provider_id: String,
    pub terms_revision: String,
    pub catalog_revision: u64,
    pub application_namespace: String,
    pub modalities: Vec<NativePrivateEvaluationModalityV1>,
    pub acknowledgement_sha256: String,
    pub promotion_supported: bool,
    pub publication_supported: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativePrivateEvaluationModalityV1 {
    Llm,
    Embeddings,
    Tts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeConsumedProviderRoute {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTurnInputMode {
    Typed,
    PushToTalk,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativePushToTalkState {
    NotRequested,
    TranscriptReady,
    CaptureUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnInputEvidence {
    pub mode: NativeTurnInputMode,
    pub push_to_talk_state: NativePushToTalkState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_stt_receipt: Option<crate::sidecar_protocol::NativeSelectedSttTurnEvidenceV1>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTurnDeliveryState {
    Delivered,
    Cancelled,
    ManualRetryRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeDeliveryCommitState {
    Committed,
    NotCommitted,
    CommitDeferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSubtitleCue {
    pub sentence_id: u64,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSubtitlePresentationReceipt {
    pub receipt_id: String,
    pub sentence_id: u64,
    pub provenance: NativeSubtitlePresentationProvenance,
    pub presentation_id: u64,
    pub target_geometry_epoch: u64,
    pub capture_sequence: u64,
    pub graphics_generation: u64,
    pub layer_hash_hex: String,
    pub presented_qpc_ticks: String,
    pub desktop_x_px: i32,
    pub desktop_y_px: i32,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub direction: NativeSubtitleDirection,
    pub bidi_shaping_applied: bool,
    pub grapheme_clusters_preserved: bool,
    pub used_bottom_center_fallback: bool,
    pub color_treatment: NativeSubtitleColorTreatment,
    pub renderer_authority_revision: u64,
    pub renderer_authority_sha256: String,
    pub renderer_authority_sources: npc_subtitle_engine::SubtitleRendererAuthoritySourcesV1,
    pub renderer_style_id: String,
    pub renderer_safe_area_dp: f32,
    pub renderer_text_scale: f32,
    pub renderer_backplate_enabled: bool,
    pub renderer_opacity: f32,
    pub committed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSubtitlePresentationProvenance {
    TrustedNativeCapture,
    ConsoleBottomCenterUnavailable,
    DeterministicFixture,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSubtitleDirection {
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSubtitleColorTreatment {
    SdrPremultipliedSourceOver,
    ScrgbLinearSourceOver,
    Hdr10ToneMappedSourceOver,
    WindowsCompositorSdrWhiteMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeAudioReceiptSummary {
    pub receipt_id: String,
    pub sentence_id: u64,
    pub sink: String,
    #[serde(default)]
    pub transport_schema_version: Option<u32>,
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    pub source_frames: u64,
    pub device_frames: u64,
    pub duration_ms: u64,
    pub peak: Option<f32>,
    pub rms: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_selection_mode: Option<NativeAudioOutputSelectionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_endpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_endpoint_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    pub submitted: bool,
    pub drained: bool,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeAudioOutputSelectionMode {
    SystemDefault,
    EndpointId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NativeTurnExecutionDegradation {
    TypedInput {
        reason: String,
    },
    AudioOnly {
        reason: String,
    },
    SubtitleOnly {
        reason: String,
    },
    ManualRetryRequired {
        failed_role: String,
        provider_id: Option<String>,
        reason: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnExecutionSuccess {
    pub llm_provider_live: bool,
    pub tts_provider_live: bool,
    pub stt_skipped: bool,
    pub subtitle_delivered: bool,
    #[serde(default)]
    pub subtitle_receipt_count: usize,
    pub audio_submitted: bool,
    pub audio_drained: bool,
    pub audio_receipt_count: usize,
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
            fixture_first_audio_ms: Some(418),
            runtime_fixture_only: true,
            delivered_text: "The harbor remembers.".into(),
            turn_execution: None,
            character_context: None,
            visual_presentation: None,
        };
        let wire = serde_json::to_value(event).expect("serialize simulation event");
        assert_eq!(wire["type"], "completed");
        assert_eq!(wire["simulationId"], "simulation-fixture");
        assert_eq!(wire["fixtureFirstAudioMs"], 418);
        assert_eq!(wire["runtimeFixtureOnly"], true);
        assert_eq!(wire["deliveredText"], "The harbor remembers.");
        assert!(wire.get("turnExecution").is_none());
        assert!(wire.get("visualPresentation").is_none());
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
    fn webview_turn_request_cannot_mint_identity_engine_evidence() {
        let base = serde_json::json!({
            "gameProfileId": "skyrim-special-edition",
            "characterName": null,
            "characterId": "lydia",
            "transcript": "Hello",
            "executionMode": "hybrid",
            "enabledSpoilerTiers": []
        });
        let forged = serde_json::json!({
                "state": "matched",
                "encounter_id": "forged",
                "subject_id": "lydia",
                "subject_similarity": 1.0,
                "top_candidate_subject_id": "lydia",
                "top_candidate_similarity": 1.0,
                "runner_up_similarity": null,
                "top1_top2_margin": 1.0,
                "supporting_frames": 99,
                "window_frames": 99,
                "held_by_hysteresis": false
        });
        for field in ["identityDecision", "nativeIdentityDecision"] {
            let mut wire = base.clone();
            wire[field] = forged.clone();
            assert!(
                serde_json::from_value::<StartSimulationRequest>(wire).is_err(),
                "WebView request must reject {field}"
            );
        }
    }

    #[test]
    fn pending_provider_evidence_has_stable_camel_case_wire_value() {
        let event = SimulationEvent::Started {
            simulation_id: "simulation-live-route".into(),
            generation: 3,
            sequence: 1,
            measurement_basis: MeasurementBasis::PendingProviderEvidence,
        };
        let wire = serde_json::to_value(event).expect("serialize started event");
        assert_eq!(wire["measurementBasis"], "pendingProviderEvidence");
        assert!(wire.get("measurement_basis").is_none());
    }

    #[test]
    fn simulation_request_omits_dev_live_tts_by_default() {
        let wire = serde_json::to_value(StartSimulationRequest::default())
            .expect("serialize default simulation request");
        assert!(wire.get("devLiveTts").is_none());
        assert!(wire.get("selectedSttReceipt").is_none());
    }

    #[test]
    fn selected_stt_receipt_is_opaque_nonzero_and_separate_from_typed_input() {
        let receipt = SelectedSttReceiptReferenceV1 {
            receipt_id: "c57278dc-2d58-44aa-b7f8-c1a52036caef".into(),
            generation: 3,
        };
        assert!(StartSimulationRequest {
            transcript: None,
            selected_stt_receipt: Some(receipt.clone()),
            ..StartSimulationRequest::default()
        }
        .validate()
        .is_ok());
        assert!(StartSimulationRequest {
            transcript: Some("forged transcript".into()),
            selected_stt_receipt: Some(receipt.clone()),
            ..StartSimulationRequest::default()
        }
        .validate()
        .is_err());
        assert!(StartSimulationRequest {
            selected_stt_receipt: Some(SelectedSttReceiptReferenceV1 {
                receipt_id: "not-a-native-receipt".into(),
                generation: 0,
            }),
            ..StartSimulationRequest::default()
        }
        .validate()
        .is_err());
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
