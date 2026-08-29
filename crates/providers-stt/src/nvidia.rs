//! Experimental NVIDIA NIM / Riva streaming ASR adapter.
//!
//! The curated route follows NVIDIA's hosted NIM build page. Profiles cannot supply an endpoint,
//! function ID, or model name. Riva protobuf serialization and tonic/grpcio integration live behind
//! [`NvidiaGrpcTransportFactory`], so this crate does not add a Python or gRPC runtime dependency.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::time::{timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::{
    AudioEncoding, AudioFormat, FlushReason, ProviderLifecycle, RecognitionConfig,
    RecognitionEvent, RecognizerCapabilities, RetentionControl, RetentionPolicy, SecretString,
    SttError, SttErrorKind, TranscriptEvent, TranscriptStatus, TurnEndReason,
};

pub const NVIDIA_NIM_ENDPOINT: &str = "grpc.nvcf.nvidia.com:443";
pub const NVIDIA_NIM_FUNCTION_ID: &str = "bb0837de-8c7b-481f-9ec8-ef5663e9c1fa";
pub const NVIDIA_NIM_MODEL: &str = "nemotron-asr-streaming";

const PROVIDER_ID: &str = "nvidia-nim-asr";
const AUDIO: &[AudioFormat] = &[AudioFormat::PCM_16KHZ_MONO];
const LANGUAGES: &[&str] = &["en-US"];
static CAPABILITIES: RecognizerCapabilities = RecognizerCapabilities {
    provider_id: PROVIDER_ID,
    display_name: "NVIDIA NIM Nemotron Streaming ASR",
    default_model: NVIDIA_NIM_MODEL,
    capability_revision: "2026-08-28-experimental",
    languages: LANGUAGES,
    accepted_audio: AUDIO,
    partial_revisions: true,
    provider_turn_detection: true,
    manual_flush: true,
    dynamic_keyword_hints: false,
    context_hints: true,
    resume_events: false,
    retention_control: RetentionControl::ProviderPolicyOnly,
    sends_audio_to_provider: true,
    sends_context_to_provider: true,
    privacy_policy_url: "https://www.nvidia.com/en-us/about-nvidia/privacy-policy/",
    lifecycle: ProviderLifecycle::Experimental,
    availability_note: Some(
        "Free hosted development API; rate limited. Experimental until live-audio qualification.",
    ),
};

#[derive(Clone)]
enum NvidiaRoute {
    Curated,
    TrustedCatalog(TrustedNvidiaRouteOverride),
}

impl NvidiaRoute {
    fn endpoint(&self) -> &str {
        match self {
            Self::Curated => NVIDIA_NIM_ENDPOINT,
            Self::TrustedCatalog(route) => &route.endpoint,
        }
    }

    fn function_id(&self) -> &str {
        match self {
            Self::Curated => NVIDIA_NIM_FUNCTION_ID,
            Self::TrustedCatalog(route) => &route.function_id,
        }
    }

    fn validate(&self) -> Result<(), SttError> {
        match self {
            Self::Curated => {
                if self.endpoint() != NVIDIA_NIM_ENDPOINT
                    || self.function_id() != NVIDIA_NIM_FUNCTION_ID
                {
                    return Err(SttError::invalid_request(
                        "untrusted NVIDIA NIM route override was rejected",
                    ));
                }
            }
            Self::TrustedCatalog(route) => route.validate()?,
        }
        Ok(())
    }
}

/// Opaque route proven by the signed-catalog boundary.
///
/// It has no public constructor or deserializer, so profiles and ordinary application input cannot
/// manufacture an override. The signed catalog loader in this crate is the intended producer.
#[derive(Clone)]
pub struct TrustedNvidiaRouteOverride {
    endpoint: String,
    function_id: String,
    catalog_revision: u64,
    signing_key_id: String,
}

impl fmt::Debug for TrustedNvidiaRouteOverride {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedNvidiaRouteOverride")
            .field("endpoint", &self.endpoint)
            .field("function_id", &self.function_id)
            .field("catalog_revision", &self.catalog_revision)
            .field("signing_key_id", &self.signing_key_id)
            .finish()
    }
}

