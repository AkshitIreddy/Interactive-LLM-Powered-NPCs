//! Deterministic bridge-to-broker proof for provider timing cues.
//!
//! The fixture terminates the authenticated named-pipe protocol in memory. It
//! verifies commands accepted by the production broker transport; it does not
//! claim that PCM reached a physical device or that pixels were composited.

#![cfg(windows)]

use std::{
    collections::{BTreeMap, VecDeque},
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use npc_providers_tts::{
    AudioFormat, HostedTtsProviderId, PcmChunk, PcmEncoding, ProviderCapabilities, PushOutcome,
    SessionIdentity, SessionState, StreamingTtsProvider, StreamingTtsSession, TimingSymbolKind,
    TtsError, TtsEvent, TtsSessionRequest, VisemeEvent,
};
use npc_runtime_core::{
    AudioSink, DataClass, ProviderDescriptor, ProviderLocation, ProviderModality,
    RuntimeDependencyError, SpeechRequest, TtsProvider, TurnIdentity,
};
use npc_runtime_host::{
    audio_output::broker::{BrokerAudioPlaybackLease, BrokerAudioSink},
    tts_bridge::{RuntimeTtsBridge, RuntimeTtsBridgeConfig},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::Notify,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 2;
const VISUAL_CUE_SCHEMA_VERSION: u32 = 1;
const SAMPLE_RATE_HZ: u32 = 24_000;
const CHANNELS: u16 = 1;
const GENERATION: u64 = 23;
const OUTPUT_GENERATION: u64 = 41;
const TOKEN_BYTE: u8 = 0x11;
const TOKEN_BYTES: usize = 32;

#[derive(Clone)]
struct CueProvider {
    request: Arc<Mutex<Option<TtsSessionRequest>>>,
    cancellations: Arc<AtomicUsize>,
}

#[async_trait]
impl StreamingTtsProvider for CueProvider {
    fn id(&self) -> HostedTtsProviderId {
        HostedTtsProviderId::Cartesia
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming_input: true,
            streaming_pcm: true,
            alignment: true,
            visemes_or_phonemes: true,
            cancellation: true,
            usage: false,
        }
    }

    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
        *self.request.lock().expect("request mutex poisoned") = Some(request.clone());
        Ok(Box::new(CueSession {
            identity: request.identity,
            events: VecDeque::from([
                TtsEvent::Viseme(vec![VisemeEvent {
                    symbol_kind: TimingSymbolKind::Phoneme,
                    symbol: "p".to_owned(),
                    start_ms: 60,
                    duration_ms: 20,
                }]),
                TtsEvent::Audio(PcmChunk {
                    sequence: 0,
                    format: AudioFormat {
                        encoding: PcmEncoding::PcmS16Le,
                        sample_rate_hz: SAMPLE_RATE_HZ,
                        channels: CHANNELS,
                    },
                    data: [1_000_i16, -1_000, 500, -500]
                        .into_iter()
                        .flat_map(i16::to_le_bytes)
                        .collect(),
                }),
            ]),
            cancellations: Arc::clone(&self.cancellations),
        }))
    }
}

struct CueSession {
    identity: SessionIdentity,
    events: VecDeque<TtsEvent>,
    cancellations: Arc<AtomicUsize>,
}

#[async_trait]
impl StreamingTtsSession for CueSession {
    fn provider_id(&self) -> HostedTtsProviderId {
        HostedTtsProviderId::Cartesia
    }

    fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    fn state(&self) -> SessionState {
        SessionState::Finishing
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
        Ok(())
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        match self.events.pop_front() {
            Some(event) => Some(Ok(event)),
            None => pending().await,
        }
    }

