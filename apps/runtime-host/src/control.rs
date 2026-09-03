use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use npc_protocol::{
    control_response_v1, decode_frame, encode_frame, envelope_v1, negotiate, ControlOperationV1,
    ControlResponseV1, EnvelopeV1, EnvelopeValidationContext, ErrorCode, IpcErrorV1, LaunchNonce,
    NegotiationAcceptedV1, OrderingPolicy, RequestId, SequenceTracker, SessionId, TraceId, TurnId,
    VersionRange, DEFAULT_MAX_FRAME_BYTES,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    audio_input::{BrokerAudioInputLease, BrokerPcmInputSource},
    framing::{read_frame, write_frame, FrameError},
    simulation::SimulationError,
    stt_bridge::{
        selected_hosted_stt, SelectedSttError, SelectedSttResult, SelectedSttTurnRequest,
    },
    tts_bridge::{TtsVoiceDiscoveryRequest, TtsVoiceDiscoveryResult},
    HostState, SelectedProviderRoute, SimulationRequest, MAX_CONTROL_MESSAGE_BYTES,
};

#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub endpoint_override: Option<PathBuf>,
    pub launch_nonce: LaunchNonce,
    pub shutdown: CancellationToken,
}

impl ServeOptions {
    #[must_use]
    pub fn new(launch_nonce: LaunchNonce) -> Self {
        Self {
            endpoint_override: None,
            launch_nonce,
            shutdown: CancellationToken::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Ping,
    Doctor,
    ValidateProfiles,
    DiscoverTtsVoices(TtsVoiceDiscoveryRequest),
    SimulateTurn(Box<SimulationRequest>),
    TranscribeSelectedStt(Box<SelectedSttControlRequest>),
    Cancel { new_generation: u64 },
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlResponse {
    Pong,
    Doctor {
        report: Box<crate::DoctorReport>,
    },
    Profiles {
        profiles: Vec<crate::profiles::ProfileSummary>,
    },
    TtsVoices {
        result: Box<TtsVoiceDiscoveryResult>,
    },
    Simulation {
        result: Box<crate::SimulationResult>,
    },
    SelectedStt {
        result: Box<SelectedSttControlResult>,
    },
    Cancelled {
        generation: u64,
    },
    ShuttingDown,
}

pub struct SelectedSttControlRequest {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub route_snapshot_generation: u64,
    pub route: SelectedProviderRoute,
    pub turn: SelectedSttTurnRequest,
    pub lease: BrokerAudioInputLease,
}

impl<'de> Deserialize<'de> for SelectedSttControlRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            schema_version: u32,
            source_loadout_id: String,
            route_snapshot_generation: u64,
            route: SelectedProviderRoute,
            turn: SelectedSttTurnRequest,
            lease: BrokerAudioInputLease,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire.schema_version,
            source_loadout_id: wire.source_loadout_id,
            route_snapshot_generation: wire.route_snapshot_generation,
            route: wire.route,
            turn: wire.turn,
            lease: wire.lease,
        })
    }
}

impl std::fmt::Debug for SelectedSttControlRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SelectedSttControlRequest")
            .field("schema_version", &self.schema_version)
            .field("source_loadout_id", &self.source_loadout_id)
            .field("route_snapshot_generation", &self.route_snapshot_generation)
            .field("route", &self.route)
            .field("identity", &self.turn.identity)
            .field("attempt", &self.turn.attempt)
            .field(
                "context_hint_bytes",
                &self.turn.context_hint.as_ref().map(String::len),
            )
            .field("lease", &self.lease)
            .finish()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttControlResult {
    pub schema_version: u32,
    pub source_loadout_id: String,
    pub route_snapshot_generation: u64,
    pub result: SelectedSttResult,
}

impl std::fmt::Debug for SelectedSttControlResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SelectedSttControlResult")
            .field("schema_version", &self.schema_version)
            .field("source_loadout_id", &self.source_loadout_id)
            .field("route_snapshot_generation", &self.route_snapshot_generation)
            .field("result", &self.result)
            .finish()
    }
}

const MAX_CONTROL_BODY_BYTES: usize = MAX_CONTROL_MESSAGE_BYTES - 4 * 1024;
const MAX_CONTROL_JSON_BYTES: usize = MAX_CONTROL_MESSAGE_BYTES / 2;
const MAX_CONTROL_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct HandshakeState {
    session: SessionId,
    request_tracker: SequenceTracker,
    qpc_frequency_hz: u64,
}

#[derive(Debug)]
struct ResponseIntent {
    request_id: RequestId,
    request_sequence: u64,
    operation: ControlOperationV1,
    turn_id: Option<TurnId>,
    trace_id: TraceId,
    deadline_qpc_ticks: u64,
    cancellation_generation: u64,
    body: control_response_v1::Body,
}

impl HostState {
    pub async fn serve(self: Arc<Self>, options: ServeOptions) -> Result<(), ControlError> {
        #[cfg(windows)]
        {
            crate::control::windows::serve_named_pipe(self, options).await
        }
        #[cfg(not(windows))]
        {
            crate::control::portable::serve_unix(self, options).await
        }
    }
}

