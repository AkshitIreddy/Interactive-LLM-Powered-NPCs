use std::collections::VecDeque;
use std::future::pending;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use npc_providers_stt::{
    AssemblyAi, AudioFormat, ClientFrame, ConnectRequest, DeepgramFlux, ElevenLabsScribe,
    EndpointingMode, FlushReason, HostedRecognizer, OpenAiRealtime, RecognitionConfig,
    RecognitionEvent, RetentionPolicy, SecretString, ServerFrame, StreamingTransport, SttErrorKind,
    TranscriptStatus, TransportAuth, TransportError, TransportFactory,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct MockFactory {
    state: Arc<MockState>,
}

struct MockState {
    inbound: Mutex<VecDeque<ServerFrame>>,
    sent: Mutex<Vec<ClientFrame>>,
    requests: Mutex<Vec<CapturedRequest>>,
    close_count: Mutex<usize>,
}

#[derive(Clone)]
struct CapturedRequest {
    url: &'static str,
    query: Vec<(String, String)>,
    auth_name: &'static str,
    auth_scheme: Option<&'static str>,
    credential_len: usize,
}

impl MockFactory {
    fn new(frames: impl IntoIterator<Item = ServerFrame>) -> Self {
        Self {
            state: Arc::new(MockState {
                inbound: Mutex::new(frames.into_iter().collect()),
                sent: Mutex::new(Vec::new()),
                requests: Mutex::new(Vec::new()),
                close_count: Mutex::new(0),
            }),
        }
    }

    fn sent_text_types(&self) -> Vec<String> {
        self.state
            .sent
            .lock()
            .expect("sent lock")
            .iter()
            .filter_map(|frame| match frame {
                ClientFrame::Text(text) => serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("type")
                            .or_else(|| value.get("message_type"))
                            .cloned()
                    })
                    .and_then(|value| value.as_str().map(str::to_owned)),
                _ => None,
            })
            .collect()
    }

    fn request(&self) -> CapturedRequest {
        self.state.requests.lock().expect("request lock")[0].clone()
    }
}

struct MockTransport {
    state: Arc<MockState>,
}

struct PendingFactory;

#[async_trait]
impl TransportFactory for PendingFactory {
    async fn connect(
        &self,
        _request: ConnectRequest<'_>,
    ) -> Result<Box<dyn StreamingTransport>, TransportError> {
        pending().await
    }
}

#[async_trait]
impl StreamingTransport for MockTransport {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), TransportError> {
        self.state.sent.lock().expect("sent lock").push(frame);
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<ServerFrame>, TransportError> {
        Ok(self.state.inbound.lock().expect("inbound lock").pop_front())
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        *self.state.close_count.lock().expect("close lock") += 1;
        Ok(())
    }
}

#[async_trait]
impl TransportFactory for MockFactory {
    async fn connect(
        &self,
        request: ConnectRequest<'_>,
    ) -> Result<Box<dyn StreamingTransport>, TransportError> {
        let npc_providers_stt::TransportAuth::Header {
            name,
            scheme,
            value,
        } = request.auth;
        self.state
            .requests
            .lock()
            .expect("request lock")
            .push(CapturedRequest {
                url: request.url,
                query: request.query,
                auth_name: name,
                auth_scheme: scheme,
                // The mock observes only non-secret metadata while the credential borrow is valid.
                credential_len: value.expose().len(),
            });
        Ok(Box::new(MockTransport {
            state: Arc::clone(&self.state),
        }))
    }
}

fn frame(value: serde_json::Value) -> ServerFrame {
    ServerFrame::Text(serde_json::to_string(&value).expect("fixture json"))
}

fn config(model: &str) -> RecognitionConfig {
    RecognitionConfig {
        model: model.into(),
        audio: AudioFormat::PCM_16KHZ_MONO,
        languages: vec!["en".into()],
        endpointing: EndpointingMode::Manual,
        interim_results: true,
        keyword_hints: vec!["Night City".into()],
        context_hint: None,
        retention: RetentionPolicy::ProviderDefaultAllowed,
        timeouts: Default::default(),
    }
}

