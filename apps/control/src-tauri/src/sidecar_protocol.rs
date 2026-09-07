use crate::domain::NativeTurnExecutionEvidence;
use crate::media_broker::{AudioInputRehearsalLease, AudioPlaybackLease};
use npc_protocol::{
    control_response_v1, decode_frame, encode_frame, envelope_v1, ControlOperationV1,
    ControlRequestV1, EnvelopeV1, EnvelopeValidationContext, EnvelopeValidationError, LaunchNonce,
    NegotiationHelloV1, OrderingPolicy, ProtocolVersion, RequestId, SequenceTracker, SessionId,
    TraceId, TurnId, TurnProfileSafetyPolicyV1, TurnSafetyContextV1, TurnSafetyEvidenceStateV1,
    DEFAULT_MAX_FRAME_BYTES,
};
use npc_provider_loadouts::{
    EgressClassV1, ExecutionLocationV1, ExplicitFallbackV1, ProviderModelRouteV1, ProviderRole,
    TurnRouteDegradationCodeV1, TurnRouteSnapshotV1,
};
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{oneshot, Mutex};

pub const MAX_CONTROL_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_JSON_BYTES: usize = MAX_CONTROL_MESSAGE_BYTES / 2;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONTROL_DEADLINE: Duration = Duration::from_secs(60);

