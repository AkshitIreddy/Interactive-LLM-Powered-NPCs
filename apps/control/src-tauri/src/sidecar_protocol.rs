use npc_protocol::{
    control_response_v1, decode_frame, encode_frame, envelope_v1, ControlOperationV1,
    ControlRequestV1, EnvelopeV1, EnvelopeValidationContext, EnvelopeValidationError, LaunchNonce,
    NegotiationHelloV1, OrderingPolicy, ProtocolVersion, RequestId, SequenceTracker, SessionId,
    TraceId, TurnId, DEFAULT_MAX_FRAME_BYTES,
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

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireRequest {
    Ping,
    Doctor,
    ValidateProfiles,
    SimulateTurn(Box<NativeSimulationRequest>),
    Cancel { new_generation: u64 },
    Shutdown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSimulationRequest {
    pub session_id: String,
    pub turn_id: String,
    pub game_id: String,
    pub character_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generic_selection: Option<NativeGenericGameSelection>,
    #[serde(
        default,
        skip_serializing_if = "NativeSimulationSafetyContext::is_clear"
    )]
    pub safety_context: NativeSimulationSafetyContext,
    pub transcript: String,
    pub locale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<NativeExecutionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_live_tts: Option<NativeDevLiveTtsRequest>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDevLiveTtsRequest {
    pub provider_id: String,
    pub model_id: String,
    pub voice_id: String,
    pub explicit_user_authorization: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeSimulationSafetyContext {
    pub protected_online_detected: bool,
    pub anti_cheat_detected: bool,
}

impl NativeSimulationSafetyContext {
    fn is_clear(&self) -> bool {
        !self.protected_online_detected && !self.anti_cheat_detected
    }
}

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
pub enum WireResponse {
    Pong {},
    Doctor { report: NativeDoctorReport },
    Profiles { profiles: Vec<NativeProfile> },
    Simulation { result: NativeSimulationResult },
    Cancelled { generation: u64 },
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
    let turn_id = (operation == ControlOperationV1::SimulateTurn).then(TurnId::new);
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
                    serde_json::from_slice(&json).map_err(|_| ClientError::Malformed)
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
        WireRequest::SimulateTurn(_) => ControlOperationV1::SimulateTurn,
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
    }

    #[test]
    fn boxed_simulation_request_preserves_wire_shape() {
        let request = WireRequest::SimulateTurn(Box::new(NativeSimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "eclipse-harbor".into(),
            character_id: Some("mara-venn".into()),
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Can you hear me?".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
        }));
        let encoded = serde_json::to_string(&request).expect("serialize legacy simulation request");
        assert_eq!(
            encoded,
            r#"{"type":"simulate_turn","sessionId":"session-1","turnId":"turn-1","gameId":"eclipse-harbor","characterId":"mara-venn","transcript":"Can you hear me?","locale":"en-US"}"#
        );

        assert_eq!(
            serde_json::to_value(request).expect("serialize simulation request"),
            serde_json::json!({
                "type": "simulate_turn",
                "sessionId": "session-1",
                "turnId": "turn-1",
                "gameId": "eclipse-harbor",
                "characterId": "mara-venn",
                "transcript": "Can you hear me?",
                "locale": "en-US"
            })
        );
    }

    #[test]
    fn authorized_dev_live_tts_wire_contains_route_identifiers_only() {
        let request = WireRequest::SimulateTurn(Box::new(NativeSimulationRequest {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            game_id: "generic-game".into(),
            character_id: None,
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Can you hear me?".into(),
            locale: "en-US".into(),
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
        for forbidden in ["apiKey", "credential", "secret", "token"] {
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
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext {
                protected_online_detected: true,
                anti_cheat_detected: false,
            },
            transcript: "This route must be refused.".into(),
            locale: "en-US".into(),
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
