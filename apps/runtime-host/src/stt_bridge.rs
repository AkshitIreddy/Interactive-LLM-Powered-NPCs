//! Selected hosted STT route driven exclusively by a trusted native PCM source.
//!
//! `PcmInputSource` is intentionally not serializable and is never part of control/WebView IPC.
//! The native media broker owns the authenticated pipe lease and implements this interface. Every
//! chunk is bound to session/turn/generation and the selected input endpoint generation before it
//! can leave the device.

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use interactive_npcs_credential_vault::CredentialVault;
use npc_providers_stt::{
    AssemblyAi, AudioFormat, EndpointingMode, FlushReason, HostedRecognizer,
    HostedWebSocketTransportFactory, RecognitionConfig, RecognitionEvent, RetentionPolicy,
    SecretString, SessionTimeouts, TranscriptStatus, TransportFactory,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{RouteExecution, SelectedProviderRoute};

const CREDENTIAL_TARGET: &str = "providers/assemblyai";
const MODEL_ID: &str = "u3-rt-pro";
const REQUIRED_EGRESS: &str = "microphone_audio_and_optional_non_secret_context";
const MAX_PCM_CHUNK_BYTES: usize = 64 * 1_024;
const MAX_TRANSCRIPT_BYTES: usize = 64 * 1_024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePcmIdentity {
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
}

impl NativePcmIdentity {
    fn validate(&self) -> Result<(), SelectedSttError> {
        if self.session_id.is_empty()
            || self.session_id.len() > 128
            || self.turn_id.is_empty()
            || self.turn_id.len() > 128
            || self.generation == 0
            || self.input_endpoint_id.is_empty()
            || self.input_endpoint_id.len() > 1_024
            || self.input_endpoint_generation == 0
            || self.session_id.chars().any(char::is_control)
            || self.turn_id.chars().any(char::is_control)
            || self.input_endpoint_id.chars().any(char::is_control)
        {
            return Err(SelectedSttError::InvalidNativeInput);
        }
        Ok(())
    }
}

pub struct NativePcmChunk {
    pub identity: NativePcmIdentity,
    pub sequence: u64,
    pub first_frame_qpc: u64,
    pub first_frame_index: u64,
    pub frame_count: u32,
    pub pcm_s16le: Vec<u8>,
}

/// Broker-attested push-to-talk activation bound to this one-use input stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePcmActivation {
    pub ptt_virtual_key: u32,
    pub ptt_press_transition_sequence: u64,
    pub ptt_pressed_qpc: u64,
}

impl NativePcmActivation {
    fn validate(&self) -> Result<(), SelectedSttError> {
        if self.ptt_virtual_key == 0
            || self.ptt_virtual_key > 0xff
            || self.ptt_press_transition_sequence == 0
            || self.ptt_pressed_qpc == 0
        {
            return Err(SelectedSttError::InvalidNativeInput);
        }
        Ok(())
    }
}