pub trait ControlStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> ControlStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}
pub type BoxedControlStream = Box<dyn ControlStream>;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum WireRequest {
    Ping,
    Doctor,
    ValidateProfiles,
    DiscoverTtsVoices(NativeTtsVoiceDiscoveryRequest),
    SimulateTurn(Box<NativeSimulationRequest>),
    TranscribeSelectedStt(Box<NativeSelectedSttControlRequest>),
    Cancel { new_generation: u64 },
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeControlTurnIdentity {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeSelectedSttControlRequest {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub route_snapshot_generation: u64,
    pub route: NativeSelectedProviderRoute,
    pub turn: NativeSelectedSttTurnRequest,
    pub lease: AudioInputRehearsalLease,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeSelectedSttTurnRequest {
    pub identity: NativeSelectedSttPcmIdentity,
    pub attempt: NativeSelectedSttAttemptAuthority,
    pub context_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeSelectedSttPcmIdentity {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum NativeSelectedSttAttemptAuthority {
    Initial,
    ManualRetry {
        prior_generation: u64,
        user_authorized: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSelectedSttControlResult {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub route_snapshot_generation: u64,
    pub result: NativeSelectedSttResult,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSelectedSttResult {
    pub transcript: String,
    pub route: NativeSelectedSttRouteReceipt,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSelectedSttRouteReceipt {
    pub provider_id: String,
    pub model_id: String,
    pub credential_reference: String,
    pub egress: String,
    pub generation: u64,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
    pub manual_retry: bool,
    pub automatic_fallback: bool,
    pub captured_frames: u64,
    pub ptt_virtual_key: u32,
    pub ptt_press_transition_sequence: u64,
    pub ptt_pressed_qpc: u64,
    pub ptt_release_transition_sequence: u64,
    pub ptt_released_qpc: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTtsVoiceDiscoveryRequest {
    pub schema_version: u32,
    pub provider_id: String,
    pub model_id: String,
    pub force_refresh: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTtsVoiceDiscoveryStatus {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTtsVoiceProvenance {
    ProviderStockDiscovery,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeDiscoveredStockVoice {
    pub voice_id: String,
    pub display_name: String,
    pub language: String,
    pub styles: Vec<String>,
    pub provenance: NativeTtsVoiceProvenance,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTtsVoiceRefreshEvidence {
    pub requested: bool,
    pub performed: bool,
    pub cache_hit: bool,
    pub refreshed_at_epoch_ms: Option<u64>,
    pub expires_at_epoch_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTtsVoiceDiscoveryError {
    pub code: String,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTtsVoiceDiscoveryResult {
    pub schema_version: u32,
    pub provider_id: String,
    pub model_id: String,
    pub status: NativeTtsVoiceDiscoveryStatus,
    pub voices: Vec<NativeDiscoveredStockVoice>,
    pub provenance: NativeTtsVoiceProvenance,
    pub refresh: NativeTtsVoiceRefreshEvidence,
    pub error: Option<NativeTtsVoiceDiscoveryError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSimulationRequest {
    pub session_id: String,
    pub turn_id: String,
    pub game_id: String,
    pub character_id: Option<String>,
    /// Effective data-only profile selected and validated by native Tauri.
    /// The authenticated runtime revalidates it against its bundled immutable
    /// game, detection, safety, and capability contract before use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_game_profile: Option<npc_game_profile::GameProfileV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_identity_decision: Option<npc_identity_engine::IdentityDecisionV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_spoiler_tiers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generic_selection: Option<NativeGenericGameSelection>,
    pub safety_context: NativeSimulationSafetyContext,
    pub transcript: String,
    pub locale: String,
    pub route_snapshot: NativeSelectedRouteSnapshot,
    pub input: NativeTurnInputSnapshot,
    pub delivery: NativeTurnDeliveryRequest,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_playback_leases: Vec<AudioPlaybackLease>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_evaluation_acknowledgements:
        Vec<crate::provider_loadouts::ProviderPrivateEvaluationAcknowledgementV1>,
    /// Native application identifier. This is never accepted from the WebView;
    /// runtime binds private-evaluation acknowledgements to this exact value.
    pub application_namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_presentation_context: Option<NativeSubtitlePresentationContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<NativeExecutionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_live_tts: Option<NativeDevLiveTtsRequest>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSubtitlePresentationContext {
    pub schema_version: u32,
    pub provenance: NativeSubtitleContextProvenance,
    pub target: Option<NativeSubtitleTargetIdentity>,
    pub viewport_px: NativeSubtitleRectPx,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub target_color_space: NativeSubtitleTargetColorSpace,
    pub sdr_white_level_nits: f32,
    /// Broker command 23 generation. `graphics_generation` carries the same
    /// identity through the existing presenter receipt contract.
    pub capture_device_generation: Option<u64>,
    pub geometry_epoch: Option<u64>,
    pub capture_sequence: Option<u64>,
    pub capture_qpc: Option<u64>,
    pub graphics_generation: Option<u64>,
    pub attested_at_qpc: Option<u64>,
    pub qpc_frequency: Option<u64>,
    pub attestation_id: Option<u64>,
    pub hud_exclusions_px: Vec<NativeSubtitleRectPx>,
    /// Atomic native-only renderer authority. The WebView never supplies or
    /// mutates this value; runtime and presenter must preserve its digest.
    pub renderer_authority: npc_subtitle_engine::SubtitleRendererAuthorityV1,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSubtitleContextProvenance {
    TrustedNativeCapture,
    ConsoleBottomCenterUnavailable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSubtitleTargetIdentity {
    pub process_id: u32,
    pub window_handle: u64,
    pub executable_name: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSubtitleTargetColorSpace {
    SdrSrgb,
    SdrScRgb,
    Hdr10Pq,
    HdrScRgb,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSubtitleRectPx {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl NativeSubtitlePresentationContext {
    /// Explicit sentinel for control-console presentation when no trusted
    /// capture geometry exists. Zero values are unavailable markers, not
    /// measured viewport, DPI, HDR, or luminance claims.
    pub fn console_unavailable(
        renderer_authority: npc_subtitle_engine::SubtitleRendererAuthorityV1,
    ) -> Self {
        Self {
            schema_version: 1,
            provenance: NativeSubtitleContextProvenance::ConsoleBottomCenterUnavailable,
            target: None,
            viewport_px: NativeSubtitleRectPx {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            dpi_x: 0,
            dpi_y: 0,
            target_color_space: NativeSubtitleTargetColorSpace::Unknown,
            sdr_white_level_nits: 0.0,
            capture_device_generation: None,
            geometry_epoch: None,
            capture_sequence: None,
            capture_qpc: None,
            graphics_generation: None,
            attested_at_qpc: None,
            qpc_frequency: None,
            attestation_id: None,
            hud_exclusions_px: Vec::new(),
            renderer_authority,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDevLiveTtsRequest {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: String,
    pub explicit_user_authorization: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSelectedRouteSnapshot {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub inheritance_chain: Vec<String>,
    pub generation: u64,
    pub roles: NativeSelectedRouteRoles,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSelectedRouteRoles {
    pub llm: NativeSelectedRoleRoute,
    pub stt: NativeSelectedRoleRoute,
    pub tts: NativeSelectedRoleRoute,
    pub embeddings: NativeSelectedRoleRoute,
    pub vision: NativeSelectedRoleRoute,
    #[serde(rename = "lipsync")]
    pub lip_sync: NativeSelectedRoleRoute,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeSelectedRouteState {
    Ready,
    Disabled,
    Degraded,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSelectedRoleRoute {
    pub state: NativeSelectedRouteState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<NativeSelectedProviderRoute>,
    pub fallbacks: Vec<NativeManualFallbackRoute>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation: Option<NativeRouteDegradation>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeRouteExecution {
    Cloud,
    Local,
    Off,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSelectedProviderRoute {
    pub provider_id: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub execution: NativeRouteExecution,
    pub egress: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_reference: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeManualFallbackRoute {
    #[serde(flatten)]
    pub route: NativeSelectedProviderRoute,
    pub activation: NativeManualFallbackActivation,
    pub user_authorized: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeManualFallbackActivation {
    ManualOnly,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeRouteDegradation {
    pub code: String,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativeTurnInputMode {
    Typed,
    PushToTalk,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativePushToTalkCaptureState {
    NotRequested,
    TranscriptReady,
    CaptureUnavailable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnInputSnapshot {
    pub mode: NativeTurnInputMode,
    pub push_to_talk_state: NativePushToTalkCaptureState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_stt_receipt: Option<NativeSelectedSttTurnEvidenceV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSelectedSttTurnEvidenceV1 {
    pub schema_version: u32,
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub capture_session_id: String,
    pub capture_turn_id: String,
    pub capture_generation: u64,
    pub game_id: String,
    pub character_id: Option<String>,
    pub source_loadout_id: String,
    pub route: NativeSelectedSttRouteReceipt,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeTurnDeliveryRequest {
    pub audio: bool,
    pub subtitles: bool,
}

impl From<&TurnRouteSnapshotV1> for NativeSelectedRouteSnapshot {
    fn from(snapshot: &TurnRouteSnapshotV1) -> Self {
        Self {
            schema_version: snapshot.schema_version,
            source_loadout_id: snapshot.source_loadout_id.to_string(),
            inheritance_chain: snapshot
                .inheritance_chain
                .iter()
                .map(ToString::to_string)
                .collect(),
            generation: snapshot.generation,
            roles: NativeSelectedRouteRoles {
                llm: native_role_snapshot(snapshot, ProviderRole::Llm),
                stt: native_role_snapshot(snapshot, ProviderRole::Stt),
                tts: native_role_snapshot(snapshot, ProviderRole::Tts),
                embeddings: native_role_snapshot(snapshot, ProviderRole::Embeddings),
                vision: native_role_snapshot(snapshot, ProviderRole::Vision),
                lip_sync: native_role_snapshot(snapshot, ProviderRole::Lipsync),
            },
        }
    }
}

fn native_role_snapshot(
    snapshot: &TurnRouteSnapshotV1,
    role: ProviderRole,
) -> NativeSelectedRoleRoute {
    let Some(route) = snapshot.role(role) else {
        return native_disabled_role();
    };
    NativeSelectedRoleRoute {
        state: match route.state {
            npc_provider_loadouts::TurnRouteStateV1::Ready => NativeSelectedRouteState::Ready,
            npc_provider_loadouts::TurnRouteStateV1::Disabled => NativeSelectedRouteState::Disabled,
            npc_provider_loadouts::TurnRouteStateV1::Degraded => NativeSelectedRouteState::Degraded,
        },
        primary: route.primary.as_ref().map(native_provider_route),
        fallbacks: route.fallbacks.iter().map(native_fallback).collect(),
        degradation: route.degradation.map(native_degradation),
    }
}

fn native_disabled_role() -> NativeSelectedRoleRoute {
    NativeSelectedRoleRoute {
        state: NativeSelectedRouteState::Disabled,
        primary: None,
        fallbacks: Vec::new(),
        degradation: Some(NativeRouteDegradation {
            code: "not_configured".into(),
            detail: "This optional route is disabled in the selected loadout.".into(),
            retryable: false,
        }),
    }
}

fn native_provider_route(route: &ProviderModelRouteV1) -> NativeSelectedProviderRoute {
    NativeSelectedProviderRoute {
        provider_id: route.provider_id.clone(),
        model_id: route.model_id.clone(),
        voice_id: route.voice_id.clone(),
        execution: match route.disclosure.execution {
            ExecutionLocationV1::Hosted => NativeRouteExecution::Cloud,
            ExecutionLocationV1::Local | ExecutionLocationV1::ExternalLocal => {
                NativeRouteExecution::Local
            }
        },
        egress: match route.disclosure.egress {
            EgressClassV1::None => "none",
            EgressClassV1::ProviderCloud => "providerCloud",
            EgressClassV1::UserConfiguredEndpoint => "userConfiguredEndpoint",
        }
        .into(),
        credential_reference: route
            .credential
            .as_ref()
            .map(|credential| format!("providers/{}", credential.provider_id)),
    }
}

fn native_fallback(fallback: &ExplicitFallbackV1) -> NativeManualFallbackRoute {
    NativeManualFallbackRoute {
        route: native_provider_route(&fallback.route),
        activation: NativeManualFallbackActivation::ManualOnly,
        user_authorized: fallback.user_authorized,
    }
}

fn native_degradation(
    degradation: npc_provider_loadouts::TurnRouteDegradationV1,
) -> NativeRouteDegradation {
    let (code, detail) = match degradation.code {
        TurnRouteDegradationCodeV1::NotConfigured => (
            "not_configured",
            "This optional route is disabled in the selected loadout.",
        ),
        TurnRouteDegradationCodeV1::CredentialUnavailable => (
            "credential_unavailable",
            "The configured credential reference is unavailable.",
        ),
        TurnRouteDegradationCodeV1::ProviderUnavailable => (
            "provider_unavailable",
            "The selected provider is unavailable for this turn.",
        ),
        TurnRouteDegradationCodeV1::ModelUnavailable => (
            "model_unavailable",
            "The selected model is unavailable for this turn.",
        ),
        TurnRouteDegradationCodeV1::PolicyBlocked => (
            "policy_blocked",
            "Safety or privacy policy blocked this route.",
        ),
        TurnRouteDegradationCodeV1::ResourceUnavailable => (
            "resource_unavailable",
            "The selected local resource is unavailable for this turn.",
        ),
    };
    NativeRouteDegradation {
        code: code.into(),
        detail: detail.into(),
        retryable: degradation.retryable,
    }
}

pub type NativeSimulationSafetyContext = TurnSafetyContextV1;
pub type NativeSafetyEvidenceState = TurnSafetyEvidenceStateV1;
pub type NativeProfileSafetyPolicy = TurnProfileSafetyPolicyV1;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeExecutionMode {
    Cloud,
    Hybrid,
    Local,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeGenericGameSelection {
    pub game_name: String,
    pub executable_name: String,
    pub character_name: String,
    pub protected_online_detected: bool,
    pub anti_cheat_detected: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)]
pub enum WireResponse {
    Pong {},
    Doctor {
        report: NativeDoctorReport,
    },
    Profiles {
        profiles: Vec<NativeProfile>,
    },
    TtsVoices {
        result: NativeTtsVoiceDiscoveryResult,
    },
    Simulation {
        result: NativeSimulationResult,
    },
    SelectedStt {
        result: Box<NativeSelectedSttControlResult>,
    },
    Cancelled {
        generation: u64,
    },
    ShuttingDown {},
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProfile {
    pub id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDoctorReport {
    pub schema_version: String,
    pub status: String,
    pub profile_count: usize,
    pub authored_game_profile_count: usize,
    pub synthetic_review_profile_count: usize,
    pub synthetic_review_profile_ids: Vec<String>,
    pub provider_count: usize,
    pub model_count: usize,
    pub discovered_installation_count: usize,
    pub discovery_error_count: usize,
    pub vector_backend: String,
    pub hosted_provider_contracts: Vec<String>,
    pub model_manifest_example: String,
    pub performance_measurements_captured: bool,
    pub power_profile_changed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSimulationResult {
    pub schema_version: String,
    pub fixture_only: bool,
    #[serde(default = "legacy_simulation_integration_mode")]
    pub integration_mode: String,
    #[serde(default)]
    pub capability_notices: Vec<String>,
    pub events: Vec<Value>,
    pub outcome: Value,
    #[serde(default)]
    pub character_context: Option<NativeCharacterContextEvidence>,
    #[serde(default)]
    pub turn_execution: Option<Box<NativeTurnExecutionEvidence>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeCharacterContextEvidence {
    pub profile_id: String,
    pub character_id: String,
    #[serde(default)]
    pub selection: Option<npc_character_db::SelectionOutcomeV1>,
    pub identity_source: String,
    pub explicit_selection: bool,
    #[serde(default)]
    pub encounter: Option<npc_character_db::EncounterRecordV1>,
    #[serde(default)]
    pub prompt: Option<NativePromptAssemblyEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativePromptAssemblyEvidence {
    pub schema_version: String,
    pub profile_id: String,
    pub character_id: String,
    pub authorities: Vec<npc_game_profile::PromptAuthority>,
    pub record_count: usize,
    pub retrieval_provenance: npc_character_db::RetrievalProvenanceV1,
    #[serde(default)]
    pub scoped_memory_item_ids: Vec<String>,
    #[serde(default)]
    pub scoped_memory_classes: Vec<String>,
}

fn legacy_simulation_integration_mode() -> String {
    "legacy_schema_1".into()
}

struct PendingRequest {
    request_sequence: u64,
    operation: ControlOperationV1,
    cancellation_generation: u64,
    turn_id: Option<TurnId>,
    trace_id: TraceId,
    deadline_qpc_ticks: u64,
    sender: oneshot::Sender<Result<WireResponse, ClientError>>,
}

struct WriterState {
    writer: tokio::io::WriteHalf<BoxedControlStream>,
    next_sequence: u64,
    cancellation_generation: u64,
    launch_nonce: LaunchNonce,
    session_id: SessionId,
}

struct HandshakeState {
    launch_nonce: LaunchNonce,
    session_id: SessionId,
    qpc_frequency_hz: u64,
    response_tracker: SequenceTracker,
}

#[derive(Clone)]
pub struct ControlClient {
    writer: Arc<Mutex<WriterState>>,
    pending: Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
}

impl std::fmt::Debug for ControlClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ControlClient { authenticated: true }")
    }
}

impl ControlClient {
    pub async fn connect(
        mut stream: BoxedControlStream,
        nonce: LaunchNonce,
        peer_build: &str,
    ) -> Result<Self, ClientError> {
        let session = SessionId::new();
        let handshake = handshake(&mut stream, nonce, session, peer_build).await?;
        let (reader, writer) = tokio::io::split(stream);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let launch_nonce = handshake.launch_nonce.clone();
        let session_id = handshake.session_id.clone();
        tauri::async_runtime::spawn(read_responses(reader, Arc::clone(&pending), handshake));
        Ok(Self {
            writer: Arc::new(Mutex::new(WriterState {
                writer,
                next_sequence: 2,
                cancellation_generation: 0,
                launch_nonce,
                session_id,
            })),
            pending,
        })
    }

    pub async fn ping(&self) -> Result<(), ClientError> {
        match self
            .request_with_timeout(WireRequest::Ping, HEALTH_TIMEOUT)
            .await?
        {
            WireResponse::Pong {} => Ok(()),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn doctor(&self) -> Result<NativeDoctorReport, ClientError> {
        match self.request(WireRequest::Doctor).await? {
            WireResponse::Doctor { report } => Ok(report),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn profiles(&self) -> Result<Vec<NativeProfile>, ClientError> {
        match self.request(WireRequest::ValidateProfiles).await? {
            WireResponse::Profiles { profiles } => Ok(profiles),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn discover_tts_voices(
        &self,
        request: NativeTtsVoiceDiscoveryRequest,
    ) -> Result<NativeTtsVoiceDiscoveryResult, ClientError> {
        match self
            .request(WireRequest::DiscoverTtsVoices(request))
            .await?
        {
            WireResponse::TtsVoices { result } => Ok(result),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn simulate(
        &self,
        request: NativeSimulationRequest,
    ) -> Result<NativeSimulationResult, ClientError> {
        match self
            .request(WireRequest::SimulateTurn(Box::new(request)))
            .await?
        {
            WireResponse::Simulation { result } => Ok(result),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub(crate) async fn prepare_selected_stt_turn_identity(&self) -> NativeControlTurnIdentity {
        let writer = self.writer.lock().await;
        NativeControlTurnIdentity {
            session_id: writer.session_id.to_string(),
            turn_id: TurnId::new().to_string(),
        }
    }

    pub(crate) async fn transcribe_selected_stt(
        &self,
        request: NativeSelectedSttControlRequest,
    ) -> Result<NativeSelectedSttControlResult, ClientError> {
        match self
            .request_with_timeout(
                WireRequest::TranscribeSelectedStt(Box::new(request)),
                MAX_CONTROL_DEADLINE,
            )
            .await?
        {
            WireResponse::SelectedStt { result } => Ok(*result),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn cancel(&self) -> Result<u64, ClientError> {
        let (request_id, receiver) = {
            let mut writer = self.writer.lock().await;
            let new_generation = writer.cancellation_generation.saturating_add(1);
            let frame_generation = writer.cancellation_generation;
            let request = WireRequest::Cancel { new_generation };
            let pair = enqueue_and_write(
                &mut writer,
                &self.pending,
                request,
                frame_generation,
                new_generation,
                HEALTH_TIMEOUT,
            )
            .await?;
            writer.cancellation_generation = new_generation;
            cancel_superseded_pending(&self.pending, new_generation, &pair.0).await;
            pair
        };
        let response = await_response(&self.pending, &request_id, receiver, HEALTH_TIMEOUT).await?;
        match response {
            WireResponse::Cancelled { generation } => Ok(generation),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    pub async fn shutdown(&self) -> Result<(), ClientError> {
        match self
            .request_with_timeout(WireRequest::Shutdown, HEALTH_TIMEOUT)
            .await?
        {
            WireResponse::ShuttingDown {} => Ok(()),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    async fn request(&self, request: WireRequest) -> Result<WireResponse, ClientError> {
        self.request_with_timeout(request, REQUEST_TIMEOUT).await
    }

    async fn request_with_timeout(
        &self,
        request: WireRequest,
        timeout: Duration,
    ) -> Result<WireResponse, ClientError> {
        let (request_id, receiver) = {
            let mut writer = self.writer.lock().await;
            let generation = writer.cancellation_generation;
            enqueue_and_write(
                &mut writer,
                &self.pending,
                request,
                generation,
                generation,
                timeout,
            )
            .await?
        };
        await_response(&self.pending, &request_id, receiver, timeout).await
    }
}

async fn enqueue_and_write(
    writer: &mut WriterState,
    pending: &Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    request: WireRequest,
    frame_generation: u64,
    minimum_response_generation: u64,
    timeout: Duration,
) -> Result<
    (
        RequestId,
        oneshot::Receiver<Result<WireResponse, ClientError>>,
    ),
    ClientError,
> {
    let request_id = RequestId::new();
    let operation = wire_operation(&request);
    let turn_id = match &request {
        WireRequest::SimulateTurn(_) => Some(TurnId::new()),
        WireRequest::TranscribeSelectedStt(request) => {
            if request.turn.identity.session_id != writer.session_id.to_string() {
                return Err(ClientError::Malformed);
            }
            Some(
                request
                    .turn
                    .identity
                    .turn_id
                    .parse()
                    .map_err(|_| ClientError::Malformed)?,
            )
        }
        _ => None,
    };
    let trace_id = TraceId::new();
    let request_json = serde_json::to_vec(&request).map_err(|_| ClientError::Malformed)?;
    if request_json.len() > MAX_RESPONSE_JSON_BYTES {
        return Err(ClientError::PayloadTooLarge);
    }
    let sequence = writer.next_sequence;
    writer.next_sequence = writer.next_sequence.saturating_add(1);
    let now = qpc_now();
    let deadline_qpc_ticks = now.0.saturating_add(duration_to_qpc_ticks(timeout, now.1));
    let envelope = EnvelopeV1::new(
        writer.launch_nonce.clone(),
        writer.session_id.clone(),
        turn_id.clone(),
        trace_id.clone(),
        sequence,
        now.0,
        now.1,
        deadline_qpc_ticks,
        frame_generation,
        envelope_v1::Body::ControlRequest(ControlRequestV1 {
            request_id: Some(request_id.clone()),
            operation: operation as i32,
            payload_json: request_json,
        }),
    )
    .map_err(|_| ClientError::Malformed)?;
    let context = control_context(now.0, &writer.launch_nonce, &writer.session_id);
    let frame = encode_frame(&envelope, &context).map_err(|_| ClientError::Malformed)?;
    let (sender, receiver) = oneshot::channel();
    pending.lock().await.insert(
        request_id.clone(),
        PendingRequest {
            request_sequence: sequence,
            operation,
            cancellation_generation: minimum_response_generation,
            turn_id,
            trace_id,
            deadline_qpc_ticks,
            sender,
        },
    );
    if let Err(error) =
        write_frame(&mut writer.writer, &frame[4..], MAX_CONTROL_MESSAGE_BYTES).await
    {
        pending.lock().await.remove(&request_id);
        return Err(error);
    }
    Ok((request_id, receiver))
}

async fn await_response(
    pending: &Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    request_id: &RequestId,
    receiver: oneshot::Receiver<Result<WireResponse, ClientError>>,
    timeout: Duration,
) -> Result<WireResponse, ClientError> {
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(ClientError::Disconnected),
        Err(_) => {
            pending.lock().await.remove(request_id);
            Err(ClientError::Timeout)
        }
    }
}

async fn read_responses(
    mut reader: tokio::io::ReadHalf<BoxedControlStream>,
    pending: Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    mut handshake: HandshakeState,
) {
    loop {
        let frame = match read_frame(&mut reader, MAX_CONTROL_MESSAGE_BYTES).await {
            Ok(frame) => frame,
            Err(_) => break,
        };
        let now = qpc_now();
        let mut context = control_context(now.0, &handshake.launch_nonce, &handshake.session_id);
        let envelope = match EnvelopeV1::decode(&frame[4..]) {
            Ok(envelope) => envelope,
            Err(_) => break,
        };
        let late = match envelope.validate(&context) {
            Ok(()) => false,
            Err(EnvelopeValidationError::DeadlineExceeded) => {
                // A response may race a local timeout. Validate every other
                // field at the original deadline, consume its sequence, then
                // discard it without reconnecting or resurrecting the request.
                context.now_qpc_ticks = envelope.deadline_qpc_ticks;
                true
            }
            Err(_) => break,
        };
        if validate_control_timing(&envelope, now, handshake.qpc_frequency_hz).is_err() {
            break;
        }
        if handshake
            .response_tracker
            .observe(&envelope, context)
            .is_err()
        {
            break;
        }
        let response_frame = match envelope.body {
            Some(envelope_v1::Body::ControlResponse(response)) => response,
            _ => break,
        };
        let request_id = match response_frame.request_id.clone() {
            Some(request_id) => request_id,
            None => break,
        };
        if late {
            if let Some(waiting) = pending.lock().await.remove(&request_id) {
                let _ = waiting.sender.send(Err(ClientError::Timeout));
            }
            continue;
        }
        let Some(waiting) = pending.lock().await.remove(&request_id) else {
            continue;
        };
        let metadata_valid = response_frame.request_sequence == waiting.request_sequence
            && response_frame.operation() == waiting.operation
            && envelope.cancellation_generation == waiting.cancellation_generation
            && envelope.turn_id == waiting.turn_id
            && envelope.trace_id.as_ref() == Some(&waiting.trace_id)
            && envelope.deadline_qpc_ticks == waiting.deadline_qpc_ticks;
        let response = if !metadata_valid {
            Err(ClientError::Malformed)
        } else {
            match response_frame.body {
                Some(control_response_v1::Body::SuccessJson(json))
                    if json.len() <= MAX_RESPONSE_JSON_BYTES =>
                {
                    serde_json::from_slice(&json)
                        .map_err(|_| ClientError::Malformed)
                        .and_then(|response: WireResponse| {
                            let execution_contract_valid = match &response {
                                WireResponse::Simulation { result } => {
                                    match result.turn_execution.as_ref() {
                                        Some(evidence) => {
                                            evidence.input_evidence_valid()
                                                && evidence.subtitle_receipts_valid()
                                                && evidence.audio_receipts_valid()
                                        }
                                        None => true,
                                    }
                                }
                                _ => true,
                            };
                            execution_contract_valid
                                .then_some(response)
                                .ok_or(ClientError::Malformed)
                        })
                }
                Some(control_response_v1::Body::Error(error)) => Err(ClientError::Remote {
                    code: error.message,
                    retryable: error.retryable,
                }),
                _ => Err(ClientError::Malformed),
            }
        };
        let malformed = matches!(response, Err(ClientError::Malformed));
        let _ = waiting.sender.send(response);
        if malformed {
            break;
        }
    }
    fail_all_pending(&pending, ClientError::Disconnected).await;
}

async fn fail_all_pending(
    pending: &Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    error: ClientError,
) {
    let drained: Vec<_> = pending
        .lock()
        .await
        .drain()
        .map(|(_, value)| value)
        .collect();
    for request in drained {
        let _ = request.sender.send(Err(error.clone()));
    }
}

async fn cancel_superseded_pending(
    pending: &Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    generation: u64,
    except: &RequestId,
) {
    let mut pending = pending.lock().await;
    let obsolete: Vec<_> = pending
        .iter()
        .filter(|(id, request)| *id != except && request.cancellation_generation < generation)
        .map(|(id, _)| id.clone())
        .collect();
    for id in obsolete {
        if let Some(request) = pending.remove(&id) {
            let _ = request.sender.send(Err(ClientError::Cancelled));
        }
    }
}

async fn handshake(
    stream: &mut BoxedControlStream,
    nonce: LaunchNonce,
    session: SessionId,
    peer_build: &str,
) -> Result<HandshakeState, ClientError> {
    let (ticks, frequency) = qpc_now();
    let trace = TraceId::new();
    let hello = EnvelopeV1::new(
        nonce.clone(),
        session.clone(),
        None,
        trace.clone(),
        1,
        ticks,
        frequency,
        ticks.saturating_add(frequency.saturating_mul(10)),
        0,
        envelope_v1::Body::NegotiationHello(NegotiationHelloV1 {
            minimum: Some(ProtocolVersion::CURRENT),
            maximum: Some(ProtocolVersion::CURRENT),
            optional_features: vec![
                "envelope-control-v1".into(),
                "cancellation-generation".into(),
            ],
            peer_name: "response-console".into(),
            peer_build: peer_build.chars().take(64).collect(),
        }),
    )
    .map_err(|_| ClientError::Handshake)?;
    let mut context = EnvelopeValidationContext::permissive_for_time(ticks);
    context.expected_launch_nonce = Some(nonce.clone());
    context.expected_session_id = Some(session.clone());
    let frame = encode_frame(&hello, &context).map_err(|_| ClientError::Handshake)?;
    write_frame(stream, &frame[4..], DEFAULT_MAX_FRAME_BYTES).await?;
    let accepted =
        tokio::time::timeout(HEALTH_TIMEOUT, read_frame(stream, DEFAULT_MAX_FRAME_BYTES))
            .await
            .map_err(|_| ClientError::Timeout)??;
    let envelope = decode_frame(&accepted, &context).map_err(|_| ClientError::Handshake)?;
    validate_control_timing(&envelope, qpc_now(), frequency).map_err(|_| ClientError::Handshake)?;
    if envelope.trace_id.as_ref() != Some(&trace)
        || envelope.turn_id.is_some()
        || envelope.cancellation_generation != 0
    {
        return Err(ClientError::Handshake);
    }
    let mut response_tracker = SequenceTracker::new(
        nonce.clone(),
        session.clone(),
        OrderingPolicy::StrictContiguous,
    );
    response_tracker
        .observe(&envelope, context)
        .map_err(|_| ClientError::Handshake)?;
    let accepted = match envelope.body {
        Some(envelope_v1::Body::NegotiationAccepted(accepted)) => accepted,
        _ => return Err(ClientError::Handshake),
    };
    if accepted.selected != Some(ProtocolVersion::CURRENT)
        || !accepted
            .enabled_features
            .iter()
            .any(|feature| feature == "envelope-control-v1")
    {
        return Err(ClientError::Handshake);
    }
    Ok(HandshakeState {
        launch_nonce: nonce,
        session_id: session,
        qpc_frequency_hz: frequency,
        response_tracker,
    })
}

fn wire_operation(request: &WireRequest) -> ControlOperationV1 {
    match request {
        WireRequest::Ping => ControlOperationV1::Ping,
        WireRequest::Doctor => ControlOperationV1::Doctor,
        WireRequest::ValidateProfiles => ControlOperationV1::ValidateProfiles,
        WireRequest::DiscoverTtsVoices(_) => ControlOperationV1::DiscoverTtsVoices,
        WireRequest::SimulateTurn(_) => ControlOperationV1::SimulateTurn,
        WireRequest::TranscribeSelectedStt(_) => ControlOperationV1::TranscribeSelectedStt,
        WireRequest::Cancel { .. } => ControlOperationV1::Cancel,
        WireRequest::Shutdown => ControlOperationV1::Shutdown,
    }
}

fn control_context(
    now_qpc_ticks: u64,
    launch_nonce: &LaunchNonce,
    session_id: &SessionId,
) -> EnvelopeValidationContext {
    let mut context = EnvelopeValidationContext::permissive_for_time(now_qpc_ticks);
    context.expected_launch_nonce = Some(launch_nonce.clone());
    context.expected_session_id = Some(session_id.clone());
    context.max_body_bytes = MAX_CONTROL_MESSAGE_BYTES - 4 * 1024;
    context.max_frame_bytes = MAX_CONTROL_MESSAGE_BYTES;
    context
}

fn validate_control_timing(
    envelope: &EnvelopeV1,
    now: (u64, u64),
    negotiated_frequency: u64,
) -> Result<(), ClientError> {
    if envelope.qpc_frequency_hz != negotiated_frequency
        || envelope.qpc_frequency_hz != now.1
        || envelope.qpc_timestamp_ticks > now.0.saturating_add(now.1)
        || envelope.deadline_qpc_ticks
            > now
                .0
                .saturating_add(now.1.saturating_mul(MAX_CONTROL_DEADLINE.as_secs()))
    {
        return Err(ClientError::Malformed);
    }
    Ok(())
}

fn duration_to_qpc_ticks(duration: Duration, frequency: u64) -> u64 {
    let whole = duration.as_secs().saturating_mul(frequency);
    let fractional = u64::from(duration.subsec_nanos()).saturating_mul(frequency) / 1_000_000_000;
    whole.saturating_add(fractional).max(1)
}

pub async fn read_frame<R>(reader: &mut R, maximum: usize) -> Result<Vec<u8>, ClientError>
where
    R: AsyncRead + Unpin + ?Sized,
{
    let mut length = [0_u8; 4];
    reader
        .read_exact(&mut length)
        .await
        .map_err(|_| ClientError::Disconnected)?;
    let body_len = u32::from_le_bytes(length) as usize;
    if body_len == 0 || body_len.saturating_add(4) > maximum {
        return Err(ClientError::PayloadTooLarge);
    }
    let mut frame = Vec::with_capacity(body_len + 4);
    frame.extend_from_slice(&length);
    frame.resize(body_len + 4, 0);
    reader
        .read_exact(&mut frame[4..])
        .await
        .map_err(|_| ClientError::Disconnected)?;
    Ok(frame)
}

pub async fn write_frame<W>(writer: &mut W, body: &[u8], maximum: usize) -> Result<(), ClientError>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    if body.is_empty() || body.len().saturating_add(4) > maximum {
        return Err(ClientError::PayloadTooLarge);
    }
    let length = u32::try_from(body.len()).map_err(|_| ClientError::PayloadTooLarge)?;
    writer
        .write_all(&length.to_le_bytes())
        .await
        .map_err(|_| ClientError::Disconnected)?;
    writer
        .write_all(body)
        .await
        .map_err(|_| ClientError::Disconnected)?;
    writer.flush().await.map_err(|_| ClientError::Disconnected)
}

fn qpc_now() -> (u64, u64) {
    #[cfg(windows)]
    {
        let mut value = 0_i64;
        let mut frequency = 0_i64;
        // SAFETY: both APIs write to valid stack-owned i64 values.
        unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value);
            windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
        }
        (value.max(0) as u64, frequency.max(1) as u64)
    }
    #[cfg(not(windows))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        (
            START.get_or_init(Instant::now).elapsed().as_nanos() as u64,
            1_000_000_000,
        )
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
    #[error("runtime control handshake failed")]
    Handshake,
    #[error("runtime control message was malformed")]
    Malformed,
    #[error("runtime control payload exceeded its bound")]
    PayloadTooLarge,
    #[error("runtime control connection ended")]
    Disconnected,
    #[error("runtime control request timed out")]
    Timeout,
    #[error("runtime control request was superseded by cancellation")]
    Cancelled,
    #[error("runtime returned an unexpected response")]
    UnexpectedResponse,
    #[error("runtime rejected the request with code {code}")]
    Remote { code: String, retryable: bool },
}

impl ClientError {
    pub fn should_restart_runtime(&self) -> bool {
        !matches!(self, Self::Remote { .. })
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments, clippy::unwrap_used)]
mod tests {
    use super::*;
    use npc_protocol::{control_response_v1, ControlResponseV1, NegotiationAcceptedV1};
    use npc_provider_loadouts::{LoadoutContextV1, RoleOverrideV1, ValidationContextV1};

    fn selected_routes(generation: u64) -> NativeSelectedRouteSnapshot {
        let snapshot = crate::provider_loadouts::starter_document()
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("starter routes resolve")
            .pin_turn_routes(generation);
        NativeSelectedRouteSnapshot::from(&snapshot)
    }

    #[test]
    fn authenticated_stock_voice_discovery_wire_is_exact_and_credential_free() {
        let request = WireRequest::DiscoverTtsVoices(NativeTtsVoiceDiscoveryRequest {
            schema_version: 1,
            provider_id: "nvidia-nim-magpie".into(),
            model_id: "magpie-tts-multilingual".into(),
            force_refresh: true,
        });
        assert_eq!(
            wire_operation(&request),
            ControlOperationV1::DiscoverTtsVoices
        );
        let wire = serde_json::to_value(&request).expect("voice discovery request wire");
        assert_eq!(wire["type"], "discover_tts_voices");
        assert_eq!(wire["schemaVersion"], 1);
        assert_eq!(wire["providerId"], "nvidia-nim-magpie");
        assert_eq!(wire["modelId"], "magpie-tts-multilingual");
        assert_eq!(wire["forceRefresh"], true);
        let encoded = wire.to_string();
        assert!(!encoded.to_ascii_lowercase().contains("credential"));
        assert!(!encoded.to_ascii_lowercase().contains("authorization"));

        let response = br#"{
            "type":"tts_voices",
            "result":{
                "schemaVersion":1,
                "providerId":"nvidia-nim-magpie",
                "modelId":"magpie-tts-multilingual",
                "status":"available",
                "voices":[{"voiceId":"Magpie-Multilingual.EN-US.Aria","displayName":"Aria","language":"en-US","styles":[],"provenance":"providerStockDiscovery"}],
                "provenance":"providerStockDiscovery",
                "refresh":{"requested":true,"performed":true,"cacheHit":false,"refreshedAtEpochMs":123,"expiresAtEpochMs":300123},
                "error":null
            }
        }"#;
        let WireResponse::TtsVoices { result } =
            serde_json::from_slice::<WireResponse>(response).expect("voice discovery response")
        else {
            panic!("voice discovery response expected")
        };
        assert_eq!(result.status, NativeTtsVoiceDiscoveryStatus::Available);
        assert_eq!(result.voices.len(), 1);
        assert_eq!(result.voices[0].display_name, "Aria");
        assert!(result.error.is_none());
    }

    fn typed_input() -> NativeTurnInputSnapshot {
        NativeTurnInputSnapshot {
            mode: NativeTurnInputMode::Typed,
            push_to_talk_state: NativePushToTalkCaptureState::NotRequested,
            selected_stt_receipt: None,
        }
    }

    fn delivery() -> NativeTurnDeliveryRequest {
        NativeTurnDeliveryRequest {
            audio: true,
            subtitles: true,
        }
    }

    fn response_envelope(
        nonce: &LaunchNonce,
        session: &SessionId,
        request_id: RequestId,
        trace_id: TraceId,
        response_sequence: u64,
        request_sequence: u64,
        timestamp: u64,
        frequency: u64,
        deadline: u64,
    ) -> EnvelopeV1 {
        EnvelopeV1::new(
            nonce.clone(),
            session.clone(),
            None,
            trace_id,
            response_sequence,
            timestamp,
            frequency,
            deadline,
            0,
            envelope_v1::Body::ControlResponse(ControlResponseV1 {
                request_id: Some(request_id),
                request_sequence,
                operation: ControlOperationV1::Ping as i32,
                body: Some(control_response_v1::Body::SuccessJson(
                    br#"{"type":"pong"}"#.to_vec(),
                )),
            }),
        )
        .expect("response envelope")
    }

    #[tokio::test]
    async fn oversized_prefix_is_rejected_before_allocation() {
        let (mut writer, mut reader) = tokio::io::duplex(16);
        tokio::spawn(async move {
            writer
                .write_all(&u32::MAX.to_le_bytes())
                .await
                .expect("write length");
        });
        assert!(matches!(
            read_frame(&mut reader, 1024).await,
            Err(ClientError::PayloadTooLarge)
        ));
    }

    #[test]
    fn response_contract_rejects_unknown_fields() {
        let response = br#"{"type":"pong","secret":"must-not-pass"}"#;
        // Serde's tagged unit variant rejects the extra content as a malformed
        // response rather than creating a generic map that could leak it.
        assert!(serde_json::from_slice::<WireResponse>(response).is_err());
    }

    #[test]
    fn schema_one_simulation_response_defaults_new_metadata() {
        let response = br#"{
            "type":"simulation",
            "result":{
                "schemaVersion":"1.0.0",
                "fixtureOnly":true,
                "events":[],
                "outcome":{}
            }
        }"#;
        let WireResponse::Simulation { result } =
            serde_json::from_slice::<WireResponse>(response).expect("legacy schema-one response")
        else {
            panic!("simulation response expected");
        };
        assert_eq!(result.integration_mode, "legacy_schema_1");
        assert!(result.capability_notices.is_empty());
        assert!(result.turn_execution.is_none());
    }

    #[test]
    fn simulation_response_preserves_receipt_backed_turn_execution_evidence() {
        let response = br#"{
            "type":"simulation",
            "result":{
                "schemaVersion":"1.0.0",
                "fixtureOnly":false,
                "events":[],
                "outcome":{"lifecycle":"completed"},
                "turnExecution":{
                    "consumedRoute":{"schemaVersion":1,"sourceLoadoutId":"api-first-starter","generation":9,"sha256":"abc123","llm":{"providerId":"nvidia-nim","modelId":"nemotron","voiceId":null},"tts":{"providerId":"elevenlabs","modelId":"eleven_flash_v2_5","voiceId":"EXAVITQu4vr4xnSDxMaL"}},
                    "input":{"mode":"typed","pushToTalkState":"notRequested"},
                    "deliveryState":"delivered",
                    "commitState":"committed",
                    "subtitles":[{"sentenceId":1,"textStartBytes":0,"textEndBytes":5,"speaker":"Mara","text":"Hello"}],
                    "subtitlePresentationReceipts":[{"receiptId":"subtitle-session-turn-1","sentenceId":1,"provenance":"consoleBottomCenterUnavailable","presentationId":4,"targetGeometryEpoch":0,"captureSequence":0,"graphicsGeneration":0,"layerHashHex":"0123456789abcdef","presentedQpcTicks":"123456789","desktopXPx":440,"desktopYPx":760,"widthPx":640,"heightPx":120,"dpiX":144,"dpiY":144,"direction":"leftToRight","bidiShapingApplied":true,"graphemeClustersPreserved":true,"usedBottomCenterFallback":true,"colorTreatment":"windowsCompositorSdrWhiteMapping","committed":true}],
                    "audioReceipts":[{"receiptId":"receipt-turn-9-sentence-1","sentenceId":1,"sink":"nativeBrokerSubmission","transportSchemaVersion":2,"streamId":"pcm-turn-9-sentence-1","sessionId":"response-console-simulation","turnId":"runtime-simulation-000009","generation":9,"sourceFrames":24000,"deviceFrames":24000,"durationMs":500,"peak":0.5,"rms":0.1,"outputSelectionMode":"systemDefault","outputEndpointId":"{0.0.0.00000000}.fixture-output","outputEndpointGeneration":17,"cancelled":false,"submitted":true,"drained":true,"completed":true}],
                    "degradations":[{"type":"manualRetryRequired","failedRole":"llm","providerId":"nvidia-nim","reason":"Provider timed out; choose the authorized fallback for a new turn.","retryable":true}],
                    "success":{"llmProviderLive":true,"ttsProviderLive":true,"sttSkipped":true,"subtitleDelivered":true,"subtitleReceiptCount":1,"audioSubmitted":true,"audioDrained":true,"audioReceiptCount":1}
                }
            }
        }"#;
        let mut response_value: serde_json::Value =
            serde_json::from_slice(response).expect("turn execution fixture JSON");
        let authority = npc_subtitle_engine::bundled_default_renderer_authority()
            .expect("bundled renderer authority");
        let receipt = response_value
            .pointer_mut("/result/turnExecution/subtitlePresentationReceipts/0")
            .and_then(serde_json::Value::as_object_mut)
            .expect("subtitle receipt fixture");
        receipt.insert(
            "rendererAuthorityRevision".into(),
            serde_json::json!(authority.revision),
        );
        receipt.insert(
            "rendererAuthoritySha256".into(),
            serde_json::json!(authority.authority_sha256),
        );
        receipt.insert(
            "rendererAuthoritySources".into(),
            serde_json::to_value(&authority.sources).expect("authority sources"),
        );
        receipt.insert(
            "rendererStyleId".into(),
            serde_json::json!(authority.style.id),
        );
        receipt.insert(
            "rendererSafeAreaDp".into(),
            serde_json::json!(authority.style.geometry.safe_margin_dp),
        );
        receipt.insert(
            "rendererTextScale".into(),
            serde_json::json!(authority.text_scale),
        );
        receipt.insert(
            "rendererBackplateEnabled".into(),
            serde_json::json!(authority.style.effects.backplate.enabled),
        );
        receipt.insert(
            "rendererOpacity".into(),
            serde_json::json!(authority.opacity),
        );
        let WireResponse::Simulation { result } =
            serde_json::from_value::<WireResponse>(response_value)
                .expect("turn execution response")
        else {
            panic!("simulation response expected");
        };
        let evidence = result.turn_execution.expect("turn execution evidence");
        assert!(evidence.input_evidence_valid());
        let mut contradictory_input = evidence.clone();
        contradictory_input.success.stt_skipped = false;
        assert!(!contradictory_input.input_evidence_valid());
        let mut receipt_backed_input = evidence.clone();
        receipt_backed_input.input.mode = crate::domain::NativeTurnInputMode::PushToTalk;
        receipt_backed_input.input.push_to_talk_state =
            crate::domain::NativePushToTalkState::TranscriptReady;
        receipt_backed_input.input.selected_stt_receipt = Some(NativeSelectedSttTurnEvidenceV1 {
            schema_version: 1,
            receipt_id: "c57278dc-2d58-44aa-b7f8-c1a52036caef".into(),
            receipt_sha256: "a".repeat(64),
            capture_session_id: "stt-session-1".into(),
            capture_turn_id: "stt-turn-1".into(),
            capture_generation: 7,
            game_id: "eclipse-harbor".into(),
            character_id: Some("mara-venn".into()),
            source_loadout_id: "api-first-starter".into(),
            route: NativeSelectedSttRouteReceipt {
                provider_id: "assemblyai".into(),
                model_id: "u3-rt-pro".into(),
                credential_reference: "providers/assemblyai".into(),
                egress: "microphone_audio_and_optional_non_secret_context".into(),
                generation: 7,
                input_endpoint_id: "{0.0.1.00000000}.fixture-input".into(),
                input_endpoint_generation: 3,
                manual_retry: false,
                automatic_fallback: false,
                captured_frames: 16_000,
                ptt_virtual_key: 0x77,
                ptt_press_transition_sequence: 8,
                ptt_pressed_qpc: 10,
                ptt_release_transition_sequence: 9,
                ptt_released_qpc: 20,
            },
            chunks_sent: 2,
            pcm_bytes_sent: 64_000,
            partial_events: 1,
        });
        receipt_backed_input.success.stt_skipped = false;
        assert!(receipt_backed_input.input_evidence_valid());
        receipt_backed_input
            .input
            .selected_stt_receipt
            .as_mut()
            .expect("receipt")
            .route
            .generation = 8;
        assert!(!receipt_backed_input.input_evidence_valid());
        assert!(evidence.subtitle_receipts_valid());
        let mut unbacked = evidence.clone();
        unbacked.subtitle_presentation_receipts.clear();
        assert!(!unbacked.subtitle_receipts_valid());
        let mut fabricated_console = evidence.clone();
        fabricated_console.subtitle_presentation_receipts[0].capture_sequence = 77;
        assert!(!fabricated_console.subtitle_receipts_valid());
        let mut invalid_authority = evidence.clone();
        invalid_authority.subtitle_presentation_receipts[0].renderer_authority_sha256 =
            "g".repeat(64);
        assert!(!invalid_authority.subtitle_receipts_valid());
        let mut invalid_opacity = evidence.clone();
        invalid_opacity.subtitle_presentation_receipts[0].renderer_opacity = 0.0;
        assert!(!invalid_opacity.subtitle_receipts_valid());
        assert!(evidence.success.tts_provider_live);
        assert!(evidence.success.audio_submitted);
        assert!(evidence.success.audio_drained);
        assert_eq!(evidence.success.audio_receipt_count, 1);
        assert_eq!(evidence.success.subtitle_receipt_count, 1);
        assert!(evidence.audio_receipts_valid());
        assert_eq!(evidence.subtitle_presentation_receipts.len(), 1);
        let subtitle = &evidence.subtitle_presentation_receipts[0];
        assert!(subtitle.committed);
        assert_eq!(subtitle.receipt_id, "subtitle-session-turn-1");
        assert_eq!(subtitle.presented_qpc_ticks, "123456789");
        assert_eq!(subtitle.width_px, 640);
        assert_eq!(subtitle.dpi_x, 144);
        assert!(matches!(
            subtitle.provenance,
            crate::domain::NativeSubtitlePresentationProvenance::ConsoleBottomCenterUnavailable
        ));
        assert_eq!(evidence.audio_receipts[0].source_frames, 24_000);
        assert!(matches!(
            evidence.audio_receipts[0].output_selection_mode,
            Some(crate::domain::NativeAudioOutputSelectionMode::SystemDefault)
        ));
        assert_eq!(
            evidence.audio_receipts[0].output_endpoint_id.as_deref(),
            Some("{0.0.0.00000000}.fixture-output")
        );
        assert_eq!(
            evidence.audio_receipts[0].output_endpoint_generation,
            Some(17)
        );
        assert_eq!(
            evidence.audio_receipts[0].receipt_id,
            "receipt-turn-9-sentence-1"
        );
        assert!(evidence.audio_receipts[0].completed);
        let mut stale_endpoint = evidence.clone();
        stale_endpoint.audio_receipts[0].output_endpoint_generation = Some(18);
        // Internally coherent endpoint evidence still requires the router's
        // exact allocated-lease match; this guard proves transport semantics.
        assert!(stale_endpoint.audio_receipts_valid());
        let mut cancelled_audio = evidence.clone();
        cancelled_audio.audio_receipts[0].cancelled = Some(true);
        assert!(!cancelled_audio.audio_receipts_valid());
        let mut legacy_audio = evidence.clone();
        legacy_audio.audio_receipts[0].transport_schema_version = Some(1);
        assert!(!legacy_audio.audio_receipts_valid());
        let mut duplicate_audio = evidence.clone();
        duplicate_audio
            .audio_receipts
            .push(duplicate_audio.audio_receipts[0].clone());
        duplicate_audio.success.audio_receipt_count = 2;
        assert!(!duplicate_audio.audio_receipts_valid());
        assert!(matches!(
            &evidence.degradations[0],
            crate::domain::NativeTurnExecutionDegradation::ManualRetryRequired {
                failed_role,
                provider_id: Some(provider_id),
                retryable: true,
                ..
            } if failed_role == "llm" && provider_id == "nvidia-nim"
        ));
    }

    #[test]
    fn boxed_simulation_request_preserves_wire_shape() {
        let renderer_authority = npc_subtitle_engine::bundled_default_renderer_authority()
            .expect("bundled renderer authority");
        let request = WireRequest::SimulateTurn(Box::new(NativeSimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "eclipse-harbor".into(),
            character_id: Some("mara-venn".into()),
            effective_game_profile: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: vec!["main_story".into()],
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Can you hear me?".into(),
            locale: "en-US".into(),
            route_snapshot: selected_routes(1),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE
                .into(),
            subtitle_presentation_context: Some(
                NativeSubtitlePresentationContext::console_unavailable(renderer_authority.clone()),
            ),
            execution_mode: None,
            dev_live_tts: None,
        }));
        let wire = serde_json::to_value(request).expect("serialize simulation request");
        assert_eq!(wire["type"], "simulate_turn");
        assert_eq!(
            wire["routeSnapshot"]["sourceLoadoutId"],
            "api-first-starter"
        );
        assert_eq!(wire["routeSnapshot"]["roles"]["tts"]["state"], "ready");
        assert_eq!(
            wire["routeSnapshot"]["roles"]["tts"]["primary"]["voiceId"],
            "a0e99841-438c-4a64-b679-ae501e7d6091"
        );
        assert_eq!(
            wire["routeSnapshot"]["roles"]["vision"]["state"],
            "disabled"
        );
        assert_eq!(
            wire["routeSnapshot"]["roles"]["lipsync"]["state"],
            "disabled"
        );
        assert_eq!(wire["input"]["mode"], "typed");
        assert_eq!(wire["delivery"]["audio"], true);
        assert_eq!(wire["delivery"]["subtitles"], true);
        assert_eq!(
            wire["enabledSpoilerTiers"],
            serde_json::json!(["main_story"])
        );
        assert!(wire.get("nativeIdentityDecision").is_none());
        assert!(wire.get("identityDecision").is_none());
        assert!(wire.get("audioPlaybackLeases").is_none());
        assert_eq!(
            wire["applicationNamespace"],
            crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE
        );
        let subtitle = &wire["subtitlePresentationContext"];
        assert_eq!(subtitle["schemaVersion"], 1);
        assert_eq!(subtitle["provenance"], "consoleBottomCenterUnavailable");
        assert!(subtitle["target"].is_null());
        assert_eq!(subtitle["viewportPx"]["width"], 0);
        assert_eq!(subtitle["dpiX"], 0);
        assert_eq!(
            subtitle["rendererAuthority"]["authoritySha256"],
            renderer_authority.authority_sha256
        );
        assert_eq!(
            subtitle["rendererAuthority"]["style"]["id"],
            "cinematic_glass"
        );
    }

    #[test]
    fn character_context_result_preserves_typed_nested_selection() {
        let result = serde_json::from_value::<NativeSimulationResult>(serde_json::json!({
            "schemaVersion": "2.0.0",
            "fixtureOnly": true,
            "integrationMode": "selected_route_turn",
            "events": [],
            "outcome": {},
            "characterContext": {
                "profileId": "skyrim-special-edition",
                "characterId": "lydia",
                "selection": {
                    "status": "known",
                    "character_id": "lydia",
                    "reason": "explicit"
                },
                "identitySource": "manual_explicit_selection",
                "explicitSelection": true,
                "prompt": {
                    "schemaVersion": "2.0.0",
                    "profileId": "skyrim-special-edition",
                    "characterId": "lydia",
                    "authorities": ["character_profile", "retrieved_memory"],
                    "recordCount": 2,
                    "retrievalProvenance": {
                        "schema_version": "2.0.0",
                        "policy_fingerprint_sha256": "policy",
                        "query_sha256": "query",
                        "selected_profile_knowledge_ids": [],
                        "selected_memory_item_ids": ["memory-1"],
                        "embedding_metadata_ids": []
                    },
                    "scopedMemoryItemIds": ["memory-1"],
                    "scopedMemoryClasses": ["episodic"]
                }
            }
        }))
        .expect("typed character context response");
        let context = result.character_context.expect("character context");
        assert_eq!(context.profile_id, "skyrim-special-edition");
        assert_eq!(context.character_id, "lydia");
        assert_eq!(context.identity_source, "manual_explicit_selection");
        assert!(context.explicit_selection);
        assert!(context
            .prompt
            .as_ref()
            .expect("prompt evidence")
            .authorities
            .contains(&npc_game_profile::PromptAuthority::RetrievedMemory));
        assert!(matches!(
            context.selection,
            Some(npc_character_db::SelectionOutcomeV1::Known {
                character_id,
                reason: npc_character_db::SelectionReason::Explicit,
            }) if character_id == "lydia"
        ));
    }

    #[test]
    fn authorized_dev_live_tts_wire_contains_route_identifiers_only() {
        let request = WireRequest::SimulateTurn(Box::new(NativeSimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "generic-game".into(),
            character_id: None,
            effective_game_profile: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Can you hear me?".into(),
            locale: "en-US".into(),
            route_snapshot: selected_routes(1),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE
                .into(),
            subtitle_presentation_context: None,
            execution_mode: Some(NativeExecutionMode::Hybrid),
            dev_live_tts: Some(NativeDevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
        }));

        let wire = serde_json::to_value(request).expect("serialize dev TTS request");
        assert_eq!(
            wire["devLiveTts"],
            serde_json::json!({
                "providerId": "elevenlabs",
                "modelId": "eleven_flash_v2_5",
                "voiceId": "EXAVITQu4vr4xnSDxMaL",
                "explicitUserAuthorization": true
            })
        );
        assert_eq!(wire["executionMode"], "hybrid");
        let encoded = serde_json::to_string(&wire).expect("encode wire JSON");
        for forbidden in ["apiKey", "credentialValue", "secret", "token"] {
            assert!(!encoded.contains(forbidden), "forbidden field {forbidden}");
        }
    }

    #[test]
    fn authorized_manual_fallback_chain_is_flattened_and_never_contains_values() {
        let mut document = crate::provider_loadouts::starter_document();
        let loadout = document
            .loadouts
            .values_mut()
            .next()
            .expect("starter loadout");
        let RoleOverrideV1::Route(llm) = loadout.roles.get_mut(&ProviderRole::Llm).expect("llm")
        else {
            panic!("expected route")
        };
        let mut fallback = llm.primary.clone();
        fallback.provider_id = "anthropic".into();
        fallback.model_id = "claude-sonnet-4".into();
        fallback.credential = Some(npc_provider_loadouts::CredentialReferenceV1 {
            provider_id: "anthropic".into(),
            reference_id: "personal".into(),
        });
        llm.fallbacks.push(ExplicitFallbackV1 {
            route: fallback,
            activation: npc_provider_loadouts::FallbackActivationV1::ManualOnly,
            user_authorized: true,
        });
        document.validate().expect("authorized fallback document");
        let snapshot = document
            .resolve(&LoadoutContextV1::global(), &ValidationContextV1::online())
            .expect("resolve fallback document")
            .pin_turn_routes(7);
        let wire = serde_json::to_value(NativeSelectedRouteSnapshot::from(&snapshot))
            .expect("serialize native route snapshot");
        let fallback = &wire["roles"]["llm"]["fallbacks"][0];
        assert_eq!(fallback["providerId"], "anthropic");
        assert_eq!(fallback["modelId"], "claude-sonnet-4");
        assert_eq!(fallback["activation"], "manualOnly");
        assert_eq!(fallback["userAuthorized"], true);
        assert_eq!(fallback["credentialReference"], "providers/anthropic");
        assert!(fallback.get("route").is_none());
        let encoded = serde_json::to_string(&wire).expect("encode fallback snapshot");
        for forbidden in ["apiKey", "credentialValue", "secret", "token"] {
            assert!(!encoded.contains(forbidden), "forbidden field {forbidden}");
        }
    }

    #[test]
    fn detected_trusted_safety_context_crosses_the_native_boundary() {
        let request = WireRequest::SimulateTurn(Box::new(NativeSimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "cyberpunk-2077".into(),
            character_id: None,
            effective_game_profile: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext {
                evidence_state: NativeSafetyEvidenceState::Blocked,
                profile_policy: NativeProfileSafetyPolicy::OfflineOnly,
                visuals_allowed: false,
                protected_online_detected: true,
                anti_cheat_detected: false,
            },
            transcript: "This route must be refused.".into(),
            locale: "en-US".into(),
            route_snapshot: selected_routes(1),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: crate::provider_loadouts::PRODUCTION_APPLICATION_NAMESPACE
                .into(),
            subtitle_presentation_context: None,
            execution_mode: Some(NativeExecutionMode::Hybrid),
            dev_live_tts: Some(NativeDevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
        }));

        let wire = serde_json::to_value(request).expect("serialize protected route");
        assert_eq!(wire["safetyContext"]["protectedOnlineDetected"], true);
        assert_eq!(wire["safetyContext"]["antiCheatDetected"], false);
        assert_eq!(wire["safetyContext"]["evidenceState"], "blocked");
        assert_eq!(wire["safetyContext"]["profilePolicy"], "offlineOnly");
        assert_eq!(wire["safetyContext"]["visualsAllowed"], false);
    }

    #[tokio::test]
    async fn late_response_is_consumed_but_cannot_complete_or_break_later_requests() {
        let nonce = LaunchNonce::new();
        let session = SessionId::new();
        let _ = qpc_now();
        tokio::time::sleep(Duration::from_millis(1)).await;
        let (now, frequency) = qpc_now();
        let mut tracker = SequenceTracker::new(
            nonce.clone(),
            session.clone(),
            OrderingPolicy::StrictContiguous,
        );
        let accepted = EnvelopeV1::new(
            nonce.clone(),
            session.clone(),
            None,
            TraceId::new(),
            1,
            now,
            frequency,
            now.saturating_add(frequency),
            0,
            envelope_v1::Body::NegotiationAccepted(NegotiationAcceptedV1 {
                selected: Some(ProtocolVersion::CURRENT),
                enabled_features: vec!["envelope-control-v1".into()],
            }),
        )
        .unwrap();
        tracker
            .observe(&accepted, control_context(now, &nonce, &session))
            .unwrap();

        let late_id = RequestId::new();
        let live_id = RequestId::new();
        let late_trace = TraceId::new();
        let live_trace = TraceId::new();
        let late_deadline = now.saturating_sub(1);
        let (late_sender, late_receiver) = oneshot::channel();
        let (live_sender, live_receiver) = oneshot::channel();
        let pending = Arc::new(Mutex::new(HashMap::from([
            (
                late_id.clone(),
                PendingRequest {
                    request_sequence: 2,
                    operation: ControlOperationV1::Ping,
                    cancellation_generation: 0,
                    turn_id: None,
                    trace_id: late_trace.clone(),
                    deadline_qpc_ticks: late_deadline,
                    sender: late_sender,
                },
            ),
            (
                live_id.clone(),
                PendingRequest {
                    request_sequence: 3,
                    operation: ControlOperationV1::Ping,
                    cancellation_generation: 0,
                    turn_id: None,
                    trace_id: live_trace.clone(),
                    deadline_qpc_ticks: now.saturating_add(frequency),
                    sender: live_sender,
                },
            ),
        ])));
        let (mut server, client) = tokio::io::duplex(64 * 1024);
        let reader = tokio::io::split(Box::new(client) as BoxedControlStream).0;
        let task = tokio::spawn(read_responses(
            reader,
            Arc::clone(&pending),
            HandshakeState {
                launch_nonce: nonce.clone(),
                session_id: session.clone(),
                qpc_frequency_hz: frequency,
                response_tracker: tracker,
            },
        ));

        let late = response_envelope(
            &nonce,
            &session,
            late_id,
            late_trace,
            2,
            2,
            late_deadline.saturating_sub(1),
            frequency,
            late_deadline,
        );
        write_frame(
            &mut server,
            &late.encode_to_vec(),
            MAX_CONTROL_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        let live = response_envelope(
            &nonce,
            &session,
            live_id,
            live_trace,
            3,
            3,
            now,
            frequency,
            now.saturating_add(frequency),
        );
        let encoded = encode_frame(&live, &control_context(now, &nonce, &session)).unwrap();
        write_frame(&mut server, &encoded[4..], MAX_CONTROL_MESSAGE_BYTES)
            .await
            .unwrap();

        assert!(matches!(
            late_receiver.await.unwrap(),
            Err(ClientError::Timeout)
        ));
        assert!(matches!(
            live_receiver.await.unwrap(),
            Ok(WireResponse::Pong {})
        ));
        drop(server);
        task.await.unwrap();
    }
}