async fn serve_connection<S>(
    state: Arc<HostState>,
    stream: S,
    launch_nonce: LaunchNonce,
    shutdown: CancellationToken,
) -> Result<(), ControlError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut handshake = handshake(&mut reader, &mut writer, &launch_nonce).await?;
    let session = handshake.session.clone();
    let qpc_frequency_hz = handshake.qpc_frequency_hz;
    let highest_generation = Arc::new(AtomicU64::new(0));
    let writer_generation = Arc::clone(&highest_generation);
    let writer_nonce = launch_nonce.clone();
    let writer_session = session.clone();
    let (response_tx, mut response_rx) = mpsc::channel::<ResponseIntent>(32);
    let mut writer_task = tokio::spawn(async move {
        let mut response_sequence = 2_u64;
        while let Some(intent) = response_rx.recv().await {
            let now = qpc_now();
            if intent.cancellation_generation < writer_generation.load(Ordering::Acquire)
                || now.0 >= intent.deadline_qpc_ticks
            {
                // Obsolete or late work is intentionally not put on the wire.
                continue;
            }
            let response = ControlResponseV1 {
                request_id: Some(intent.request_id),
                request_sequence: intent.request_sequence,
                operation: intent.operation as i32,
                body: Some(intent.body),
            };
            let envelope = EnvelopeV1::new(
                writer_nonce.clone(),
                writer_session.clone(),
                intent.turn_id,
                intent.trace_id,
                response_sequence,
                now.0,
                now.1,
                intent.deadline_qpc_ticks,
                intent.cancellation_generation,
                envelope_v1::Body::ControlResponse(response),
            )
            .map_err(|_| FrameError::InvalidLength)?;
            let context = control_context(
                now.0,
                &writer_nonce,
                &writer_session,
                MAX_CONTROL_MESSAGE_BYTES,
            );
            let frame = encode_frame(&envelope, &context).map_err(|_| FrameError::InvalidLength)?;
            write_frame(&mut writer, &frame[4..], MAX_CONTROL_MESSAGE_BYTES).await?;
            response_sequence = response_sequence.saturating_add(1);
        }
        Ok::<(), FrameError>(())
    });

    let mut active: Option<CancellationToken> = None;
    loop {
        let bytes = tokio::select! {
            _ = shutdown.cancelled() => break,
            result = read_frame(&mut reader, MAX_CONTROL_MESSAGE_BYTES) => match result {
                Ok(frame) => frame,
                Err(FrameError::EndOfStream) => break,
                Err(error) => return Err(error.into()),
            }
        };
        let now = qpc_now();
        let context = control_context(now.0, &launch_nonce, &session, MAX_CONTROL_MESSAGE_BYTES);
        let envelope = decode_frame(&bytes, &context).map_err(|_| ControlError::Malformed)?;
        validate_control_timing(&envelope, now, qpc_frequency_hz)?;
        handshake
            .request_tracker
            .observe(&envelope, context)
            .map_err(|_| ControlError::Malformed)?;
        let request_frame = match envelope.body.as_ref() {
            Some(envelope_v1::Body::ControlRequest(value)) => value,
            _ => return Err(ControlError::Malformed),
        };
        let operation = request_frame.operation();
        validate_turn_scope(operation, envelope.turn_id.as_ref())?;
        let request_id = request_frame
            .request_id
            .clone()
            .ok_or(ControlError::Malformed)?;
        let request_json = &request_frame.payload_json;
        let request: ControlRequest =
            serde_json::from_slice(request_json).map_err(|_| ControlError::Malformed)?;
        if control_operation(&request) != operation {
            return Err(ControlError::Malformed);
        }
        let current_generation = highest_generation.load(Ordering::Acquire);
        if envelope.cancellation_generation != current_generation {
            return Err(ControlError::Malformed);
        }
        let response_meta = ResponseMetadata {
            request_id,
            request_sequence: envelope.sequence,
            operation,
            turn_id: envelope.turn_id.clone(),
            trace_id: envelope.trace_id.clone().ok_or(ControlError::Malformed)?,
            deadline_qpc_ticks: envelope.deadline_qpc_ticks,
        };
        if active.as_ref().is_some_and(CancellationToken::is_cancelled) {
            active = None;
        }
        match request {
            ControlRequest::Cancel { new_generation } => {
                if new_generation <= current_generation {
                    send_error(
                        &response_tx,
                        response_meta,
                        current_generation,
                        ErrorCode::OutOfOrder,
                        "stale_cancellation_generation",
                        false,
                    )
                    .await?;
                    continue;
                }
                highest_generation.store(new_generation, Ordering::Release);
                if let Some(token) = active.take() {
                    token.cancel();
                }
                send_success(
                    &response_tx,
                    response_meta,
                    new_generation,
                    ControlResponse::Cancelled {
                        generation: new_generation,
                    },
                )
                .await?;
            }
            ControlRequest::Shutdown => {
                if let Some(token) = active.take() {
                    token.cancel();
                }
                send_success(
                    &response_tx,
                    response_meta,
                    current_generation,
                    ControlResponse::ShuttingDown,
                )
                .await?;
                shutdown.cancel();
                break;
            }
            ControlRequest::TranscribeSelectedStt(request) => {
                if active.is_some() {
                    send_error(
                        &response_tx,
                        response_meta,
                        current_generation,
                        ErrorCode::PolicyBlocked,
                        "foreground_turn_busy",
                        true,
                    )
                    .await?;
                    continue;
                }
                let envelope_turn = response_meta
                    .turn_id
                    .as_ref()
                    .ok_or(ControlError::Malformed)?;
                if !selected_stt_control_request_valid(&request, &session, envelope_turn) {
                    send_error(
                        &response_tx,
                        response_meta,
                        current_generation,
                        ErrorCode::InvalidArgument,
                        "selected_stt_request_invalid",
                        false,
                    )
                    .await?;
                    continue;
                }
                let cancellation = CancellationToken::new();
                active = Some(cancellation.clone());
                let completion_marker = cancellation.clone();
                let state = Arc::clone(&state);
                let response_tx = response_tx.clone();
                let generation = current_generation;
                let remaining = qpc_remaining(response_meta.deadline_qpc_ticks, qpc_now());
                tokio::spawn(async move {
                    let SelectedSttControlRequest {
                        schema_version: _,
                        source_loadout_id,
                        route_snapshot_generation,
                        route,
                        turn,
                        lease,
                    } = *request;
                    let operation = async {
                        let selected = selected_hosted_stt(&route, Arc::clone(&state.vault))?;
                        let input = BrokerPcmInputSource::connect(lease, cancellation.clone())
                            .await
                            .map_err(|_| SelectedSttError::NativeInputUnavailable)?;
                        selected
                            .transcribe_push_to_talk(turn, Box::new(input), cancellation.clone())
                            .await
                    };
                    match tokio::time::timeout(remaining, operation).await {
                        Ok(Ok(result)) => {
                            let _ = send_success(
                                &response_tx,
                                response_meta,
                                generation,
                                ControlResponse::SelectedStt {
                                    result: Box::new(SelectedSttControlResult {
                                        schema_version: 1,
                                        source_loadout_id,
                                        route_snapshot_generation,
                                        result,
                                    }),
                                },
                            )
                            .await;
                        }
                        Ok(Err(error)) => {
                            let (code, stable_message, retryable) =
                                selected_stt_control_failure(error);
                            let _ = send_error(
                                &response_tx,
                                response_meta,
                                generation,
                                code,
                                stable_message,
                                retryable,
                            )
                            .await;
                        }
                        Err(_) => cancellation.cancel(),
                    }
                    completion_marker.cancel();
                });
            }
            ControlRequest::SimulateTurn(request) => {
                if active.is_some() {
                    send_error(
                        &response_tx,
                        response_meta,
                        current_generation,
                        ErrorCode::PolicyBlocked,
                        "foreground_turn_busy",
                        true,
                    )
                    .await?;
                    continue;
                }
                let cancellation = CancellationToken::new();
                active = Some(cancellation.clone());
                let completion_marker = cancellation.clone();
                let state = Arc::clone(&state);
                let response_tx = response_tx.clone();
                let generation = current_generation;
                let remaining = qpc_remaining(response_meta.deadline_qpc_ticks, qpc_now());
                tokio::spawn(async move {
                    let operation = state.simulate_turn_cancellable(*request, cancellation.clone());
                    match tokio::time::timeout(remaining, operation).await {
                        Ok(Ok(result)) => {
                            let _ = send_success(
                                &response_tx,
                                response_meta,
                                generation,
                                ControlResponse::Simulation {
                                    result: Box::new(result),
                                },
                            )
                            .await;
                        }
                        Ok(Err(error)) => {
                            let (code, stable_message, retryable) =
                                simulation_control_failure(&error);
                            let _ = send_error(
                                &response_tx,
                                response_meta,
                                generation,
                                code,
                                stable_message,
                                retryable,
                            )
                            .await;
                        }
                        Err(_) => cancellation.cancel(),
                    }
                    completion_marker.cancel();
                });
            }
            request => {
                let remaining = qpc_remaining(response_meta.deadline_qpc_ticks, qpc_now());
                let operation = async {
                    match request {
                        ControlRequest::Ping => ControlResponse::Pong,
                        ControlRequest::Doctor => ControlResponse::Doctor {
                            report: Box::new(state.doctor().await),
                        },
                        ControlRequest::ValidateProfiles => ControlResponse::Profiles {
                            profiles: state.profiles.summaries(),
                        },
                        ControlRequest::DiscoverTtsVoices(request) => ControlResponse::TtsVoices {
                            result: Box::new(state.tts_voice_discovery.discover(request).await),
                        },
                        _ => unreachable!("handled above"),
                    }
                };
                if let Ok(response) = tokio::time::timeout(remaining, operation).await {
                    send_success(&response_tx, response_meta, current_generation, response).await?;
                }
            }
        }
    }
    drop(response_tx);
    match tokio::time::timeout(Duration::from_secs(2), &mut writer_task).await {
        Ok(Ok(Ok(()))) | Err(_) => {}
        Ok(Ok(Err(error))) => return Err(error.into()),
        Ok(Err(_)) => return Err(ControlError::Internal),
    }
    if !writer_task.is_finished() {
        writer_task.abort();
    }
    Ok(())
}