impl fmt::Debug for NativePcmChunk {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativePcmChunk")
            .field("identity", &self.identity)
            .field("sequence", &self.sequence)
            .field("first_frame_qpc", &self.first_frame_qpc)
            .field("first_frame_index", &self.first_frame_index)
            .field("frame_count", &self.frame_count)
            .field(
                "pcm_s16le",
                &format_args!("[REDACTED; {} bytes]", self.pcm_s16le.len()),
            )
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePcmAck {
    Continue,
    Stop,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePcmReceipt {
    pub identity: NativePcmIdentity,
    pub activation: NativePcmActivation,
    pub captured_frames: u64,
    pub source_capture_complete: bool,
    pub cancelled: bool,
    pub device_lost: bool,
    pub ptt_release_transition_sequence: u64,
    pub ptt_released_qpc: u64,
}

/// Native-only input boundary implemented by the authenticated media-broker consumer.
#[async_trait]
pub trait PcmInputSource: Send {
    fn identity(&self) -> &NativePcmIdentity;
    fn activation(&self) -> &NativePcmActivation;
    fn format(&self) -> AudioFormat;
    async fn next_chunk(&mut self) -> Result<Option<NativePcmChunk>, PcmInputSourceError>;
    async fn acknowledge(
        &mut self,
        sequence: u64,
        action: NativePcmAck,
    ) -> Result<(), PcmInputSourceError>;
    async fn finish(&mut self) -> Result<NativePcmReceipt, PcmInputSourceError>;
    async fn cancel(&mut self) -> Result<(), PcmInputSourceError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("native PCM input source failed")]
pub struct PcmInputSourceError;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SttAttemptAuthority {
    Initial,
    ManualRetry {
        prior_generation: u64,
        user_authorized: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedSttTurnRequest {
    pub identity: NativePcmIdentity,
    pub attempt: SttAttemptAuthority,
    pub context_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttRouteReceipt {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSttResult {
    pub transcript: String,
    pub route: SelectedSttRouteReceipt,
    pub chunks_sent: u64,
    pub pcm_bytes_sent: u64,
    pub partial_events: u64,
}

impl fmt::Debug for SelectedSttResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectedSttResult")
            .field(
                "transcript",
                &format_args!("[REDACTED; {} bytes]", self.transcript.len()),
            )
            .field("route", &self.route)
            .field("chunks_sent", &self.chunks_sent)
            .field("pcm_bytes_sent", &self.pcm_bytes_sent)
            .field("partial_events", &self.partial_events)
            .finish()
    }
}

pub struct SelectedHostedStt {
    route: SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
    recognizer: HostedRecognizer<AssemblyAi>,
}

impl fmt::Debug for SelectedHostedStt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectedHostedStt")
            .field("route", &self.route)
            .field("vault", &"[CREDENTIAL_VAULT]")
            .finish()
    }
}

pub fn selected_hosted_stt(
    route: &SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
) -> Result<SelectedHostedStt, SelectedSttError> {
    selected_hosted_stt_with_factory(
        route,
        vault,
        Arc::new(HostedWebSocketTransportFactory::default()),
    )
}

fn selected_hosted_stt_with_factory(
    route: &SelectedProviderRoute,
    vault: Arc<dyn CredentialVault>,
    factory: Arc<dyn TransportFactory>,
) -> Result<SelectedHostedStt, SelectedSttError> {
    if route.execution != RouteExecution::Cloud
        || route.provider_id != "assemblyai"
        || route.model_id != MODEL_ID
        || route.egress != REQUIRED_EGRESS
    {
        return Err(SelectedSttError::UnsupportedRoute);
    }
    if route.credential_reference.as_deref() != Some(CREDENTIAL_TARGET) {
        return Err(SelectedSttError::CredentialReferenceMismatch);
    }
    Ok(SelectedHostedStt {
        route: route.clone(),
        vault,
        recognizer: HostedRecognizer::new(AssemblyAi, factory),
    })
}

impl SelectedHostedStt {
    pub async fn transcribe_push_to_talk(
        &self,
        request: SelectedSttTurnRequest,
        mut input: Box<dyn PcmInputSource>,
        cancellation: CancellationToken,
    ) -> Result<SelectedSttResult, SelectedSttError> {
        validate_turn_request(&request, input.as_ref())?;
        if cancellation.is_cancelled() {
            let _ = input.cancel().await;
            return Err(SelectedSttError::Cancelled);
        }
        let value = self
            .vault
            .get(CREDENTIAL_TARGET)
            .map_err(|_| SelectedSttError::CredentialUnavailable)?;
        let credential = String::from_utf8(value.expose().to_vec())
            .map_err(|_| SelectedSttError::CredentialUnavailable)?;
        let mut session = self
            .recognizer
            .start_session(
                RecognitionConfig {
                    model: MODEL_ID.into(),
                    audio: AudioFormat::PCM_16KHZ_MONO,
                    languages: Vec::new(),
                    endpointing: EndpointingMode::Manual,
                    interim_results: true,
                    keyword_hints: Vec::new(),
                    context_hint: request.context_hint.clone(),
                    retention: RetentionPolicy::ProviderDefaultAllowed,
                    timeouts: SessionTimeouts {
                        connect: Duration::from_secs(15),
                        send: Duration::from_secs(5),
                        receive_idle: Duration::from_secs(30),
                        flush: Duration::from_secs(10),
                    },
                },
                SecretString::new(credential),
                cancellation.clone(),
            )
            .await
            .map_err(map_stt_error)?;

        let mut expected_sequence = 1_u64;
        let mut chunks_sent = 0_u64;
        let mut pcm_bytes_sent = 0_u64;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    let _ = input.cancel().await;
                    let _ = session.cancel().await;
                    return Err(SelectedSttError::Cancelled);
                }
                next = input.next_chunk() => next.map_err(|_| SelectedSttError::NativeInputUnavailable)?,
            };
            let Some(chunk) = next else {
                break;
            };
            validate_chunk(&chunk, &request.identity, expected_sequence)?;
            session
                .send_audio(&chunk.pcm_s16le)
                .await
                .map_err(map_stt_error)?;
            input
                .acknowledge(chunk.sequence, NativePcmAck::Continue)
                .await
                .map_err(|_| SelectedSttError::NativeInputUnavailable)?;
            expected_sequence = expected_sequence.saturating_add(1);
            chunks_sent = chunks_sent.saturating_add(1);
            pcm_bytes_sent = pcm_bytes_sent.saturating_add(chunk.pcm_s16le.len() as u64);
        }
        if chunks_sent == 0 {
            let _ = input.cancel().await;
            let _ = session.cancel().await;
            return Err(SelectedSttError::InvalidNativeInput);
        }
        let receipt = input
            .finish()
            .await
            .map_err(|_| SelectedSttError::NativeInputUnavailable)?;
        if receipt.identity != request.identity
            || receipt.activation != *input.activation()
            || receipt.cancelled
            || receipt.device_lost
            || !receipt.source_capture_complete
            || receipt.captured_frames == 0
            || receipt.ptt_release_transition_sequence
                <= receipt.activation.ptt_press_transition_sequence
            || receipt.ptt_released_qpc < receipt.activation.ptt_pressed_qpc
        {
            let _ = session.cancel().await;
            return Err(SelectedSttError::InvalidNativeInput);
        }
        session
            .flush(FlushReason::PushToTalkReleased)
            .await
            .map_err(map_stt_error)?;

