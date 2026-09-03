//! Trusted runtime-host boundary.
//!
//! This process owns policy, profile/catalog loading, the authoritative SQLite
//! database, control IPC, and worker lifecycle boundaries. It deliberately does
//! not accept credentials or conversation text in diagnostics/log labels.

pub mod audio_input;
pub mod audio_output;
pub mod bootstrap;
pub mod cli;
pub mod control;
pub mod doctor;
pub mod framing;
pub mod llm_bridge;
pub mod profile_replays;
pub mod profiles;
pub mod retrieval_bridge;
pub mod runtime_timing;
pub mod simulation;
pub mod stt_bridge;
pub mod subtitle_bridge;
pub mod supervisor;
pub mod tts_bridge;
pub mod turn_contract;

pub use bootstrap::{CatalogTrustState, HostConfig, HostState};
pub use control::{ControlRequest, ControlResponse, ServeOptions};
pub use doctor::{DoctorReport, DoctorStatus};
pub use profiles::{LoadedProfile, ProfileCorpus, RuntimeIntegrationPolicy};
pub use simulation::{SimulationRequest, SimulationResult};
pub use tts_bridge::{
    DiscoveredStockVoice, TtsVoiceDiscoveryError, TtsVoiceDiscoveryRequest,
    TtsVoiceDiscoveryResult, TtsVoiceDiscoveryStatus, TtsVoiceProvenance, TtsVoiceRefreshEvidence,
};
pub use turn_contract::{
    AudioReceiptSummary, ConsumedPrivateEvaluationAcknowledgementV1, ConsumedProviderRoute,
    ConsumedRouteSnapshot, DeliveryCommitState, ManualFallbackActivation, ManualFallbackRoute,
    PrivateEvaluationModalityV1, PrivateEvaluationModeV1,
    ProviderPrivateEvaluationAcknowledgementV1, PushToTalkCaptureState, RouteDegradation,
    RouteExecution, RuntimeClockDomainV1, RuntimeClockStampV1, RuntimeProviderRouteBindingV1,
    RuntimeTurnCancellationReceiptV1, RuntimeTurnTimingReceiptV1, SelectedProviderRoute,
    SelectedRoleRoute, SelectedRouteRoles, SelectedRouteSnapshot, SelectedRouteState,
    SelectedSttRouteReceiptV1, SelectedSttTurnEvidenceV1, SubtitleColorTreatment, SubtitleCue,
    SubtitleDirection, SubtitlePresentationProvenance, SubtitlePresentationReceiptSummary,
    TurnDeliveryRequest, TurnDeliveryState, TurnExecutionDegradation, TurnExecutionEvidence,
    TurnExecutionSuccessMetadata, TurnInputMode, TurnInputSnapshot,
    NVIDIA_MAGPIE_PRIVATE_EVALUATION_TERMS_REVISION, NVIDIA_PRIVATE_EVALUATION_TERMS_REVISION,
    PRIVATE_EVALUATION_ACKNOWLEDGEMENT_SCHEMA_VERSION,
    PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE,
    PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE, PRODUCTION_APPLICATION_NAMESPACE,
    TURN_ROUTE_SCHEMA_VERSION,
};

pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REQUIRED_AUTHORED_GAME_PROFILE_COUNT: usize = 20;
pub const REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT: usize = 1;
pub const SYNTHETIC_REVIEW_PROFILE_ID: &str = "eclipse-harbor";
pub const REQUIRED_PROFILE_COUNT: usize =
    REQUIRED_AUTHORED_GAME_PROFILE_COUNT + REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT;
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 1024 * 1024;