impl TrustedNvidiaRouteOverride {
    /// Called only after the catalog signature and trusted key ID were verified.
    #[allow(
        dead_code,
        reason = "reserved for the signed-catalog loader; profiles cannot construct this type"
    )]
    pub(crate) fn from_verified_catalog(
        endpoint: String,
        function_id: String,
        catalog_revision: u64,
        signing_key_id: String,
    ) -> Result<Self, SttError> {
        let route = Self {
            endpoint,
            function_id,
            catalog_revision,
            signing_key_id,
        };
        route.validate()?;
        Ok(route)
    }

    fn validate(&self) -> Result<(), SttError> {
        if self.catalog_revision == 0
            || self.signing_key_id.trim().is_empty()
            || self.function_id.trim().is_empty()
            || !self.endpoint.ends_with(":443")
            || self.endpoint.contains('/')
            || self.endpoint.chars().any(char::is_whitespace)
        {
            return Err(SttError::invalid_request(
                "trusted NVIDIA catalog route is invalid",
            ));
        }
        Ok(())
    }
}

/// Borrowed authentication metadata for the initial Riva streaming RPC.
pub struct NvidiaGrpcConnectRequest<'request> {
    pub endpoint: &'request str,
    pub use_tls: bool,
    pub function_id: &'request str,
    pub authorization_scheme: &'static str,
    pub credential: &'request SecretString,
    pub deadline: Instant,
}

impl fmt::Debug for NvidiaGrpcConnectRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NvidiaGrpcConnectRequest")
            .field("endpoint", &self.endpoint)
            .field("use_tls", &self.use_tls)
            .field("function_id", &self.function_id)
            .field("authorization_scheme", &self.authorization_scheme)
            .field("credential", &self.credential)
            .field("deadline", &self.deadline)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RivaAudioEncoding {
    LinearPcm,
}

/// Provider-neutral representation of Riva's first `StreamingRecognizeRequest` configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct RivaStreamingConfig {
    pub encoding: RivaAudioEncoding,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub language_code: String,
    pub model: String,
    pub interim_results: bool,
    pub enable_automatic_punctuation: bool,
    pub max_alternatives: u32,
    pub speech_contexts: Vec<String>,
}