fn simulation_control_failure(error: &SimulationError) -> (ErrorCode, &'static str, bool) {
    match error {
        SimulationError::InvalidRequest
        | SimulationError::UnknownGame
        | SimulationError::UnknownCharacter
        | SimulationError::CharacterDatabaseUnavailable
        | SimulationError::InvalidIdentityEvidence
        | SimulationError::IdentityEvidenceWrongGame
        | SimulationError::IdentityAmbiguous
        | SimulationError::ExplicitCharacterSelectionRequired
        | SimulationError::GenericSelectionRequired
        | SimulationError::Generic(_) => (
            ErrorCode::InvalidArgument,
            "simulation_request_invalid",
            false,
        ),
        SimulationError::ProtectedOnlineBlocked | SimulationError::AntiCheatBlocked => {
            (ErrorCode::PolicyBlocked, "simulation_policy_blocked", false)
        }
        SimulationError::SafetyEvidenceUnverified => (
            ErrorCode::PolicyBlocked,
            "simulation_safety_evidence_unverified",
            false,
        ),
        SimulationError::DevLiveTtsUnavailable => (
            ErrorCode::ProviderUnavailable,
            "dev_live_tts_unavailable",
            true,
        ),
        SimulationError::DevLiveTtsTurnIncomplete => (
            ErrorCode::ProviderUnavailable,
            "dev_live_tts_turn_incomplete",
            true,
        ),
        SimulationError::DevLiveTtsAudioNotDelivered => (
            ErrorCode::DeviceUnavailable,
            "dev_live_tts_audio_not_delivered",
            true,
        ),
        SimulationError::DevLiveTtsProviderNotSelected => (
            ErrorCode::ProviderUnavailable,
            "dev_live_tts_provider_not_selected",
            true,
        ),
        SimulationError::DevLiveTtsReceiptCountMismatch => (
            ErrorCode::DeviceUnavailable,
            "dev_live_tts_receipt_count_mismatch",
            true,
        ),
        SimulationError::DevLiveTtsReceiptMismatch => (
            ErrorCode::DeviceUnavailable,
            "dev_live_tts_receipt_mismatch",
            true,
        ),
        SimulationError::DevLiveTtsReceiptIncomplete => (
            ErrorCode::DeviceUnavailable,
            "dev_live_tts_receipt_incomplete",
            true,
        ),
        SimulationError::Runtime => (
            ErrorCode::ProviderUnavailable,
            "simulation_runtime_failed",
            true,
        ),
    }
}

