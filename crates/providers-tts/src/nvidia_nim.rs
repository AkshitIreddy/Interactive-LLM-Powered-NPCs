//! Experimental NVIDIA NIM Magpie multilingual TTS adapter.
//!
//! This route is intentionally curated: callers cannot replace the NVCF
//! function, HTTP origin, gRPC authority, or Riva method. The request contract
//! contains no Riva `ZeroShotData`/audio-prompt field, so voice cloning cannot be
//! enabled through configuration or model output.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use async_trait::async_trait;

use crate::{
    CredentialResolveError, HostedTtsProviderId, PcmChunk, PcmEncoding, ProviderCapabilities,
    ProviderCredentialResolver, PushOutcome, SemanticClauseBuffer, SensitiveHeaderValue,
    SensitiveString, SessionIdentity, SessionState, StreamingTtsProvider, StreamingTtsSession,
    TtsError, TtsErrorKind, TtsEvent, TtsSessionRequest, VoiceBinding, VoiceBindings,
    WordAlignment, MAX_AUDIO_CHUNK_BYTES, MAX_PUSH_TEXT_CHARS,
};

pub const NVIDIA_MAGPIE_FUNCTION_ID: &str = "877104f7-e885-42b9-8de8-f6e4c6303969";
pub const NVIDIA_MAGPIE_MODEL_ID: &str = "magpie-tts-multilingual";
pub const NVIDIA_MAGPIE_GRPC_AUTHORITY: &str = "grpc.nvcf.nvidia.com:443";
pub const NVIDIA_MAGPIE_HTTP_ORIGIN: &str =
    "https://877104f7-e885-42b9-8de8-f6e4c6303969.invocation.api.nvcf.nvidia.com";
pub const NVIDIA_MAGPIE_LIST_VOICES_PATH: &str = "/v1/audio/list_voices";
pub const NVIDIA_MAGPIE_SYNTHESIZE_PATH: &str = "/v1/audio/synthesize";
pub const RIVA_TTS_SYNTHESIZE_ONLINE_METHOD: &str =
    "/nvidia.riva.tts.RivaSpeechSynthesis/SynthesizeOnline";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NvidiaMagpieEndpointDescriptor {
    pub function_id: &'static str,
    pub model_id: &'static str,
    pub grpc_authority: &'static str,
    pub grpc_method: &'static str,
    pub http_origin: &'static str,
    pub http_list_voices_path: &'static str,
    pub http_synthesize_path: &'static str,
}

pub const NVIDIA_MAGPIE_ENDPOINTS: NvidiaMagpieEndpointDescriptor =
    NvidiaMagpieEndpointDescriptor {
        function_id: NVIDIA_MAGPIE_FUNCTION_ID,
        model_id: NVIDIA_MAGPIE_MODEL_ID,
        grpc_authority: NVIDIA_MAGPIE_GRPC_AUTHORITY,
        grpc_method: RIVA_TTS_SYNTHESIZE_ONLINE_METHOD,
        http_origin: NVIDIA_MAGPIE_HTTP_ORIGIN,
        http_list_voices_path: NVIDIA_MAGPIE_LIST_VOICES_PATH,
        http_synthesize_path: NVIDIA_MAGPIE_SYNTHESIZE_PATH,
    };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvidiaMagpieLifecycle {
    /// The fixed HTTP route produced a non-silent, unclipped stock-voice smoke
    /// sample. The gRPC `SynthesizeOnline` route remains replay-only and must be
    /// live-qualified before this adapter can graduate from experimental.
    ExperimentalHttpSmokeQualifiedAwaitingGrpcStreamingQualification,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvidiaStockVoice {
    pub id: String,
    pub display_name: String,
    pub locale: String,
    pub origin: NvidiaVoiceOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvidiaVoiceOrigin {
    ProviderStock,
    UserOrCustom,
}

impl NvidiaStockVoice {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.origin != NvidiaVoiceOrigin::ProviderStock || !is_stock_voice_id(&self.id) {
            return Err("invalid_nvidia_stock_voice_id");
        }
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err("invalid_nvidia_voice_name");
        }
        if self.locale.trim().is_empty() || self.locale.len() > 35 {
            return Err("invalid_locale");
        }
        Ok(())
    }
}

fn is_stock_voice_id(value: &str) -> bool {
    value.starts_with("Magpie-Multilingual.")
        && value.len() <= 256
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
}