        let mut transcript = String::new();
        let mut partial_events = 0_u64;
        loop {
            let event = session.next_event().await.map_err(map_stt_error)?;
            let Some(event) = event else { break };
            match event {
                RecognitionEvent::Transcript(value) => match value.status {
                    TranscriptStatus::Final => {
                        if value.text.is_empty() || value.text.len() > MAX_TRANSCRIPT_BYTES {
                            return Err(SelectedSttError::ProviderProtocol);
                        }
                        transcript = value.text;
                    }
                    TranscriptStatus::Partial | TranscriptStatus::EagerFinal => {
                        partial_events = partial_events.saturating_add(1);
                    }
                },
                RecognitionEvent::TurnEnded { .. } if !transcript.is_empty() => break,
                RecognitionEvent::Cancelled => {
                    let _ = input.cancel().await;
                    return Err(SelectedSttError::Cancelled);
                }
                RecognitionEvent::SessionStarted { .. }
                | RecognitionEvent::TurnStarted { .. }
                | RecognitionEvent::TurnResumed { .. }
                | RecognitionEvent::TurnEnded { .. }
                | RecognitionEvent::Warning { .. } => {}
            }
        }
        session.close().await.map_err(map_stt_error)?;
        if transcript.is_empty() {
            return Err(SelectedSttError::ProviderProtocol);
        }
        Ok(SelectedSttResult {
            transcript,
            route: SelectedSttRouteReceipt {
                provider_id: self.route.provider_id.clone(),
                model_id: self.route.model_id.clone(),
                credential_reference: CREDENTIAL_TARGET.into(),
                egress: REQUIRED_EGRESS.into(),
                generation: request.identity.generation,
                input_endpoint_id: request.identity.input_endpoint_id,
                input_endpoint_generation: request.identity.input_endpoint_generation,
                manual_retry: matches!(request.attempt, SttAttemptAuthority::ManualRetry { .. }),
                automatic_fallback: false,
                captured_frames: receipt.captured_frames,
                ptt_virtual_key: receipt.activation.ptt_virtual_key,
                ptt_press_transition_sequence: receipt.activation.ptt_press_transition_sequence,
                ptt_pressed_qpc: receipt.activation.ptt_pressed_qpc,
                ptt_release_transition_sequence: receipt.ptt_release_transition_sequence,
                ptt_released_qpc: receipt.ptt_released_qpc,
            },
            chunks_sent,
            pcm_bytes_sent,
            partial_events,
        })
    }
}

fn validate_turn_request(
    request: &SelectedSttTurnRequest,
    input: &dyn PcmInputSource,
) -> Result<(), SelectedSttError> {
    request.identity.validate()?;
    input.activation().validate()?;
    if input.identity() != &request.identity || input.format() != AudioFormat::PCM_16KHZ_MONO {
        return Err(SelectedSttError::InvalidNativeInput);
    }
    if request
        .context_hint
        .as_ref()
        .is_some_and(|value| value.len() > 4_096 || value.chars().any(|c| c == '\0'))
    {
        return Err(SelectedSttError::InvalidRequest);
    }
    if let SttAttemptAuthority::ManualRetry {
        prior_generation,
        user_authorized,
    } = request.attempt
    {
        if !user_authorized
            || prior_generation == 0
            || request.identity.generation <= prior_generation
        {
            return Err(SelectedSttError::ManualRetryNotAuthorized);
        }
    }
    Ok(())
}

