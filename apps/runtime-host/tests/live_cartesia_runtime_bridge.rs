//! Explicit live qualification of the production Cartesia transport through
//! `RuntimeTtsBridge`. The test consumes PCM in memory and never opens an OS
//! audio endpoint or claims native-broker delivery.

#![cfg(windows)]

use std::{
    collections::BTreeMap,
    env,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::StreamExt;
use npc_providers_tts::{
    CartesiaConfig, CartesiaProvider, CartesiaWebSocketTransport, CredentialResolveError,
    HostedTtsProviderId, ProviderCapabilities, ProviderCredentialResolver, PushOutcome,
    SensitiveString, SessionIdentity, SessionState, StreamingTtsProvider, StreamingTtsSession,
    TtsError, TtsEvent, TtsSessionRequest, VoiceBinding, VoiceBindings,
    CARTESIA_QUALIFIED_AUDIO_FORMAT, CARTESIA_QUALIFIED_MODEL_ID,
    CARTESIA_QUALIFIED_STOCK_VOICE_ID,
};
use npc_runtime_core::{
    DataClass, ProviderDescriptor, ProviderLocation, ProviderModality, SpeechRequest,
    SpeechStreamItem, TtsProvider, TurnIdentity,
};
use npc_runtime_host::tts_bridge::{RuntimeTtsBridge, RuntimeTtsBridgeConfig};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

const KEY_ENV: &str = "CARTESIA_API_KEY";
const REPORT_ENV: &str = "CARTESIA_RUNTIME_BRIDGE_REPORT";
const TEXT: &str = "The harbor lantern is ready for tonight's test.";

#[derive(Clone, Copy)]
struct EnvironmentCredential;

#[async_trait]
impl ProviderCredentialResolver for EnvironmentCredential {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        if provider_id != HostedTtsProviderId::Cartesia {
            return Err(CredentialResolveError::Missing);
        }
        env::var(KEY_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .map(SensitiveString::new)
            .ok_or(CredentialResolveError::Missing)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSessionTiming {
    session_started_us: u64,
    first_pcm_us: Option<u64>,
    completed_us: Option<u64>,
}

struct ObservedProvider {
    inner: Arc<dyn StreamingTtsProvider>,
    origin: Arc<Instant>,
    sessions: Arc<Mutex<Vec<ProviderSessionTiming>>>,
}

#[async_trait]
impl StreamingTtsProvider for ObservedProvider {
    fn id(&self) -> HostedTtsProviderId {
        self.inner.id()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
        let inner = self.inner.start_session(request).await?;
        let index = {
            let mut sessions = self.sessions.lock().expect("timing mutex poisoned");
            let index = sessions.len();
            sessions.push(ProviderSessionTiming {
                session_started_us: elapsed_us(&self.origin),
                ..ProviderSessionTiming::default()
            });
            index
        };
        Ok(Box::new(ObservedSession {
            inner,
            origin: Arc::clone(&self.origin),
            sessions: Arc::clone(&self.sessions),
            index,
        }))
    }
}

struct ObservedSession {
    inner: Box<dyn StreamingTtsSession>,
    origin: Arc<Instant>,
    sessions: Arc<Mutex<Vec<ProviderSessionTiming>>>,
    index: usize,
}

#[async_trait]
impl StreamingTtsSession for ObservedSession {
    fn provider_id(&self) -> HostedTtsProviderId {
        self.inner.provider_id()
    }

    fn identity(&self) -> &SessionIdentity {
        self.inner.identity()
    }

    fn state(&self) -> SessionState {
        self.inner.state()
    }

    fn has_started_utterance(&self) -> bool {
        self.inner.has_started_utterance()
    }

    async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError> {
        self.inner.push_text(text).await
    }

    async fn finish(&mut self) -> Result<(), TtsError> {
        self.inner.finish().await
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        let event = self.inner.next_event().await;
        if let Some(Ok(event)) = &event {
            let now = elapsed_us(&self.origin);
            let mut sessions = self.sessions.lock().expect("timing mutex poisoned");
            let timing = sessions.get_mut(self.index).expect("session timing exists");
            match event {
                TtsEvent::Audio(chunk) if !chunk.data.is_empty() => {
                    timing.first_pcm_us.get_or_insert(now);
                }
                TtsEvent::Completed => timing.completed_us = Some(now),
                _ => {}
            }
        }
        event
    }

    async fn cancel(&mut self) -> Result<(), TtsError> {
        self.inner.cancel().await
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeSentenceTiming {
    public_turn_id: String,
    sentence_id: u64,
    request_us: u64,
    provider_session_started_us: u64,
    provider_first_pcm_us: u64,
    bridge_first_audio_us: u64,
    provider_to_bridge_first_audio_us: u64,
    bridge_complete_us: u64,
    provider_complete_us: u64,
    pcm_bytes: usize,
    audio_chunks: usize,
    alignment_events: usize,
    end_of_stream: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    receipt_type: &'static str,
    timing_scope: &'static str,
    provider_id: &'static str,
    model_id: &'static str,
    voice_id: &'static str,
    sample_rate_hz: u32,
    channels: u16,
    first_same_turn_sentence: BridgeSentenceTiming,
    second_same_turn_sentence: BridgeSentenceTiming,
    second_turn_sentence: BridgeSentenceTiming,
    fresh_connections: u64,
    reused_connections: u64,
    stale_evictions: u64,
    last_connection_reused: bool,
    contains_credentials: bool,
    audio_delivery_claimed: bool,
    physical_audibility_claimed: bool,
    native_broker_receipt: bool,
}

fn elapsed_us(origin: &Instant) -> u64 {
    origin.elapsed().as_micros().try_into().unwrap_or(u64::MAX)
}

fn identity(turn_id: &str, generation: u64) -> TurnIdentity {
    TurnIdentity {
        session_id: "cartesia-runtime-bridge-live".into(),
        turn_id: turn_id.into(),
        cancellation_generation: generation,
    }
}

async fn synthesize(
    session: &mut dyn npc_runtime_core::TtsSession,
    identity: TurnIdentity,
    sentence_id: u64,
    origin: &Instant,
    provider_timings: &Arc<Mutex<Vec<ProviderSessionTiming>>>,
) -> BridgeSentenceTiming {
    let provider_index = provider_timings
        .lock()
        .expect("timing mutex poisoned")
        .len();
    let request_us = elapsed_us(origin);
    let mut speech = session
        .synthesize(
            SpeechRequest {
                identity: identity.clone(),
                sentence_id,
                text: TEXT.into(),
                locale: "en-US".into(),
                voice_hint: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("start production bridge synthesis");
    let mut bridge_first_audio_us = None;
    let mut pcm_bytes = 0_usize;
    let mut audio_chunks = 0_usize;
    let mut alignment_events = 0_usize;
    let mut end_of_stream = false;
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), speech.next())
        .await
        .expect("bridge event deadline")
    {
        match item.expect("bridge stream event") {
            SpeechStreamItem::Audio(chunk) => {
                assert_eq!(chunk.sample_rate_hz, 24_000);
                assert_eq!(chunk.channels, 1);
                bridge_first_audio_us.get_or_insert_with(|| elapsed_us(origin));
                pcm_bytes = pcm_bytes.saturating_add(chunk.pcm_s16le.len());
                audio_chunks = audio_chunks.saturating_add(1);
                end_of_stream |= chunk.end_of_stream;
            }
            SpeechStreamItem::Alignment(_) => alignment_events = alignment_events.saturating_add(1),
        }
    }
    let bridge_complete_us = elapsed_us(origin);
    assert!(end_of_stream && pcm_bytes > 0 && audio_chunks > 1);
    let provider = provider_timings
        .lock()
        .expect("timing mutex poisoned")
        .get(provider_index)
        .cloned()
        .expect("provider timing for bridge sentence");
    let provider_first_pcm_us = provider.first_pcm_us.expect("provider first PCM timing");
    let bridge_first_audio_us = bridge_first_audio_us.expect("bridge first audio timing");
    assert!(bridge_first_audio_us >= provider_first_pcm_us);
    BridgeSentenceTiming {
        public_turn_id: identity.turn_id,
        sentence_id,
        request_us,
        provider_session_started_us: provider.session_started_us,
        provider_first_pcm_us,
        bridge_first_audio_us,
        provider_to_bridge_first_audio_us: bridge_first_audio_us - provider_first_pcm_us,
        bridge_complete_us,
        provider_complete_us: provider.completed_us.expect("provider completion timing"),
        pcm_bytes,
        audio_chunks,
        alignment_events,
        end_of_stream,
    }
}

#[tokio::test]
#[ignore = "explicit low-cost Cartesia production RuntimeTtsBridge qualification"]
async fn pooled_cartesia_is_measured_across_same_turn_sentences_and_a_second_turn() {
    let report_path = PathBuf::from(env::var_os(REPORT_ENV).expect("report path is required"));
    assert!(
        !report_path.exists(),
        "qualification report must be create-new"
    );

    let bindings = VoiceBindings::new([VoiceBinding {
        intent_id: "selected.stock.voice".into(),
        provider_id: HostedTtsProviderId::Cartesia,
        voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID.into(),
        model_id: CARTESIA_QUALIFIED_MODEL_ID.into(),
        provider_options: BTreeMap::new(),
    }])
    .expect("exact Cartesia voice binding");
    let transport = Arc::new(CartesiaWebSocketTransport::default());
    let provider: Arc<dyn StreamingTtsProvider> = Arc::new(CartesiaProvider::new(
        transport.clone(),
        Arc::new(EnvironmentCredential),
        bindings,
        CartesiaConfig::default(),
    ));
    let origin = Arc::new(Instant::now());
    let provider_timings = Arc::new(Mutex::new(Vec::new()));
    let observed: Arc<dyn StreamingTtsProvider> = Arc::new(ObservedProvider {
        inner: provider,
        origin: Arc::clone(&origin),
        sessions: Arc::clone(&provider_timings),
    });
    let descriptor = ProviderDescriptor {
        id: "cartesia".into(),
        display_name: "Cartesia".into(),
        modality: ProviderModality::Speech,
        location: ProviderLocation::Cloud {
            service: "cartesia".into(),
        },
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::new(),
    };
    let bridge = RuntimeTtsBridge::new(
        observed,
        RuntimeTtsBridgeConfig::selected_stock(
            descriptor,
            "selected.stock.voice",
            CARTESIA_QUALIFIED_MODEL_ID,
            CARTESIA_QUALIFIED_AUDIO_FORMAT,
        ),
    )
    .expect("production bridge configuration");

    let first_identity = identity("turn-1", 1);
    let mut first_turn = bridge
        .start_session(&first_identity, "en-US", CancellationToken::new())
        .await
        .expect("first runtime TTS session");
    let first = synthesize(
        first_turn.as_mut(),
        first_identity.clone(),
        1,
        &origin,
        &provider_timings,
    )
    .await;
    let second = synthesize(
        first_turn.as_mut(),
        first_identity,
        2,
        &origin,
        &provider_timings,
    )
    .await;
    first_turn
        .close()
        .await
        .expect("close first runtime session");

    let second_identity = identity("turn-2", 2);
    let mut second_turn = bridge
        .start_session(&second_identity, "en-US", CancellationToken::new())
        .await
        .expect("second runtime TTS session");
    let third = synthesize(
        second_turn.as_mut(),
        second_identity,
        1,
        &origin,
        &provider_timings,
    )
    .await;
    second_turn
        .close()
        .await
        .expect("close second runtime session");

    let stats = transport.connection_stats();
    assert_eq!(stats.fresh_connections, 1);
    assert_eq!(stats.reused_connections, 2);
    let report = Report {
        schema_version: 1,
        receipt_type: "headlessSynthesisQualification",
        timing_scope: "request before upstream session connect through provider PCM, bridge Audio emission, and stream completion; includes TLS/WebSocket setup on the first sentence",
        provider_id: "cartesia",
        model_id: CARTESIA_QUALIFIED_MODEL_ID,
        voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID,
        sample_rate_hz: CARTESIA_QUALIFIED_AUDIO_FORMAT.sample_rate_hz,
        channels: CARTESIA_QUALIFIED_AUDIO_FORMAT.channels,
        first_same_turn_sentence: first,
        second_same_turn_sentence: second,
        second_turn_sentence: third,
        fresh_connections: stats.fresh_connections,
        reused_connections: stats.reused_connections,
        stale_evictions: stats.stale_evictions,
        last_connection_reused: stats.last_connection_reused,
        contains_credentials: false,
        audio_delivery_claimed: false,
        physical_audibility_claimed: false,
        native_broker_receipt: false,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)
        .expect("create qualification report");
    file.write_all(&serde_json::to_vec_pretty(&report).expect("serialize report"))
        .expect("write qualification report");
}