fn selected_stt_control_request_valid(
    request: &SelectedSttControlRequest,
    session: &SessionId,
    turn: &TurnId,
) -> bool {
    request.schema_version == 1
        && !request.source_loadout_id.is_empty()
        && request.source_loadout_id.len() <= 128
        && !request.source_loadout_id.chars().any(char::is_control)
        && request.route_snapshot_generation > 0
        && request.turn.identity.session_id == session.to_string()
        && request.turn.identity.turn_id == turn.to_string()
}

fn selected_stt_control_failure(error: SelectedSttError) -> (ErrorCode, &'static str, bool) {
    match error {
        SelectedSttError::Cancelled => (ErrorCode::Cancelled, "selected_stt_cancelled", false),
        SelectedSttError::UnsupportedRoute
        | SelectedSttError::CredentialReferenceMismatch
        | SelectedSttError::InvalidNativeInput
        | SelectedSttError::ManualRetryNotAuthorized
        | SelectedSttError::InvalidRequest => (
            ErrorCode::PolicyBlocked,
            "selected_stt_policy_blocked",
            false,
        ),
        SelectedSttError::CredentialUnavailable => (
            ErrorCode::ProviderUnavailable,
            "selected_stt_credential_unavailable",
            false,
        ),
        SelectedSttError::NativeInputUnavailable => (
            ErrorCode::DeviceUnavailable,
            "selected_stt_input_unavailable",
            true,
        ),
        SelectedSttError::ProviderUnavailable | SelectedSttError::ProviderProtocol => (
            ErrorCode::ProviderUnavailable,
            "selected_stt_provider_unavailable",
            true,
        ),
    }
}

async fn handshake<R, W>(
    reader: &mut R,
    writer: &mut W,
    launch_nonce: &LaunchNonce,
) -> Result<HandshakeState, ControlError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let frame = read_frame(reader, DEFAULT_MAX_FRAME_BYTES).await?;
    let now = qpc_now();
    let mut context = EnvelopeValidationContext::permissive_for_time(now.0);
    context.expected_launch_nonce = Some(launch_nonce.clone());
    let envelope = decode_frame(&frame, &context).map_err(|_| ControlError::Handshake)?;
    validate_control_timing(&envelope, now, envelope.qpc_frequency_hz)
        .map_err(|_| ControlError::Handshake)?;
    let hello = match envelope.body.as_ref() {
        Some(envelope_v1::Body::NegotiationHello(hello)) => hello,
        _ => return Err(ControlError::Handshake),
    };
    let selected = negotiate(
        VersionRange::V1,
        hello.range().map_err(|_| ControlError::Handshake)?,
    )
    .map_err(|_| ControlError::Handshake)?;
    let session = envelope.session_id.clone().ok_or(ControlError::Handshake)?;
    let mut tracker = SequenceTracker::new(
        launch_nonce.clone(),
        session.clone(),
        OrderingPolicy::StrictContiguous,
    );
    tracker
        .observe(&envelope, context)
        .map_err(|_| ControlError::Handshake)?;
    let response = EnvelopeV1::new(
        launch_nonce.clone(),
        session.clone(),
        None,
        envelope.trace_id.clone().unwrap_or_else(TraceId::new),
        1,
        now.0,
        now.1,
        now.0.saturating_add(now.1.saturating_mul(10)),
        envelope.cancellation_generation,
        envelope_v1::Body::NegotiationAccepted(NegotiationAcceptedV1 {
            selected: Some(selected),
            enabled_features: vec![
                "envelope-control-v1".to_owned(),
                "cancellation-generation".to_owned(),
                "tts-stock-voice-discovery-v1".to_owned(),
            ],
        }),
    )
    .map_err(|_| ControlError::Handshake)?;
    let mut response_context = EnvelopeValidationContext::permissive_for_time(now.0);
    response_context.expected_launch_nonce = Some(launch_nonce.clone());
    response_context.expected_session_id = Some(session.clone());
    let encoded =
        encode_frame(&response, &response_context).map_err(|_| ControlError::Handshake)?;
    write_frame(writer, &encoded[4..], DEFAULT_MAX_FRAME_BYTES).await?;
    Ok(HandshakeState {
        session,
        request_tracker: tracker,
        qpc_frequency_hz: envelope.qpc_frequency_hz,
    })
}