pub struct NvidiaVoiceListRequest {
    origin: &'static str,
    path: &'static str,
    authorization: SensitiveHeaderValue,
    deadline: Duration,
}

impl NvidiaVoiceListRequest {
    #[must_use]
    pub fn origin(&self) -> &'static str {
        self.origin
    }

    #[must_use]
    pub fn path(&self) -> &'static str {
        self.path
    }

    #[must_use]
    pub fn authorization(&self) -> &SensitiveHeaderValue {
        &self.authorization
    }

    #[must_use]
    pub fn deadline(&self) -> Duration {
        self.deadline
    }
}

impl fmt::Debug for NvidiaVoiceListRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NvidiaVoiceListRequest")
            .field("origin", &self.origin)
            .field("path", &self.path)
            .field("authorization", &self.authorization)
            .field("deadline", &self.deadline)
            .finish()
    }
}

pub struct NvidiaSynthesizeOnlineRequest {
    authority: &'static str,
    tls_required: bool,
    method: &'static str,
    function_id: &'static str,
    authorization: SensitiveHeaderValue,
    request_id: String,
    text: SensitiveString,
    language_code: String,
    encoding: NvidiaRivaAudioEncoding,
    sample_rate_hz: u32,
    voice_name: String,
    deadline: Duration,
}

impl NvidiaSynthesizeOnlineRequest {
    #[must_use]
    pub fn authority(&self) -> &'static str {
        self.authority
    }

    #[must_use]
    pub fn tls_required(&self) -> bool {
        self.tls_required
    }

    #[must_use]
    pub fn method(&self) -> &'static str {
        self.method
    }

    #[must_use]
    pub fn function_id(&self) -> &'static str {
        self.function_id
    }

    #[must_use]
    pub fn authorization(&self) -> &SensitiveHeaderValue {
        &self.authorization
    }

    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    #[must_use]
    pub fn text(&self) -> &SensitiveString {
        &self.text
    }

    #[must_use]
    pub fn language_code(&self) -> &str {
        &self.language_code
    }

    #[must_use]
    pub fn encoding(&self) -> NvidiaRivaAudioEncoding {
        self.encoding
    }

    #[must_use]
    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    #[must_use]
    pub fn voice_name(&self) -> &str {
        &self.voice_name
    }

    #[must_use]
    pub fn deadline(&self) -> Duration {
        self.deadline
    }
}