fn validate_chunk(
    chunk: &NativePcmChunk,
    identity: &NativePcmIdentity,
    sequence: u64,
) -> Result<(), SelectedSttError> {
    if &chunk.identity != identity
        || chunk.sequence != sequence
        || chunk.first_frame_qpc == 0
        || chunk.pcm_s16le.is_empty()
        || chunk.pcm_s16le.len() > MAX_PCM_CHUNK_BYTES
        || !chunk.pcm_s16le.len().is_multiple_of(2)
        || usize::try_from(chunk.frame_count)
            .ok()
            .and_then(|frames| frames.checked_mul(2))
            != Some(chunk.pcm_s16le.len())
    {
        return Err(SelectedSttError::InvalidNativeInput);
    }
    Ok(())
}

fn map_stt_error(error: npc_providers_stt::SttError) -> SelectedSttError {
    use npc_providers_stt::SttErrorKind;
    match error.kind {
        SttErrorKind::Cancelled => SelectedSttError::Cancelled,
        SttErrorKind::Authentication => SelectedSttError::CredentialUnavailable,
        SttErrorKind::InvalidRequest | SttErrorKind::PrivacyPolicy => {
            SelectedSttError::InvalidRequest
        }
        SttErrorKind::Protocol => SelectedSttError::ProviderProtocol,
        SttErrorKind::Timeout
        | SttErrorKind::RateLimited
        | SttErrorKind::QuotaExceeded
        | SttErrorKind::Unavailable => SelectedSttError::ProviderUnavailable,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectedSttError {
    #[error("selected STT route is unsupported")]
    UnsupportedRoute,
    #[error("selected STT credential reference does not match the fixed provider target")]
    CredentialReferenceMismatch,
    #[error("selected STT credential is unavailable")]
    CredentialUnavailable,
    #[error("native PCM input is invalid")]
    InvalidNativeInput,
    #[error("native PCM input source is unavailable")]
    NativeInputUnavailable,
    #[error("manual STT retry was not explicitly authorized with a new generation")]
    ManualRetryNotAuthorized,
    #[error("selected STT request is invalid")]
    InvalidRequest,
    #[error("selected STT provider is unavailable")]
    ProviderUnavailable,
    #[error("selected STT provider returned an invalid protocol result")]
    ProviderProtocol,
    #[error("selected STT turn was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::{collections::VecDeque, sync::Mutex};

    use interactive_npcs_credential_vault::{MemoryCredentialVault, SecretValue};
    use npc_providers_stt::{
        ClientFrame, ConnectRequest, ServerFrame, StreamingTransport, TransportError,
    };

    use super::*;

    struct MockTransport {
        frames: VecDeque<ServerFrame>,
    }

    #[async_trait]
    impl StreamingTransport for MockTransport {
        async fn send(&mut self, _frame: ClientFrame) -> Result<(), TransportError> {
            Ok(())
        }
        async fn receive(&mut self) -> Result<Option<ServerFrame>, TransportError> {
            Ok(self.frames.pop_front())
        }
        async fn close(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    struct MockFactory(Mutex<Option<VecDeque<ServerFrame>>>);

    #[async_trait]
    impl TransportFactory for MockFactory {
        async fn connect(
            &self,
            _request: ConnectRequest<'_>,
        ) -> Result<Box<dyn StreamingTransport>, TransportError> {
            let frames = self.0.lock().expect("lock").take().ok_or(TransportError)?;
            Ok(Box::new(MockTransport { frames }))
        }
    }

    struct FixtureInput {
        identity: NativePcmIdentity,
        activation: NativePcmActivation,
        chunks: VecDeque<NativePcmChunk>,
        acknowledgements: Arc<Mutex<Vec<NativePcmAck>>>,
    }

    #[async_trait]
    impl PcmInputSource for FixtureInput {
        fn identity(&self) -> &NativePcmIdentity {
            &self.identity
        }
        fn activation(&self) -> &NativePcmActivation {
            &self.activation
        }
        fn format(&self) -> AudioFormat {
            AudioFormat::PCM_16KHZ_MONO
        }
        async fn next_chunk(&mut self) -> Result<Option<NativePcmChunk>, PcmInputSourceError> {
            Ok(self.chunks.pop_front())
        }
        async fn acknowledge(
            &mut self,
            _sequence: u64,
            action: NativePcmAck,
        ) -> Result<(), PcmInputSourceError> {
            self.acknowledgements.lock().expect("ack lock").push(action);
            Ok(())
        }
        async fn finish(&mut self) -> Result<NativePcmReceipt, PcmInputSourceError> {
            Ok(NativePcmReceipt {
                identity: self.identity.clone(),
                activation: self.activation.clone(),
                captured_frames: 2,
                source_capture_complete: true,
                cancelled: false,
                device_lost: false,
                ptt_release_transition_sequence: 9,
                ptt_released_qpc: 20,
            })
        }
        async fn cancel(&mut self) -> Result<(), PcmInputSourceError> {
            Ok(())
        }
    }

    fn identity(generation: u64) -> NativePcmIdentity {
        NativePcmIdentity {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            generation,
            input_endpoint_id: "endpoint-fixture".into(),
            input_endpoint_generation: 3,
        }
    }

    fn route() -> SelectedProviderRoute {
        SelectedProviderRoute {
            provider_id: "assemblyai".into(),
            model_id: MODEL_ID.into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: REQUIRED_EGRESS.into(),
            credential_reference: Some(CREDENTIAL_TARGET.into()),
        }
    }

    #[tokio::test]
    async fn native_pcm_reaches_selected_route_and_final_is_debug_redacted() {
        let frames = VecDeque::from([
            ServerFrame::Text(r#"{"type":"Begin","id":"fixture-session"}"#.into()),
            ServerFrame::Text(r#"{"type":"Turn","turn_order":1,"end_of_turn":false,"transcript":"North"}"#.into()),
            ServerFrame::Text(r#"{"type":"Turn","turn_order":1,"end_of_turn":true,"transcript":"North beacon ready."}"#.into()),
        ]);
        let vault = MemoryCredentialVault::default();
        vault
            .put(
                CREDENTIAL_TARGET,
                None,
                &SecretValue::new(b"fixture-key".to_vec()).unwrap(),
            )
            .unwrap();
        let selected = selected_hosted_stt_with_factory(
            &route(),
            Arc::new(vault),
            Arc::new(MockFactory(Mutex::new(Some(frames)))),
        )
        .unwrap();
        let id = identity(4);
        let acknowledgements = Arc::new(Mutex::new(Vec::new()));
        let input = FixtureInput {
            identity: id.clone(),
            activation: NativePcmActivation {
                ptt_virtual_key: 0x58,
                ptt_press_transition_sequence: 8,
                ptt_pressed_qpc: 10,
            },
            chunks: VecDeque::from([NativePcmChunk {
                identity: id.clone(),
                sequence: 1,
                first_frame_qpc: 10,
                first_frame_index: 0,
                frame_count: 2,
                pcm_s16le: vec![1, 0, 2, 0],
            }]),
            acknowledgements: acknowledgements.clone(),
        };
        let result = selected
            .transcribe_push_to_talk(
                SelectedSttTurnRequest {
                    identity: id,
                    attempt: SttAttemptAuthority::Initial,
                    context_hint: None,
                },
                Box::new(input),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.transcript, "North beacon ready.");
        assert!(!format!("{result:?}").contains("North beacon ready"));
        assert!(!result.route.automatic_fallback);
        assert_eq!(result.route.captured_frames, 2);
        assert_eq!(result.route.ptt_virtual_key, 0x58);
        assert_eq!(result.route.ptt_press_transition_sequence, 8);
        assert_eq!(result.route.ptt_pressed_qpc, 10);
        assert_eq!(result.route.ptt_release_transition_sequence, 9);
        assert_eq!(result.route.ptt_released_qpc, 20);
        assert_eq!(
            *acknowledgements.lock().unwrap(),
            vec![NativePcmAck::Continue]
        );
    }

    #[test]
    fn route_and_manual_retry_authority_fail_closed() {
        let vault = Arc::new(MemoryCredentialVault::default());
        let mut redirected = route();
        redirected.credential_reference = Some("providers/cohere".into());
        assert!(matches!(
            selected_hosted_stt(&redirected, vault),
            Err(SelectedSttError::CredentialReferenceMismatch)
        ));
        let id = identity(4);
        let input = FixtureInput {
            identity: id.clone(),
            activation: NativePcmActivation {
                ptt_virtual_key: 0x58,
                ptt_press_transition_sequence: 8,
                ptt_pressed_qpc: 10,
            },
            chunks: VecDeque::new(),
            acknowledgements: Arc::new(Mutex::new(Vec::new())),
        };
        let request = SelectedSttTurnRequest {
            identity: id,
            attempt: SttAttemptAuthority::ManualRetry {
                prior_generation: 4,
                user_authorized: true,
            },
            context_hint: None,
        };
        assert_eq!(
            validate_turn_request(&request, &input),
            Err(SelectedSttError::ManualRetryNotAuthorized)
        );
    }
}