#[derive(Debug)]
struct ResponseMetadata {
    request_id: RequestId,
    request_sequence: u64,
    operation: ControlOperationV1,
    turn_id: Option<TurnId>,
    trace_id: TraceId,
    deadline_qpc_ticks: u64,
}

fn control_context(
    now_qpc_ticks: u64,
    launch_nonce: &LaunchNonce,
    session: &SessionId,
    maximum: usize,
) -> EnvelopeValidationContext {
    let mut context = EnvelopeValidationContext::permissive_for_time(now_qpc_ticks);
    context.expected_launch_nonce = Some(launch_nonce.clone());
    context.expected_session_id = Some(session.clone());
    context.max_body_bytes = MAX_CONTROL_BODY_BYTES;
    context.max_frame_bytes = maximum;
    context
}

fn validate_control_timing(
    envelope: &EnvelopeV1,
    now: (u64, u64),
    negotiated_frequency: u64,
) -> Result<(), ControlError> {
    if envelope.qpc_frequency_hz != negotiated_frequency
        || envelope.qpc_frequency_hz != now.1
        || envelope.qpc_timestamp_ticks > now.0.saturating_add(now.1)
        || envelope.deadline_qpc_ticks
            > now
                .0
                .saturating_add(now.1.saturating_mul(MAX_CONTROL_DEADLINE.as_secs()))
    {
        return Err(ControlError::Malformed);
    }
    Ok(())
}

fn validate_turn_scope(
    operation: ControlOperationV1,
    turn_id: Option<&TurnId>,
) -> Result<(), ControlError> {
    let turn_bound = matches!(
        operation,
        ControlOperationV1::SimulateTurn | ControlOperationV1::TranscribeSelectedStt
    );
    if turn_bound != turn_id.is_some() {
        return Err(ControlError::Malformed);
    }
    Ok(())
}

fn control_operation(request: &ControlRequest) -> ControlOperationV1 {
    match request {
        ControlRequest::Ping => ControlOperationV1::Ping,
        ControlRequest::Doctor => ControlOperationV1::Doctor,
        ControlRequest::ValidateProfiles => ControlOperationV1::ValidateProfiles,
        ControlRequest::DiscoverTtsVoices(_) => ControlOperationV1::DiscoverTtsVoices,
        ControlRequest::SimulateTurn(_) => ControlOperationV1::SimulateTurn,
        ControlRequest::TranscribeSelectedStt(_) => ControlOperationV1::TranscribeSelectedStt,
        ControlRequest::Cancel { .. } => ControlOperationV1::Cancel,
        ControlRequest::Shutdown => ControlOperationV1::Shutdown,
    }
}

async fn send_success(
    tx: &mpsc::Sender<ResponseIntent>,
    metadata: ResponseMetadata,
    generation: u64,
    response: ControlResponse,
) -> Result<(), ControlError> {
    let json = serde_json::to_vec(&response).map_err(|_| ControlError::Internal)?;
    if json.len() > MAX_CONTROL_JSON_BYTES {
        return Err(ControlError::PayloadTooLarge);
    }
    send_intent(
        tx,
        metadata,
        generation,
        control_response_v1::Body::SuccessJson(json),
    )
    .await
}

async fn send_error(
    tx: &mpsc::Sender<ResponseIntent>,
    metadata: ResponseMetadata,
    generation: u64,
    code: ErrorCode,
    stable_message: &'static str,
    retryable: bool,
) -> Result<(), ControlError> {
    let mut error = IpcErrorV1::new(code, stable_message).at_stage("control");
    if retryable {
        error = error.retryable(None);
    }
    send_intent(
        tx,
        metadata,
        generation,
        control_response_v1::Body::Error(error),
    )
    .await
}

async fn send_intent(
    tx: &mpsc::Sender<ResponseIntent>,
    metadata: ResponseMetadata,
    cancellation_generation: u64,
    body: control_response_v1::Body,
) -> Result<(), ControlError> {
    tx.send(ResponseIntent {
        request_id: metadata.request_id,
        request_sequence: metadata.request_sequence,
        operation: metadata.operation,
        turn_id: metadata.turn_id,
        trace_id: metadata.trace_id,
        deadline_qpc_ticks: metadata.deadline_qpc_ticks,
        cancellation_generation,
        body,
    })
    .await
    .map_err(|_| ControlError::Disconnected)
}

fn qpc_remaining(deadline_qpc_ticks: u64, now: (u64, u64)) -> Duration {
    let remaining = deadline_qpc_ticks.saturating_sub(now.0);
    Duration::from_secs_f64(remaining as f64 / now.1.max(1) as f64)
}