    async fn cancel(&mut self) -> Result<(), TtsError> {
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
enum CommandKind {
    Begin = 1,
    Chunk = 2,
    Finish = 3,
    Cancel = 4,
    VisualCue = 5,
}

#[derive(Debug)]
struct ObservedCommand {
    kind: CommandKind,
    sequence: u64,
    deadline_qpc: u64,
    generation: u64,
    stream_id: String,
    session_id: String,
    turn_id: String,
    token: [u8; TOKEN_BYTES],
    payload: Vec<u8>,
}

impl ObservedCommand {
    fn decode(frame: &[u8]) -> Self {
        let mut cursor = Cursor::new(frame);
        assert_eq!(cursor.u32(), u32::try_from(frame.len() - 4).unwrap());
        assert_eq!(cursor.bytes(4), b"NPCP");
        assert_eq!(cursor.u32(), SCHEMA_VERSION);
        let kind = match cursor.u16() {
            1 => CommandKind::Begin,
            2 => CommandKind::Chunk,
            3 => CommandKind::Finish,
            4 => CommandKind::Cancel,
            5 => CommandKind::VisualCue,
            other => panic!("unexpected producer command {other}"),
        };
        assert_eq!(cursor.u16(), 0, "producer frame flags must remain zero");
        let sequence = cursor.u64();
        let deadline_qpc = cursor.u64();
        let generation = cursor.u64();
        let stream_id = cursor.string();
        let session_id = cursor.string();
        let turn_id = cursor.string();
        let token: [u8; TOKEN_BYTES] = cursor
            .bytes(TOKEN_BYTES)
            .try_into()
            .expect("fixed token width");
        let payload_len = usize::try_from(cursor.u32()).expect("bounded payload length");
        let payload = cursor.bytes(payload_len).to_vec();
        assert_eq!(
            cursor.position,
            frame.len(),
            "producer frame has no trailing data"
        );
        Self {
            kind,
            sequence,
            deadline_qpc,
            generation,
            stream_id,
            session_id,
            turn_id,
            token,
            payload,
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn bytes(&mut self, count: usize) -> &'a [u8] {
        let end = self.position.checked_add(count).expect("cursor overflow");
        let value = self
            .bytes
            .get(self.position..end)
            .expect("complete protocol field");
        self.position = end;
        value
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.bytes(2).try_into().expect("u16 field"))
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes(4).try_into().expect("u32 field"))
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.bytes(8).try_into().expect("u64 field"))
    }

    fn string(&mut self) -> String {
        let length = usize::from(self.u16());
        std::str::from_utf8(self.bytes(length))
            .expect("protocol identifier is UTF-8")
            .to_owned()
    }
}

async fn read_command(server: &mut NamedPipeServer) -> ObservedCommand {
    let mut prefix = [0_u8; 4];
    server
        .read_exact(&mut prefix)
        .await
        .expect("read command length");
    let body_len = usize::try_from(u32::from_le_bytes(prefix)).expect("bounded command length");
    assert!(body_len > 0 && body_len <= 65_536 + 4_096);
    let mut frame = Vec::with_capacity(body_len + 4);
    frame.extend_from_slice(&prefix);
    frame.resize(body_len + 4, 0);
    server
        .read_exact(&mut frame[4..])
        .await
        .expect("read command body");
    ObservedCommand::decode(&frame)
}

async fn respond_ok(server: &mut NamedPipeServer, command: &ObservedCommand) {
    let accepted_frames = if command.kind == CommandKind::Chunk {
        u64::try_from(command.payload.len() / (usize::from(CHANNELS) * 2))
            .expect("bounded PCM frame count")
    } else {
        0
    };
    let mut body = Vec::new();
    body.extend_from_slice(b"NPCR");
    body.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    body.extend_from_slice(&0_u16.to_le_bytes());
    body.extend_from_slice(&0_u16.to_le_bytes());
    body.extend_from_slice(&command.sequence.to_le_bytes());
    body.extend_from_slice(&accepted_frames.to_le_bytes());
    let mut frame = Vec::new();
    frame.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    frame.extend_from_slice(&body);
    server
        .write_all(&frame)
        .await
        .expect("write broker response");
    server.flush().await.expect("flush broker response");
}

