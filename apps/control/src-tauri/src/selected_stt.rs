use crate::diagnostics_v2::DiagnosticsV2Manager;
use crate::media_broker::{
    AudioInputRehearsalRequest, InputActivationSource, MediaBrokerSupervisor, PttActivationState,
};
use crate::sidecar_protocol::{
    ClientError, NativeRouteExecution, NativeSelectedProviderRoute,
    NativeSelectedSttAttemptAuthority, NativeSelectedSttControlRequest,
    NativeSelectedSttControlResult, NativeSelectedSttPcmIdentity, NativeSelectedSttTurnRequest,
};
use crate::sidecar_supervisor::{RuntimeSupervisor, SupervisorError};
use interactive_npcs_diagnostics::{DiagnosticStatus, Severity};
use npc_provider_loadouts::{
    EgressClassV1, ExecutionLocationV1, ProviderRole, ResolvedProviderLoadoutV1,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tokio::sync::Mutex;

const STT_REQUEST_SCHEMA_V1: u32 = 1;
const STT_SAMPLE_RATE: u32 = 16_000;
const STT_CHANNELS: u16 = 1;
const STT_MAX_DURATION_MS: u32 = 10_000;
const STT_MAX_FRAMES: u64 = STT_SAMPLE_RATE as u64 * 10;
const STT_EGRESS: &str = "microphone_audio_and_optional_non_secret_context";
const STT_RECEIPT_TTL: Duration = Duration::from_secs(120);
const MAX_COMPLETED_STT_RECEIPTS: usize = 8;
const STT_ARM_TIMEOUT: Duration = Duration::from_secs(8);
const STT_PTT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSelectedSttPushToTalkRequestV1 {
    pub schema_version: u32,
    pub game_profile_id: Option<String>,
    pub character_id: Option<String>,
    pub context_hint: Option<String>,
    pub attempt: SelectedSttAttemptRequestV1,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SelectedSttAttemptRequestV1 {
    Initial,
    ManualRetry {
        prior_generation: u64,
        user_authorized: bool,
    },
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectedSttCaptureStatusV1 {
    Arming,
    Capturing,
    Idle,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttRouteSummaryV1 {
    pub provider_id: String,
    pub model_id: String,
    pub egress: String,
    pub automatic_fallback: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttCapturingV1 {
    pub schema_version: u32,
    pub status: SelectedSttCaptureStatusV1,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
    pub route: SelectedSttRouteSummaryV1,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectedSttResultStatusV1 {
    TranscriptReady,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttPushToTalkEventV1 {
    pub schema_version: u32,
    pub status: SelectedSttResultStatusV1,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub receipt_id: Option<String>,
    pub receipt_sha256: Option<String>,
    pub route: Option<crate::sidecar_protocol::NativeSelectedSttRouteReceipt>,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
    pub error_code: Option<String>,
    pub retryable: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttStatusV1 {
    pub schema_version: u32,
    pub status: SelectedSttCaptureStatusV1,
    pub generation: u64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub input_endpoint_id: Option<String>,
    pub input_endpoint_generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectedSttCancelOutcomeV1 {
    CancellationRequested,
    AlreadyIdle,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttCancelResultV1 {
    pub schema_version: u32,
    pub outcome: SelectedSttCancelOutcomeV1,
    pub generation: u64,
}

#[derive(Clone, Debug)]
struct ActiveSelectedStt {
    session_id: String,
    turn_id: String,
    generation: u64,
    stream_id: String,
    input_endpoint_id: String,
    input_endpoint_generation: u64,
}

#[derive(Clone, Debug)]
struct CompletedSelectedStt {
    receipt_id: String,
    receipt_sha256: String,
    expires_at: Instant,
    capture_session_id: String,
    capture_turn_id: String,
    game_profile_id: Option<String>,
    character_id: Option<String>,
    source_loadout_id: String,
    generation: u64,
    transcript: String,
    route: crate::sidecar_protocol::NativeSelectedSttRouteReceipt,
    chunks_sent: u64,
    pcm_bytes_sent: u64,
    partial_events: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ConsumedSelectedStt {
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub capture_session_id: String,
    pub capture_turn_id: String,
    pub generation: u64,
    pub game_id: String,
    pub character_id: Option<String>,
    pub source_loadout_id: String,
    pub transcript: String,
    pub route: crate::sidecar_protocol::NativeSelectedSttRouteReceipt,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
}

#[derive(Debug, Default)]
struct SelectedSttState {
    generation: u64,
    reservation_generation: Option<u64>,
    active: Option<ActiveSelectedStt>,
    last_terminal_generation: Option<u64>,
    cancelled_generations: BTreeSet<u64>,
    completed: Vec<CompletedSelectedStt>,
}

#[derive(Debug)]
enum SelectedSttCancelTarget {
    Active(ActiveSelectedStt),
    Arming(u64),
    Idle(u64),
}

#[derive(Clone, Debug)]
pub struct SelectedSttController {
    runtime: RuntimeSupervisor,
    broker: MediaBrokerSupervisor,
    diagnostics: Arc<DiagnosticsV2Manager>,
    state: Arc<Mutex<SelectedSttState>>,
}

impl SelectedSttController {
    pub fn new(
        runtime: RuntimeSupervisor,
        broker: MediaBrokerSupervisor,
        diagnostics: Arc<DiagnosticsV2Manager>,
    ) -> Self {
        Self {
            runtime,
            broker,
            diagnostics,
            state: Arc::new(Mutex::new(SelectedSttState::default())),
        }
    }

    pub async fn status(&self) -> SelectedSttStatusV1 {
        let state = self.state.lock().await;
        let active = state.active.as_ref();
        SelectedSttStatusV1 {
            schema_version: STT_REQUEST_SCHEMA_V1,
            status: if active.is_some() {
                SelectedSttCaptureStatusV1::Capturing
            } else if state.reservation_generation.is_some() {
                SelectedSttCaptureStatusV1::Arming
            } else {
                SelectedSttCaptureStatusV1::Idle
            },
            generation: active.map_or(state.generation, |active| active.generation),
            session_id: active.map(|active| active.session_id.clone()),
            turn_id: active.map(|active| active.turn_id.clone()),
            input_endpoint_id: active.map(|active| active.input_endpoint_id.clone()),
            input_endpoint_generation: active.map(|active| active.input_endpoint_generation),
        }
    }

    pub(crate) async fn consume(
        &self,
        receipt_id: &str,
        generation: u64,
        game_profile_id: Option<&str>,
        character_id: Option<&str>,
        source_loadout_id: &str,
    ) -> Result<ConsumedSelectedStt, SelectedSttError> {
        let mut state = self.state.lock().await;
        take_completed_selected_stt(
            &mut state,
            receipt_id,
            generation,
            game_profile_id,
            character_id,
            source_loadout_id,
            Instant::now(),
        )
    }

    pub async fn start(
        &self,
        request: StartSelectedSttPushToTalkRequestV1,
        resolved: &ResolvedProviderLoadoutV1,
        events: Channel<SelectedSttPushToTalkEventV1>,
    ) -> Result<SelectedSttCapturingV1, SelectedSttError> {
        validate_start_request(&request)?;
        let route = selected_stt_route(resolved)?;
        let (generation, attempt) = {
            let mut state = self.state.lock().await;
            if state.active.is_some() || state.reservation_generation.is_some() {
                return Err(SelectedSttError::AlreadyActive);
            }
            let next = state.generation.saturating_add(1);
            let attempt = match request.attempt {
                SelectedSttAttemptRequestV1::Initial => NativeSelectedSttAttemptAuthority::Initial,
                SelectedSttAttemptRequestV1::ManualRetry {
                    prior_generation,
                    user_authorized,
                } => {
                    if !user_authorized
                        || prior_generation == 0
                        || state.last_terminal_generation != Some(prior_generation)
                        || next <= prior_generation
                    {
                        return Err(SelectedSttError::ManualRetryNotAuthorized);
                    }
                    NativeSelectedSttAttemptAuthority::ManualRetry {
                        prior_generation,
                        user_authorized: true,
                    }
                }
            };
            state.generation = next;
            state.reservation_generation = Some(next);
            (next, attempt)
        };

        let prepared = async {
            let control_identity = self.runtime.prepare_selected_stt_turn_identity().await?;
            self.wait_for_new_ptt_press(generation).await?;
            let lease = self
                .broker
                .allocate_audio_input_rehearsal(AudioInputRehearsalRequest {
                    session_id: control_identity.session_id.clone(),
                    turn_id: control_identity.turn_id.clone(),
                    generation,
                    duration_ms: STT_MAX_DURATION_MS,
                    sample_rate: STT_SAMPLE_RATE,
                    channels: STT_CHANNELS,
                    max_frames: STT_MAX_FRAMES,
                    activation_source: InputActivationSource::PushToTalk,
                })
                .await?;
            Ok::<_, SelectedSttError>((control_identity, lease))
        }
        .await;
        let (control_identity, lease) = match prepared {
            Ok(value) => value,
            Err(error) => {
                let mut state = self.state.lock().await;
                if state.reservation_generation == Some(generation) {
                    state.reservation_generation = None;
                    state.last_terminal_generation = Some(generation);
                }
                return Err(error);
            }
        };
        let stream_id = lease.stream_id.clone();
        let input_endpoint_id = lease.input_endpoint_id.clone();
        let input_endpoint_generation = lease.input_endpoint_generation;
        let source_loadout_id = resolved.leaf_loadout_id.to_string();
        let game_profile_id = request.game_profile_id.clone();
        let character_id = request.character_id.clone();
        let native_request = NativeSelectedSttControlRequest {
            schema_version: STT_REQUEST_SCHEMA_V1,
            source_loadout_id: source_loadout_id.clone(),
            route_snapshot_generation: generation,
            route: route.clone(),
            turn: NativeSelectedSttTurnRequest {
                identity: NativeSelectedSttPcmIdentity {
                    session_id: control_identity.session_id.clone(),
                    turn_id: control_identity.turn_id.clone(),
                    generation,
                    input_endpoint_id: input_endpoint_id.clone(),
                    input_endpoint_generation,
                },
                attempt,
                context_hint: request.context_hint,
            },
            lease,
        };
        let reservation_is_current = {
            let mut state = self.state.lock().await;
            if state.reservation_generation != Some(generation) {
                false
            } else {
                state.reservation_generation = None;
                state.active = Some(ActiveSelectedStt {
                    session_id: control_identity.session_id.clone(),
                    turn_id: control_identity.turn_id.clone(),
                    generation,
                    stream_id: stream_id.clone(),
                    input_endpoint_id: input_endpoint_id.clone(),
                    input_endpoint_generation,
                });
                true
            }
        };
        if !reservation_is_current {
            let _ = self
                .broker
                .cancel_audio_input_rehearsal(&stream_id, generation)
                .await;
            return Err(SelectedSttError::State);
        }
        let controller = self.clone();
        let event_session_id = control_identity.session_id.clone();
        let event_turn_id = control_identity.turn_id.clone();
        let event_input_endpoint_id = input_endpoint_id.clone();
        tauri::async_runtime::spawn(async move {
            controller
                .run_selected_stt(
                    native_request,
                    source_loadout_id,
                    route,
                    stream_id,
                    event_session_id,
                    event_turn_id,
                    generation,
                    event_input_endpoint_id,
                    input_endpoint_generation,
                    game_profile_id,
                    character_id,
                    events,
                )
                .await;
        });
        Ok(SelectedSttCapturingV1 {
            schema_version: STT_REQUEST_SCHEMA_V1,
            status: SelectedSttCaptureStatusV1::Capturing,
            session_id: control_identity.session_id,
            turn_id: control_identity.turn_id,
            generation,
            input_endpoint_id,
            input_endpoint_generation,
            route: SelectedSttRouteSummaryV1 {
                provider_id: "assemblyai".into(),
                model_id: "u3-rt-pro".into(),
                egress: STT_EGRESS.into(),
                automatic_fallback: false,
            },
        })
    }

    async fn wait_for_new_ptt_press(&self, generation: u64) -> Result<(), SelectedSttError> {
        let baseline = self.broker.query_ptt_activation_state().await?;
        validate_ptt_baseline(&baseline)?;
        let deadline = Instant::now() + STT_ARM_TIMEOUT;
        loop {
            tokio::time::sleep(STT_PTT_POLL_INTERVAL).await;
            {
                let state = self.state.lock().await;
                if state.reservation_generation != Some(generation) {
                    return Err(SelectedSttError::ArmCancelled);
                }
            }
            if Instant::now() >= deadline {
                return Err(SelectedSttError::ArmTimeout);
            }
            let current = self.broker.query_ptt_activation_state().await?;
            if current.is_strictly_newer_press_than(baseline) {
                return Ok(());
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_selected_stt(
        &self,
        request: NativeSelectedSttControlRequest,
        source_loadout_id: String,
        route: NativeSelectedProviderRoute,
        stream_id: String,
        session_id: String,
        turn_id: String,
        generation: u64,
        input_endpoint_id: String,
        input_endpoint_generation: u64,
        game_profile_id: Option<String>,
        character_id: Option<String>,
        events: Channel<SelectedSttPushToTalkEventV1>,
    ) {
        let expected_manual_retry = matches!(
            request.turn.attempt,
            NativeSelectedSttAttemptAuthority::ManualRetry { .. }
        );
        let result = self.runtime.transcribe_selected_stt(request).await;
        let mut completed = None;
        let mut terminal = match result {
            Ok(result)
                if selected_stt_result_valid(
                    &result,
                    &source_loadout_id,
                    &route,
                    generation,
                    &input_endpoint_id,
                    input_endpoint_generation,
                    expected_manual_retry,
                ) =>
            {
                let receipt_id = uuid::Uuid::new_v4().to_string();
                let receipt_sha256 = format!(
                    "{:x}",
                    sha2::Sha256::digest(
                        serde_json::to_vec(&result)
                            .expect("selected STT receipt uses only infallible JSON fields")
                    )
                );
                completed = Some(CompletedSelectedStt {
                    receipt_id: receipt_id.clone(),
                    receipt_sha256: receipt_sha256.clone(),
                    expires_at: Instant::now() + STT_RECEIPT_TTL,
                    capture_session_id: session_id.clone(),
                    capture_turn_id: turn_id.clone(),
                    game_profile_id,
                    character_id,
                    source_loadout_id: source_loadout_id.clone(),
                    generation,
                    transcript: result.result.transcript,
                    route: result.result.route.clone(),
                    chunks_sent: result.result.chunks_sent,
                    pcm_bytes_sent: result.result.pcm_bytes_sent,
                    partial_events: result.result.partial_events,
                });
                SelectedSttPushToTalkEventV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    status: SelectedSttResultStatusV1::TranscriptReady,
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    generation,
                    receipt_id: Some(receipt_id),
                    receipt_sha256: Some(receipt_sha256),
                    route: Some(result.result.route),
                    chunks_sent: result.result.chunks_sent,
                    pcm_bytes_sent: result.result.pcm_bytes_sent,
                    partial_events: result.result.partial_events,
                    error_code: None,
                    retryable: false,
                }
            }
            Ok(_) => {
                let _ = self
                    .broker
                    .cancel_audio_input_rehearsal(&stream_id, generation)
                    .await;
                SelectedSttPushToTalkEventV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    status: SelectedSttResultStatusV1::Failed,
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    generation,
                    receipt_id: None,
                    receipt_sha256: None,
                    route: None,
                    chunks_sent: 0,
                    pcm_bytes_sent: 0,
                    partial_events: 0,
                    error_code: Some("selected_stt_receipt_mismatch".into()),
                    retryable: false,
                }
            }
            Err(SupervisorError::Client(ClientError::Cancelled)) => {
                let _ = self
                    .broker
                    .cancel_audio_input_rehearsal(&stream_id, generation)
                    .await;
                SelectedSttPushToTalkEventV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    status: SelectedSttResultStatusV1::Cancelled,
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    generation,
                    receipt_id: None,
                    receipt_sha256: None,
                    route: None,
                    chunks_sent: 0,
                    pcm_bytes_sent: 0,
                    partial_events: 0,
                    error_code: None,
                    retryable: false,
                }
            }
            Err(error) => {
                let _ = self
                    .broker
                    .cancel_audio_input_rehearsal(&stream_id, generation)
                    .await;
                let (error_code, retryable) = match error {
                    SupervisorError::Client(ClientError::Remote { code, retryable }) => {
                        (code, retryable)
                    }
                    SupervisorError::Client(ClientError::Timeout) => {
                        ("selected_stt_timeout".into(), true)
                    }
                    _ => ("selected_stt_runtime_unavailable".into(), true),
                };
                SelectedSttPushToTalkEventV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    status: SelectedSttResultStatusV1::Failed,
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    generation,
                    receipt_id: None,
                    receipt_sha256: None,
                    route: None,
                    chunks_sent: 0,
                    pcm_bytes_sent: 0,
                    partial_events: 0,
                    error_code: Some(error_code),
                    retryable,
                }
            }
        };
        let (should_emit, was_cancelled) = {
            let mut state = self.state.lock().await;
            finalize_selected_stt(&mut state, generation, completed.take())
        };
        if was_cancelled {
            terminal = SelectedSttPushToTalkEventV1 {
                schema_version: STT_REQUEST_SCHEMA_V1,
                status: SelectedSttResultStatusV1::Cancelled,
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                generation,
                receipt_id: None,
                receipt_sha256: None,
                route: None,
                chunks_sent: 0,
                pcm_bytes_sent: 0,
                partial_events: 0,
                error_code: None,
                retryable: false,
            };
        }
        if should_emit {
            let (event_name, severity, status, error_code) = match terminal.status {
                SelectedSttResultStatusV1::TranscriptReady => (
                    "selected_stt.transcript_ready",
                    Severity::Info,
                    DiagnosticStatus::Ok,
                    None,
                ),
                SelectedSttResultStatusV1::Cancelled => (
                    "selected_stt.cancelled",
                    Severity::Info,
                    DiagnosticStatus::Degraded,
                    Some("selected_stt_cancelled"),
                ),
                SelectedSttResultStatusV1::Failed => (
                    "selected_stt.failed",
                    Severity::Error,
                    DiagnosticStatus::Failed,
                    Some("selected_stt_failed"),
                ),
            };
            let _ = self.diagnostics.record_native_turn_event(
                "stt", event_name, severity, status, error_code, &turn_id,
            );
            let _ = events.send(terminal);
        }
    }

    pub async fn cancel(&self) -> Result<SelectedSttCancelResultV1, SelectedSttError> {
        let target = {
            let mut state = self.state.lock().await;
            take_selected_stt_cancel_target(&mut state)
        };
        let active = match target {
            SelectedSttCancelTarget::Idle(generation) => {
                return Ok(SelectedSttCancelResultV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    outcome: SelectedSttCancelOutcomeV1::AlreadyIdle,
                    generation,
                });
            }
            SelectedSttCancelTarget::Arming(generation) => {
                return Ok(SelectedSttCancelResultV1 {
                    schema_version: STT_REQUEST_SCHEMA_V1,
                    outcome: SelectedSttCancelOutcomeV1::CancellationRequested,
                    generation,
                });
            }
            SelectedSttCancelTarget::Active(active) => active,
        };
        let broker = self
            .broker
            .cancel_audio_input_rehearsal(&active.stream_id, active.generation)
            .await;
        let runtime = self.runtime.cancel().await;
        broker?;
        runtime?;
        Ok(SelectedSttCancelResultV1 {
            schema_version: STT_REQUEST_SCHEMA_V1,
            outcome: SelectedSttCancelOutcomeV1::CancellationRequested,
            generation: active.generation,
        })
    }
}

fn take_selected_stt_cancel_target(state: &mut SelectedSttState) -> SelectedSttCancelTarget {
    if let Some(active) = state.active.take() {
        state.cancelled_generations.insert(active.generation);
        state.last_terminal_generation = Some(active.generation);
        return SelectedSttCancelTarget::Active(active);
    }
    if let Some(generation) = state.reservation_generation.take() {
        state.last_terminal_generation = Some(generation);
        return SelectedSttCancelTarget::Arming(generation);
    }
    SelectedSttCancelTarget::Idle(state.generation)
}

fn validate_ptt_baseline(baseline: &PttActivationState) -> Result<(), SelectedSttError> {
    if baseline.schema_version != 1
        || baseline.virtual_key == 0
        || baseline.virtual_key > 0xff
        || baseline.transition_sequence == 0
        || baseline.transition_qpc == 0
    {
        return Err(SelectedSttError::PttUnavailable);
    }
    Ok(())
}

fn take_completed_selected_stt(
    state: &mut SelectedSttState,
    receipt_id: &str,
    generation: u64,
    game_profile_id: Option<&str>,
    character_id: Option<&str>,
    source_loadout_id: &str,
    now: Instant,
) -> Result<ConsumedSelectedStt, SelectedSttError> {
    state.completed.retain(|receipt| receipt.expires_at > now);
    let index = state
        .completed
        .iter()
        .position(|receipt| receipt.receipt_id == receipt_id)
        .ok_or(SelectedSttError::ReceiptUnavailable)?;
    let receipt = &state.completed[index];
    if generation == 0
        || receipt.generation != generation
        || receipt.game_profile_id.as_deref() != game_profile_id
        || receipt.character_id.as_deref() != character_id
        || receipt.source_loadout_id != source_loadout_id
    {
        return Err(SelectedSttError::ReceiptMismatch);
    }
    let receipt = state.completed.remove(index);
    Ok(ConsumedSelectedStt {
        receipt_id: receipt.receipt_id,
        receipt_sha256: receipt.receipt_sha256,
        capture_session_id: receipt.capture_session_id,
        capture_turn_id: receipt.capture_turn_id,
        generation: receipt.generation,
        game_id: receipt
            .game_profile_id
            .unwrap_or_else(|| "generic-game".into()),
        character_id: receipt.character_id,
        source_loadout_id: receipt.source_loadout_id,
        transcript: receipt.transcript,
        route: receipt.route,
        chunks_sent: receipt.chunks_sent,
        pcm_bytes_sent: receipt.pcm_bytes_sent,
        partial_events: receipt.partial_events,
    })
}

fn finalize_selected_stt(
    state: &mut SelectedSttState,
    generation: u64,
    completed: Option<CompletedSelectedStt>,
) -> (bool, bool) {
    let was_active = state
        .active
        .as_ref()
        .is_some_and(|active| active.generation == generation);
    if was_active {
        state.active = None;
    }
    let was_cancelled = state.cancelled_generations.remove(&generation);
    if !(was_active || was_cancelled) {
        return (false, false);
    }
    state.last_terminal_generation = Some(generation);
    if !was_cancelled {
        if let Some(completed) = completed {
            state.completed.push(completed);
            if state.completed.len() > MAX_COMPLETED_STT_RECEIPTS {
                state.completed.remove(0);
            }
        }
    }
    (true, was_cancelled)
}

fn validate_start_request(
    request: &StartSelectedSttPushToTalkRequestV1,
) -> Result<(), SelectedSttError> {
    if request.schema_version != STT_REQUEST_SCHEMA_V1
        || (request.character_id.is_some() && request.game_profile_id.is_none())
        || request.context_hint.as_ref().is_some_and(|value| {
            value.len() > 4_096 || value.chars().any(|character| character == '\0')
        })
        || request
            .game_profile_id
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
        || request
            .character_id
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
    {
        return Err(SelectedSttError::InvalidRequest);
    }
    Ok(())
}

fn selected_stt_route(
    resolved: &ResolvedProviderLoadoutV1,
) -> Result<NativeSelectedProviderRoute, SelectedSttError> {
    let route = &resolved
        .roles
        .get(&ProviderRole::Stt)
        .ok_or(SelectedSttError::RouteUnavailable)?
        .primary;
    let credential = route
        .credential
        .as_ref()
        .ok_or(SelectedSttError::RouteUnavailable)?;
    if route.provider_id != "assemblyai"
        || route.model_id != "u3-rt-pro"
        || route.voice_id.is_some()
        || route.disclosure.execution != ExecutionLocationV1::Hosted
        || route.disclosure.egress != EgressClassV1::ProviderCloud
        || credential.provider_id != "assemblyai"
        || credential.reference_id != "personal"
        || !route.explicit_user_selection
    {
        return Err(SelectedSttError::RouteUnavailable);
    }
    Ok(NativeSelectedProviderRoute {
        provider_id: route.provider_id.clone(),
        model_id: route.model_id.clone(),
        voice_id: None,
        execution: NativeRouteExecution::Cloud,
        egress: STT_EGRESS.into(),
        credential_reference: Some("providers/assemblyai".into()),
    })
}

fn selected_stt_result_valid(
    response: &NativeSelectedSttControlResult,
    source_loadout_id: &str,
    route: &NativeSelectedProviderRoute,
    generation: u64,
    input_endpoint_id: &str,
    input_endpoint_generation: u64,
    expected_manual_retry: bool,
) -> bool {
    let result = &response.result;
    response.schema_version == STT_REQUEST_SCHEMA_V1
        && response.source_loadout_id == source_loadout_id
        && response.route_snapshot_generation == generation
        && !result.transcript.trim().is_empty()
        && result.transcript.len() <= 64 * 1_024
        && result.route.provider_id == route.provider_id
        && result.route.model_id == route.model_id
        && result.route.credential_reference == "providers/assemblyai"
        && result.route.egress == STT_EGRESS
        && result.route.generation == generation
        && result.route.input_endpoint_id == input_endpoint_id
        && result.route.input_endpoint_generation == input_endpoint_generation
        && result.route.manual_retry == expected_manual_retry
        && !result.route.automatic_fallback
        && result.route.captured_frames > 0
        && result.route.ptt_virtual_key > 0
        && result.route.ptt_virtual_key <= 0xff
        && result.route.ptt_press_transition_sequence > 0
        && result.route.ptt_pressed_qpc > 0
        && result.route.ptt_release_transition_sequence > result.route.ptt_press_transition_sequence
        && result.route.ptt_released_qpc >= result.route.ptt_pressed_qpc
        && result.chunks_sent > 0
        && result.pcm_bytes_sent > 0
}

#[derive(Debug, thiserror::Error)]
pub enum SelectedSttError {
    #[error("selected STT request is invalid")]
    InvalidRequest,
    #[error("selected AssemblyAI STT route is unavailable")]
    RouteUnavailable,
    #[error("selected STT capture is already active")]
    AlreadyActive,
    #[error("manual retry is not explicitly authorized against the last terminal generation")]
    ManualRetryNotAuthorized,
    #[error("selected STT state is unavailable")]
    State,
    #[error("push-to-talk is unavailable or not configured")]
    PttUnavailable,
    #[error("push-to-talk arm timed out before a newer physical press")]
    ArmTimeout,
    #[error("push-to-talk arm was cancelled before allocation")]
    ArmCancelled,
    #[error("selected STT receipt is unavailable, expired, cancelled, or already consumed")]
    ReceiptUnavailable,
    #[error(
        "selected STT receipt does not match the current game, character, loadout, or generation"
    )]
    ReceiptMismatch,
    #[error(transparent)]
    Broker(#[from] crate::media_broker::MediaBrokerError),
    #[error(transparent)]
    Runtime(#[from] SupervisorError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt_route() -> crate::sidecar_protocol::NativeSelectedSttRouteReceipt {
        crate::sidecar_protocol::NativeSelectedSttRouteReceipt {
            provider_id: "assemblyai".into(),
            model_id: "u3-rt-pro".into(),
            credential_reference: "providers/assemblyai".into(),
            egress: STT_EGRESS.into(),
            generation: 7,
            input_endpoint_id: "capture-endpoint-1".into(),
            input_endpoint_generation: 3,
            manual_retry: false,
            automatic_fallback: false,
            captured_frames: 320,
            ptt_virtual_key: 0x77,
            ptt_press_transition_sequence: 8,
            ptt_pressed_qpc: 10,
            ptt_release_transition_sequence: 9,
            ptt_released_qpc: 20,
        }
    }

    #[test]
    fn webview_request_cannot_supply_native_identity_endpoint_or_lease() {
        let safe = serde_json::json!({
            "schemaVersion": 1,
            "gameProfileId": "skyrim-special-edition",
            "characterId": "lydia",
            "contextHint": "Address the selected character.",
            "attempt": { "kind": "initial" }
        });
        serde_json::from_value::<StartSelectedSttPushToTalkRequestV1>(safe)
            .expect("safe start request");
        for forbidden in [
            "sessionId",
            "turnId",
            "generation",
            "inputEndpointId",
            "producerEndpoint",
            "oneTimeToken",
            "pcm",
        ] {
            let mut forged = serde_json::json!({
                "schemaVersion": 1,
                "gameProfileId": null,
                "characterId": null,
                "contextHint": null,
                "attempt": { "kind": "initial" }
            });
            forged[forbidden] = serde_json::json!("forged");
            assert!(serde_json::from_value::<StartSelectedSttPushToTalkRequestV1>(forged).is_err());
        }
    }

    #[test]
    fn terminal_receipt_requires_exact_route_generation_and_endpoint() {
        let route = NativeSelectedProviderRoute {
            provider_id: "assemblyai".into(),
            model_id: "u3-rt-pro".into(),
            voice_id: None,
            execution: NativeRouteExecution::Cloud,
            egress: STT_EGRESS.into(),
            credential_reference: Some("providers/assemblyai".into()),
        };
        let mut result = NativeSelectedSttControlResult {
            schema_version: STT_REQUEST_SCHEMA_V1,
            source_loadout_id: "selected-loadout".into(),
            route_snapshot_generation: 7,
            result: crate::sidecar_protocol::NativeSelectedSttResult {
                transcript: "The road is clear.".into(),
                route: receipt_route(),
                chunks_sent: 2,
                pcm_bytes_sent: 640,
                partial_events: 1,
            },
        };
        assert!(selected_stt_result_valid(
            &result,
            "selected-loadout",
            &route,
            7,
            "capture-endpoint-1",
            3,
            false,
        ));
        result.result.route.input_endpoint_generation = 4;
        assert!(!selected_stt_result_valid(
            &result,
            "selected-loadout",
            &route,
            7,
            "capture-endpoint-1",
            3,
            false,
        ));
        result.result.route.input_endpoint_generation = 3;
        result.result.route.automatic_fallback = true;
        assert!(!selected_stt_result_valid(
            &result,
            "selected-loadout",
            &route,
            7,
            "capture-endpoint-1",
            3,
            false,
        ));
    }

    #[test]
    fn terminal_event_has_status_contract_and_no_native_capability() {
        let event = SelectedSttPushToTalkEventV1 {
            schema_version: 1,
            status: SelectedSttResultStatusV1::Cancelled,
            session_id: "session".into(),
            turn_id: "turn".into(),
            generation: 2,
            receipt_id: None,
            receipt_sha256: None,
            route: None,
            chunks_sent: 0,
            pcm_bytes_sent: 0,
            partial_events: 0,
            error_code: None,
            retryable: false,
        };
        let wire = serde_json::to_value(event).expect("serialize terminal event");
        assert_eq!(wire["status"], "cancelled");
        for forbidden in ["producerEndpoint", "oneTimeToken", "lease", "pcm"] {
            assert!(wire.get(forbidden).is_none());
        }
    }

    #[test]
    fn delayed_success_after_cancel_cannot_mint_a_receipt() {
        let now = Instant::now();
        let mut state = SelectedSttState {
            generation: 7,
            cancelled_generations: BTreeSet::from([7]),
            ..SelectedSttState::default()
        };
        let completed = CompletedSelectedStt {
            receipt_id: "c57278dc-2d58-44aa-b7f8-c1a52036caef".into(),
            receipt_sha256: "a".repeat(64),
            expires_at: now + Duration::from_secs(30),
            capture_session_id: "capture-session".into(),
            capture_turn_id: "capture-turn".into(),
            game_profile_id: Some("skyrim-special-edition".into()),
            character_id: Some("lydia".into()),
            source_loadout_id: "selected-loadout".into(),
            generation: 7,
            transcript: "Must be discarded.".into(),
            route: receipt_route(),
            chunks_sent: 2,
            pcm_bytes_sent: 640,
            partial_events: 1,
        };
        assert_eq!(
            finalize_selected_stt(&mut state, 7, Some(completed)),
            (true, true)
        );
        assert!(state.completed.is_empty());
        assert_eq!(state.last_terminal_generation, Some(7));
    }

    #[test]
    fn arming_cancel_revokes_reservation_before_any_allocation() {
        let mut state = SelectedSttState {
            generation: 9,
            reservation_generation: Some(9),
            ..SelectedSttState::default()
        };
        assert!(matches!(
            take_selected_stt_cancel_target(&mut state),
            SelectedSttCancelTarget::Arming(9)
        ));
        assert!(state.reservation_generation.is_none());
        assert!(state.cancelled_generations.is_empty());
        assert_eq!(state.last_terminal_generation, Some(9));
        assert!(state.active.is_none());
    }

    #[test]
    fn already_pressed_baseline_requires_a_strictly_newer_physical_press() {
        use crate::media_broker::PttActivationStateKind;
        let baseline = PttActivationState {
            schema_version: 1,
            virtual_key: 0x77,
            state: PttActivationStateKind::Pressed,
            transition_sequence: 8,
            transition_qpc: 100,
            release_transition_sequence: 7,
            released_qpc: 90,
        };
        validate_ptt_baseline(&baseline).expect("configured baseline");
        assert!(!baseline.is_strictly_newer_press_than(baseline));
        let released = PttActivationState {
            state: PttActivationStateKind::Released,
            transition_sequence: 9,
            transition_qpc: 110,
            release_transition_sequence: 9,
            released_qpc: 110,
            ..baseline
        };
        assert!(!released.is_strictly_newer_press_than(baseline));
        let newer_press = PttActivationState {
            state: PttActivationStateKind::Pressed,
            transition_sequence: 10,
            transition_qpc: 120,
            ..released
        };
        assert!(newer_press.is_strictly_newer_press_than(baseline));
    }

    #[test]
    fn completed_receipt_is_scope_bound_expiring_and_one_time() {
        let now = Instant::now();
        let completed = CompletedSelectedStt {
            receipt_id: "c57278dc-2d58-44aa-b7f8-c1a52036caef".into(),
            receipt_sha256: "a".repeat(64),
            expires_at: now + Duration::from_secs(30),
            capture_session_id: "capture-session".into(),
            capture_turn_id: "capture-turn".into(),
            game_profile_id: Some("skyrim-special-edition".into()),
            character_id: Some("lydia".into()),
            source_loadout_id: "selected-loadout".into(),
            generation: 7,
            transcript: "The road is clear.".into(),
            route: receipt_route(),
            chunks_sent: 2,
            pcm_bytes_sent: 640,
            partial_events: 1,
        };
        let mut state = SelectedSttState {
            completed: vec![completed],
            ..SelectedSttState::default()
        };
        assert!(matches!(
            take_completed_selected_stt(
                &mut state,
                "c57278dc-2d58-44aa-b7f8-c1a52036caef",
                7,
                Some("different-game"),
                Some("lydia"),
                "selected-loadout",
                now,
            ),
            Err(SelectedSttError::ReceiptMismatch)
        ));
        assert_eq!(
            state.completed.len(),
            1,
            "scope mismatch must not burn receipt"
        );
        let consumed = take_completed_selected_stt(
            &mut state,
            "c57278dc-2d58-44aa-b7f8-c1a52036caef",
            7,
            Some("skyrim-special-edition"),
            Some("lydia"),
            "selected-loadout",
            now,
        )
        .expect("exact receipt consumes once");
        assert_eq!(consumed.transcript, "The road is clear.");
        assert!(matches!(
            take_completed_selected_stt(
                &mut state,
                "c57278dc-2d58-44aa-b7f8-c1a52036caef",
                7,
                Some("skyrim-special-edition"),
                Some("lydia"),
                "selected-loadout",
                now,
            ),
            Err(SelectedSttError::ReceiptUnavailable)
        ));

        state.completed.push(CompletedSelectedStt {
            receipt_id: "9bbca0a1-198a-49aa-94f5-6a2822f94a57".into(),
            receipt_sha256: "b".repeat(64),
            expires_at: now,
            capture_session_id: "capture-session".into(),
            capture_turn_id: "capture-turn".into(),
            game_profile_id: None,
            character_id: None,
            source_loadout_id: "selected-loadout".into(),
            generation: 8,
            transcript: "Expired".into(),
            route: receipt_route(),
            chunks_sent: 1,
            pcm_bytes_sent: 320,
            partial_events: 0,
        });
        assert!(matches!(
            take_completed_selected_stt(
                &mut state,
                "9bbca0a1-198a-49aa-94f5-6a2822f94a57",
                8,
                None,
                None,
                "selected-loadout",
                now,
            ),
            Err(SelectedSttError::ReceiptUnavailable)
        ));
        assert!(state.completed.is_empty());
    }
}
