//! NVIDIA NIM Magpie multilingual TTS adapter.
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
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderValue, ACCEPT, AUTHORIZATION, RETRY_AFTER},
    Client, StatusCode,
};
use serde_json::Value;
use tokio::time::Instant;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

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
pub const NVIDIA_MAGPIE_DEFAULT_VOICE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

const NVIDIA_MAGPIE_MIN_VOICE_CACHE_TTL: Duration = Duration::from_millis(100);
const NVIDIA_MAGPIE_MAX_VOICE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_NVIDIA_VOICE_LIST_BYTES: usize = 2 * 1_048_576;
const MAX_NVIDIA_VOICE_RECORDS: usize = 4_096;

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
    /// The fixed HTTP discovery route and concrete TLS gRPC `SynthesizeOnline`
    /// transport produced bounded, non-silent, unclipped stock-voice samples.
    /// This is development API evidence, not a production entitlement claim.
    ExperimentalGrpcStreamingQualified,
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

/// Bounded production HTTP transport for Magpie stock-voice discovery.
///
/// The hosted constructor cannot be pointed at another service. The loopback
/// fixture constructor exists solely so contract tests can exercise the real
/// HTTP encoder and decoder without sending credentials over the network.
#[derive(Clone)]
pub struct ReqwestNvidiaNimHttpTransport {
    client: Client,
    target_origin: Url,
}

impl fmt::Debug for ReqwestNvidiaNimHttpTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReqwestNvidiaNimHttpTransport")
            .field("target_origin", &self.target_origin)
            .field("client", &"<HTTP_CLIENT>")
            .finish()
    }
}

impl ReqwestNvidiaNimHttpTransport {
    pub fn new() -> Result<Self, NvidiaNvcfError> {
        let target_origin =
            Url::parse(NVIDIA_MAGPIE_HTTP_ORIGIN).map_err(|_| NvidiaNvcfError::Protocol)?;
        Self::with_target_origin(target_origin)
    }

    /// Builds the concrete transport against an HTTP loopback fixture.
    /// Arbitrary hosts, credentials, URL paths, queries, and fragments fail
    /// closed so this cannot become a configurable production proxy.
    pub fn with_loopback_fixture(target_origin: Url) -> Result<Self, NvidiaNvcfError> {
        if target_origin.scheme() != "http"
            || !target_origin
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|host| host.is_loopback())
            || target_origin.username() != ""
            || target_origin.password().is_some()
            || target_origin.query().is_some()
            || target_origin.fragment().is_some()
            || target_origin.path() != "/"
        {
            return Err(NvidiaNvcfError::Protocol);
        }
        Self::with_target_origin(target_origin)
    }

    fn with_target_origin(target_origin: Url) -> Result<Self, NvidiaNvcfError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("interactive-npcs/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| NvidiaNvcfError::Unavailable)?;
        Ok(Self {
            client,
            target_origin,
        })
    }
}

#[async_trait]
impl NvidiaNimHttpTransport for ReqwestNvidiaNimHttpTransport {
    async fn list_stock_voices(
        &self,
        request: NvidiaVoiceListRequest,
    ) -> Result<Vec<NvidiaStockVoice>, NvidiaNvcfError> {
        if request.origin != NVIDIA_MAGPIE_HTTP_ORIGIN
            || request.path != NVIDIA_MAGPIE_LIST_VOICES_PATH
            || request.deadline < Duration::from_millis(100)
            || request.deadline > Duration::from_secs(120)
        {
            return Err(NvidiaNvcfError::Protocol);
        }

        let (scheme, credential) = request.authorization.expose_parts();
        if scheme != Some("Bearer") || credential.is_empty() {
            return Err(NvidiaNvcfError::Authentication);
        }
        let mut bearer = Zeroizing::new(Vec::with_capacity(7 + credential.len()));
        bearer.extend_from_slice(b"Bearer ");
        bearer.extend_from_slice(credential.as_bytes());
        let mut authorization =
            HeaderValue::from_bytes(&bearer).map_err(|_| NvidiaNvcfError::Authentication)?;
        authorization.set_sensitive(true);
        bearer.zeroize();

        let mut endpoint = self.target_origin.clone();
        endpoint.set_path(NVIDIA_MAGPIE_LIST_VOICES_PATH);
        let response = self
            .client
            .get(endpoint)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, authorization)
            .timeout(request.deadline)
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(classify_http_status(status, retry_after));
        }

        let mut bytes = Vec::new();
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(classify_reqwest_error)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_NVIDIA_VOICE_LIST_BYTES {
                return Err(NvidiaNvcfError::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| NvidiaNvcfError::Protocol)?;
        parse_stock_voices(&value)
    }
}

fn classify_reqwest_error(error: reqwest::Error) -> NvidiaNvcfError {
    if error.is_timeout() {
        NvidiaNvcfError::DeadlineExceeded
    } else {
        NvidiaNvcfError::Unavailable
    }
}