fn qpc_now() -> (u64, u64) {
    #[cfg(windows)]
    {
        let mut value = 0_i64;
        let mut frequency = 0_i64;
        // SAFETY: both APIs write only to valid stack-owned i64 output locations.
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

#[cfg(not(windows))]
mod portable {
    use super::*;
    use tokio::net::UnixListener;

    pub async fn serve_unix(
        state: Arc<HostState>,
        options: ServeOptions,
    ) -> Result<(), ControlError> {
        let path = options.endpoint_override.unwrap_or_else(|| {
            state
                .config
                .app_data
                .join("runtime")
                .join("npc-runtime.sock")
        });
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
                return Err(ControlError::UnsafeEndpoint);
            }
            std::fs::remove_file(&path).map_err(|_| ControlError::Bind)?;
        }
        let listener = UnixListener::bind(&path).map_err(|_| ControlError::Bind)?;
        set_unix_permissions(&path)?;
        loop {
            let accepted = tokio::select! {
                _ = options.shutdown.cancelled() => break,
                value = listener.accept() => value.map_err(|_| ControlError::Bind)?,
            };
            let state = Arc::clone(&state);
            let nonce = options.launch_nonce.clone();
            let shutdown = options.shutdown.clone();
            tokio::spawn(async move {
                let _ = serve_connection(state, accepted.0, nonce, shutdown).await;
            });
        }
        let _ = std::fs::remove_file(path);
        state.children.terminate_all();
        Ok(())
    }

    fn set_unix_permissions(path: &std::path::Path) -> Result<(), ControlError> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| ControlError::Bind)
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use tokio::net::windows::named_pipe::ServerOptions;

    pub async fn serve_named_pipe(
        state: Arc<HostState>,
        options: ServeOptions,
    ) -> Result<(), ControlError> {
        let pipe_name = scoped_pipe_name()?;
        loop {
            let mut security = PipeSecurity::current_owner_only()?;
            let mut server_options = ServerOptions::new();
            server_options
                .first_pipe_instance(false)
                .reject_remote_clients(true)
                .max_instances(4);
            // SAFETY: `security.attributes` is a fully initialized SECURITY_ATTRIBUTES
            // value whose descriptor remains alive until CreateNamedPipeW returns.
            let server = unsafe {
                server_options.create_with_security_attributes_raw(
                    &pipe_name,
                    (&mut security.attributes
                        as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES)
                        .cast(),
                )
            }
            .map_err(|_| ControlError::Bind)?;
            tokio::select! {
                _ = options.shutdown.cancelled() => break,
                result = server.connect() => result.map_err(|_| ControlError::Bind)?,
            }
            let state = Arc::clone(&state);
            let nonce = options.launch_nonce.clone();
            let shutdown = options.shutdown.clone();
            tokio::spawn(async move {
                let _ = serve_connection(state, server, nonce, shutdown).await;
            });
        }
        state.children.terminate_all();
        Ok(())
    }

    fn scoped_pipe_name() -> Result<String, ControlError> {
        use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        // SAFETY: GetCurrentProcessId has no preconditions.
        let process = unsafe { GetCurrentProcessId() };
        let mut session = 0_u32;
        // SAFETY: `session` is a valid writable u32 for the duration of the call.
        if unsafe { ProcessIdToSessionId(process, &mut session) } == 0 {
            return Err(ControlError::Bind);
        }
        // The endpoint is session-scoped; its explicit DACL permits only the
        // object owner and LocalSystem. Remote clients are rejected separately.
        Ok(format!(r"\\.\pipe\interactive-npcs-v2-session-{session}"))
    }

    struct PipeSecurity {
        descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
        attributes: windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    }

    impl PipeSecurity {
        fn current_owner_only() -> Result<Self, ControlError> {
            use windows_sys::Win32::Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            };
            // OW is the object owner (the current logon user for this pipe); SY
            // permits LocalSystem for service diagnostics. D:P makes the DACL protected.
            let sddl: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GA;;;SY)"
                .encode_utf16()
                .chain([0])
                .collect();
            let mut descriptor = std::ptr::null_mut();
            // SAFETY: `sddl` is NUL-terminated UTF-16 and `descriptor` is a valid
            // writable output pointer released with LocalFree in Drop.
            let converted = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if converted == 0 || descriptor.is_null() {
                return Err(ControlError::Bind);
            }
            Ok(Self {
                descriptor,
                attributes: windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>(
                    ) as u32,
                    lpSecurityDescriptor: descriptor.cast(),
                    bInheritHandle: 0,
                },
            })
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: the descriptor was allocated by the SDDL conversion API and
            // is released exactly once when this owner is dropped.
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(self.descriptor.cast());
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("control endpoint could not be bound")]
    Bind,
    #[error("control endpoint path is unsafe")]
    UnsafeEndpoint,
    #[error("control protocol handshake failed")]
    Handshake,
    #[error("control message is malformed, stale, or out of order")]
    Malformed,
    #[error("control response exceeded its size limit")]
    PayloadTooLarge,
    #[error("control peer disconnected")]
    Disconnected,
    #[error("control server failed internally")]
    Internal,
    #[error(transparent)]
    Frame(#[from] FrameError),
}