impl fmt::Debug for NvidiaSynthesizeOnlineRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NvidiaSynthesizeOnlineRequest")
            .field("authority", &self.authority)
            .field("tls_required", &self.tls_required)
            .field("method", &self.method)
            .field("function_id", &self.function_id)
            .field("authorization", &self.authorization)
            .field("request_id", &self.request_id)
            .field("text", &self.text)
            .field("language_code", &self.language_code)
            .field("encoding", &self.encoding)
            .field("sample_rate_hz", &self.sample_rate_hz)
            .field("voice_name", &self.voice_name)
            .field("deadline", &self.deadline)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvidiaRivaAudioEncoding {
    LinearPcm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvidiaWordOffset {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text_start: Option<usize>,
    pub source_text_length: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvidiaSynthesizeFrame {
    pub pcm: Vec<u8>,
    /// Populated only when the deployed protocol supplies word offsets. Riva's
    /// experimental predicted token durations are not guessed into word timing.
    pub word_offsets: Vec<NvidiaWordOffset>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NvidiaNvcfError {
    #[error("NVIDIA authentication failed")]
    Authentication,
    #[error("NVIDIA request was rate limited")]
    RateLimited { retry_after: Option<Duration> },
    #[error("NVIDIA service is unavailable")]
    Unavailable,
    #[error("NVIDIA request deadline exceeded")]
    DeadlineExceeded,
    #[error("NVIDIA function identifier was rejected")]
    BadFunction,
    #[error("NVIDIA protocol response was invalid")]
    Protocol,
    #[error("NVIDIA stream was cancelled")]
    Cancelled,
}

#[async_trait]
pub trait NvidiaNimHttpTransport: Send + Sync {
    async fn list_stock_voices(
        &self,
        request: NvidiaVoiceListRequest,
    ) -> Result<Vec<NvidiaStockVoice>, NvidiaNvcfError>;
}

#[async_trait]
pub trait NvidiaNimSynthesisStream: Send {
    async fn next_frame(&mut self) -> Option<Result<NvidiaSynthesizeFrame, NvidiaNvcfError>>;
    async fn cancel(&mut self) -> Result<(), NvidiaNvcfError>;
}

#[async_trait]
pub trait NvidiaNimGrpcTransport: Send + Sync {
    async fn synthesize_online(
        &self,
        request: NvidiaSynthesizeOnlineRequest,
    ) -> Result<Box<dyn NvidiaNimSynthesisStream>, NvidiaNvcfError>;
}

#[derive(Clone)]
pub struct NvidiaNimMagpie {
    grpc: Arc<dyn NvidiaNimGrpcTransport>,
    http: Arc<dyn NvidiaNimHttpTransport>,
    credential_resolver: Arc<dyn ProviderCredentialResolver>,
    bindings: VoiceBindings,
    stock_voices: Arc<RwLock<BTreeMap<String, NvidiaStockVoice>>>,
    request_deadline: Duration,
}

impl NvidiaNimMagpie {
    #[must_use]
    pub fn new(
        grpc: Arc<dyn NvidiaNimGrpcTransport>,
        http: Arc<dyn NvidiaNimHttpTransport>,
        credential_resolver: Arc<dyn ProviderCredentialResolver>,
        bindings: VoiceBindings,
    ) -> Self {
        Self {
            grpc,
            http,
            credential_resolver,
            bindings,
            stock_voices: Arc::new(RwLock::new(BTreeMap::new())),
            request_deadline: Duration::from_secs(30),
        }
    }

    #[must_use]
    pub fn lifecycle(&self) -> NvidiaMagpieLifecycle {
        NvidiaMagpieLifecycle::ExperimentalHttpSmokeQualifiedAwaitingGrpcStreamingQualification
    }

    pub fn set_request_deadline(&mut self, deadline: Duration) -> Result<(), &'static str> {
        if deadline < Duration::from_millis(100) || deadline > Duration::from_secs(120) {
            return Err("invalid_nvidia_deadline");
        }
        self.request_deadline = deadline;
        Ok(())
    }

    /// Uses the fixed function-id subdomain and installs only validated stock
    /// voices. The credential exists only inside this request.
    pub async fn discover_stock_voices(&self) -> Result<Vec<NvidiaStockVoice>, TtsError> {
        let provider_id = HostedTtsProviderId::NvidiaNimMagpie;
        let credential = resolve_credential(&self.credential_resolver, provider_id).await?;
        let voices = self
            .http
            .list_stock_voices(NvidiaVoiceListRequest {
                origin: NVIDIA_MAGPIE_HTTP_ORIGIN,
                path: NVIDIA_MAGPIE_LIST_VOICES_PATH,
                authorization: SensitiveHeaderValue::with_scheme("Bearer", credential),
                deadline: self.request_deadline,
            })
            .await
            .map_err(map_nvidia_error)?;
        if voices.is_empty() || voices.len() > 4_096 {
            return Err(protocol_error("invalid_nvidia_voice_list"));
        }
        let mut validated = BTreeMap::new();
        for voice in &voices {
            voice
                .validate()
                .map_err(|_| protocol_error("invalid_nvidia_voice_list"))?;
            if validated.insert(voice.id.clone(), voice.clone()).is_some() {
                return Err(protocol_error("duplicate_nvidia_voice"));
            }
        }
        *self
            .stock_voices
            .write()
            .expect("NVIDIA stock voice lock poisoned") = validated;
        Ok(voices)
    }

    #[must_use]
    pub fn discovered_stock_voices(&self) -> Vec<NvidiaStockVoice> {
        self.stock_voices
            .read()
            .expect("NVIDIA stock voice lock poisoned")
            .values()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl StreamingTtsProvider for NvidiaNimMagpie {
    fn id(&self) -> HostedTtsProviderId {
        HostedTtsProviderId::NvidiaNimMagpie
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming_input: false,
            streaming_pcm: true,
            alignment: true,
            visemes_or_phonemes: false,
            cancellation: true,
            usage: false,
        }
    }

    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
        request
            .identity
            .validate()
            .and_then(|_| request.output.validate().map(|_| ()))
            .and_then(|_| request.clause_policy.validate())
            .map_err(invalid_error)?;
        if request.output.encoding != PcmEncoding::PcmS16Le || request.output.channels != 1 {
            return Err(invalid_error("nvidia_magpie_requires_mono_linear_pcm"));
        }
        if !matches!(request.output.sample_rate_hz, 22_050 | 44_100) {
            return Err(invalid_error("nvidia_magpie_sample_rate_not_curated"));
        }
        let binding = self
            .bindings
            .resolve(&request.voice_intent_id, self.id())
            .cloned()
            .ok_or_else(|| invalid_error("voice_binding_missing"))?;
        validate_nvidia_binding(&binding)?;
        let voice = self
            .stock_voices
            .read()
            .expect("NVIDIA stock voice lock poisoned")
            .get(&binding.voice_id)
            .cloned()
            .ok_or_else(|| invalid_error("nvidia_stock_voice_not_discovered"))?;
        if voice.locale != request.locale {
            return Err(invalid_error("nvidia_stock_voice_locale_mismatch"));
        }

        Ok(Box::new(NvidiaMagpieSession {
            grpc: Arc::clone(&self.grpc),
            credential_resolver: Arc::clone(&self.credential_resolver),
            binding,
            request: request.clone(),
            deadline: self.request_deadline,
            clause_buffer: SemanticClauseBuffer::new(request.clause_policy),
            streams: VecDeque::new(),
            pending_events: VecDeque::new(),
            state: SessionState::AcceptingText,
            utterance_started: false,
            sequence: 0,
            clause_sequence: 0,
        }))
    }
}

fn validate_nvidia_binding(binding: &VoiceBinding) -> Result<(), TtsError> {
    if !is_stock_voice_id(&binding.voice_id) {
        return Err(invalid_error("invalid_nvidia_stock_voice_id"));
    }
    if binding.model_id != NVIDIA_MAGPIE_MODEL_ID {
        return Err(invalid_error("invalid_nvidia_magpie_model"));
    }
    // This route intentionally supports no provider-specific escape hatch. In
    // particular, zero-shot data, audio prompts, cloning, and custom endpoints
    // cannot be smuggled through provider options.
    if !binding.provider_options.is_empty() {
        return Err(invalid_error("nvidia_provider_options_forbidden"));
    }
    Ok(())
}

struct NvidiaMagpieSession {
    grpc: Arc<dyn NvidiaNimGrpcTransport>,
    credential_resolver: Arc<dyn ProviderCredentialResolver>,
    binding: VoiceBinding,
    request: TtsSessionRequest,
    deadline: Duration,
    clause_buffer: SemanticClauseBuffer,
    streams: VecDeque<Box<dyn NvidiaNimSynthesisStream>>,
    pending_events: VecDeque<TtsEvent>,
    state: SessionState,
    utterance_started: bool,
    sequence: u64,
    clause_sequence: u64,
}

impl NvidiaMagpieSession {
    async fn start_clause(&mut self, text: String) -> Result<(), TtsError> {
        let provider_id = HostedTtsProviderId::NvidiaNimMagpie;
        let credential = resolve_credential(&self.credential_resolver, provider_id).await?;
        let request_id = format!(
            "{}:{}:{}:{}",
            self.request.identity.session_id,
            self.request.identity.turn_id,
            self.request.identity.cancellation_generation,
            self.clause_sequence
        );
        self.clause_sequence = self.clause_sequence.saturating_add(1);
        let stream = self
            .grpc
            .synthesize_online(NvidiaSynthesizeOnlineRequest {
                authority: NVIDIA_MAGPIE_GRPC_AUTHORITY,
                tls_required: true,
                method: RIVA_TTS_SYNTHESIZE_ONLINE_METHOD,
                function_id: NVIDIA_MAGPIE_FUNCTION_ID,
                authorization: SensitiveHeaderValue::with_scheme("Bearer", credential),
                request_id,
                text: SensitiveString::new(text),
                language_code: self.request.locale.clone(),
                encoding: NvidiaRivaAudioEncoding::LinearPcm,
                sample_rate_hz: self.request.output.sample_rate_hz,
                voice_name: self.binding.voice_id.clone(),
                deadline: self.deadline,
            })
            .await
            .map_err(map_nvidia_error)?;
        self.streams.push_back(stream);
        self.utterance_started = true;
        Ok(())
    }

    async fn protocol_fault(&mut self, code: &'static str) -> Option<Result<TtsEvent, TtsError>> {
        self.state = SessionState::Faulted;
        while let Some(mut stream) = self.streams.pop_front() {
            let _ignored = stream.cancel().await;
        }
        Some(Err(protocol_error(code)))
    }
}

#[async_trait]
impl StreamingTtsSession for NvidiaMagpieSession {
    fn provider_id(&self) -> HostedTtsProviderId {
        HostedTtsProviderId::NvidiaNimMagpie
    }

    fn identity(&self) -> &SessionIdentity {
        &self.request.identity
    }

    fn state(&self) -> SessionState {
        self.state
    }

    fn has_started_utterance(&self) -> bool {
        self.utterance_started
    }

    async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError> {
        if self.state != SessionState::AcceptingText {
            return Err(invalid_error("session_not_accepting_text"));
        }
        let accepted_chars = text.chars().count();
        if accepted_chars > MAX_PUSH_TEXT_CHARS || text.contains('\0') {
            return Err(invalid_error("invalid_text_chunk"));
        }
        let clauses = self.clause_buffer.push(text);
        let submitted = clauses.len();
        for clause in clauses {
            if let Err(error) = self.start_clause(clause).await {
                self.state = SessionState::Faulted;
                return Err(error);
            }
        }
        Ok(PushOutcome {
            accepted_chars,
            clauses_submitted: submitted,
            buffered_chars: self.clause_buffer.pending_chars(),
        })
    }

    async fn finish(&mut self) -> Result<(), TtsError> {
        if self.state != SessionState::AcceptingText {
            return Err(invalid_error("session_cannot_finish"));
        }
        let clauses = self.clause_buffer.finish();
        if clauses.is_empty() && !self.utterance_started {
            return Err(invalid_error("empty_utterance"));
        }
        for clause in clauses {
            if let Err(error) = self.start_clause(clause).await {
                self.state = SessionState::Faulted;
                return Err(error);
            }
        }
        self.state = SessionState::Finishing;
        Ok(())
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Some(Ok(event));
        }
        if self.state.is_terminal() {
            return None;
        }
        loop {
            let Some(stream) = self.streams.front_mut() else {
                if self.state == SessionState::Finishing {
                    self.state = SessionState::Completed;
                    return Some(Ok(TtsEvent::Completed));
                }
                return None;
            };
            match stream.next_frame().await {
                Some(Ok(frame)) => {
                    if frame.pcm.len() > MAX_AUDIO_CHUNK_BYTES {
                        return self.protocol_fault("audio_chunk_too_large").await;
                    }
                    if frame.word_offsets.len() > 16_384
                        || frame.word_offsets.iter().any(|word| {
                            word.word.is_empty()
                                || word.word.len() > 512
                                || word.end_ms < word.start_ms
                        })
                    {
                        return self.protocol_fault("invalid_alignment").await;
                    }
                    if !frame.word_offsets.is_empty() {
                        self.pending_events.push_back(TtsEvent::Alignment(
                            frame
                                .word_offsets
                                .into_iter()
                                .map(|word| WordAlignment {
                                    word: word.word,
                                    start_ms: word.start_ms,
                                    end_ms: word.end_ms,
                                    source_text_start: word.source_text_start,
                                    source_text_length: word.source_text_length,
                                })
                                .collect(),
                        ));
                    }
                    if frame.pcm.is_empty() {
                        if let Some(event) = self.pending_events.pop_front() {
                            return Some(Ok(event));
                        }
                        continue;
                    }
                    let sequence = self.sequence;
                    self.sequence = self.sequence.saturating_add(1);
                    return Some(Ok(TtsEvent::Audio(PcmChunk {
                        sequence,
                        format: self.request.output,
                        data: frame.pcm,
                    })));
                }
                Some(Err(NvidiaNvcfError::Cancelled)) if self.state == SessionState::Cancelled => {
                    return None;
                }
                Some(Err(error)) => {
                    self.state = SessionState::Faulted;
                    return Some(Err(map_nvidia_error(error)));
                }
                None => {
                    self.streams.pop_front();
                    continue;
                }
            }
        }
    }

    async fn cancel(&mut self) -> Result<(), TtsError> {
        if self.state == SessionState::Cancelled {
            return Ok(());
        }
        if self.state.is_terminal() {
            return Err(invalid_error("session_already_terminal"));
        }
        let mut first_error = None;
        while let Some(mut stream) = self.streams.pop_front() {
            if let Err(error) = stream.cancel().await {
                first_error.get_or_insert(error);
            }
        }
        self.state = SessionState::Cancelled;
        self.pending_events
            .push_back(TtsEvent::Interrupted { reason: "barge_in" });
        first_error.map(map_nvidia_error).map_or(Ok(()), Err)
    }
}

async fn resolve_credential(
    resolver: &Arc<dyn ProviderCredentialResolver>,
    provider_id: HostedTtsProviderId,
) -> Result<SensitiveString, TtsError> {
    let credential = resolver.resolve(provider_id).await.map_err(|error| {
        let (kind, code, retryable) = match error {
            CredentialResolveError::Missing => {
                (TtsErrorKind::Authentication, "credential_missing", false)
            }
            CredentialResolveError::Unavailable => (
                TtsErrorKind::Unavailable,
                "credential_vault_unavailable",
                true,
            ),
        };
        TtsError::new(provider_id.as_str(), kind, code, retryable)
    })?;
    if credential.is_empty() {
        return Err(TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Authentication,
            "credential_missing",
            false,
        ));
    }
    Ok(credential)
}

