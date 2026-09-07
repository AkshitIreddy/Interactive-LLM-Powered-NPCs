use std::{collections::BTreeMap, env, error::Error, io, sync::Arc, time::Instant};

use async_trait::async_trait;
use npc_providers_tts::{
    AudioFormat, CartesiaConfig, CartesiaConnectionStats, CartesiaProvider,
    CartesiaWebSocketTransport, CredentialResolveError, DeepgramConfig, DeepgramProvider,
    DeepgramWebSocketTransport, HostedTtsProviderId, InworldConfig, InworldProvider,
    InworldWebSocketTransport, ProviderCredentialResolver, SemanticClausePolicy, SensitiveString,
    SessionIdentity, StreamingTtsProvider, TimingSymbolKind, TtsEvent, TtsSessionRequest,
    VoiceBinding, VoiceBindings, CARTESIA_QUALIFIED_AUDIO_FORMAT, CARTESIA_QUALIFIED_MODEL_ID,
    CARTESIA_QUALIFIED_STOCK_VOICE_ID,
};
use serde::Serialize;
use tokio::time::{timeout, Duration};

const FIXTURE_TEXT: &str = "The harbor lantern is ready for tonight's test.";
const EVENT_TIMEOUT: Duration = Duration::from_secs(45);

type DynError = Box<dyn Error + Send + Sync>;

#[derive(Clone, Copy)]
struct EnvironmentCredentialResolver;