fn descriptor() -> ProviderDescriptor {
    ProviderDescriptor {
        id: "cartesia".into(),
        display_name: "Cartesia".into(),
        modality: ProviderModality::Speech,
        location: ProviderLocation::Cloud {
            service: "cartesia".into(),
        },
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::new(),
    }
}

fn identity() -> TurnIdentity {
    TurnIdentity {
        session_id: "cue-clock-session".into(),
        turn_id: "cue-clock-turn".into(),
        cancellation_generation: GENERATION,
    }
}

fn lease(endpoint: String) -> BrokerAudioPlaybackLease {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": SCHEMA_VERSION,
        "streamId": "cue-clock-stream",
        "producerEndpoint": endpoint,
        "oneTimeToken": "11".repeat(TOKEN_BYTES),
        "sessionId": "cue-clock-session",
        "turnId": "cue-clock-turn",
        "generation": GENERATION,
        "sampleRate": SAMPLE_RATE_HZ,
        "channels": CHANNELS,
        "maxFrames": SAMPLE_RATE_HZ * 10,
        "maxChunkBytes": 65_536,
        "expiresQpc": u64::MAX,
        "outputSelectionMode": "systemDefault",
        "outputEndpointId": "{0.0.0.00000000}.controlled-headless-output",
        "outputEndpointGeneration": OUTPUT_GENERATION
    }))
    .expect("valid controlled playback lease")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_cue_and_pcm_share_the_admitted_audio_clock_until_cancellation() {
    let endpoint = format!(
        r"\\.\pipe\npc-media-playback-cue-clock-{}",
        Uuid::new_v4().simple()
    );
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&endpoint)
        .expect("create controlled native broker endpoint");
    let chunk_seen = Arc::new(Notify::new());
    let server_chunk_seen = Arc::clone(&chunk_seen);
    let server_task = tokio::spawn(async move {
        let mut server = server;
        server
            .connect()
            .await
            .expect("connect production broker client");
        let mut commands = Vec::new();
        loop {
            let command = read_command(&mut server).await;
            respond_ok(&mut server, &command).await;
            let terminal = command.kind == CommandKind::Cancel;
            if command.kind == CommandKind::Chunk {
                server_chunk_seen.notify_one();
            }
            commands.push(command);
            if terminal {
                break commands;
            }
        }
    });

    let captured_request = Arc::new(Mutex::new(None));
    let upstream_cancellations = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn StreamingTtsProvider> = Arc::new(CueProvider {
        request: Arc::clone(&captured_request),
        cancellations: Arc::clone(&upstream_cancellations),
    });
    let output = AudioFormat {
        encoding: PcmEncoding::PcmS16Le,
        sample_rate_hz: SAMPLE_RATE_HZ,
        channels: CHANNELS,
    };
    let bridge = RuntimeTtsBridge::new(
        provider,
        RuntimeTtsBridgeConfig::selected_stock(
            descriptor(),
            "selected.stock.voice",
            "sonic-3.6",
            output,
        ),
    )
    .expect("construct production TTS bridge");
    let identity = identity();
    let cancellation = CancellationToken::new();
    let mut session = bridge
        .start_session(&identity, "en-US", cancellation.clone())
        .await
        .expect("start runtime TTS session");
    let speech = session
        .synthesize(
            SpeechRequest {
                identity: identity.clone(),
                sentence_id: 1,
                text: "Please open the sealed gate.".into(),
                locale: "en-US".into(),
                voice_hint: None,
            },
            cancellation.clone(),
        )
        .await
        .expect("start cue-bearing speech stream");

    let sink = Arc::new(BrokerAudioSink::production(vec![lease(endpoint)]));
    let playback_sink = Arc::clone(&sink);
    let playback_identity = identity.clone();
    let playback_cancellation = cancellation.clone();
    let playback = tokio::spawn(async move {
        playback_sink
            .play_submitted(&playback_identity, speech, playback_cancellation)
            .await
    });

    tokio::time::timeout(Duration::from_secs(2), chunk_seen.notified())
        .await
        .expect("first PCM command must reach the controlled broker");
    sink.stop(&identity)
        .await
        .expect("sink cancellation reaches its active broker operation");
    let error = tokio::time::timeout(Duration::from_secs(2), playback)
        .await
        .expect("cancelled playback settles")
        .expect("playback task does not panic")
        .expect_err("cancellation cannot produce a delivery receipt");
    assert!(
        matches!(error, RuntimeDependencyError::Cancelled),
        "unexpected cancellation result: {error:?}"
    );
    let commands = tokio::time::timeout(Duration::from_secs(2), server_task)
        .await
        .expect("controlled broker observes cancellation")
        .expect("controlled broker task does not panic");

    assert_eq!(
        sink.remaining_leases().unwrap(),
        0,
        "one lease is consumed once"
    );
    assert_eq!(
        commands
            .iter()
            .map(|command| command.kind)
            .collect::<Vec<_>>(),
        vec![
            CommandKind::Begin,
            CommandKind::VisualCue,
            CommandKind::Chunk,
            CommandKind::Cancel,
        ],
        "the native cue must precede the first PCM chunk and cancellation closes the same stream"
    );
    assert_eq!(
        commands
            .iter()
            .map(|command| command.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    for command in &commands {
        assert!(command.deadline_qpc > 0);
        assert_eq!(command.generation, GENERATION);
        assert_eq!(command.stream_id, "cue-clock-stream");
        assert_eq!(command.session_id, identity.session_id);
        assert_eq!(command.turn_id, identity.turn_id);
        assert_eq!(command.token, [TOKEN_BYTE; TOKEN_BYTES]);
    }

    let cue = commands
        .iter()
        .find(|command| command.kind == CommandKind::VisualCue)
        .expect("provider phoneme becomes one native visual cue");
    assert_eq!(cue.payload.len(), 24);
    assert_eq!(
        u32::from_le_bytes(cue.payload[0..4].try_into().unwrap()),
        VISUAL_CUE_SCHEMA_VERSION
    );
    assert_eq!(
        u64::from_le_bytes(cue.payload[4..12].try_into().unwrap()),
        1_440,
        "60 ms is represented on the admitted 24 kHz source-sample clock"
    );
    assert_eq!(
        u64::from_le_bytes(cue.payload[12..20].try_into().unwrap()),
        480,
        "20 ms is represented on the admitted 24 kHz source-sample clock"
    );
    assert_eq!(cue.payload[20], 1, "Cartesia phoneme p selects bilabial");
    assert_eq!(cue.payload[21], 0);
    assert_eq!(
        u16::from_le_bytes(cue.payload[22..24].try_into().unwrap()),
        32_767
    );

    let chunk = commands
        .iter()
        .find(|command| command.kind == CommandKind::Chunk)
        .expect("first bridge PCM reaches the broker");
    assert_eq!(
        chunk
            .payload
            .chunks_exact(2)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect::<Vec<_>>(),
        vec![1_000, -1_000, 500],
        "the bridge withholds only the final PCM frame for an eventual EOS marker"
    );

    let request = captured_request
        .lock()
        .expect("request mutex poisoned")
        .clone()
        .expect("bridge started one provider session");
    assert_eq!(request.identity.session_id, identity.session_id);
    assert_eq!(request.identity.turn_id, identity.turn_id);
    assert_eq!(request.identity.cancellation_generation, GENERATION);
    assert_eq!(request.output, output);
    assert!(request.request_alignment);
    assert!(request.request_visemes);
    assert!(
        !cancellation.is_cancelled(),
        "sink-scoped cancellation must not poison the parent turn token"
    );
    assert!(
        upstream_cancellations.load(Ordering::SeqCst) <= 1,
        "dropping a cancelled bridge stream may race its best-effort upstream cancel"
    );
}