async fn collect_events(session: &mut npc_providers_stt::HostedSession) -> Vec<RecognitionEvent> {
    let mut events = Vec::new();
    while let Some(event) = session.next_event().await.expect("normalized event") {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn deepgram_normalizes_revisioned_eager_resume_and_final_events() {
    let factory = MockFactory::new([
        frame(serde_json::json!({"type":"Connected","request_id":"dg-session"})),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"StartOfTurn","turn_index":0,"transcript":""
        })),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"Update","turn_index":0,
            "transcript":"Hello","audio_window_start":0.0,"audio_window_end":0.4,
            "end_of_turn_confidence":0.2,"languages":["en"]
        })),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"EagerEndOfTurn","turn_index":0,
            "transcript":"Hello there.","audio_window_start":0.0,"audio_window_end":0.8,
            "end_of_turn_confidence":0.5,"languages":["en"]
        })),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"TurnResumed","turn_index":0,"transcript":"Hello there"
        })),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"Update","turn_index":0,
            "transcript":"Hello there V","audio_window_start":0.0,"audio_window_end":1.0,
            "end_of_turn_confidence":0.3,"languages":["en"]
        })),
        frame(serde_json::json!({
            "type":"TurnInfo","event":"EndOfTurn","turn_index":0,
            "transcript":"Hello there, V.","audio_window_start":0.0,"audio_window_end":1.2,
            "end_of_turn_confidence":0.9,"languages":["en"]
        })),
    ]);
    let recognizer = HostedRecognizer::new(DeepgramFlux, Arc::new(factory.clone()));
    let mut session = recognizer
        .start_session(
            config("flux-general-multi"),
            SecretString::new("deepgram-secret"),
            CancellationToken::new(),
        )
        .await
        .expect("session starts");

    let events = collect_events(&mut session).await;
    let revisions: Vec<(u64, TranscriptStatus)> = events
        .iter()
        .filter_map(|event| match event {
            RecognitionEvent::Transcript(transcript) => {
                Some((transcript.revision, transcript.status))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        revisions,
        vec![
            (1, TranscriptStatus::Partial),
            (2, TranscriptStatus::EagerFinal),
            (4, TranscriptStatus::Partial),
            (5, TranscriptStatus::Final),
        ]
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, RecognitionEvent::TurnResumed { revision: 3, .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event, RecognitionEvent::TurnEnded { revision: 5, .. })));

    let request = factory.request();
    assert!(request
        .query
        .contains(&("keyterm".into(), "Night City".into())));
    assert_eq!(request.url, "wss://api.deepgram.com/v2/listen");
    assert_eq!(request.auth_name, "Authorization");
    assert_eq!(request.auth_scheme, Some("Token"));
    assert_eq!(request.credential_len, "deepgram-secret".len());
}

#[tokio::test]
async fn assemblyai_flushes_and_maps_running_turn_to_partial_then_final() {
    let factory = MockFactory::new([
        frame(serde_json::json!({"type":"Begin","id":"assembly-session"})),
        frame(serde_json::json!({
            "type":"Turn","turn_order":7,"end_of_turn":false,
            "transcript":"Need a ripperdoc","utterance":"","language_code":"en"
        })),
        frame(serde_json::json!({
            "type":"Turn","turn_order":7,"end_of_turn":true,
            "transcript":"Need a ripperdoc.","utterance":"Need a ripperdoc.",
            "end_of_turn_confidence":0.91,"language_code":"en"
        })),
    ]);
    let recognizer = HostedRecognizer::new(AssemblyAi, Arc::new(factory.clone()));
    let mut assembly_config = config("u3-rt-pro");
    assembly_config.context_hint = Some("Cyberpunk character names".into());
    let mut session = recognizer
        .start_session(
            assembly_config,
            SecretString::new("assembly-secret"),
            CancellationToken::new(),
        )
        .await
        .expect("session starts");
    session
        .flush(FlushReason::PushToTalkReleased)
        .await
        .expect("flush accepted");
    let events = collect_events(&mut session).await;
    assert!(events.iter().any(|event| matches!(
        event,
        RecognitionEvent::Transcript(transcript)
            if transcript.revision == 1 && transcript.status == TranscriptStatus::Partial
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RecognitionEvent::Transcript(transcript)
            if transcript.revision == 2 && transcript.status == TranscriptStatus::Final
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RecognitionEvent::TurnEnded {
            reason: npc_providers_stt::TurnEndReason::ManualFlush,
            ..
        }
    )));
    assert!(factory.sent_text_types().contains(&"ForceEndpoint".into()));
}