#[async_trait]
impl ProviderCredentialResolver for EnvironmentCredentialResolver {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        let name = match provider_id {
            HostedTtsProviderId::Cartesia => "CARTESIA_API_KEY",
            HostedTtsProviderId::Inworld => "INWORLD_API_KEY",
            HostedTtsProviderId::Deepgram => "DEEPGRAM_API_KEY",
            _ => return Err(CredentialResolveError::Missing),
        };
        env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .map(SensitiveString::new)
            .ok_or(CredentialResolveError::Missing)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionEvidence {
    request_to_first_pcm_ms: f64,
    request_to_complete_ms: f64,
    pcm_bytes: usize,
    pcm_chunks: usize,
    duration_seconds: f64,
    rms: f64,
    peak: f64,
    alignment_items: usize,
    viseme_items: usize,
    provider_viseme_items: usize,
    phoneme_items: usize,
    usage_events: usize,
    sequence_contiguous: bool,
    exact_pcm_format: bool,
    terminal_completed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancellationEvidence {
    request_to_first_pcm_ms: f64,
    first_pcm_bytes: usize,
    cancel_complete_ms: f64,
    interrupted_event_received: bool,
    terminal_stream_closed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionStatsEvidence {
    fresh_connections: u64,
    reused_connections: u64,
    stale_evictions: u64,
    last_connect_ms: f64,
    last_connection_reused: bool,
}

impl From<CartesiaConnectionStats> for ConnectionStatsEvidence {
    fn from(value: CartesiaConnectionStats) -> Self {
        Self {
            fresh_connections: value.fresh_connections,
            reused_connections: value.reused_connections,
            stale_evictions: value.stale_evictions,
            last_connect_ms: value.last_connect_micros as f64 / 1_000.0,
            last_connection_reused: value.last_connection_reused,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CartesiaEvidence {
    endpoint: &'static str,
    model_id: &'static str,
    voice_id: &'static str,
    voice_name: &'static str,
    first: SessionEvidence,
    stats_after_first: ConnectionStatsEvidence,
    second_same_identity: SessionEvidence,
    stats_after_second: ConnectionStatsEvidence,
    cancelled_same_identity: CancellationEvidence,
    stats_after_cancel: ConnectionStatsEvidence,
    after_cancel_isolation_same_identity: SessionEvidence,
    final_stats: ConnectionStatsEvidence,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderEvidence {
    endpoint: &'static str,
    model_id: &'static str,
    voice_id: &'static str,
    request_visemes: bool,
    session: SessionEvidence,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QualificationReport {
    schema_version: u32,
    purpose: &'static str,
    contains_credential_values: bool,
    contains_dialogue_text: bool,
    timing_scope: &'static str,
    pcm_contract: &'static str,
    same_public_session_identity_used_for_cartesia: bool,
    cartesia: CartesiaEvidence,
    inworld: ProviderEvidence,
    deepgram: ProviderEvidence,
}

struct PcmMeter {
    bytes: usize,
    chunks: usize,
    samples: u64,
    sum_squares: f64,
    peak: i32,
    next_sequence: u64,
    sequence_contiguous: bool,
    exact_format: bool,
}

impl PcmMeter {
    fn new() -> Self {
        Self {
            bytes: 0,
            chunks: 0,
            samples: 0,
            sum_squares: 0.0,
            peak: 0,
            next_sequence: 0,
            sequence_contiguous: true,
            exact_format: true,
        }
    }

    fn observe(&mut self, sequence: u64, format: AudioFormat, data: &[u8]) -> Result<(), DynError> {
        if data.is_empty() || !data.len().is_multiple_of(2) {
            return Err(failure("provider emitted empty or odd-length PCM"));
        }
        self.sequence_contiguous &= sequence == self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.exact_format &= format == CARTESIA_QUALIFIED_AUDIO_FORMAT;
        self.bytes += data.len();
        self.chunks += 1;
        for pair in data.chunks_exact(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]) as i32;
            self.samples += 1;
            self.sum_squares += f64::from(sample * sample);
            self.peak = self.peak.max(sample.abs());
        }
        Ok(())
    }

    fn duration_seconds(&self) -> f64 {
        self.samples as f64 / f64::from(CARTESIA_QUALIFIED_AUDIO_FORMAT.sample_rate_hz)
    }

    fn rms(&self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            (self.sum_squares / self.samples as f64).sqrt() / 32768.0
        }
    }

    fn peak(&self) -> f64 {
        f64::from(self.peak) / 32768.0
    }
}

fn failure(message: &'static str) -> DynError {
    Box::new(io::Error::other(message))
}

fn identity() -> SessionIdentity {
    SessionIdentity {
        session_id: "adapter-qualification-session".into(),
        turn_id: "shared-public-turn".into(),
        cancellation_generation: 7,
    }
}

fn request(request_visemes: bool) -> TtsSessionRequest {
    TtsSessionRequest {
        identity: identity(),
        locale: "en-US".into(),
        voice_intent_id: "qualified-stock".into(),
        output: CARTESIA_QUALIFIED_AUDIO_FORMAT,
        request_alignment: request_visemes,
        request_visemes,
        clause_policy: SemanticClausePolicy::default(),
    }
}

fn bindings(
    provider_id: HostedTtsProviderId,
    voice_id: &str,
    model_id: &str,
) -> Result<VoiceBindings, DynError> {
    VoiceBindings::new([VoiceBinding {
        intent_id: "qualified-stock".into(),
        provider_id,
        voice_id: voice_id.into(),
        model_id: model_id.into(),
        provider_options: BTreeMap::new(),
    }])
    .map_err(|_| failure("voice binding rejected"))
}

async fn complete_session(
    provider: &dyn StreamingTtsProvider,
    request: TtsSessionRequest,
) -> Result<SessionEvidence, DynError> {
    let started = Instant::now();
    let mut session = provider.start_session(request).await?;
    session.push_text(FIXTURE_TEXT).await?;
    session.finish().await?;

    let mut first_pcm_ms = None;
    let mut meter = PcmMeter::new();
    let mut alignment_items = 0;
    let mut viseme_items = 0;
    let mut provider_viseme_items = 0;
    let mut phoneme_items = 0;
    let mut usage_events = 0;
    let mut completed = false;
    loop {
        let event = timeout(EVENT_TIMEOUT, session.next_event())
            .await
            .map_err(|_| failure("provider event timeout"))?;
        let Some(event) = event else {
            break;
        };
        match event? {
            TtsEvent::Audio(chunk) => {
                first_pcm_ms.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1_000.0);
                meter.observe(chunk.sequence, chunk.format, &chunk.data)?;
            }
            TtsEvent::Alignment(items) => alignment_items += items.len(),
            TtsEvent::Viseme(items) => {
                viseme_items += items.len();
                provider_viseme_items += items
                    .iter()
                    .filter(|item| item.symbol_kind == TimingSymbolKind::ProviderViseme)
                    .count();
                phoneme_items += items
                    .iter()
                    .filter(|item| item.symbol_kind == TimingSymbolKind::Phoneme)
                    .count();
            }
            TtsEvent::Usage(_) => usage_events += 1,
            TtsEvent::Completed => {
                completed = true;
                break;
            }
            TtsEvent::Interrupted { .. } => return Err(failure("unexpected interrupted event")),
        }
    }
    let complete_ms = started.elapsed().as_secs_f64() * 1_000.0;
    if !completed || meter.bytes == 0 || meter.rms() < 0.0001 {
        return Err(failure("session did not yield completed non-silent PCM"));
    }
    Ok(SessionEvidence {
        request_to_first_pcm_ms: first_pcm_ms.ok_or_else(|| failure("first PCM missing"))?,
        request_to_complete_ms: complete_ms,
        pcm_bytes: meter.bytes,
        pcm_chunks: meter.chunks,
        duration_seconds: meter.duration_seconds(),
        rms: meter.rms(),
        peak: meter.peak(),
        alignment_items,
        viseme_items,
        provider_viseme_items,
        phoneme_items,
        usage_events,
        sequence_contiguous: meter.sequence_contiguous,
        exact_pcm_format: meter.exact_format,
        terminal_completed: completed,
    })
}

async fn cancel_after_first_pcm(
    provider: &dyn StreamingTtsProvider,
) -> Result<CancellationEvidence, DynError> {
    let started = Instant::now();
    let mut session = provider.start_session(request(true)).await?;
    session.push_text(FIXTURE_TEXT).await?;
    session.finish().await?;
    let first_pcm = loop {
        let event = timeout(EVENT_TIMEOUT, session.next_event())
            .await
            .map_err(|_| failure("cancel probe event timeout"))?
            .ok_or_else(|| failure("cancel probe closed before PCM"))??;
        if let TtsEvent::Audio(chunk) = event {
            break chunk;
        }
    };
    let first_pcm_ms = started.elapsed().as_secs_f64() * 1_000.0;
    if first_pcm.format != CARTESIA_QUALIFIED_AUDIO_FORMAT || first_pcm.data.is_empty() {
        return Err(failure("cancel probe PCM invalid"));
    }
    session.cancel().await?;
    let cancel_complete_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let interrupted = matches!(
        timeout(EVENT_TIMEOUT, session.next_event())
            .await
            .map_err(|_| failure("cancel terminal timeout"))?
            .ok_or_else(|| failure("cancel interrupted event missing"))??,
        TtsEvent::Interrupted { reason: "barge_in" }
    );
    let terminal_stream_closed = timeout(EVENT_TIMEOUT, session.next_event())
        .await
        .map_err(|_| failure("cancel terminal-close timeout"))?
        .is_none();
    Ok(CancellationEvidence {
        request_to_first_pcm_ms: first_pcm_ms,
        first_pcm_bytes: first_pcm.data.len(),
        cancel_complete_ms,
        interrupted_event_received: interrupted,
        terminal_stream_closed,
    })
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    let resolver: Arc<dyn ProviderCredentialResolver> = Arc::new(EnvironmentCredentialResolver);

    let cartesia_transport = Arc::new(CartesiaWebSocketTransport::default());
    let cartesia = CartesiaProvider::new(
        cartesia_transport.clone(),
        Arc::clone(&resolver),
        bindings(
            HostedTtsProviderId::Cartesia,
            CARTESIA_QUALIFIED_STOCK_VOICE_ID,
            CARTESIA_QUALIFIED_MODEL_ID,
        )?,
        CartesiaConfig::default(),
    );
    let first = complete_session(&cartesia, request(true)).await?;
    let stats_after_first = cartesia_transport.connection_stats().into();
    let second = complete_session(&cartesia, request(true)).await?;
    let stats_after_second = cartesia_transport.connection_stats().into();
    let cancelled = cancel_after_first_pcm(&cartesia).await?;
    let stats_after_cancel = cartesia_transport.connection_stats().into();
    let after_cancel = complete_session(&cartesia, request(true)).await?;
    let final_stats = cartesia_transport.connection_stats().into();

    let inworld = InworldProvider::new(
        Arc::new(InworldWebSocketTransport::default()),
        Arc::clone(&resolver),
        bindings(
            HostedTtsProviderId::Inworld,
            "Dennis",
            "inworld-tts-2-flash",
        )?,
        InworldConfig::default(),
    );
    let inworld_session = complete_session(&inworld, request(true)).await?;
    if inworld_session.provider_viseme_items == 0 {
        return Err(failure("Inworld provider visemes missing"));
    }

    let deepgram = DeepgramProvider::new(
        Arc::new(DeepgramWebSocketTransport::default()),
        Arc::clone(&resolver),
        bindings(HostedTtsProviderId::Deepgram, "Arcas", "aura-2-arcas-en")?,
        DeepgramConfig::default(),
    );
    let deepgram_session = complete_session(&deepgram, request(false)).await?;

    let report = QualificationReport {
        schema_version: 1,
        purpose: "production-rust-hosted-tts-adapter-live-qualification",
        contains_credential_values: false,
        contains_dialogue_text: false,
        timing_scope: "before start_session through first PCM and terminal completion",
        pcm_contract: "mono pcm_s16le 24000 Hz, non-empty, even-length, non-silent",
        same_public_session_identity_used_for_cartesia: true,
        cartesia: CartesiaEvidence {
            endpoint: "wss://api.cartesia.ai/tts/websocket",
            model_id: CARTESIA_QUALIFIED_MODEL_ID,
            voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID,
            voice_name: "Greg",
            first,
            stats_after_first,
            second_same_identity: second,
            stats_after_second,
            cancelled_same_identity: cancelled,
            stats_after_cancel,
            after_cancel_isolation_same_identity: after_cancel,
            final_stats,
        },
        inworld: ProviderEvidence {
            endpoint: "wss://api.inworld.ai/tts/v1/voice:streamBidirectional",
            model_id: "inworld-tts-2-flash",
            voice_id: "Dennis",
            request_visemes: true,
            session: inworld_session,
        },
        deepgram: ProviderEvidence {
            endpoint: "wss://api.deepgram.com/v1/speak",
            model_id: "aura-2-arcas-en",
            voice_id: "Arcas",
            request_visemes: false,
            session: deepgram_session,
        },
    };
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