impl fmt::Debug for RivaStreamingConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RivaStreamingConfig")
            .field("encoding", &self.encoding)
            .field("sample_rate_hz", &self.sample_rate_hz)
            .field("channels", &self.channels)
            .field("language_code", &self.language_code)
            .field("model", &self.model)
            .field("interim_results", &self.interim_results)
            .field(
                "enable_automatic_punctuation",
                &self.enable_automatic_punctuation,
            )
            .field("max_alternatives", &self.max_alternatives)
            .field("speech_context_count", &self.speech_contexts.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum RivaStreamingRequest {
    Config(RivaStreamingConfig),
    Audio(Vec<u8>),
}

impl fmt::Debug for RivaStreamingRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(config) => formatter.debug_tuple("Config").field(config).finish(),
            Self::Audio(audio) => formatter
                .debug_tuple("Audio")
                .field(&format_args!("[REDACTED; {} bytes]", audio.len()))
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct RivaStreamingResult {
    pub transcript: String,
    pub is_final: bool,
    pub stability: Option<f32>,
    pub confidence: Option<f32>,
    pub audio_processed_seconds: Option<f32>,
}

impl fmt::Debug for RivaStreamingResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RivaStreamingResult")
            .field(
                "transcript",
                &format_args!("[REDACTED; {} bytes]", self.transcript.len()),
            )
            .field("is_final", &self.is_final)
            .field("stability", &self.stability)
            .field("confidence", &self.confidence)
            .field("audio_processed_seconds", &self.audio_processed_seconds)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RivaStreamingResponse {
    pub results: Vec<RivaStreamingResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcStatusCode {
    Unauthenticated,
    PermissionDenied,
    ResourceExhausted,
    InvalidArgument,
    DeadlineExceeded,
    Unavailable,
    Internal,
    Unknown,
}

/// Content-free gRPC failure. Provider descriptions and trailers are deliberately not retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("NVIDIA gRPC transport failed with {code:?}")]
pub struct NvidiaGrpcError {
    pub code: GrpcStatusCode,
    pub http_status: Option<u16>,
    pub retry_after: Option<Duration>,
}

#[async_trait]
pub trait NvidiaGrpcStream: Send {
    async fn send(&mut self, request: RivaStreamingRequest) -> Result<(), NvidiaGrpcError>;
    async fn finish_input(&mut self) -> Result<(), NvidiaGrpcError>;
    async fn receive(&mut self) -> Result<Option<RivaStreamingResponse>, NvidiaGrpcError>;
    async fn cancel(&mut self) -> Result<(), NvidiaGrpcError>;
}

#[async_trait]
pub trait NvidiaGrpcTransportFactory: Send + Sync {
    /// Starts the authenticated streaming RPC. The implementation may reveal the credential only
    /// while constructing `authorization: Bearer ...` metadata and must not log or retain a
    /// plaintext copy beyond the live call's requirements.
    async fn connect(
        &self,
        request: NvidiaGrpcConnectRequest<'_>,
    ) -> Result<Box<dyn NvidiaGrpcStream>, NvidiaGrpcError>;
}

pub struct NvidiaNimAsr {
    factory: Arc<dyn NvidiaGrpcTransportFactory>,
    route: NvidiaRoute,
}

impl NvidiaNimAsr {
    pub fn new(factory: Arc<dyn NvidiaGrpcTransportFactory>) -> Self {
        Self {
            factory,
            route: NvidiaRoute::Curated,
        }
    }

    /// Overrides the fixed route only with an opaque object produced by signed-catalog validation.
    pub fn with_trusted_catalog_override(
        factory: Arc<dyn NvidiaGrpcTransportFactory>,
        route: TrustedNvidiaRouteOverride,
    ) -> Self {
        Self {
            factory,
            route: NvidiaRoute::TrustedCatalog(route),
        }
    }

    pub fn capabilities(&self) -> &'static RecognizerCapabilities {
        &CAPABILITIES
    }

    pub async fn start_session(
        &self,
        config: RecognitionConfig,
        credential: SecretString,
        cancellation: CancellationToken,
    ) -> Result<NvidiaNimSession, SttError> {
        config.validate()?;
        self.route.validate()?;
        validate_config(&config)?;

        let deadline = Instant::now() + config.timeouts.connect;
        let request = NvidiaGrpcConnectRequest {
            endpoint: self.route.endpoint(),
            use_tls: true,
            function_id: self.route.function_id(),
            authorization_scheme: "Bearer",
            credential: &credential,
            deadline,
        };
        validate_connect_request(
            &request,
            matches!(&self.route, NvidiaRoute::TrustedCatalog(_)),
        )?;
        let mut stream = timeout(config.timeouts.connect, self.factory.connect(request))
            .await
            .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA gRPC connection timed out"))?
            .map_err(map_grpc_error)?;
        // The live RPC has consumed its request metadata. Clear our credential before audio or
        // recognition data begins flowing.
        drop(credential);

        let contexts = config
            .context_hint
            .iter()
            .cloned()
            .chain(config.keyword_hints.iter().cloned())
            .collect();
        let riva_config = RivaStreamingConfig {
            encoding: RivaAudioEncoding::LinearPcm,
            sample_rate_hz: config.audio.sample_rate_hz,
            channels: config.audio.channels,
            language_code: "en-US".into(),
            model: NVIDIA_NIM_MODEL.into(),
            interim_results: config.interim_results,
            enable_automatic_punctuation: true,
            max_alternatives: 1,
            speech_contexts: contexts,
        };
        timeout(
            config.timeouts.send,
            stream.send(RivaStreamingRequest::Config(riva_config)),
        )
        .await
        .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA Riva configuration timed out"))?
        .map_err(map_grpc_error)?;

        Ok(NvidiaNimSession {
            stream,
            config,
            cancellation,
            state: SessionState::Active,
            cancellation_event_pending: false,
            pending: VecDeque::from([RecognitionEvent::SessionStarted {
                provider_session_id: None,
            }]),
            utterance_index: 0,
            revision: 0,
            turn_started: false,
            flush_deadline: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Active,
    Cancelled,
    Closed,
}

pub struct NvidiaNimSession {
    stream: Box<dyn NvidiaGrpcStream>,
    config: RecognitionConfig,
    cancellation: CancellationToken,
    state: SessionState,
    cancellation_event_pending: bool,
    pending: VecDeque<RecognitionEvent>,
    utterance_index: u64,
    revision: u64,
    turn_started: bool,
    flush_deadline: Option<Instant>,
}

impl NvidiaNimSession {
    pub async fn send_audio(&mut self, pcm_s16le: &[u8]) -> Result<(), SttError> {
        self.ensure_active().await?;
        if pcm_s16le.is_empty() {
            return Ok(());
        }
        if pcm_s16le.len() > 1024 * 1024 || !pcm_s16le.len().is_multiple_of(2) {
            return Err(SttError::invalid_request(
                "NVIDIA PCM audio chunk must contain complete 16-bit samples and fit one MiB",
            ));
        }
        timeout(
            self.config.timeouts.send,
            self.stream
                .send(RivaStreamingRequest::Audio(pcm_s16le.to_vec())),
        )
        .await
        .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA audio send timed out"))?
        .map_err(map_grpc_error)
    }

    pub async fn flush(&mut self, _reason: FlushReason) -> Result<(), SttError> {
        self.ensure_active().await?;
        timeout(self.config.timeouts.send, self.stream.finish_input())
            .await
            .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA input finish timed out"))?
            .map_err(map_grpc_error)?;
        self.flush_deadline = Some(Instant::now() + self.config.timeouts.flush);
        Ok(())
    }

    pub async fn cancel(&mut self) -> Result<(), SttError> {
        if self.state != SessionState::Active {
            return Ok(());
        }
        self.state = SessionState::Cancelled;
        self.pending.clear();
        self.cancellation_event_pending = true;
        let _ = self.stream.cancel().await;
        Ok(())
    }

    pub async fn next_event(&mut self) -> Result<Option<RecognitionEvent>, SttError> {
        if self.cancellation_event_pending {
            self.cancellation_event_pending = false;
            return Ok(Some(RecognitionEvent::Cancelled));
        }
        if self.state != SessionState::Active {
            return Ok(None);
        }
        if self.cancellation.is_cancelled() {
            self.cancel().await?;
            self.cancellation_event_pending = false;
            return Ok(Some(RecognitionEvent::Cancelled));
        }
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }

        loop {
            let receive = async {
                match self.flush_deadline {
                    Some(deadline) => timeout_at(deadline.into(), self.stream.receive())
                        .await
                        .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA flush timed out"))?,
                    None => timeout(self.config.timeouts.receive_idle, self.stream.receive())
                        .await
                        .map_err(|_| SttError::timeout(PROVIDER_ID, "NVIDIA receive timed out"))?,
                }
                .map_err(map_grpc_error)
            };
            let response = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => {
                    self.cancel().await?;
                    self.cancellation_event_pending = false;
                    return Ok(Some(RecognitionEvent::Cancelled));
                }
                response = receive => response?,
            };
            let Some(response) = response else {
                self.state = SessionState::Closed;
                return Ok(None);
            };
            self.normalize_response(response)?;
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
        }
    }

    async fn ensure_active(&mut self) -> Result<(), SttError> {
        if self.cancellation.is_cancelled() && self.state == SessionState::Active {
            self.cancel().await?;
        }
        if self.state == SessionState::Active {
            Ok(())
        } else {
            Err(SttError::cancelled(PROVIDER_ID))
        }
    }

    fn normalize_response(&mut self, response: RivaStreamingResponse) -> Result<(), SttError> {
        for result in response.results {
            if result.transcript.is_empty() {
                continue;
            }
            validate_probability(result.stability)?;
            validate_probability(result.confidence)?;
            let turn_id = format!("utterance-{}", self.utterance_index);
            if !self.turn_started {
                self.turn_started = true;
                self.pending.push_back(RecognitionEvent::TurnStarted {
                    turn_id: turn_id.clone(),
                });
            }
            self.revision = self.revision.saturating_add(1);
            self.pending
                .push_back(RecognitionEvent::Transcript(TranscriptEvent {
                    turn_id: turn_id.clone(),
                    revision: self.revision,
                    text: result.transcript,
                    status: if result.is_final {
                        TranscriptStatus::Final
                    } else {
                        TranscriptStatus::Partial
                    },
                    language: Some("en-US".into()),
                    confidence: result.confidence,
                    audio_start_ms: None,
                    audio_end_ms: result.audio_processed_seconds.and_then(seconds_to_ms),
                }));
            if result.is_final {
                self.pending.push_back(RecognitionEvent::TurnEnded {
                    turn_id,
                    revision: self.revision,
                    reason: if self.flush_deadline.is_some() {
                        TurnEndReason::ManualFlush
                    } else {
                        TurnEndReason::ProviderEndpoint
                    },
                });
                self.flush_deadline = None;
                self.utterance_index = self.utterance_index.saturating_add(1);
                self.revision = 0;
                self.turn_started = false;
            }
        }
        Ok(())
    }
}

fn validate_config(config: &RecognitionConfig) -> Result<(), SttError> {
    if config.model != NVIDIA_NIM_MODEL {
        return Err(SttError::invalid_request(
            "NVIDIA NIM ASR model is fixed by the curated catalog",
        ));
    }
    if config.audio != AudioFormat::PCM_16KHZ_MONO
        || config.audio.encoding != AudioEncoding::PcmS16Le
    {
        return Err(SttError::invalid_request(
            "NVIDIA NIM ASR requires 16 kHz mono PCM S16LE",
        ));
    }
    if config
        .languages
        .iter()
        .any(|language| !matches!(language.as_str(), "en" | "en-US"))
    {
        return Err(SttError::invalid_request(
            "NVIDIA Nemotron streaming ASR is currently English-only",
        ));
    }
    if config.retention == RetentionPolicy::RequireRequestLevelOptOut {
        return Err(SttError::new(
            PROVIDER_ID,
            SttErrorKind::PrivacyPolicy,
            "NVIDIA NIM does not expose a request-level retention opt-out",
            false,
            None,
        ));
    }
    Ok(())
}

fn validate_connect_request(
    request: &NvidiaGrpcConnectRequest<'_>,
    trusted_override: bool,
) -> Result<(), SttError> {
    if !request.use_tls
        || request.authorization_scheme != "Bearer"
        || request.deadline <= Instant::now()
        || (!trusted_override
            && (request.endpoint != NVIDIA_NIM_ENDPOINT
                || request.function_id != NVIDIA_NIM_FUNCTION_ID))
    {
        return Err(SttError::invalid_request(
            "untrusted NVIDIA NIM connection metadata was rejected",
        ));
    }
    Ok(())
}

fn seconds_to_ms(seconds: f32) -> Option<u64> {
    if seconds.is_finite() && seconds >= 0.0 {
        Some((f64::from(seconds) * 1_000.0).round() as u64)
    } else {
        None
    }
}

fn validate_probability(value: Option<f32>) -> Result<(), SttError> {
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        Err(SttError::protocol(PROVIDER_ID, Some("invalid_probability")))
    } else {
        Ok(())
    }
}

fn map_grpc_error(error: NvidiaGrpcError) -> SttError {
    let (kind, message, retryable, code) = match (error.code, error.http_status) {
        (GrpcStatusCode::Unauthenticated | GrpcStatusCode::PermissionDenied, _) => (
            SttErrorKind::Authentication,
            "NVIDIA authentication or function authorization failed",
            false,
            "authentication",
        ),
        (GrpcStatusCode::ResourceExhausted, _) => (
            SttErrorKind::RateLimited,
            "NVIDIA development API rate limit was reached",
            true,
            "rate_limited",
        ),
        (GrpcStatusCode::InvalidArgument, _) => (
            SttErrorKind::InvalidRequest,
            "NVIDIA rejected the Riva recognition request",
            false,
            "invalid_argument",
        ),
        (GrpcStatusCode::DeadlineExceeded, _) => (
            SttErrorKind::Timeout,
            "NVIDIA Riva request deadline was exceeded",
            true,
            "deadline_exceeded",
        ),
        (GrpcStatusCode::Unavailable | GrpcStatusCode::Internal, _) => (
            SttErrorKind::Unavailable,
            "NVIDIA Riva service is unavailable",
            true,
            "unavailable",
        ),
        _ => (
            SttErrorKind::Protocol,
            "NVIDIA Riva transport returned an unknown failure",
            false,
            "unknown",
        ),
    };
    let mut mapped = SttError::new(PROVIDER_ID, kind, message, retryable, Some(code));
    mapped.retry_after = error.retry_after;
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_route_rejects_wrong_function_id() {
        let credential = SecretString::new("not-a-live-key");
        let request = NvidiaGrpcConnectRequest {
            endpoint: NVIDIA_NIM_ENDPOINT,
            use_tls: true,
            function_id: "wrong-function-id",
            authorization_scheme: "Bearer",
            credential: &credential,
            deadline: Instant::now() + Duration::from_secs(1),
        };
        let error = validate_connect_request(&request, false).expect_err("wrong function rejected");
        assert_eq!(error.kind, SttErrorKind::InvalidRequest);
    }

    #[test]
    fn verified_catalog_override_requires_trust_metadata_and_tls_port() {
        assert!(TrustedNvidiaRouteOverride::from_verified_catalog(
            "grpc.example.invalid:443".into(),
            "replacement-function".into(),
            0,
            "release-key".into(),
        )
        .is_err());
        assert!(TrustedNvidiaRouteOverride::from_verified_catalog(
            "grpc.example.invalid:443".into(),
            "replacement-function".into(),
            42,
            "release-key".into(),
        )
        .is_ok());
    }
}