#[tokio::test]
async fn elevenlabs_maps_commits_and_surfaces_sanitized_provider_errors() {
    let factory = MockFactory::new([
        frame(serde_json::json!({"message_type":"session_started","session_id":"scribe-session"})),
        frame(serde_json::json!({"message_type":"partial_transcript","text":"Meet me at"})),
        frame(
            serde_json::json!({"message_type":"committed_transcript","text":"Meet me at Embers."}),
        ),
        frame(serde_json::json!({
            "message_type":"rate_limited",
            "error":"sensitive provider body that must not escape"
        })),
    ]);
    let recognizer = HostedRecognizer::new(ElevenLabsScribe, Arc::new(factory));
    let mut scribe_config = config("scribe_v2_realtime");
    scribe_config.keyword_hints = vec!["Embers".into()];
    let mut session = recognizer
        .start_session(
            scribe_config,
            SecretString::new("eleven-secret"),
            CancellationToken::new(),
        )
        .await
        .expect("session starts");
    assert!(matches!(
        session.next_event().await.expect("session event"),
        Some(RecognitionEvent::SessionStarted { .. })
    ));
    assert!(matches!(
        session.next_event().await.expect("turn start"),
        Some(RecognitionEvent::TurnStarted { .. })
    ));
    assert!(matches!(
        session.next_event().await.expect("partial"),
        Some(RecognitionEvent::Transcript(ref transcript))
            if transcript.status == TranscriptStatus::Partial
    ));
    assert!(matches!(
        session.next_event().await.expect("final"),
        Some(RecognitionEvent::Transcript(ref transcript))
            if transcript.status == TranscriptStatus::Final
    ));
    assert!(matches!(
        session.next_event().await.expect("end"),
        Some(RecognitionEvent::TurnEnded { .. })
    ));
    let error = session.next_event().await.expect_err("provider error");
    assert_eq!(error.kind, SttErrorKind::RateLimited);
    assert_eq!(error.provider_code.as_deref(), Some("rate_limited"));
    assert!(!format!("{error:?}").contains("sensitive provider body"));
}

#[tokio::test]
async fn openai_accumulates_deltas_into_revisioned_complete_text() {
    let factory = MockFactory::new([
        frame(serde_json::json!({
            "type":"transcription_session.created","session":{"id":"openai-session"}
        })),
        frame(serde_json::json!({
            "type":"conversation.item.input_audio_transcription.delta",
            "item_id":"item-1","delta":"Where is "
        })),
        frame(serde_json::json!({
            "type":"conversation.item.input_audio_transcription.delta",
            "item_id":"item-1","delta":"Shadowheart?"
        })),
        frame(serde_json::json!({
            "type":"conversation.item.input_audio_transcription.completed",
            "item_id":"item-1","transcript":"Where is Shadowheart?","language":"en"
        })),
    ]);
    let recognizer = HostedRecognizer::new(OpenAiRealtime, Arc::new(factory.clone()));
    let mut openai_config = config("gpt-4o-mini-transcribe");
    openai_config.audio = AudioFormat {
        encoding: npc_providers_stt::AudioEncoding::PcmS16Le,
        sample_rate_hz: 24_000,
        channels: 1,
    };
    openai_config.context_hint = Some("Baldur's Gate companion names".into());
    let mut session = recognizer
        .start_session(
            openai_config,
            SecretString::new("openai-secret"),
            CancellationToken::new(),
        )
        .await
        .expect("session starts");
    let events = collect_events(&mut session).await;
    let transcripts: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            RecognitionEvent::Transcript(transcript) => Some(transcript),
            _ => None,
        })
        .collect();
    assert_eq!(transcripts[0].text, "Where is ");
    assert_eq!(transcripts[0].revision, 1);
    assert_eq!(transcripts[1].text, "Where is Shadowheart?");
    assert_eq!(transcripts[1].revision, 2);
    assert_eq!(transcripts[2].status, TranscriptStatus::Final);
    assert_eq!(transcripts[2].revision, 3);
    assert!(factory
        .sent_text_types()
        .contains(&"transcription_session.update".into()));
}

#[tokio::test]
async fn cancellation_emits_once_and_discards_all_queued_provider_data() {
    let factory = MockFactory::new([
        frame(serde_json::json!({"message_type":"partial_transcript","text":"must disappear"})),
        frame(serde_json::json!({"message_type":"committed_transcript","text":"also disappears"})),
    ]);
    let recognizer = HostedRecognizer::new(ElevenLabsScribe, Arc::new(factory.clone()));
    let mut scribe_config = config("scribe_v2_realtime");
    scribe_config.keyword_hints.clear();
    let cancellation = CancellationToken::new();
    let mut session = recognizer
        .start_session(
            scribe_config,
            SecretString::new("eleven-secret"),
            cancellation,
        )
        .await
        .expect("session starts");
    session.cancel().await.expect("cancel succeeds");

    assert_eq!(
        session.next_event().await.expect("cancel event"),
        Some(RecognitionEvent::Cancelled)
    );
    assert_eq!(session.next_event().await.expect("terminal none"), None);
    assert_eq!(session.next_event().await.expect("still terminal"), None);
    assert_eq!(*factory.state.close_count.lock().expect("close lock"), 1);
}

