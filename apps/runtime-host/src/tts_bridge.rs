//! Trusted adaptation between hosted streaming TTS and the runtime turn engine.
//!
//! The bridge deliberately creates one provider session for every complete
//! runtime sentence. This keeps cancellation and delivery accounting scoped to
//! one sentence and prevents provider state from leaking into a later sentence.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use interactive_npcs_credential_vault::{CredentialVault, SecretValue, VaultError};
use npc_providers_tts::{
    AudioFormat, CredentialResolveError, HostedTtsProviderId, NvidiaNimMagpie, NvidiaStockVoice,
    PcmChunk, PcmEncoding, ProviderCredentialResolver, ReqwestNvidiaNimHttpTransport,
    SemanticClausePolicy, SensitiveString, SessionIdentity, StreamingTtsProvider,
    StreamingTtsSession, TonicNvidiaNimGrpcTransport, TtsError, TtsErrorKind, TtsEvent,
    TtsSessionRequest, VoiceBindings, NVIDIA_MAGPIE_MODEL_ID,
};
use npc_runtime_core::{
    AlignmentEvent, AudioChunk, DataClass, ProviderDescriptor, ProviderError, ProviderErrorKind,
    ProviderLocation, ProviderModality, SpeechRequest, SpeechStream, SpeechStreamItem, TtsProvider,
    TtsSession, TurnIdentity,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub const ELEVENLABS_CREDENTIAL_TARGET: &str = "providers/elevenlabs";
pub const NVIDIA_NIM_CREDENTIAL_TARGET: &str = "providers/nvidia-nim";
const REQUIRED_CHANNELS: u16 = 1;
const VOICE_DISCOVERY_SCHEMA_VERSION: u32 = 1;
const MAX_DISCOVERED_STOCK_VOICES: usize = 1024;
const RUNTIME_VOICE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TtsVoiceDiscoveryRequest {
    pub schema_version: u32,
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub force_refresh: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TtsVoiceDiscoveryStatus {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TtsVoiceProvenance {
    ProviderStockDiscovery,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredStockVoice {
    pub voice_id: String,
    pub display_name: String,
    pub language: String,
    pub styles: Vec<String>,
    pub provenance: TtsVoiceProvenance,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoiceRefreshEvidence {
    pub requested: bool,
    pub performed: bool,
    pub cache_hit: bool,
    pub refreshed_at_epoch_ms: Option<u64>,
    pub expires_at_epoch_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoiceDiscoveryError {
    pub code: String,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoiceDiscoveryResult {
    pub schema_version: u32,
    pub provider_id: String,
    pub model_id: String,
    pub status: TtsVoiceDiscoveryStatus,
    pub voices: Vec<DiscoveredStockVoice>,
    pub provenance: TtsVoiceProvenance,
    pub refresh: TtsVoiceRefreshEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TtsVoiceDiscoveryError>,
}

#[async_trait]
trait RuntimeStockVoiceSource: Send + Sync {
    async fn discover(&self) -> Result<Vec<NvidiaStockVoice>, TtsError>;
}

struct HostedNvidiaStockVoiceSource {
    vault: Arc<dyn CredentialVault>,
}

#[async_trait]
impl RuntimeStockVoiceSource for HostedNvidiaStockVoiceSource {
    async fn discover(&self) -> Result<Vec<NvidiaStockVoice>, TtsError> {
        let grpc = TonicNvidiaNimGrpcTransport::connect().await.map_err(|_| {
            TtsError::new(
                HostedTtsProviderId::NvidiaNimMagpie.as_str(),
                TtsErrorKind::Unavailable,
                "nvidia_grpc_transport_unavailable",
                true,
            )
        })?;
        let http = ReqwestNvidiaNimHttpTransport::new().map_err(|_| {
            TtsError::new(
                HostedTtsProviderId::NvidiaNimMagpie.as_str(),
                TtsErrorKind::Unavailable,
                "nvidia_http_transport_unavailable",
                true,
            )
        })?;
        let provider = NvidiaNimMagpie::new(
            Arc::new(grpc),
            Arc::new(http),
            Arc::new(VaultTtsCredentialResolver::new(Arc::clone(&self.vault))),
            VoiceBindings::default(),
        );
        provider.discover_stock_voices().await
    }
}

#[derive(Clone)]
struct CachedStockVoices {
    voices: Vec<DiscoveredStockVoice>,
    refreshed_at_epoch_ms: u64,
    expires_at_epoch_ms: u64,
    expires_at: tokio::time::Instant,
}

pub struct TtsVoiceDiscoveryService {
    source: Arc<dyn RuntimeStockVoiceSource>,
    cache: tokio::sync::Mutex<Option<CachedStockVoices>>,
}

impl TtsVoiceDiscoveryService {
    #[must_use]
    pub fn hosted(vault: Arc<dyn CredentialVault>) -> Self {
        Self {
            source: Arc::new(HostedNvidiaStockVoiceSource { vault }),
            cache: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn discover(&self, request: TtsVoiceDiscoveryRequest) -> TtsVoiceDiscoveryResult {
        let requested = request.force_refresh;
        if request.schema_version != VOICE_DISCOVERY_SCHEMA_VERSION
            || request.provider_id != HostedTtsProviderId::NvidiaNimMagpie.as_str()
            || request.model_id != NVIDIA_MAGPIE_MODEL_ID
        {
            return discovery_failure(
                request,
                "unsupported_tts_voice_route",
                "Voice discovery is available only for the private-evaluation NVIDIA Magpie stock route.",
                false,
            );
        }
        let mut cache = self.cache.lock().await;
        if !request.force_refresh {
            if let Some(cached) = cache
                .as_ref()
                .filter(|cached| cached.expires_at > tokio::time::Instant::now())
            {
                return discovery_success(request, cached.clone(), requested, false, true);
            }
        }

        let voices = match self.source.discover().await {
            Ok(voices) => voices,
            Err(error) => {
                return discovery_failure(
                    request,
                    error.code,
                    "The provider stock-voice catalog could not be refreshed.",
                    error.retryable,
                )
            }
        };
        if voices.is_empty() || voices.len() > MAX_DISCOVERED_STOCK_VOICES {
            return discovery_failure(
                request,
                "invalid_stock_voice_catalog",
                "The provider returned an empty or oversized stock-voice catalog.",
                true,
            );
        }
        let mut mapped = Vec::with_capacity(voices.len());
        let mut ids = std::collections::BTreeSet::new();
        for voice in voices {
            if voice.validate().is_err() || !ids.insert(voice.id.clone()) {
                return discovery_failure(
                    request,
                    "invalid_stock_voice_catalog",
                    "The provider returned an invalid stock-voice catalog.",
                    true,
                );
            }
            mapped.push(DiscoveredStockVoice {
                voice_id: voice.id,
                display_name: voice.display_name,
                language: voice.locale,
                styles: Vec::new(),
                provenance: TtsVoiceProvenance::ProviderStockDiscovery,
            });
        }
        mapped.sort_by(|left, right| left.voice_id.cmp(&right.voice_id));
        let refreshed_at_epoch_ms = unix_epoch_ms();
        let expires_at_epoch_ms =
            refreshed_at_epoch_ms.saturating_add(RUNTIME_VOICE_CACHE_TTL.as_millis() as u64);
        let cached = CachedStockVoices {
            voices: mapped,
            refreshed_at_epoch_ms,
            expires_at_epoch_ms,
            expires_at: tokio::time::Instant::now() + RUNTIME_VOICE_CACHE_TTL,
        };
        *cache = Some(cached.clone());
        discovery_success(request, cached, requested, true, false)
    }
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn discovery_success(
    request: TtsVoiceDiscoveryRequest,
    cached: CachedStockVoices,
    requested: bool,
    performed: bool,
    cache_hit: bool,
) -> TtsVoiceDiscoveryResult {
    TtsVoiceDiscoveryResult {
        schema_version: VOICE_DISCOVERY_SCHEMA_VERSION,
        provider_id: request.provider_id,
        model_id: request.model_id,
        status: TtsVoiceDiscoveryStatus::Available,
        voices: cached.voices,
        provenance: TtsVoiceProvenance::ProviderStockDiscovery,
        refresh: TtsVoiceRefreshEvidence {
            requested,
            performed,
            cache_hit,
            refreshed_at_epoch_ms: Some(cached.refreshed_at_epoch_ms),
            expires_at_epoch_ms: Some(cached.expires_at_epoch_ms),
        },
        error: None,
    }
}

fn discovery_failure(
    request: TtsVoiceDiscoveryRequest,
    code: impl Into<String>,
    detail: impl Into<String>,
    retryable: bool,
) -> TtsVoiceDiscoveryResult {
    TtsVoiceDiscoveryResult {
        schema_version: VOICE_DISCOVERY_SCHEMA_VERSION,
        provider_id: request.provider_id,
        model_id: request.model_id,
        status: TtsVoiceDiscoveryStatus::Unavailable,
        voices: Vec::new(),
        provenance: TtsVoiceProvenance::ProviderStockDiscovery,
        refresh: TtsVoiceRefreshEvidence {
            requested: request.force_refresh,
            performed: false,
            cache_hit: false,
            refreshed_at_epoch_ms: None,
            expires_at_epoch_ms: None,
        },
        error: Some(TtsVoiceDiscoveryError {
            code: code.into(),
            detail: detail.into(),
            retryable,
        }),
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeTtsBridgeConfig {
    pub descriptor: ProviderDescriptor,
    pub voice_intent_id: String,
    pub output: AudioFormat,
    pub request_alignment: bool,
    pub request_visemes: bool,
    pub clause_policy: SemanticClausePolicy,
}

impl RuntimeTtsBridgeConfig {
    #[must_use]
    pub fn dev_elevenlabs_stock(descriptor: ProviderDescriptor) -> Self {
        Self {
            descriptor,
            voice_intent_id: "dev.elevenlabs.stock".to_owned(),
            output: AudioFormat::default(),
            request_alignment: true,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
        }
    }

    #[must_use]
    pub fn selected_stock(
        descriptor: ProviderDescriptor,
        voice_intent_id: impl Into<String>,
        output: AudioFormat,
    ) -> Self {
        Self {
            descriptor,
            voice_intent_id: voice_intent_id.into(),
            output,
            request_alignment: true,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
        }
    }

    fn validate(&self, provider_id: HostedTtsProviderId) -> Result<(), BridgeConfigError> {
        if self.descriptor.id != provider_id.as_str() {
            return Err(BridgeConfigError::ProviderIdMismatch);
        }
        if self.descriptor.modality != ProviderModality::Speech {
            return Err(BridgeConfigError::WrongModality);
        }
        match &self.descriptor.location {
            ProviderLocation::Cloud { service } if service == provider_id.as_str() => {}
            ProviderLocation::Cloud { .. } => return Err(BridgeConfigError::CloudServiceMismatch),
            ProviderLocation::Local | ProviderLocation::ExternalLocalServer => {
                return Err(BridgeConfigError::HostedProviderMarkedLocal);
            }
        }
        if !self.descriptor.may_retain_data {
            return Err(BridgeConfigError::RetentionNotDeclared);
        }
        if self.descriptor.transmitted_data.as_slice() != [DataClass::Transcript] {
            return Err(BridgeConfigError::TranscriptEgressNotDeclared);
        }
        if self.voice_intent_id.trim().is_empty() || self.voice_intent_id.len() > 128 {
            return Err(BridgeConfigError::InvalidVoiceIntent);
        }
        if self.output.encoding != PcmEncoding::PcmS16Le
            || !(8_000..=96_000).contains(&self.output.sample_rate_hz)
            || self.output.channels != REQUIRED_CHANNELS
        {
            return Err(BridgeConfigError::UnsupportedOutput);
        }
        self.clause_policy
            .validate()
            .map_err(|_| BridgeConfigError::InvalidClausePolicy)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BridgeConfigError {
    #[error("provider descriptor id does not match the upstream TTS provider")]
    ProviderIdMismatch,
    #[error("provider descriptor modality must be speech")]
    WrongModality,
    #[error("hosted TTS provider cannot be described as local")]
    HostedProviderMarkedLocal,
    #[error("cloud service id does not match the hosted TTS provider")]
    CloudServiceMismatch,
    #[error("hosted TTS descriptor must conservatively declare possible retention")]
    RetentionNotDeclared,
    #[error("hosted TTS descriptor must declare transcript-only egress")]
    TranscriptEgressNotDeclared,
    #[error("voice intent id is invalid")]
    InvalidVoiceIntent,
    #[error("runtime speech output must be curated mono PCM s16le")]
    UnsupportedOutput,
    #[error("semantic clause policy is invalid")]
    InvalidClausePolicy,
}

pub struct RuntimeTtsBridge {
    upstream: Arc<dyn StreamingTtsProvider>,
    config: RuntimeTtsBridgeConfig,
}

impl RuntimeTtsBridge {
    pub fn new(
        upstream: Arc<dyn StreamingTtsProvider>,
        mut config: RuntimeTtsBridgeConfig,
    ) -> Result<Self, BridgeConfigError> {
        config.validate(upstream.id())?;
        config.descriptor = canonical_descriptor(upstream.as_ref());
        Ok(Self { upstream, config })
    }
}

fn canonical_descriptor(provider: &dyn StreamingTtsProvider) -> ProviderDescriptor {
    let provider_id = provider.id();
    let capabilities = provider.capabilities();
    ProviderDescriptor {
        id: provider_id.as_str().to_owned(),
        display_name: match provider_id {
            HostedTtsProviderId::Cartesia => "Cartesia",
            HostedTtsProviderId::ElevenLabs => "ElevenLabs",
            HostedTtsProviderId::Inworld => "Inworld",
            HostedTtsProviderId::Deepgram => "Deepgram",
            HostedTtsProviderId::NvidiaNimMagpie => "NVIDIA NIM Magpie",
        }
        .to_owned(),
        modality: ProviderModality::Speech,
        location: ProviderLocation::Cloud {
            service: provider_id.as_str().to_owned(),
        },
        // The runtime must conservatively route hosted providers as retaining.
        // More permissive policy requires a separately reviewed privacy contract.
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::from([
            (
                "streaming_input".to_owned(),
                capabilities.streaming_input.to_string(),
            ),
            (
                "streaming_pcm".to_owned(),
                capabilities.streaming_pcm.to_string(),
            ),
            ("alignment".to_owned(), capabilities.alignment.to_string()),
            (
                "visemes_or_phonemes".to_owned(),
                capabilities.visemes_or_phonemes.to_string(),
            ),
            (
                "cancellation".to_owned(),
                capabilities.cancellation.to_string(),
            ),
            ("usage".to_owned(), capabilities.usage.to_string()),
        ]),
    }
}

struct RuntimeTtsSession {
    upstream: Arc<dyn StreamingTtsProvider>,
    config: RuntimeTtsBridgeConfig,
    identity: TurnIdentity,
    locale: String,
}

#[async_trait]
impl TtsProvider for RuntimeTtsBridge {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.config.descriptor
    }

    async fn start_session(
        &self,
        identity: &TurnIdentity,
        locale: &str,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn TtsSession>, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(&self.config.descriptor.id));
        }
        if locale.trim().is_empty() || locale.len() > 35 {
            return Err(bridge_error(
                &self.config.descriptor.id,
                ProviderErrorKind::InvalidRequest,
                "invalid_locale",
                false,
            ));
        }
        Ok(Box::new(RuntimeTtsSession {
            upstream: Arc::clone(&self.upstream),
            config: self.config.clone(),
            identity: identity.clone(),
            locale: locale.to_owned(),
        }))
    }
}

#[async_trait]
impl TtsSession for RuntimeTtsSession {
    fn provider(&self) -> &ProviderDescriptor {
        &self.config.descriptor
    }

    async fn synthesize(
        &mut self,
        request: SpeechRequest,
        cancellation: CancellationToken,
    ) -> Result<SpeechStream, ProviderError> {
        let provider_id = self.config.descriptor.id.clone();
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled(provider_id));
        }
        if request.identity != self.identity || request.locale != self.locale {
            return Err(bridge_error(
                &provider_id,
                ProviderErrorKind::InvalidRequest,
                "sentence_session_identity_mismatch",
                false,
            ));
        }
        if request.text.trim().is_empty() {
            return Err(bridge_error(
                &provider_id,
                ProviderErrorKind::InvalidRequest,
                "empty_sentence",
                false,
            ));
        }

        let upstream_request = TtsSessionRequest {
            identity: SessionIdentity {
                session_id: request.identity.session_id.clone(),
                turn_id: request.identity.turn_id.clone(),
                cancellation_generation: request.identity.cancellation_generation,
            },
            locale: request.locale.clone(),
            voice_intent_id: self.config.voice_intent_id.clone(),
            output: self.config.output,
            request_alignment: self.config.request_alignment,
            request_visemes: self.config.request_visemes,
            clause_policy: self.config.clause_policy.clone(),
        };

        let start = self.upstream.start_session(upstream_request);
        tokio::pin!(start);
        let mut upstream = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(ProviderError::cancelled(provider_id)),
            result = &mut start => result.map_err(|error| map_upstream_error(&provider_id, error))?,
        };

        let pushed = {
            let push = upstream.push_text(&request.text);
            tokio::pin!(push);
            tokio::select! {
                biased;
                () = cancellation.cancelled() => None,
                result = &mut push => Some(result),
            }
        };
        let Some(pushed) = pushed else {
            cancel_authoritatively(upstream.as_mut()).await;
            return Err(ProviderError::cancelled(provider_id));
        };
        let pushed = match pushed {
            Ok(pushed) => pushed,
            Err(error) => {
                cancel_authoritatively(upstream.as_mut()).await;
                return Err(map_upstream_error(&provider_id, error));
            }
        };
        if pushed.accepted_chars != request.text.chars().count() {
            cancel_authoritatively(upstream.as_mut()).await;
            return Err(bridge_error(
                &provider_id,
                ProviderErrorKind::Protocol,
                "sentence_not_fully_accepted",
                false,
            ));
        }

        let finished = {
            let finish = upstream.finish();
            tokio::pin!(finish);
            tokio::select! {
                biased;
                () = cancellation.cancelled() => None,
                result = &mut finish => Some(result),
            }
        };
        let Some(finished) = finished else {
            cancel_authoritatively(upstream.as_mut()).await;
            return Err(ProviderError::cancelled(provider_id));
        };
        if let Err(error) = finished {
            cancel_authoritatively(upstream.as_mut()).await;
            return Err(map_upstream_error(&provider_id, error));
        }

        Ok(validated_stream(
            upstream,
            cancellation,
            provider_id,
            self.config.output,
        ))
    }
}

fn validated_stream(
    mut upstream: Box<dyn StreamingTtsSession>,
    cancellation: CancellationToken,
    provider_id: String,
    expected_output: AudioFormat,
) -> SpeechStream {
    Box::pin(async_stream::stream! {
        let mut expected_sequence = 0_u64;
        let mut pending_audio: Option<PcmChunk> = None;
        let mut pending_metadata = VecDeque::new();
        let mut has_non_silent_sample = false;

        loop {
            let event = tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    cancel_authoritatively(upstream.as_mut()).await;
                    yield Err(ProviderError::cancelled(provider_id.clone()));
                    break;
                }
                event = upstream.next_event() => event,
            };

            let Some(event) = event else {
                yield Err(bridge_error(
                    &provider_id,
                    ProviderErrorKind::Protocol,
                    "missing_completed_event",
                    false,
                ));
                break;
            };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    cancel_authoritatively(upstream.as_mut()).await;
                    yield Err(map_upstream_error(&provider_id, error));
                    break;
                }
            };

            match event {
                TtsEvent::Audio(chunk) => {
                    if let Err(error) = validate_pcm_chunk(
                        &provider_id,
                        &chunk,
                        expected_sequence,
                        expected_output,
                    ) {
                        cancel_authoritatively(upstream.as_mut()).await;
                        yield Err(error);
                        break;
                    }
                    expected_sequence = expected_sequence.saturating_add(1);
                    has_non_silent_sample |= chunk
                        .data
                        .chunks_exact(2)
                        .any(|sample| i16::from_le_bytes([sample[0], sample[1]]) != 0);

                    if let Some(previous) = pending_audio.replace(chunk) {
                        yield Ok(SpeechStreamItem::Audio(core_audio(previous, false)));
                        while let Some(metadata) = pending_metadata.pop_front() {
                            yield Ok(metadata);
                        }
                    }
                }
                TtsEvent::Alignment(words) => {
                    pending_metadata.extend(words.into_iter().map(|word| {
                        SpeechStreamItem::Alignment(AlignmentEvent {
                            text_offset: word.source_text_start.unwrap_or(0),
                            text_length: word.source_text_length.unwrap_or(word.word.len()),
                            audio_offset: Duration::from_millis(word.start_ms),
                            viseme: None,
                        })
                    }));
                }
                TtsEvent::Viseme(visemes) => {
                    pending_metadata.extend(visemes.into_iter().map(|viseme| {
                        SpeechStreamItem::Alignment(AlignmentEvent {
                            text_offset: 0,
                            text_length: 0,
                            audio_offset: Duration::from_millis(viseme.start_ms),
                            viseme: Some(viseme.symbol),
                        })
                    }));
                }
                TtsEvent::Usage(_) => {}
                TtsEvent::Interrupted { .. } => {
                    yield Err(ProviderError::cancelled(provider_id.clone()));
                    break;
                }
                TtsEvent::Completed => {
                    let Some(last) = pending_audio.take() else {
                        yield Err(bridge_error(
                            &provider_id,
                            ProviderErrorKind::Protocol,
                            "empty_audio_stream",
                            false,
                        ));
                        break;
                    };
                    if !has_non_silent_sample {
                        yield Err(bridge_error(
                            &provider_id,
                            ProviderErrorKind::Protocol,
                            "silent_audio_stream",
                            false,
                        ));
                        break;
                    }
                    while let Some(metadata) = pending_metadata.pop_front() {
                        yield Ok(metadata);
                    }
                    yield Ok(SpeechStreamItem::Audio(core_audio(last, true)));
                    break;
                }
            }
        }
    })
}

fn validate_pcm_chunk(
    provider_id: &str,
    chunk: &PcmChunk,
    expected_sequence: u64,
    expected_output: AudioFormat,
) -> Result<(), ProviderError> {
    if chunk.sequence != expected_sequence {
        return Err(bridge_error(
            provider_id,
            ProviderErrorKind::Protocol,
            "audio_sequence_discontinuity",
            false,
        ));
    }
    if chunk.format != expected_output {
        return Err(bridge_error(
            provider_id,
            ProviderErrorKind::Protocol,
            "unexpected_audio_format",
            false,
        ));
    }
    if chunk.data.is_empty() || !chunk.data.len().is_multiple_of(2) {
        return Err(bridge_error(
            provider_id,
            ProviderErrorKind::Protocol,
            "invalid_pcm_payload",
            false,
        ));
    }
    Ok(())
}

fn core_audio(chunk: PcmChunk, end_of_stream: bool) -> AudioChunk {
    AudioChunk {
        // Hosted TTS transports use a zero-based sequence internally, while
        // every runtime audio sink reserves zero as an invalid/uninitialized
        // wire value. Normalize exactly once at this trust boundary.
        sequence: chunk.sequence.saturating_add(1),
        sample_rate_hz: chunk.format.sample_rate_hz,
        channels: chunk.format.channels,
        pcm_s16le: chunk.data,
        end_of_stream,
    }
}

async fn cancel_authoritatively(session: &mut dyn StreamingTtsSession) {
    let _ = session.cancel().await;
}

fn map_upstream_error(trusted_provider_id: &str, error: TtsError) -> ProviderError {
    let kind = match error.kind {
        TtsErrorKind::Cancelled => ProviderErrorKind::Cancelled,
        TtsErrorKind::InvalidState | TtsErrorKind::InvalidRequest => {
            ProviderErrorKind::InvalidRequest
        }
        TtsErrorKind::Authentication => ProviderErrorKind::Authentication,
        TtsErrorKind::QuotaExceeded | TtsErrorKind::RateLimited => ProviderErrorKind::RateLimited,
        TtsErrorKind::Timeout => ProviderErrorKind::Timeout,
        TtsErrorKind::Unavailable | TtsErrorKind::Transport => ProviderErrorKind::Unavailable,
        TtsErrorKind::Protocol => ProviderErrorKind::Protocol,
    };
    ProviderError {
        provider_id: trusted_provider_id.to_owned(),
        kind,
        message: error.code.to_owned(),
        retryable: error.retryable,
        retry_after: error.retry_after,
    }
}

fn bridge_error(
    provider_id: &str,
    kind: ProviderErrorKind,
    code: &'static str,
    retryable: bool,
) -> ProviderError {
    ProviderError {
        provider_id: provider_id.to_owned(),
        kind,
        message: code.to_owned(),
        retryable,
        retry_after: None,
    }
}

/// Credential adapter for the curated ElevenLabs route. The target is a
/// compile-time constant; neither the WebView nor runtime configuration can
/// redirect credential reads to an arbitrary vault entry.
pub struct VaultTtsCredentialResolver {
    vault: Arc<dyn CredentialVault>,
}

impl VaultTtsCredentialResolver {
    #[must_use]
    pub fn new(vault: Arc<dyn CredentialVault>) -> Self {
        Self { vault }
    }
}

#[async_trait]
impl ProviderCredentialResolver for VaultTtsCredentialResolver {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        let target = match provider_id {
            HostedTtsProviderId::ElevenLabs => ELEVENLABS_CREDENTIAL_TARGET,
            HostedTtsProviderId::NvidiaNimMagpie => NVIDIA_NIM_CREDENTIAL_TARGET,
            HostedTtsProviderId::Cartesia
            | HostedTtsProviderId::Inworld
            | HostedTtsProviderId::Deepgram => return Err(CredentialResolveError::Missing),
        };
        let secret = self.vault.get(target).map_err(map_vault_error)?;
        sensitive_utf8_from_vault(secret)
    }
}

fn sensitive_utf8_from_vault(
    secret: SecretValue,
) -> Result<SensitiveString, CredentialResolveError> {
    let value =
        std::str::from_utf8(secret.expose()).map_err(|_| CredentialResolveError::Unavailable)?;
    // Copy directly into the provider's zeroizing container. No ordinary
    // heap-owned String exists in the trusted bridge, and the consumed vault
    // value zeroizes its original bytes when this function returns.
    Ok(SensitiveString::new(value))
}

fn map_vault_error(error: VaultError) -> CredentialResolveError {
    match error {
        VaultError::NotFound => CredentialResolveError::Missing,
        VaultError::EmptyTarget
        | VaultError::TargetTooLong
        | VaultError::InvalidTarget
        | VaultError::EmptySecret
        | VaultError::SecretTooLarge(_)
        | VaultError::System(_)
        | VaultError::Poisoned => CredentialResolveError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, VecDeque},
        future::pending,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

    use super::*;
    use futures_util::{StreamExt, TryStreamExt};
    use interactive_npcs_credential_vault::{SecretValue, VaultError};
    use npc_providers_tts::{
        NvidiaVoiceOrigin, ProviderCapabilities, PushOutcome, SessionState, TtsEvent, WordAlignment,
    };

    #[derive(Clone)]
    struct FakeSessionPlan {
        events: VecDeque<Result<TtsEvent, TtsError>>,
        pending_when_exhausted: bool,
    }

    impl FakeSessionPlan {
        fn events(events: impl IntoIterator<Item = TtsEvent>) -> Self {
            Self {
                events: events.into_iter().map(Ok).collect(),
                pending_when_exhausted: false,
            }
        }
    }

    struct FakeProviderState {
        plans: Mutex<VecDeque<FakeSessionPlan>>,
        requests: Mutex<Vec<TtsSessionRequest>>,
        starts: AtomicUsize,
        cancels: AtomicUsize,
    }

    struct FakeProvider {
        state: Arc<FakeProviderState>,
    }

    struct FakeSession {
        identity: SessionIdentity,
        state: Arc<FakeProviderState>,
        events: VecDeque<Result<TtsEvent, TtsError>>,
        pending_when_exhausted: bool,
        lifecycle: SessionState,
    }

    #[async_trait]
    impl StreamingTtsProvider for FakeProvider {
        fn id(&self) -> HostedTtsProviderId {
            HostedTtsProviderId::ElevenLabs
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                streaming_input: true,
                streaming_pcm: true,
                alignment: true,
                visemes_or_phonemes: true,
                cancellation: true,
                usage: true,
            }
        }

        async fn start_session(
            &self,
            request: TtsSessionRequest,
        ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
            self.state.starts.fetch_add(1, Ordering::SeqCst);
            self.state
                .requests
                .lock()
                .expect("request mutex poisoned")
                .push(request.clone());
            let plan = self
                .state
                .plans
                .lock()
                .expect("plan mutex poisoned")
                .pop_front()
                .expect("fake session plan missing");
            Ok(Box::new(FakeSession {
                identity: request.identity,
                state: Arc::clone(&self.state),
                events: plan.events,
                pending_when_exhausted: plan.pending_when_exhausted,
                lifecycle: SessionState::AcceptingText,
            }))
        }
    }

    #[async_trait]
    impl StreamingTtsSession for FakeSession {
        fn provider_id(&self) -> HostedTtsProviderId {
            HostedTtsProviderId::ElevenLabs
        }

        fn identity(&self) -> &SessionIdentity {
            &self.identity
        }

        fn state(&self) -> SessionState {
            self.lifecycle
        }

        fn has_started_utterance(&self) -> bool {
            true
        }

        async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError> {
            Ok(PushOutcome {
                accepted_chars: text.chars().count(),
                clauses_submitted: 1,
                buffered_chars: 0,
            })
        }

        async fn finish(&mut self) -> Result<(), TtsError> {
            self.lifecycle = SessionState::Finishing;
            Ok(())
        }

        async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
            if let Some(event) = self.events.pop_front() {
                return Some(event);
            }
            if self.pending_when_exhausted {
                pending().await
            } else {
                None
            }
        }

        async fn cancel(&mut self) -> Result<(), TtsError> {
            self.lifecycle = SessionState::Cancelled;
            self.state.cancels.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn descriptor() -> ProviderDescriptor {
        ProviderDescriptor {
            id: "elevenlabs".to_owned(),
            display_name: "ElevenLabs fixture".to_owned(),
            modality: ProviderModality::Speech,
            location: ProviderLocation::Cloud {
                service: "elevenlabs".to_owned(),
            },
            may_retain_data: true,
            transmitted_data: vec![DataClass::Transcript],
            capabilities: BTreeMap::new(),
        }
    }

    fn identity() -> TurnIdentity {
        TurnIdentity {
            session_id: "session-7".to_owned(),
            turn_id: "turn-11".to_owned(),
            cancellation_generation: 3,
        }
    }

    fn speech(sentence_id: u64) -> SpeechRequest {
        SpeechRequest {
            identity: identity(),
            sentence_id,
            text: "The eastern lock is open.".to_owned(),
            locale: "en-US".to_owned(),
            voice_hint: None,
        }
    }

    fn pcm(sequence: u64, samples: &[i16]) -> TtsEvent {
        TtsEvent::Audio(PcmChunk {
            sequence,
            format: AudioFormat::default(),
            data: samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect(),
        })
    }

    fn fake_bridge(
        plans: impl IntoIterator<Item = FakeSessionPlan>,
    ) -> (RuntimeTtsBridge, Arc<FakeProviderState>) {
        let state = Arc::new(FakeProviderState {
            plans: Mutex::new(plans.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
            starts: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
        });
        let upstream: Arc<dyn StreamingTtsProvider> = Arc::new(FakeProvider {
            state: Arc::clone(&state),
        });
        let bridge = RuntimeTtsBridge::new(
            upstream,
            RuntimeTtsBridgeConfig::dev_elevenlabs_stock(descriptor()),
        )
        .expect("valid bridge config");
        (bridge, state)
    }

    async fn core_session(bridge: &RuntimeTtsBridge) -> Box<dyn TtsSession> {
        bridge
            .start_session(&identity(), "en-US", CancellationToken::new())
            .await
            .expect("core session starts")
    }

    #[tokio::test]
    async fn creates_one_upstream_session_per_sentence_and_marks_only_final_chunk_eos() {
        let events = || {
            FakeSessionPlan::events([
                pcm(0, &[0, 12, -12]),
                TtsEvent::Alignment(vec![WordAlignment {
                    word: "eastern".to_owned(),
                    start_ms: 8,
                    end_ms: 90,
                    source_text_start: Some(4),
                    source_text_length: Some(7),
                }]),
                pcm(1, &[24, -24]),
                TtsEvent::Completed,
            ])
        };
        let (bridge, state) = fake_bridge([events(), events()]);
        let mut session = core_session(&bridge).await;

        for sentence_id in [1, 2] {
            let output = session
                .synthesize(speech(sentence_id), CancellationToken::new())
                .await
                .expect("sentence synthesis starts")
                .collect::<Vec<_>>()
                .await;
            assert!(output.iter().all(Result::is_ok));
            let audio: Vec<_> = output
                .iter()
                .filter_map(|item| match item.as_ref().expect("checked above") {
                    SpeechStreamItem::Audio(chunk) => Some(chunk),
                    SpeechStreamItem::Alignment(_) => None,
                })
                .collect();
            assert_eq!(audio.len(), 2);
            assert_eq!(audio[0].sequence, 1);
            assert!(!audio[0].end_of_stream);
            assert_eq!(audio[1].sequence, 2);
            assert!(audio[1].end_of_stream);
        }

        assert_eq!(state.starts.load(Ordering::SeqCst), 2);
        let requests = state.requests.lock().expect("request mutex poisoned");
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request.voice_intent_id == "dev.elevenlabs.stock"
                && request.output == AudioFormat::default()
                && request.identity
                    == SessionIdentity {
                        session_id: "session-7".to_owned(),
                        turn_id: "turn-11".to_owned(),
                        cancellation_generation: 3,
                    }
        }));
    }

    #[tokio::test]
    async fn emits_alignment_before_final_eos_when_provider_sends_one_audio_chunk() {
        let plan = FakeSessionPlan::events([
            pcm(0, &[7, -7]),
            TtsEvent::Alignment(vec![WordAlignment {
                word: "eastern".to_owned(),
                start_ms: 8,
                end_ms: 90,
                source_text_start: Some(4),
                source_text_length: Some(7),
            }]),
            TtsEvent::Completed,
        ]);
        let (bridge, _) = fake_bridge([plan]);
        let mut session = core_session(&bridge).await;
        let output = session
            .synthesize(speech(1), CancellationToken::new())
            .await
            .expect("sentence synthesis starts")
            .try_collect::<Vec<_>>()
            .await
            .expect("valid stream");

        assert_eq!(output.len(), 2);
        assert!(matches!(output[0], SpeechStreamItem::Alignment(_)));
        assert!(matches!(
            &output[1],
            SpeechStreamItem::Audio(AudioChunk {
                sequence: 1,
                end_of_stream: true,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn rejects_sequence_format_silence_and_missing_eos() {
        let wrong_format = TtsEvent::Audio(PcmChunk {
            sequence: 0,
            format: AudioFormat {
                encoding: PcmEncoding::PcmS16Le,
                sample_rate_hz: 48_000,
                channels: 1,
            },
            data: 4_i16.to_le_bytes().to_vec(),
        });
        let plans = [
            FakeSessionPlan::events([pcm(1, &[1]), TtsEvent::Completed]),
            FakeSessionPlan::events([wrong_format, TtsEvent::Completed]),
            FakeSessionPlan::events([pcm(0, &[0, 0]), TtsEvent::Completed]),
            FakeSessionPlan::events([pcm(0, &[1])]),
        ];
        let expected = [
            "audio_sequence_discontinuity",
            "unexpected_audio_format",
            "silent_audio_stream",
            "missing_completed_event",
        ];
        let (bridge, _) = fake_bridge(plans);
        let mut session = core_session(&bridge).await;

        for (index, code) in expected.into_iter().enumerate() {
            let items = session
                .synthesize(speech(index as u64), CancellationToken::new())
                .await
                .expect("sentence synthesis starts")
                .collect::<Vec<_>>()
                .await;
            let error = items
                .into_iter()
                .find_map(Result::err)
                .expect("malformed stream must fail");
            assert_eq!(error.kind, ProviderErrorKind::Protocol);
            assert_eq!(error.message, code);
        }
    }

    #[tokio::test]
    async fn cancellation_stops_the_upstream_session_and_discards_late_events() {
        let plan = FakeSessionPlan {
            events: VecDeque::new(),
            pending_when_exhausted: true,
        };
        let (bridge, state) = fake_bridge([plan]);
        let mut session = core_session(&bridge).await;
        let cancellation = CancellationToken::new();
        let mut stream = session
            .synthesize(speech(1), cancellation.clone())
            .await
            .expect("sentence synthesis starts");
        cancellation.cancel();

        let error = stream
            .next()
            .await
            .expect("cancel event")
            .expect_err("cancel must fail the stream");
        assert_eq!(error.kind, ProviderErrorKind::Cancelled);
        assert_eq!(state.cancels.load(Ordering::SeqCst), 1);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn upstream_errors_keep_only_allowlisted_redacted_fields() {
        let canary = "provider-body-secret-canary";
        let plan = FakeSessionPlan {
            events: VecDeque::from([Err(TtsError::new(
                canary,
                TtsErrorKind::Unavailable,
                "transport_unavailable",
                true,
            ))]),
            pending_when_exhausted: false,
        };
        let (bridge, _) = fake_bridge([plan]);
        let mut session = core_session(&bridge).await;
        let error = session
            .synthesize(speech(1), CancellationToken::new())
            .await
            .expect("sentence synthesis starts")
            .next()
            .await
            .expect("error event")
            .expect_err("upstream error must be forwarded");

        assert_eq!(error.message, "transport_unavailable");
        assert_eq!(error.provider_id, "elevenlabs");
        assert_eq!(error.kind, ProviderErrorKind::Unavailable);
        assert!(!format!("{error:?}").contains(canary));
    }

    #[derive(Default)]
    struct RecordingVault {
        reads: Mutex<Vec<String>>,
    }

    impl CredentialVault for RecordingVault {
        fn put(
            &self,
            _target: &str,
            _username: Option<&str>,
            _secret: &SecretValue,
        ) -> Result<(), VaultError> {
            Err(VaultError::System(1))
        }

        fn get(&self, target: &str) -> Result<SecretValue, VaultError> {
            self.reads
                .lock()
                .expect("vault mutex poisoned")
                .push(target.to_owned());
            SecretValue::new(b"fixture-only-token".to_vec())
        }

        fn delete(&self, _target: &str) -> Result<(), VaultError> {
            Err(VaultError::System(1))
        }
    }

    #[tokio::test]
    async fn vault_resolver_reads_only_fixed_hosted_tts_targets() {
        let vault = Arc::new(RecordingVault::default());
        let resolver =
            VaultTtsCredentialResolver::new(Arc::clone(&vault) as Arc<dyn CredentialVault>);

        assert_eq!(
            resolver
                .resolve(HostedTtsProviderId::ElevenLabs)
                .await
                .expect("fixture credential")
                .expose(),
            "fixture-only-token"
        );
        assert_eq!(
            resolver
                .resolve(HostedTtsProviderId::NvidiaNimMagpie)
                .await
                .expect("fixture credential")
                .expose(),
            "fixture-only-token"
        );
        assert_eq!(
            resolver.resolve(HostedTtsProviderId::Cartesia).await,
            Err(CredentialResolveError::Missing)
        );
        assert_eq!(
            *vault.reads.lock().expect("vault mutex poisoned"),
            vec![
                ELEVENLABS_CREDENTIAL_TARGET.to_owned(),
                NVIDIA_NIM_CREDENTIAL_TARGET.to_owned()
            ]
        );
    }

    #[test]
    fn vault_secret_conversion_rejects_non_utf8_without_a_displayable_copy() {
        let secret = SecretValue::new(vec![0xff, 0xfe]).expect("non-empty fixture secret");
        assert_eq!(
            sensitive_utf8_from_vault(secret),
            Err(CredentialResolveError::Unavailable)
        );
    }

    #[test]
    fn bridge_rejects_non_pcm_and_noncanonical_hosted_policy_metadata() {
        let state = Arc::new(FakeProviderState {
            plans: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
            starts: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
        });
        let upstream: Arc<dyn StreamingTtsProvider> = Arc::new(FakeProvider { state });
        let mut config = RuntimeTtsBridgeConfig::dev_elevenlabs_stock(descriptor());
        config.output.sample_rate_hz = 7_999;
        assert!(matches!(
            RuntimeTtsBridge::new(Arc::clone(&upstream), config),
            Err(BridgeConfigError::UnsupportedOutput)
        ));

        let mut no_egress = descriptor();
        no_egress.transmitted_data.clear();
        let config = RuntimeTtsBridgeConfig::dev_elevenlabs_stock(no_egress);
        assert!(matches!(
            RuntimeTtsBridge::new(upstream, config),
            Err(BridgeConfigError::TranscriptEgressNotDeclared)
        ));

        let state = Arc::new(FakeProviderState {
            plans: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
            starts: AtomicUsize::new(0),
            cancels: AtomicUsize::new(0),
        });
        let upstream: Arc<dyn StreamingTtsProvider> = Arc::new(FakeProvider { state });
        let mut local_descriptor = descriptor();
        local_descriptor.location = ProviderLocation::Local;
        assert!(matches!(
            RuntimeTtsBridge::new(
                Arc::clone(&upstream),
                RuntimeTtsBridgeConfig::dev_elevenlabs_stock(local_descriptor),
            ),
            Err(BridgeConfigError::HostedProviderMarkedLocal)
        ));

        let mut no_retention = descriptor();
        no_retention.may_retain_data = false;
        assert!(matches!(
            RuntimeTtsBridge::new(
                upstream,
                RuntimeTtsBridgeConfig::dev_elevenlabs_stock(no_retention),
            ),
            Err(BridgeConfigError::RetentionNotDeclared)
        ));
    }

    #[test]
    fn bridge_stores_a_canonical_cloud_descriptor() {
        let (bridge, _) = fake_bridge([]);
        let descriptor = bridge.descriptor();
        assert_eq!(descriptor.id, "elevenlabs");
        assert_eq!(descriptor.display_name, "ElevenLabs");
        assert_eq!(
            descriptor.location,
            ProviderLocation::Cloud {
                service: "elevenlabs".to_owned()
            }
        );
        assert!(descriptor.may_retain_data);
        assert_eq!(descriptor.transmitted_data, vec![DataClass::Transcript]);
        assert_eq!(
            descriptor
                .capabilities
                .get("streaming_pcm")
                .map(String::as_str),
            Some("true")
        );
    }

    struct FixtureVoiceSource {
        calls: AtomicUsize,
        voices: Vec<NvidiaStockVoice>,
    }

    #[async_trait]
    impl RuntimeStockVoiceSource for FixtureVoiceSource {
        async fn discover(&self) -> Result<Vec<NvidiaStockVoice>, TtsError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.voices.clone())
        }
    }

    fn magpie_discovery_request(force_refresh: bool) -> TtsVoiceDiscoveryRequest {
        TtsVoiceDiscoveryRequest {
            schema_version: VOICE_DISCOVERY_SCHEMA_VERSION,
            provider_id: HostedTtsProviderId::NvidiaNimMagpie.as_str().into(),
            model_id: NVIDIA_MAGPIE_MODEL_ID.into(),
            force_refresh,
        }
    }

    #[tokio::test]
    async fn voice_discovery_is_bounded_cached_and_preserves_stock_provenance() {
        let source = Arc::new(FixtureVoiceSource {
            calls: AtomicUsize::new(0),
            voices: vec![NvidiaStockVoice {
                id: "Magpie-Multilingual.EN-US.Aria".into(),
                display_name: "Aria".into(),
                locale: "en-US".into(),
                origin: NvidiaVoiceOrigin::ProviderStock,
            }],
        });
        let service = TtsVoiceDiscoveryService {
            source: Arc::clone(&source) as Arc<dyn RuntimeStockVoiceSource>,
            cache: tokio::sync::Mutex::new(None),
        };

        let first = service.discover(magpie_discovery_request(false)).await;
        assert_eq!(first.status, TtsVoiceDiscoveryStatus::Available);
        assert!(first.refresh.performed);
        assert!(!first.refresh.cache_hit);
        assert_eq!(first.voices.len(), 1);
        assert_eq!(first.voices[0].voice_id, "Magpie-Multilingual.EN-US.Aria");
        assert_eq!(first.voices[0].display_name, "Aria");
        assert_eq!(first.voices[0].language, "en-US");
        assert!(first.voices[0].styles.is_empty());
        assert_eq!(
            first.voices[0].provenance,
            TtsVoiceProvenance::ProviderStockDiscovery
        );
        assert!(first.error.is_none());

        let cached = service.discover(magpie_discovery_request(false)).await;
        assert!(cached.refresh.cache_hit);
        assert!(!cached.refresh.performed);
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);

        let refreshed = service.discover(magpie_discovery_request(true)).await;
        assert!(refreshed.refresh.requested);
        assert!(refreshed.refresh.performed);
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn voice_discovery_rejects_unqualified_routes_without_contacting_source() {
        let source = Arc::new(FixtureVoiceSource {
            calls: AtomicUsize::new(0),
            voices: Vec::new(),
        });
        let service = TtsVoiceDiscoveryService {
            source: Arc::clone(&source) as Arc<dyn RuntimeStockVoiceSource>,
            cache: tokio::sync::Mutex::new(None),
        };
        let result = service
            .discover(TtsVoiceDiscoveryRequest {
                schema_version: VOICE_DISCOVERY_SCHEMA_VERSION,
                provider_id: "cartesia".into(),
                model_id: "sonic".into(),
                force_refresh: false,
            })
            .await;
        assert_eq!(result.status, TtsVoiceDiscoveryStatus::Unavailable);
        assert_eq!(result.voices, Vec::new());
        assert_eq!(
            result.error.as_ref().map(|error| error.code.as_str()),
            Some("unsupported_tts_voice_route")
        );
        assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    }
}
