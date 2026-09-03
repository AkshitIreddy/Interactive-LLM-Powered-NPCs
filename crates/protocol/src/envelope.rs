use crate::{
    negotiate, ActorId, IpcErrorV1, LaunchNonce, LlmEventV1, NegotiationAcceptedV1,
    NegotiationHelloV1, NpcEffectsV1, ProtocolVersion, RequestId, SessionId, SttEventV1, TraceId,
    TtsEventV1, TurnId, VersionRange, DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_FRAME_BYTES,
};
use prost::{Enumeration, Message, Oneof};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderingPolicy {
    /// Every message must be exactly the successor of the previous message.
    StrictContiguous,
    /// Gaps are permitted, but replay and reordering are rejected.
    Monotonic,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct HeartbeatV1 {
    #[prost(uint64, tag = "1")]
    pub uptime_ms: u64,
    #[prost(string, tag = "2")]
    pub component: String,
    #[prost(string, tag = "3")]
    pub state: String,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct AckV1 {
    #[prost(uint64, tag = "1")]
    pub acknowledged_sequence: u64,
    #[prost(uint64, tag = "2")]
    pub observed_cancellation_generation: u64,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct CancelRequestV1 {
    #[prost(uint64, tag = "1")]
    pub new_generation: u64,
    #[prost(string, tag = "2")]
    pub reason: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ActorSelectedV1 {
    #[prost(message, optional, tag = "1")]
    pub actor_id: Option<ActorId>,
    #[prost(string, tag = "2")]
    pub display_name: String,
    #[prost(bool, tag = "3")]
    pub explicit_user_selection: bool,
    #[prost(float, optional, tag = "4")]
    pub identity_confidence: Option<f32>,
}

/// Stable operation discriminator for the desktop-shell control plane.
///
/// Operation selection lives outside the JSON payload so a receiver can apply
/// turn, deadline, and policy rules before deserializing application data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum ControlOperationV1 {
    Unspecified = 0,
    Ping = 1,
    Doctor = 2,
    ValidateProfiles = 3,
    SimulateTurn = 4,
    Cancel = 5,
    Shutdown = 6,
    DiscoverTtsVoices = 7,
    TranscribeSelectedStt = 8,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ControlRequestV1 {
    #[prost(message, optional, tag = "1")]
    pub request_id: Option<RequestId>,
    #[prost(enumeration = "ControlOperationV1", tag = "2")]
    pub operation: i32,
    /// Strict JSON DTO for the selected operation. It is bounded and validated
    /// by the runtime before dispatch; secrets are never valid control payloads.
    #[prost(bytes = "vec", tag = "3")]
    pub payload_json: Vec<u8>,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct ControlResponseV1 {
    #[prost(message, optional, tag = "1")]
    pub request_id: Option<RequestId>,
    #[prost(uint64, tag = "2")]
    pub request_sequence: u64,
    #[prost(enumeration = "ControlOperationV1", tag = "3")]
    pub operation: i32,
    #[prost(oneof = "control_response_v1::Body", tags = "10, 11")]
    pub body: Option<control_response_v1::Body>,
}

pub mod control_response_v1 {
    use super::*;

    #[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
    pub enum Body {
        #[prost(bytes, tag = "10")]
        SuccessJson(Vec<u8>),
        #[prost(message, tag = "11")]
        Error(IpcErrorV1),
    }
}

impl ControlRequestV1 {
    pub fn validate(&self) -> Result<(), EnvelopeValidationError> {
        self.request_id
            .as_ref()
            .ok_or(EnvelopeValidationError::MalformedBody)?
            .validate()
            .map_err(|_| EnvelopeValidationError::MalformedBody)?;
        if self.operation() == ControlOperationV1::Unspecified
            || self.payload_json.len() > DEFAULT_MAX_BODY_BYTES / 2
        {
            return Err(EnvelopeValidationError::MalformedBody);
        }
        Ok(())
    }

    #[must_use]
    pub fn is_turn_bound(&self) -> bool {
        matches!(
            self.operation(),
            ControlOperationV1::SimulateTurn | ControlOperationV1::TranscribeSelectedStt
        )
    }
}

impl ControlResponseV1 {
    pub fn validate(&self) -> Result<(), EnvelopeValidationError> {
        self.request_id
            .as_ref()
            .ok_or(EnvelopeValidationError::MalformedBody)?
            .validate()
            .map_err(|_| EnvelopeValidationError::MalformedBody)?;
        if self.request_sequence == 0 || self.operation() == ControlOperationV1::Unspecified {
            return Err(EnvelopeValidationError::MalformedBody);
        }
        match self
            .body
            .as_ref()
            .ok_or(EnvelopeValidationError::MalformedBody)?
        {
            control_response_v1::Body::SuccessJson(json)
                if json.len() <= DEFAULT_MAX_BODY_BYTES / 2 =>
            {
                Ok(())
            }
            control_response_v1::Body::Error(error) => error
                .validate()
                .map_err(|_| EnvelopeValidationError::MalformedBody),
            _ => Err(EnvelopeValidationError::MalformedBody),
        }
    }

    #[must_use]
    pub fn is_turn_bound(&self) -> bool {
        matches!(
            self.operation(),
            ControlOperationV1::SimulateTurn | ControlOperationV1::TranscribeSelectedStt
        )
    }
}

/// Version-one transport envelope.
///
/// `sequence` is process-channel global, while `cancellation_generation` is
/// session monotonic. A new generation supersedes every pending operation from
/// older generations. QPC values share `qpc_frequency_hz` and are meaningful
/// only within one boot.
#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct EnvelopeV1 {
    #[prost(message, optional, tag = "1")]
    pub protocol_version: Option<ProtocolVersion>,
    #[prost(message, optional, tag = "2")]
    pub launch_nonce: Option<LaunchNonce>,
    #[prost(message, optional, tag = "3")]
    pub session_id: Option<SessionId>,
    #[prost(message, optional, tag = "4")]
    pub turn_id: Option<TurnId>,
    #[prost(message, optional, tag = "5")]
    pub trace_id: Option<TraceId>,
    #[prost(uint64, tag = "6")]
    pub sequence: u64,
    #[prost(uint64, tag = "7")]
    pub qpc_timestamp_ticks: u64,
    #[prost(uint64, tag = "8")]
    pub qpc_frequency_hz: u64,
    #[prost(uint64, tag = "9")]
    pub deadline_qpc_ticks: u64,
    #[prost(uint64, tag = "10")]
    pub cancellation_generation: u64,
    /// Encoded length of the oneof payload itself, excluding its field key/length.
    #[prost(uint32, tag = "11")]
    pub declared_body_bytes: u32,
    #[prost(
        oneof = "envelope_v1::Body",
        tags = "20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32"
    )]
    pub body: Option<envelope_v1::Body>,
}

pub mod envelope_v1 {
    use super::*;

    #[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
    pub enum Body {
        #[prost(message, tag = "20")]
        NegotiationHello(NegotiationHelloV1),
        #[prost(message, tag = "21")]
        NegotiationAccepted(NegotiationAcceptedV1),
        #[prost(message, tag = "22")]
        Heartbeat(HeartbeatV1),
        #[prost(message, tag = "23")]
        Ack(AckV1),
        #[prost(message, tag = "24")]
        Cancel(CancelRequestV1),
        #[prost(message, tag = "25")]
        Stt(SttEventV1),
        #[prost(message, tag = "26")]
        Llm(LlmEventV1),
        #[prost(message, tag = "27")]
        Tts(TtsEventV1),
        #[prost(message, tag = "28")]
        Effects(NpcEffectsV1),
        #[prost(message, tag = "29")]
        Error(IpcErrorV1),
        #[prost(message, tag = "30")]
        ActorSelected(ActorSelectedV1),
        #[prost(message, tag = "31")]
        ControlRequest(ControlRequestV1),
        #[prost(message, tag = "32")]
        ControlResponse(ControlResponseV1),
    }
}

impl EnvelopeV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        launch_nonce: LaunchNonce,
        session_id: SessionId,
        turn_id: Option<TurnId>,
        trace_id: TraceId,
        sequence: u64,
        qpc_timestamp_ticks: u64,
        qpc_frequency_hz: u64,
        deadline_qpc_ticks: u64,
        cancellation_generation: u64,
        body: envelope_v1::Body,
    ) -> Result<Self, EnvelopeValidationError> {
        let mut envelope = Self {
            protocol_version: Some(ProtocolVersion::CURRENT),
            launch_nonce: Some(launch_nonce),
            session_id: Some(session_id),
            turn_id,
            trace_id: Some(trace_id),
            sequence,
            qpc_timestamp_ticks,
            qpc_frequency_hz,
            deadline_qpc_ticks,
            cancellation_generation,
            declared_body_bytes: 0,
            body: Some(body),
        };
        envelope.synchronize_declared_size()?;
        Ok(envelope)
    }

    pub fn synchronize_declared_size(&mut self) -> Result<(), EnvelopeValidationError> {
        let body_bytes = self.body_payload_len()?;
        self.declared_body_bytes = u32::try_from(body_bytes)
            .map_err(|_| EnvelopeValidationError::BodyTooLarge(body_bytes))?;
        Ok(())
    }

    pub fn body_payload_len(&self) -> Result<usize, EnvelopeValidationError> {
        let body = self
            .body
            .as_ref()
            .ok_or(EnvelopeValidationError::MissingBody)?;
        Ok(match body {
            envelope_v1::Body::NegotiationHello(value) => value.encoded_len(),
            envelope_v1::Body::NegotiationAccepted(value) => value.encoded_len(),
            envelope_v1::Body::Heartbeat(value) => value.encoded_len(),
            envelope_v1::Body::Ack(value) => value.encoded_len(),
            envelope_v1::Body::Cancel(value) => value.encoded_len(),
            envelope_v1::Body::Stt(value) => value.encoded_len(),
            envelope_v1::Body::Llm(value) => value.encoded_len(),
            envelope_v1::Body::Tts(value) => value.encoded_len(),
            envelope_v1::Body::Effects(value) => value.encoded_len(),
            envelope_v1::Body::Error(value) => value.encoded_len(),
            envelope_v1::Body::ActorSelected(value) => value.encoded_len(),
            envelope_v1::Body::ControlRequest(value) => value.encoded_len(),
            envelope_v1::Body::ControlResponse(value) => value.encoded_len(),
        })
    }

    #[must_use]
    pub fn requires_turn(&self) -> bool {
        matches!(
            &self.body,
            Some(
                envelope_v1::Body::Stt(_)
                    | envelope_v1::Body::Llm(_)
                    | envelope_v1::Body::Tts(_)
                    | envelope_v1::Body::Effects(_)
            )
        ) || matches!(
            &self.body,
            Some(envelope_v1::Body::ControlRequest(request)) if request.is_turn_bound()
        ) || matches!(
            &self.body,
            Some(envelope_v1::Body::ControlResponse(response)) if response.is_turn_bound()
        )
    }

    pub fn validate(
        &self,
        context: &EnvelopeValidationContext,
    ) -> Result<(), EnvelopeValidationError> {
        let version = self
            .protocol_version
            .ok_or(EnvelopeValidationError::MissingVersion)?;
        if !context.accepted_versions.contains(version) {
            return Err(EnvelopeValidationError::UnsupportedVersion(version));
        }

        let launch_nonce = self
            .launch_nonce
            .as_ref()
            .ok_or(EnvelopeValidationError::MissingLaunchNonce)?;
        launch_nonce
            .validate()
            .map_err(|_| EnvelopeValidationError::InvalidLaunchNonce)?;
        if let Some(expected) = &context.expected_launch_nonce {
            if launch_nonce != expected {
                return Err(EnvelopeValidationError::StaleLaunch);
            }
        }

        let session_id = self
            .session_id
            .as_ref()
            .ok_or(EnvelopeValidationError::MissingSession)?;
        session_id
            .validate()
            .map_err(|_| EnvelopeValidationError::InvalidSession)?;
        if let Some(expected) = &context.expected_session_id {
            if session_id != expected {
                return Err(EnvelopeValidationError::WrongSession);
            }
        }
        if self.requires_turn() {
            self.turn_id
                .as_ref()
                .ok_or(EnvelopeValidationError::MissingTurn)?
                .validate()
                .map_err(|_| EnvelopeValidationError::InvalidTurn)?;
        } else if let Some(turn_id) = &self.turn_id {
            turn_id
                .validate()
                .map_err(|_| EnvelopeValidationError::InvalidTurn)?;
        }
        self.trace_id
            .as_ref()
            .ok_or(EnvelopeValidationError::MissingTrace)?
            .validate()
            .map_err(|_| EnvelopeValidationError::InvalidTrace)?;

        if self.sequence == 0 {
            return Err(EnvelopeValidationError::ZeroSequence);
        }
        if self.qpc_frequency_hz == 0 {
            return Err(EnvelopeValidationError::ZeroQpcFrequency);
        }
        if self.deadline_qpc_ticks <= self.qpc_timestamp_ticks {
            return Err(EnvelopeValidationError::InvalidDeadline);
        }
        if context.now_qpc_ticks > self.deadline_qpc_ticks {
            return Err(EnvelopeValidationError::DeadlineExceeded);
        }

        let actual_body = self.body_payload_len()?;
        if actual_body > context.max_body_bytes {
            return Err(EnvelopeValidationError::BodyTooLarge(actual_body));
        }
        if self.declared_body_bytes as usize != actual_body {
            return Err(EnvelopeValidationError::DeclaredSizeMismatch {
                declared: self.declared_body_bytes as usize,
                actual: actual_body,
            });
        }
        let frame_bytes = self.encoded_len() + 4;
        if frame_bytes > context.max_frame_bytes {
            return Err(EnvelopeValidationError::FrameTooLarge(frame_bytes));
        }
        self.validate_body()
    }

    fn validate_body(&self) -> Result<(), EnvelopeValidationError> {
        match self
            .body
            .as_ref()
            .ok_or(EnvelopeValidationError::MissingBody)?
        {
            envelope_v1::Body::NegotiationHello(hello) => {
                let remote = hello
                    .range()
                    .map_err(|_| EnvelopeValidationError::MalformedBody)?;
                negotiate(VersionRange::V1, remote)
                    .map(|_| ())
                    .map_err(|_| EnvelopeValidationError::MalformedBody)
            }
            envelope_v1::Body::NegotiationAccepted(accepted) => {
                accepted
                    .selected
                    .ok_or(EnvelopeValidationError::MalformedBody)?;
                Ok(())
            }
            envelope_v1::Body::Heartbeat(heartbeat) => {
                if heartbeat.component.is_empty()
                    || heartbeat.component.len() > 128
                    || heartbeat.state.len() > 128
                {
                    Err(EnvelopeValidationError::MalformedBody)
                } else {
                    Ok(())
                }
            }
            envelope_v1::Body::Ack(ack) => {
                if ack.acknowledged_sequence == 0 {
                    Err(EnvelopeValidationError::MalformedBody)
                } else {
                    Ok(())
                }
            }
            envelope_v1::Body::Cancel(cancel) => {
                if cancel.new_generation <= self.cancellation_generation
                    || cancel.reason.len() > 1_024
                {
                    Err(EnvelopeValidationError::MalformedBody)
                } else {
                    Ok(())
                }
            }
            envelope_v1::Body::Stt(event) => event
                .validate()
                .map_err(|_| EnvelopeValidationError::MalformedBody),
            envelope_v1::Body::Llm(event) => event
                .validate()
                .map_err(|_| EnvelopeValidationError::MalformedBody),
            envelope_v1::Body::Tts(event) => event
                .validate()
                .map_err(|_| EnvelopeValidationError::MalformedBody),
            // Effect policy is adapter/user-specific and is applied separately.
            envelope_v1::Body::Effects(_) => Ok(()),
            envelope_v1::Body::Error(error) => error
                .validate()
                .map_err(|_| EnvelopeValidationError::MalformedBody),
            envelope_v1::Body::ActorSelected(selection) => {
                selection
                    .actor_id
                    .as_ref()
                    .ok_or(EnvelopeValidationError::MalformedBody)?
                    .validate()
                    .map_err(|_| EnvelopeValidationError::MalformedBody)?;
                if selection.display_name.trim().is_empty()
                    || selection.display_name.len() > 256
                    || selection.identity_confidence.is_some_and(|confidence| {
                        !confidence.is_finite() || !(0.0..=1.0).contains(&confidence)
                    })
                {
                    Err(EnvelopeValidationError::MalformedBody)
                } else {
                    Ok(())
                }
            }
            envelope_v1::Body::ControlRequest(request) => request.validate(),
            envelope_v1::Body::ControlResponse(response) => response.validate(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnvelopeValidationContext {
    pub accepted_versions: VersionRange,
    pub expected_launch_nonce: Option<LaunchNonce>,
    pub expected_session_id: Option<SessionId>,
    pub now_qpc_ticks: u64,
    pub max_body_bytes: usize,
    pub max_frame_bytes: usize,
}

impl EnvelopeValidationContext {
    #[must_use]
    pub fn permissive_for_time(now_qpc_ticks: u64) -> Self {
        Self {
            accepted_versions: VersionRange::V1,
            expected_launch_nonce: None,
            expected_session_id: None,
            now_qpc_ticks,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SequenceTracker {
    expected_launch_nonce: LaunchNonce,
    expected_session_id: SessionId,
    last_sequence: u64,
    highest_cancellation_generation: u64,
    ordering: OrderingPolicy,
}

impl SequenceTracker {
    #[must_use]
    pub fn new(
        expected_launch_nonce: LaunchNonce,
        expected_session_id: SessionId,
        ordering: OrderingPolicy,
    ) -> Self {
        Self {
            expected_launch_nonce,
            expected_session_id,
            last_sequence: 0,
            highest_cancellation_generation: 0,
            ordering,
        }
    }

    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    #[must_use]
    pub const fn highest_cancellation_generation(&self) -> u64 {
        self.highest_cancellation_generation
    }

    /// Checks and commits ordering state atomically. Rejected envelopes do not
    /// advance either watermark.
    pub fn observe(
        &mut self,
        envelope: &EnvelopeV1,
        mut context: EnvelopeValidationContext,
    ) -> Result<(), EnvelopeValidationError> {
        context.expected_launch_nonce = Some(self.expected_launch_nonce.clone());
        context.expected_session_id = Some(self.expected_session_id.clone());
        envelope.validate(&context)?;

        if envelope.sequence <= self.last_sequence {
            return Err(EnvelopeValidationError::ReplayOrReorder {
                last: self.last_sequence,
                received: envelope.sequence,
            });
        }
        if self.ordering == OrderingPolicy::StrictContiguous
            && envelope.sequence != self.last_sequence + 1
        {
            return Err(EnvelopeValidationError::SequenceGap {
                expected: self.last_sequence + 1,
                received: envelope.sequence,
            });
        }
        if envelope.cancellation_generation < self.highest_cancellation_generation {
            return Err(EnvelopeValidationError::StaleCancellationGeneration {
                highest: self.highest_cancellation_generation,
                received: envelope.cancellation_generation,
            });
        }

        self.last_sequence = envelope.sequence;
        self.highest_cancellation_generation = self
            .highest_cancellation_generation
            .max(envelope.cancellation_generation);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Error)]
pub enum EnvelopeValidationError {
    #[error("protocol version is missing")]
    MissingVersion,
    #[error("unsupported protocol version {0:?}")]
    UnsupportedVersion(ProtocolVersion),
    #[error("launch nonce is missing")]
    MissingLaunchNonce,
    #[error("launch nonce is malformed")]
    InvalidLaunchNonce,
    #[error("message belongs to an old process launch")]
    StaleLaunch,
    #[error("session identifier is missing")]
    MissingSession,
    #[error("session identifier is malformed")]
    InvalidSession,
    #[error("message belongs to a different session")]
    WrongSession,
    #[error("turn identifier is required for this body")]
    MissingTurn,
    #[error("turn identifier is malformed")]
    InvalidTurn,
    #[error("trace identifier is missing")]
    MissingTrace,
    #[error("trace identifier is malformed")]
    InvalidTrace,
    #[error("sequence zero is reserved")]
    ZeroSequence,
    #[error("QPC frequency must be non-zero")]
    ZeroQpcFrequency,
    #[error("deadline must be later than the sender timestamp")]
    InvalidDeadline,
    #[error("message deadline has expired")]
    DeadlineExceeded,
    #[error("envelope body is missing")]
    MissingBody,
    #[error("body is malformed")]
    MalformedBody,
    #[error("declared body size {declared} does not match actual size {actual}")]
    DeclaredSizeMismatch { declared: usize, actual: usize },
    #[error("body is too large: {0} bytes")]
    BodyTooLarge(usize),
    #[error("frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("frame length prefix is missing")]
    TruncatedLengthPrefix,
    #[error("frame length prefix {declared} does not match {actual} bytes received")]
    FrameLengthMismatch { declared: usize, actual: usize },
    #[error("protobuf decode failed: {0}")]
    Decode(String),
    #[error("message replayed or reordered: last {last}, received {received}")]
    ReplayOrReorder { last: u64, received: u64 },
    #[error("sequence gap: expected {expected}, received {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("stale cancellation generation: highest {highest}, received {received}")]
    StaleCancellationGeneration { highest: u64, received: u64 },
}

pub fn encode_frame(
    envelope: &EnvelopeV1,
    context: &EnvelopeValidationContext,
) -> Result<Vec<u8>, EnvelopeValidationError> {
    envelope.validate(context)?;
    let encoded_len = envelope.encoded_len();
    if encoded_len + 4 > context.max_frame_bytes {
        return Err(EnvelopeValidationError::FrameTooLarge(encoded_len + 4));
    }
    let length = u32::try_from(encoded_len)
        .map_err(|_| EnvelopeValidationError::FrameTooLarge(encoded_len + 4))?;
    let mut frame = Vec::with_capacity(encoded_len + 4);
    frame.extend_from_slice(&length.to_le_bytes());
    envelope
        .encode(&mut frame)
        .map_err(|error| EnvelopeValidationError::Decode(error.to_string()))?;
    Ok(frame)
}

pub fn decode_frame(
    frame: &[u8],
    context: &EnvelopeValidationContext,
) -> Result<EnvelopeV1, EnvelopeValidationError> {
    if frame.len() < 4 {
        return Err(EnvelopeValidationError::TruncatedLengthPrefix);
    }
    if frame.len() > context.max_frame_bytes {
        return Err(EnvelopeValidationError::FrameTooLarge(frame.len()));
    }
    let mut length_bytes = [0_u8; 4];
    length_bytes.copy_from_slice(&frame[..4]);
    let declared = u32::from_le_bytes(length_bytes) as usize;
    let actual = frame.len() - 4;
    if declared != actual {
        return Err(EnvelopeValidationError::FrameLengthMismatch { declared, actual });
    }
    let envelope = EnvelopeV1::decode(&frame[4..])
        .map_err(|error| EnvelopeValidationError::Decode(error.to_string()))?;
    envelope.validate(context)?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        envelope_v1::Body, llm_event_v1, EmptyV1, ErrorCode, ProviderEventMetadataV1, RequestId,
    };
    use proptest::prelude::*;

    fn envelope(sequence: u64, generation: u64) -> EnvelopeV1 {
        EnvelopeV1::new(
            LaunchNonce::from_bytes([1; 16]).unwrap(),
            SessionId::from_bytes([2; 16]).unwrap(),
            Some(TurnId::from_bytes([3; 16]).unwrap()),
            TraceId::from_bytes([4; 16]).unwrap(),
            sequence,
            100,
            10_000_000,
            1_000,
            generation,
            Body::Llm(LlmEventV1 {
                metadata: Some(ProviderEventMetadataV1 {
                    provider_id: "fixture".into(),
                    model_id: "fixture".into(),
                    request_id: Some(RequestId::from_bytes([5; 16]).unwrap()),
                    provider_sequence: 1,
                    provider_elapsed_ms: 0,
                }),
                kind: Some(llm_event_v1::Kind::Started(EmptyV1 {})),
            }),
        )
        .unwrap()
    }

    fn context() -> EnvelopeValidationContext {
        EnvelopeValidationContext::permissive_for_time(100)
    }

    #[test]
    fn frame_round_trip_is_exact_and_validated() {
        let envelope = envelope(1, 0);
        let frame = encode_frame(&envelope, &context()).unwrap();
        assert_eq!(decode_frame(&frame, &context()).unwrap(), envelope);
    }

    #[test]
    fn declared_body_size_detects_tampering() {
        let mut envelope = envelope(1, 0);
        envelope.declared_body_bytes += 1;
        assert!(matches!(
            envelope.validate(&context()),
            Err(EnvelopeValidationError::DeclaredSizeMismatch { .. })
        ));
    }

    #[test]
    fn sequence_tracker_commits_only_accepted_messages() {
        let launch = LaunchNonce::from_bytes([1; 16]).unwrap();
        let session = SessionId::from_bytes([2; 16]).unwrap();
        let mut tracker = SequenceTracker::new(launch, session, OrderingPolicy::StrictContiguous);
        assert_eq!(tracker.observe(&envelope(1, 0), context()), Ok(()));
        assert!(matches!(
            tracker.observe(&envelope(3, 0), context()),
            Err(EnvelopeValidationError::SequenceGap { .. })
        ));
        assert_eq!(tracker.last_sequence(), 1);
        assert_eq!(tracker.observe(&envelope(2, 1), context()), Ok(()));
        assert_eq!(tracker.highest_cancellation_generation(), 1);
    }

    #[test]
    fn stale_work_cannot_arrive_after_barge_in() {
        let launch = LaunchNonce::from_bytes([1; 16]).unwrap();
        let session = SessionId::from_bytes([2; 16]).unwrap();
        let mut tracker = SequenceTracker::new(launch, session, OrderingPolicy::Monotonic);
        tracker.observe(&envelope(1, 3), context()).unwrap();
        assert!(matches!(
            tracker.observe(&envelope(2, 2), context()),
            Err(EnvelopeValidationError::StaleCancellationGeneration { .. })
        ));
        assert_eq!(tracker.last_sequence(), 1);
    }

    #[test]
    fn control_request_round_trips_inside_the_canonical_envelope() {
        let request_id = RequestId::from_bytes([7; 16]).unwrap();
        let mut value = envelope(1, 0);
        value.turn_id = Some(TurnId::from_bytes([3; 16]).unwrap());
        value.body = Some(Body::ControlRequest(ControlRequestV1 {
            request_id: Some(request_id.clone()),
            operation: ControlOperationV1::SimulateTurn as i32,
            payload_json: br#"{"type":"simulate_turn"}"#.to_vec(),
        }));
        value.synchronize_declared_size().unwrap();
        let frame = encode_frame(&value, &context()).unwrap();
        let decoded = decode_frame(&frame, &context()).unwrap();
        let request = match decoded.body.unwrap() {
            Body::ControlRequest(request) => request,
            _ => panic!("expected control request"),
        };
        assert_eq!(request.request_id, Some(request_id));
        assert_eq!(request.operation(), ControlOperationV1::SimulateTurn);
    }

    #[test]
    fn control_error_is_typed_and_turn_scope_is_enforced() {
        let mut value = envelope(1, 4);
        value.turn_id = None;
        value.body = Some(Body::ControlResponse(ControlResponseV1 {
            request_id: Some(RequestId::from_bytes([7; 16]).unwrap()),
            request_sequence: 9,
            operation: ControlOperationV1::SimulateTurn as i32,
            body: Some(control_response_v1::Body::Error(
                IpcErrorV1::new(ErrorCode::Cancelled, "cancelled").at_stage("control"),
            )),
        }));
        value.synchronize_declared_size().unwrap();
        assert!(matches!(
            value.validate(&context()),
            Err(EnvelopeValidationError::MissingTurn)
        ));
        value.turn_id = Some(TurnId::from_bytes([3; 16]).unwrap());
        assert_eq!(value.validate(&context()), Ok(()));

        if let Some(Body::ControlResponse(response)) = value.body.as_mut() {
            response.operation = ControlOperationV1::TranscribeSelectedStt as i32;
        }
        value.turn_id = None;
        value.synchronize_declared_size().unwrap();
        assert!(matches!(
            value.validate(&context()),
            Err(EnvelopeValidationError::MissingTurn)
        ));
    }

    proptest! {
        #[test]
        fn any_length_prefix_mismatch_is_rejected(extra in 1usize..128) {
            let mut frame = encode_frame(&envelope(1, 0), &context()).unwrap();
            frame.extend(std::iter::repeat(0).take(extra));
            let rejected = matches!(
                decode_frame(&frame, &context()),
                Err(EnvelopeValidationError::FrameLengthMismatch { .. })
            );
            prop_assert!(rejected);
        }

        #[test]
        fn sequences_are_strictly_monotonic(a in 1u64..u64::MAX / 2, gap in 1u64..1024) {
            let launch = LaunchNonce::from_bytes([1; 16]).unwrap();
            let session = SessionId::from_bytes([2; 16]).unwrap();
            let mut tracker = SequenceTracker::new(launch, session, OrderingPolicy::Monotonic);
            // First monotonic message may begin at any non-zero offset.
            prop_assert_eq!(tracker.observe(&envelope(a, 0), context()), Ok(()));
            prop_assert_eq!(tracker.observe(&envelope(a + gap, 0), context()), Ok(()));
            let rejected = matches!(
                tracker.observe(&envelope(a, 0), context()),
                Err(EnvelopeValidationError::ReplayOrReorder { .. })
            );
            prop_assert!(rejected);
        }
    }
}