fn map_nvidia_error(error: NvidiaNvcfError) -> TtsError {
    let provider = HostedTtsProviderId::NvidiaNimMagpie.as_str();
    match error {
        NvidiaNvcfError::Authentication => TtsError::new(
            provider,
            TtsErrorKind::Authentication,
            "nvidia_authentication_failed",
            false,
        ),
        NvidiaNvcfError::RateLimited { retry_after } => {
            let mut error = TtsError::new(
                provider,
                TtsErrorKind::RateLimited,
                "nvidia_rate_limited",
                true,
            );
            error.retry_after = retry_after;
            error
        }
        NvidiaNvcfError::Unavailable => TtsError::new(
            provider,
            TtsErrorKind::Unavailable,
            "nvidia_unavailable",
            true,
        ),
        NvidiaNvcfError::DeadlineExceeded => TtsError::new(
            provider,
            TtsErrorKind::Timeout,
            "nvidia_deadline_exceeded",
            true,
        ),
        NvidiaNvcfError::BadFunction => TtsError::new(
            provider,
            TtsErrorKind::InvalidRequest,
            "nvidia_function_rejected",
            false,
        ),
        NvidiaNvcfError::Protocol => protocol_error("nvidia_protocol_failure"),
        NvidiaNvcfError::Cancelled => {
            TtsError::new(provider, TtsErrorKind::Cancelled, "request_cancelled", true)
        }
    }
}