#[tokio::test]
async fn external_cancellation_preempts_queued_data_and_blocks_future_audio() {
    let factory = MockFactory::new([frame(serde_json::json!({
        "type":"TurnInfo","event":"Update","turn_index":0,"transcript":"stale"
    }))]);
    let recognizer = HostedRecognizer::new(DeepgramFlux, Arc::new(factory));
    let cancellation = CancellationToken::new();
    let mut deepgram_config = config("flux-general-en");
    deepgram_config.keyword_hints.clear();
    let mut session = recognizer
        .start_session(
            deepgram_config,
            SecretString::new("deepgram-secret"),
            cancellation.clone(),
        )
        .await
        .expect("session starts");
    cancellation.cancel();

    assert_eq!(
        session.next_event().await.expect("cancel event"),
        Some(RecognitionEvent::Cancelled)
    );
    let error = session
        .send_audio(&[0, 0])
        .await
        .expect_err("audio rejected");
    assert_eq!(error.kind, SttErrorKind::Cancelled);
    assert_eq!(session.next_event().await.expect("no stale data"), None);
}

#[tokio::test]
async fn connect_timeout_is_normalized_without_transport_details() {
    let recognizer = HostedRecognizer::new(DeepgramFlux, Arc::new(PendingFactory));
    let mut deepgram_config = config("flux-general-en");
    deepgram_config.keyword_hints.clear();
    deepgram_config.timeouts.connect = Duration::from_millis(1);
    let error = match recognizer
        .start_session(
            deepgram_config,
            SecretString::new("deepgram-secret"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("connection should time out"),
        Err(error) => error,
    };
    assert_eq!(error.kind, SttErrorKind::Timeout);
    assert_eq!(error.message, "provider connection timed out");
}

#[tokio::test]
async fn strict_elevenlabs_retention_fails_closed_when_provider_warns() {
    let factory = MockFactory::new([frame(serde_json::json!({
        "message_type":"warning",
        "warning":"Zero retention mode could not be applied; session logging remains enabled"
    }))]);
    let recognizer = HostedRecognizer::new(ElevenLabsScribe, Arc::new(factory.clone()));
    let mut scribe_config = config("scribe_v2_realtime");
    scribe_config.keyword_hints.clear();
    scribe_config.retention = RetentionPolicy::RequireRequestLevelOptOut;
    let mut session = recognizer
        .start_session(
            scribe_config,
            SecretString::new("eleven-secret"),
            CancellationToken::new(),
        )
        .await
        .expect("session starts pending provider confirmation");
    let error = session
        .next_event()
        .await
        .expect_err("privacy downgrade rejected");
    assert_eq!(error.kind, SttErrorKind::PrivacyPolicy);
    assert!(!format!("{error:?}").contains("session logging remains enabled"));
    assert!(factory
        .request()
        .query
        .contains(&("enable_logging".into(), "false".into())));
}

#[test]
fn secrets_and_frames_are_redacted_from_debug_output() {
    let secret = SecretString::new("top-secret-key");
    assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
    let request = ConnectRequest {
        url: "wss://example.invalid",
        query: vec![("prompt".into(), "private game context".into())],
        auth: TransportAuth::Header {
            name: "Authorization",
            scheme: Some("Bearer"),
            value: &secret,
        },
    };
    let debug = format!("{request:?}");
    assert!(debug.contains("prompt"));
    assert!(!debug.contains("private game context"));
    assert!(!debug.contains("top-secret-key"));

    let frame = ClientFrame::Text("private transcript".into());
    let debug = format!("{frame:?}");
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("private transcript"));

    let transcript = npc_providers_stt::TranscriptEvent {
        turn_id: "turn-1".into(),
        revision: 1,
        text: "private transcript".into(),
        status: TranscriptStatus::Partial,
        language: Some("en".into()),
        confidence: None,
        audio_start_ms: None,
        audio_end_ms: None,
    };
    let debug = format!("{transcript:?}");
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("private transcript"));
}