fn classify_http_status(status: StatusCode, retry_after: Option<Duration>) -> NvidiaNvcfError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => NvidiaNvcfError::Authentication,
        StatusCode::TOO_MANY_REQUESTS => NvidiaNvcfError::RateLimited { retry_after },
        StatusCode::NOT_FOUND => NvidiaNvcfError::BadFunction,
        status if status.is_server_error() => NvidiaNvcfError::Unavailable,
        _ => NvidiaNvcfError::Protocol,
    }
}

fn parse_stock_voices(value: &Value) -> Result<Vec<NvidiaStockVoice>, NvidiaNvcfError> {
    let mut records = Vec::new();
    collect_voice_records(value, 0, &mut records)?;
    let mut voices = BTreeMap::new();
    for record in records {
        let Some((id, display_name, locale)) = parse_voice_record(record) else {
            continue;
        };
        if !is_stock_voice_id(&id) {
            continue;
        }
        let voice = NvidiaStockVoice {
            id: id.clone(),
            display_name,
            locale,
            origin: NvidiaVoiceOrigin::ProviderStock,
        };
        voice.validate().map_err(|_| NvidiaNvcfError::Protocol)?;
        voices.entry(id).or_insert(voice);
        if voices.len() > MAX_NVIDIA_VOICE_RECORDS {
            return Err(NvidiaNvcfError::Protocol);
        }
    }
    if voices.is_empty() {
        return Err(NvidiaNvcfError::Protocol);
    }
    Ok(voices.into_values().collect())
}

fn collect_voice_records<'a>(
    value: &'a Value,
    depth: usize,
    records: &mut Vec<&'a Value>,
) -> Result<(), NvidiaNvcfError> {
    if depth > 8 || records.len() > MAX_NVIDIA_VOICE_RECORDS * 2 {
        return Err(NvidiaNvcfError::Protocol);
    }
    match value {
        Value::Array(values) => {
            if records.len().saturating_add(values.len()) > MAX_NVIDIA_VOICE_RECORDS * 2 {
                return Err(NvidiaNvcfError::Protocol);
            }
            records.extend(values);
        }
        Value::Object(object) => {
            if let Some(voices) = object.get("voices") {
                collect_voice_records(voices, depth + 1, records)?;
            } else {
                for child in object.values() {
                    if child.is_object() || child.is_array() {
                        collect_voice_records(child, depth + 1, records)?;
                    }
                }
            }
        }
        _ => {}
    }
    if records.len() > MAX_NVIDIA_VOICE_RECORDS * 2 {
        return Err(NvidiaNvcfError::Protocol);
    }
    Ok(())
}

fn parse_voice_record(record: &Value) -> Option<(String, String, String)> {
    let (id, display_name, locale) = match record {
        Value::String(id) => (id.as_str(), None, None),
        Value::Object(object) => {
            let id = ["name", "voice", "voice_id"]
                .into_iter()
                .find_map(|key| object.get(key).and_then(Value::as_str))?;
            let display_name = object.get("display_name").and_then(Value::as_str);
            let locale = ["locale", "language", "language_code"]
                .into_iter()
                .find_map(|key| object.get(key).and_then(Value::as_str));
            (id, display_name, locale)
        }
        _ => return None,
    };
    let inferred_locale = infer_voice_locale(id)?;
    let inferred_name = id.rsplit('.').next()?;
    Some((
        id.to_owned(),
        display_name.unwrap_or(inferred_name).to_owned(),
        locale.unwrap_or(&inferred_locale).to_owned(),
    ))
}

fn infer_voice_locale(id: &str) -> Option<String> {
    id.split('.').find_map(|part| {
        let (language, region) = part.split_once('-')?;
        (language.len() == 2
            && region.len() == 2
            && language.chars().all(|value| value.is_ascii_alphabetic())
            && region.chars().all(|value| value.is_ascii_alphabetic()))
        .then(|| {
            format!(
                "{}-{}",
                language.to_ascii_lowercase(),
                region.to_ascii_uppercase()
            )
        })
    })
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
    stock_voices: Arc<RwLock<NvidiaVoiceCache>>,
    discovery_gate: Arc<tokio::sync::Mutex<()>>,
    voice_cache_ttl: Duration,
    request_deadline: Duration,
}