fn invalid_error(code: &'static str) -> TtsError {
    TtsError::new(
        HostedTtsProviderId::NvidiaNimMagpie.as_str(),
        TtsErrorKind::InvalidRequest,
        code,
        false,
    )
}

fn protocol_error(code: &'static str) -> TtsError {
    TtsError::new(
        HostedTtsProviderId::NvidiaNimMagpie.as_str(),
        TtsErrorKind::Protocol,
        code,
        false,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedNvidiaVoiceListRequest {
    pub origin: &'static str,
    pub path: &'static str,
    pub deadline: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedNvidiaSynthesisRequest {
    pub authority: &'static str,
    pub tls_required: bool,
    pub method: &'static str,
    pub function_id: &'static str,
    pub request_id: String,
    pub text_chars: usize,
    pub language_code: String,
    pub encoding: NvidiaRivaAudioEncoding,
    pub sample_rate_hz: u32,
    pub voice_name: String,
    pub deadline: Duration,
}

type NvidiaVoiceListResponse = Result<Vec<NvidiaStockVoice>, NvidiaNvcfError>;

#[derive(Clone)]
pub struct MockNvidiaHttpTransport {
    response: Arc<Mutex<Option<NvidiaVoiceListResponse>>>,
    requests: Arc<Mutex<Vec<RecordedNvidiaVoiceListRequest>>>,
    request_debugs: Arc<Mutex<Vec<String>>>,
}

impl MockNvidiaHttpTransport {
    #[must_use]
    pub fn returning(response: Result<Vec<NvidiaStockVoice>, NvidiaNvcfError>) -> Self {
        Self {
            response: Arc::new(Mutex::new(Some(response))),
            requests: Arc::new(Mutex::new(Vec::new())),
            request_debugs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn requests(&self) -> Vec<RecordedNvidiaVoiceListRequest> {
        self.requests
            .lock()
            .expect("NVIDIA HTTP mock lock poisoned")
            .clone()
    }

    #[must_use]
    pub fn request_debugs(&self) -> Vec<String> {
        self.request_debugs
            .lock()
            .expect("NVIDIA HTTP mock lock poisoned")
            .clone()
    }
}

#[async_trait]
impl NvidiaNimHttpTransport for MockNvidiaHttpTransport {
    async fn list_stock_voices(
        &self,
        request: NvidiaVoiceListRequest,
    ) -> Result<Vec<NvidiaStockVoice>, NvidiaNvcfError> {
        self.request_debugs
            .lock()
            .expect("NVIDIA HTTP mock lock poisoned")
            .push(format!("{request:?}"));
        self.requests
            .lock()
            .expect("NVIDIA HTTP mock lock poisoned")
            .push(RecordedNvidiaVoiceListRequest {
                origin: request.origin,
                path: request.path,
                deadline: request.deadline,
            });
        self.response
            .lock()
            .expect("NVIDIA HTTP mock lock poisoned")
            .take()
            .unwrap_or(Err(NvidiaNvcfError::Unavailable))
    }
}

#[derive(Clone, Debug, Default)]
pub struct MockNvidiaStreamScript {
    pub frames: VecDeque<Result<NvidiaSynthesizeFrame, NvidiaNvcfError>>,
    pub start_error: Option<NvidiaNvcfError>,
}

#[derive(Clone, Default)]
pub struct MockNvidiaGrpcTransport {
    scripts: Arc<Mutex<VecDeque<MockNvidiaStreamScript>>>,
    requests: Arc<Mutex<Vec<RecordedNvidiaSynthesisRequest>>>,
    request_debugs: Arc<Mutex<Vec<String>>>,
    cancellations: Arc<AtomicUsize>,
}

impl MockNvidiaGrpcTransport {
    #[must_use]
    pub fn scripted(scripts: impl IntoIterator<Item = MockNvidiaStreamScript>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn requests(&self) -> Vec<RecordedNvidiaSynthesisRequest> {
        self.requests
            .lock()
            .expect("NVIDIA gRPC mock lock poisoned")
            .clone()
    }

    #[must_use]
    pub fn request_debugs(&self) -> Vec<String> {
        self.request_debugs
            .lock()
            .expect("NVIDIA gRPC mock lock poisoned")
            .clone()
    }

    #[must_use]
    pub fn cancellation_count(&self) -> usize {
        self.cancellations.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl NvidiaNimGrpcTransport for MockNvidiaGrpcTransport {
    async fn synthesize_online(
        &self,
        request: NvidiaSynthesizeOnlineRequest,
    ) -> Result<Box<dyn NvidiaNimSynthesisStream>, NvidiaNvcfError> {
        self.request_debugs
            .lock()
            .expect("NVIDIA gRPC mock lock poisoned")
            .push(format!("{request:?}"));
        self.requests
            .lock()
            .expect("NVIDIA gRPC mock lock poisoned")
            .push(RecordedNvidiaSynthesisRequest {
                authority: request.authority,
                tls_required: request.tls_required,
                method: request.method,
                function_id: request.function_id,
                request_id: request.request_id,
                text_chars: request.text.expose().chars().count(),
                language_code: request.language_code,
                encoding: request.encoding,
                sample_rate_hz: request.sample_rate_hz,
                voice_name: request.voice_name,
                deadline: request.deadline,
            });
        let script = self
            .scripts
            .lock()
            .expect("NVIDIA gRPC mock lock poisoned")
            .pop_front()
            .unwrap_or_default();
        if let Some(error) = script.start_error {
            return Err(error);
        }
        Ok(Box::new(MockNvidiaStream {
            frames: script.frames,
            cancellations: Arc::clone(&self.cancellations),
            cancelled: false,
        }))
    }
}

struct MockNvidiaStream {
    frames: VecDeque<Result<NvidiaSynthesizeFrame, NvidiaNvcfError>>,
    cancellations: Arc<AtomicUsize>,
    cancelled: bool,
}

#[async_trait]
impl NvidiaNimSynthesisStream for MockNvidiaStream {
    async fn next_frame(&mut self) -> Option<Result<NvidiaSynthesizeFrame, NvidiaNvcfError>> {
        if self.cancelled {
            return Some(Err(NvidiaNvcfError::Cancelled));
        }
        self.frames.pop_front()
    }

    async fn cancel(&mut self) -> Result<(), NvidiaNvcfError> {
        if !self.cancelled {
            self.cancelled = true;
            self.cancellations.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}