#[allow(dead_code)]
fn _connection_timeout() -> Duration {
    Duration::from_secs(10)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use npc_protocol::{ControlRequestV1, NegotiationHelloV1, ProtocolVersion};
    use prost::Message;

    async fn state() -> Arc<HostState> {
        let app_data = tempfile::tempdir().unwrap();
        let app_data = app_data.keep();
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        Arc::new(
            HostState::initialize(crate::HostConfig {
                repo_root,
                app_data,
            })
            .await
            .unwrap(),
        )
    }

    async fn authenticated_pair() -> (
        tokio::io::DuplexStream,
        tokio::task::JoinHandle<Result<(), ControlError>>,
        LaunchNonce,
        SessionId,
    ) {
        let nonce = LaunchNonce::new();
        let session = SessionId::new();
        let shutdown = CancellationToken::new();
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(serve_connection(
            state().await,
            server,
            nonce.clone(),
            shutdown,
        ));
        let now = qpc_now();
        let hello = EnvelopeV1::new(
            nonce.clone(),
            session.clone(),
            None,
            TraceId::new(),
            1,
            now.0,
            now.1,
            now.0 + now.1 * 10,
            0,
            envelope_v1::Body::NegotiationHello(NegotiationHelloV1 {
                minimum: Some(ProtocolVersion::CURRENT),
                maximum: Some(ProtocolVersion::CURRENT),
                optional_features: vec!["envelope-control-v1".into()],
                peer_name: "fixture-client".into(),
                peer_build: "test".into(),
            }),
        )
        .unwrap();
        let mut context = EnvelopeValidationContext::permissive_for_time(now.0);
        context.expected_launch_nonce = Some(nonce.clone());
        context.expected_session_id = Some(session.clone());
        let bytes = encode_frame(&hello, &context).unwrap();
        write_frame(&mut client, &bytes[4..], DEFAULT_MAX_FRAME_BYTES)
            .await
            .unwrap();
        let accepted = read_frame(&mut client, DEFAULT_MAX_FRAME_BYTES)
            .await
            .unwrap();
        assert!(matches!(
            decode_frame(&accepted, &context).unwrap().body,
            Some(envelope_v1::Body::NegotiationAccepted(_))
        ));
        (client, server_task, nonce, session)
    }

    fn request(
        nonce: LaunchNonce,
        session: SessionId,
        sequence: u64,
        generation: u64,
        operation: ControlOperationV1,
        json: &[u8],
    ) -> EnvelopeV1 {
        let now = qpc_now();
        EnvelopeV1::new(
            nonce,
            session,
            (operation == ControlOperationV1::SimulateTurn).then(TurnId::new),
            TraceId::new(),
            sequence,
            now.0,
            now.1,
            now.0.saturating_add(now.1.saturating_mul(10)),
            generation,
            envelope_v1::Body::ControlRequest(ControlRequestV1 {
                request_id: Some(RequestId::new()),
                operation: operation as i32,
                payload_json: json.to_vec(),
            }),
        )
        .unwrap()
    }

    async fn write_raw(client: &mut tokio::io::DuplexStream, envelope: &EnvelopeV1) {
        write_frame(client, &envelope.encode_to_vec(), MAX_CONTROL_MESSAGE_BYTES)
            .await
            .unwrap();
    }

    async fn response(
        client: &mut tokio::io::DuplexStream,
        nonce: &LaunchNonce,
        session: &SessionId,
    ) -> EnvelopeV1 {
        let frame = read_frame(client, MAX_CONTROL_MESSAGE_BYTES).await.unwrap();
        let now = qpc_now();
        let context = control_context(now.0, nonce, session, MAX_CONTROL_MESSAGE_BYTES);
        decode_frame(&frame, &context).unwrap()
    }

    #[tokio::test]
    async fn authenticated_envelope_ping_and_shutdown_have_clean_lifecycle() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let ping = request(
            nonce.clone(),
            session.clone(),
            2,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        let ping_request = match ping.body.as_ref().unwrap() {
            envelope_v1::Body::ControlRequest(request) => request.clone(),
            _ => unreachable!(),
        };
        write_raw(&mut client, &ping).await;
        let pong = response(&mut client, &nonce, &session).await;
        assert_eq!(pong.sequence, 2);
        assert_eq!(pong.protocol_version, ping.protocol_version);
        assert_eq!(pong.launch_nonce, ping.launch_nonce);
        assert_eq!(pong.session_id, ping.session_id);
        assert_eq!(pong.turn_id, ping.turn_id);
        assert_eq!(pong.trace_id, ping.trace_id);
        assert_eq!(pong.deadline_qpc_ticks, ping.deadline_qpc_ticks);
        assert_eq!(pong.cancellation_generation, ping.cancellation_generation);
        let pong = match pong.body.unwrap() {
            envelope_v1::Body::ControlResponse(response) => response,
            _ => panic!("expected control response"),
        };
        assert_eq!(pong.request_id, ping_request.request_id);
        assert_eq!(pong.request_sequence, 2);
        assert_eq!(pong.operation(), ControlOperationV1::Ping);
        let body = match pong.body.unwrap() {
            control_response_v1::Body::SuccessJson(json) => json,
            _ => panic!("expected success response"),
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["type"],
            "pong"
        );

        let shutdown = request(
            nonce.clone(),
            session.clone(),
            3,
            0,
            ControlOperationV1::Shutdown,
            br#"{"type":"shutdown"}"#,
        );
        write_raw(&mut client, &shutdown).await;
        let shutdown_response = response(&mut client, &nonce, &session).await;
        assert_eq!(shutdown_response.sequence, 3);
        drop(client);
        assert!(server_task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn wrong_launch_nonce_is_rejected_before_dispatch() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let mut envelope = request(
            nonce,
            session,
            2,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        envelope.launch_nonce = Some(LaunchNonce::new());
        write_raw(&mut client, &envelope).await;
        drop(client);
        assert!(matches!(
            server_task.await.unwrap(),
            Err(ControlError::Malformed)
        ));
    }

    #[tokio::test]
    async fn wrong_session_is_rejected_before_dispatch() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let mut envelope = request(
            nonce,
            session,
            2,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        envelope.session_id = Some(SessionId::new());
        write_raw(&mut client, &envelope).await;
        drop(client);
        assert!(matches!(
            server_task.await.unwrap(),
            Err(ControlError::Malformed)
        ));
    }

    #[tokio::test]
    async fn expired_deadline_is_rejected_before_dispatch() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let mut envelope = request(
            nonce,
            session,
            2,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        envelope.qpc_timestamp_ticks = 1;
        envelope.deadline_qpc_ticks = 2;
        write_raw(&mut client, &envelope).await;
        drop(client);
        assert!(matches!(
            server_task.await.unwrap(),
            Err(ControlError::Malformed)
        ));
    }

    #[tokio::test]
    async fn sequence_gap_and_generation_jump_are_rejected() {
        for (sequence, generation) in [(3, 0), (2, 1)] {
            let (mut client, server_task, nonce, session) = authenticated_pair().await;
            let envelope = request(
                nonce,
                session,
                sequence,
                generation,
                ControlOperationV1::Ping,
                br#"{"type":"ping"}"#,
            );
            write_raw(&mut client, &envelope).await;
            drop(client);
            assert!(matches!(
                server_task.await.unwrap(),
                Err(ControlError::Malformed)
            ));
        }
    }

    #[tokio::test]
    async fn stale_generation_after_valid_cancellation_is_rejected() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let cancel = request(
            nonce.clone(),
            session.clone(),
            2,
            0,
            ControlOperationV1::Cancel,
            br#"{"type":"cancel","new_generation":1}"#,
        );
        write_raw(&mut client, &cancel).await;
        let cancelled = response(&mut client, &nonce, &session).await;
        assert_eq!(cancelled.cancellation_generation, 1);

        let stale = request(
            nonce,
            session,
            3,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        write_raw(&mut client, &stale).await;
        drop(client);
        assert!(matches!(
            server_task.await.unwrap(),
            Err(ControlError::Malformed)
        ));
    }

    #[tokio::test]
    async fn tampered_declared_body_size_is_rejected() {
        let (mut client, server_task, nonce, session) = authenticated_pair().await;
        let mut envelope = request(
            nonce,
            session,
            2,
            0,
            ControlOperationV1::Ping,
            br#"{"type":"ping"}"#,
        );
        envelope.declared_body_bytes = envelope.declared_body_bytes.saturating_add(1);
        write_raw(&mut client, &envelope).await;
        drop(client);
        assert!(matches!(
            server_task.await.unwrap(),
            Err(ControlError::Malformed)
        ));
    }

    #[test]
    fn stock_voice_discovery_request_has_a_distinct_authenticated_operation() {
        let request: ControlRequest = serde_json::from_value(serde_json::json!({
            "type": "discover_tts_voices",
            "schemaVersion": 1,
            "providerId": "nvidia-nim-magpie",
            "modelId": "magpie-tts-multilingual",
            "forceRefresh": false
        }))
        .expect("strict discovery request");
        assert_eq!(
            control_operation(&request),
            ControlOperationV1::DiscoverTtsVoices
        );
        assert!(serde_json::from_value::<ControlRequest>(serde_json::json!({
            "type": "discover_tts_voices",
            "schemaVersion": 1,
            "providerId": "nvidia-nim-magpie",
            "modelId": "magpie-tts-multilingual",
            "forceRefresh": false,
            "credential": "must-not-cross-wire"
        }))
        .is_err());
    }

    #[test]
    fn selected_stt_control_request_is_turn_bound_strict_and_debug_redacted() {
        let session = SessionId::new();
        let turn = TurnId::new();
        let request: ControlRequest = serde_json::from_value(serde_json::json!({
            "type": "transcribe_selected_stt",
            "schemaVersion": 1,
            "sourceLoadoutId": "review-loadout",
            "routeSnapshotGeneration": 7,
            "route": {
                "providerId": "assemblyai",
                "modelId": "u3-rt-pro",
                "execution": "cloud",
                "egress": "microphone_audio_and_optional_non_secret_context",
                "credentialReference": "providers/assemblyai"
            },
            "turn": {
                "identity": {
                    "sessionId": session.to_string(),
                    "turnId": turn.to_string(),
                    "generation": 4,
                    "inputEndpointId": "endpoint-fixture",
                    "inputEndpointGeneration": 3
                },
                "attempt": { "kind": "initial" },
                "contextHint": "sensitive fixture context"
            },
            "lease": {
                "schemaVersion": 2,
                "streamId": "mic-fixture",
                "producerEndpoint": r"\\.\pipe\npc-media-input-fixture",
                "oneTimeToken": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
                "sessionId": session.to_string(),
                "turnId": turn.to_string(),
                "generation": 4,
                "durationMs": 500,
                "sampleRate": 16000,
                "channels": 1,
                "maxFrames": 8000,
                "maxChunkBytes": 65536,
                "expiresQpc": 99,
                "qpcFrequency": 10000000,
                "inputSelectionMode": "systemDefault",
                "inputEndpointId": "endpoint-fixture",
                "inputEndpointGeneration": 3,
                "activationSource": "pushToTalk",
                "pttVirtualKey": 88,
                "pttPressTransitionSequence": 8,
                "pttPressedQpc": 10
            }
        }))
        .expect("strict selected STT request");
        assert_eq!(
            control_operation(&request),
            ControlOperationV1::TranscribeSelectedStt
        );
        assert!(
            validate_turn_scope(ControlOperationV1::TranscribeSelectedStt, Some(&turn)).is_ok()
        );
        let ControlRequest::TranscribeSelectedStt(request) = &request else {
            panic!("selected STT request");
        };
        assert!(selected_stt_control_request_valid(request, &session, &turn));
        let debug = format!("{request:?}");
        assert!(debug.contains("context_hint_bytes"));
        assert!(!debug.contains("sensitive fixture context"));
        assert!(!debug.contains("010203040506"));
    }
}