#[derive(Clone, Debug, Default)]
struct NvidiaVoiceCache {
    voices: BTreeMap<String, NvidiaStockVoice>,
    expires_at: Option<Instant>,
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
            stock_voices: Arc::new(RwLock::new(NvidiaVoiceCache::default())),
            discovery_gate: Arc::new(tokio::sync::Mutex::new(())),
            voice_cache_ttl: NVIDIA_MAGPIE_DEFAULT_VOICE_CACHE_TTL,
            request_deadline: Duration::from_secs(30),
        }
    }

    #[must_use]
    pub fn lifecycle(&self) -> NvidiaMagpieLifecycle {
        NvidiaMagpieLifecycle::ExperimentalGrpcStreamingQualified
    }

    pub fn set_request_deadline(&mut self, deadline: Duration) -> Result<(), &'static str> {
        if deadline < Duration::from_millis(100) || deadline > Duration::from_secs(120) {
            return Err("invalid_nvidia_deadline");
        }
        self.request_deadline = deadline;
        Ok(())
    }

    pub fn set_voice_cache_ttl(&mut self, ttl: Duration) -> Result<(), &'static str> {
        if !(NVIDIA_MAGPIE_MIN_VOICE_CACHE_TTL..=NVIDIA_MAGPIE_MAX_VOICE_CACHE_TTL).contains(&ttl) {
            return Err("invalid_nvidia_voice_cache_ttl");
        }
        self.voice_cache_ttl = ttl;
        Ok(())
    }

    /// Uses the fixed function-id subdomain and installs only validated stock
    /// voices. The credential exists only inside this request.
    pub async fn discover_stock_voices(&self) -> Result<Vec<NvidiaStockVoice>, TtsError> {
        if let Some(voices) = self.fresh_cached_voices() {
            return Ok(voices);
        }
        let _gate = tokio::time::timeout(self.request_deadline, self.discovery_gate.lock())
            .await
            .map_err(|_| map_nvidia_error(NvidiaNvcfError::DeadlineExceeded))?;
        if let Some(voices) = self.fresh_cached_voices() {
            return Ok(voices);
        }
        let provider_id = HostedTtsProviderId::NvidiaNimMagpie;
        let credential = resolve_credential_with_deadline(
            &self.credential_resolver,
            provider_id,
            self.request_deadline,
        )
        .await?;
        let voices = tokio::time::timeout(
            self.request_deadline,
            self.http.list_stock_voices(NvidiaVoiceListRequest {
                origin: NVIDIA_MAGPIE_HTTP_ORIGIN,
                path: NVIDIA_MAGPIE_LIST_VOICES_PATH,
                authorization: SensitiveHeaderValue::with_scheme("Bearer", credential),
                deadline: self.request_deadline,
            }),
        )
        .await
        .map_err(|_| map_nvidia_error(NvidiaNvcfError::DeadlineExceeded))?
        .map_err(map_nvidia_error)?;
        if voices.is_empty() || voices.len() > MAX_NVIDIA_VOICE_RECORDS {
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
            .expect("NVIDIA stock voice lock poisoned") = NvidiaVoiceCache {
            voices: validated,
            expires_at: Some(Instant::now() + self.voice_cache_ttl),
        };
        Ok(voices)
    }

    fn fresh_cached_voices(&self) -> Option<Vec<NvidiaStockVoice>> {
        let cache = self
            .stock_voices
            .read()
            .expect("NVIDIA stock voice lock poisoned");
        cache
            .expires_at
            .filter(|expires_at| *expires_at > Instant::now())
            .map(|_| cache.voices.values().cloned().collect())
    }

    #[must_use]
    pub fn discovered_stock_voices(&self) -> Vec<NvidiaStockVoice> {
        self.stock_voices
            .read()
            .expect("NVIDIA stock voice lock poisoned")
            .voices
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
            .voices
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
        let credential =
            resolve_credential_with_deadline(&self.credential_resolver, provider_id, self.deadline)
                .await?;
        let request_id = format!(
            "{}:{}:{}:{}",
            self.request.identity.session_id,
            self.request.identity.turn_id,
            self.request.identity.cancellation_generation,
            self.clause_sequence
        );
        self.clause_sequence = self.clause_sequence.saturating_add(1);
        let stream = tokio::time::timeout(
            self.deadline,
            self.grpc.synthesize_online(NvidiaSynthesizeOnlineRequest {
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
            }),
        )
        .await
        .map_err(|_| map_nvidia_error(NvidiaNvcfError::DeadlineExceeded))?
        .map_err(map_nvidia_error)?;
        self.streams.push_back(stream);
        self.utterance_started = true;
        Ok(())
    }

    async fn protocol_fault(&mut self, code: &'static str) -> Option<Result<TtsEvent, TtsError>> {
        self.state = SessionState::Faulted;
        while let Some(mut stream) = self.streams.pop_front() {
            let _ignored = tokio::time::timeout(self.deadline, stream.cancel()).await;
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
            let next_frame = match tokio::time::timeout(self.deadline, stream.next_frame()).await {
                Ok(frame) => frame,
                Err(_) => {
                    self.state = SessionState::Faulted;
                    return Some(Err(map_nvidia_error(NvidiaNvcfError::DeadlineExceeded)));
                }
            };
            match next_frame {
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
            match tokio::time::timeout(self.deadline, stream.cancel()).await {
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    first_error.get_or_insert(NvidiaNvcfError::DeadlineExceeded);
                }
                Ok(Ok(())) => {}
            }
        }
        self.state = SessionState::Cancelled;
        self.pending_events
            .push_back(TtsEvent::Interrupted { reason: "barge_in" });
        first_error.map(map_nvidia_error).map_or(Ok(()), Err)
    }
}

async fn resolve_credential_with_deadline(
    resolver: &Arc<dyn ProviderCredentialResolver>,
    provider_id: HostedTtsProviderId,
    deadline: Duration,
) -> Result<SensitiveString, TtsError> {
    let credential = tokio::time::timeout(deadline, resolver.resolve(provider_id))
        .await
        .map_err(|_| map_nvidia_error(NvidiaNvcfError::DeadlineExceeded))?
        .map_err(|error| {
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
