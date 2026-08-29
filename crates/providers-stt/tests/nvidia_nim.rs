use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use npc_providers_stt::{
    AudioFormat, FlushReason, GrpcStatusCode, NvidiaGrpcConnectRequest, NvidiaGrpcError,
    NvidiaGrpcStream, NvidiaGrpcTransportFactory, NvidiaNimAsr, ProviderLifecycle,
    RecognitionConfig, RecognitionEvent, RetentionPolicy, RivaStreamingRequest,
    RivaStreamingResponse, RivaStreamingResult, SecretString, SttErrorKind, TranscriptStatus,
    TurnEndReason, NVIDIA_NIM_ENDPOINT, NVIDIA_NIM_FUNCTION_ID, NVIDIA_NIM_MODEL,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct MockGrpcFactory {
    state: Arc<MockState>,
}

struct MockState {
    connect_error: Option<NvidiaGrpcError>,
    inbound: Mutex<VecDeque<Result<Option<RivaStreamingResponse>, NvidiaGrpcError>>>,
    sent: Mutex<Vec<RivaStreamingRequest>>,
    connection: Mutex<Option<CapturedConnection>>,
    finish_count: Mutex<usize>,
    cancel_count: Mutex<usize>,
}

#[derive(Clone)]
struct CapturedConnection {
    endpoint: String,
    tls: bool,
    function_id: String,
    authorization_scheme: &'static str,
    credential_len: usize,
    debug: String,
}

impl MockGrpcFactory {
    fn responses(
        responses: impl IntoIterator<Item = Result<Option<RivaStreamingResponse>, NvidiaGrpcError>>,
    ) -> Self {
        Self {
            state: Arc::new(MockState {
                connect_error: None,
                inbound: Mutex::new(responses.into_iter().collect()),
                sent: Mutex::new(Vec::new()),
                connection: Mutex::new(None),
                finish_count: Mutex::new(0),
                cancel_count: Mutex::new(0),
            }),
        }
    }

    fn connect_error(error: NvidiaGrpcError) -> Self {
        Self {
            state: Arc::new(MockState {
                connect_error: Some(error),
                inbound: Mutex::new(VecDeque::new()),
                sent: Mutex::new(Vec::new()),
                connection: Mutex::new(None),
                finish_count: Mutex::new(0),
                cancel_count: Mutex::new(0),
            }),
        }
    }

    fn connection(&self) -> CapturedConnection {
        self.state
            .connection
            .lock()
            .expect("connection lock")
            .clone()
            .expect("connection captured")
    }
}

struct MockGrpcStream {
    state: Arc<MockState>,
}

#[async_trait]
impl NvidiaGrpcStream for MockGrpcStream {
    async fn send(&mut self, request: RivaStreamingRequest) -> Result<(), NvidiaGrpcError> {
        self.state.sent.lock().expect("sent lock").push(request);
        Ok(())
    }

    async fn finish_input(&mut self) -> Result<(), NvidiaGrpcError> {
        *self.state.finish_count.lock().expect("finish lock") += 1;
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<RivaStreamingResponse>, NvidiaGrpcError> {
        self.state
            .inbound
            .lock()
            .expect("inbound lock")
            .pop_front()
            .unwrap_or(Ok(None))
    }

    async fn cancel(&mut self) -> Result<(), NvidiaGrpcError> {
        *self.state.cancel_count.lock().expect("cancel lock") += 1;
        Ok(())
    }
}

#[async_trait]
impl NvidiaGrpcTransportFactory for MockGrpcFactory {
    async fn connect(
        &self,
        request: NvidiaGrpcConnectRequest<'_>,
    ) -> Result<Box<dyn NvidiaGrpcStream>, NvidiaGrpcError> {
        *self.state.connection.lock().expect("connection lock") = Some(CapturedConnection {
            endpoint: request.endpoint.to_owned(),
            tls: request.use_tls,
            function_id: request.function_id.to_owned(),
            authorization_scheme: request.authorization_scheme,
            credential_len: request.credential.expose().len(),
            debug: format!("{request:?}"),
        });
        if let Some(error) = self.state.connect_error {
            return Err(error);
        }
        Ok(Box::new(MockGrpcStream {
            state: Arc::clone(&self.state),
        }))
    }
}

fn config() -> RecognitionConfig {
    RecognitionConfig {
        model: NVIDIA_NIM_MODEL.into(),
        audio: AudioFormat::PCM_16KHZ_MONO,
        languages: vec!["en-US".into()],
        endpointing: npc_providers_stt::EndpointingMode::ProviderVad,
        interim_results: true,
        keyword_hints: vec!["Night City".into()],
        context_hint: Some("Cyberpunk character names".into()),
        retention: RetentionPolicy::ProviderDefaultAllowed,
        timeouts: Default::default(),
    }
}

fn response(result: RivaStreamingResult) -> Result<Option<RivaStreamingResponse>, NvidiaGrpcError> {
    Ok(Some(RivaStreamingResponse {
        results: vec![result],
    }))
}

#[tokio::test]
async fn curated_config_and_metadata_are_fixed_redacted_and_experimental() {
    let factory = MockGrpcFactory::responses([Ok(None)]);
    let recognizer = NvidiaNimAsr::new(Arc::new(factory.clone()));
    let mut session = recognizer
        .start_session(
            config(),
            SecretString::new("nvapi-secret-value"),
            CancellationToken::new(),
        )
        .await
        .expect("NVIDIA session starts");
    session
        .send_audio(&[0, 0, 1, 0])
        .await
        .expect("audio accepted");
    session
        .flush(FlushReason::PushToTalkReleased)
        .await
        .expect("input half-close accepted");

    let connection = factory.connection();
    assert_eq!(connection.endpoint, NVIDIA_NIM_ENDPOINT);
    assert!(connection.tls);
    assert_eq!(connection.function_id, NVIDIA_NIM_FUNCTION_ID);
    assert_eq!(connection.authorization_scheme, "Bearer");
    assert_eq!(connection.credential_len, "nvapi-secret-value".len());
    assert!(!connection.debug.contains("nvapi-secret-value"));
    assert!(connection.debug.contains("[REDACTED]"));

    let sent = factory.state.sent.lock().expect("sent lock");
    let RivaStreamingRequest::Config(riva_config) = &sent[0] else {
        panic!("configuration must be the first Riva request");
    };
    assert_eq!(riva_config.model, NVIDIA_NIM_MODEL);
    assert_eq!(riva_config.language_code, "en-US");
    assert_eq!(riva_config.sample_rate_hz, 16_000);
    assert_eq!(riva_config.channels, 1);
    assert!(riva_config.interim_results);
    assert_eq!(riva_config.speech_contexts.len(), 2);
    let config_debug = format!("{riva_config:?}");
    assert!(!config_debug.contains("Night City"));
    assert!(!config_debug.contains("Cyberpunk"));
    assert!(matches!(sent[1], RivaStreamingRequest::Audio(_)));
    assert!(!format!("{:?}", sent[1]).contains("[0, 0, 1, 0]"));
    assert_eq!(*factory.state.finish_count.lock().expect("finish lock"), 1);

    let capabilities = recognizer.capabilities();
    assert_eq!(capabilities.lifecycle, ProviderLifecycle::Experimental);
    assert_eq!(capabilities.default_model, NVIDIA_NIM_MODEL);
    assert!(capabilities
        .availability_note
        .is_some_and(|note| note.contains("rate limited")));
}

#[tokio::test]
async fn riva_partial_and_final_results_get_monotonic_revisions_and_endpoint() {
    let factory = MockGrpcFactory::responses([
        response(RivaStreamingResult {
            transcript: "Wake up".into(),
            is_final: false,
            stability: Some(0.61),
            confidence: None,
            audio_processed_seconds: Some(0.4),
        }),
        response(RivaStreamingResult {
            transcript: "Wake up, Samurai.".into(),
            is_final: true,
            stability: None,
            confidence: Some(0.94),
            audio_processed_seconds: Some(0.9),
        }),
        Ok(None),
    ]);
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let mut session = recognizer
        .start_session(
            config(),
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
        .expect("NVIDIA session starts");
    let mut events = Vec::new();
    while let Some(event) = session.next_event().await.expect("normalized event") {
        events.push(event);
    }

    let transcripts: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            RecognitionEvent::Transcript(transcript) => Some(transcript),
            _ => None,
        })
        .collect();
    assert_eq!(transcripts.len(), 2);
    assert_eq!(transcripts[0].revision, 1);
    assert_eq!(transcripts[0].status, TranscriptStatus::Partial);
    assert_eq!(transcripts[0].audio_end_ms, Some(400));
    assert_eq!(transcripts[1].revision, 2);
    assert_eq!(transcripts[1].status, TranscriptStatus::Final);
    assert!(events.iter().any(|event| matches!(
        event,
        RecognitionEvent::TurnEnded {
            revision: 2,
            reason: TurnEndReason::ProviderEndpoint,
            ..
        }
    )));
}

#[tokio::test]
async fn cancellation_discards_queued_riva_data_and_emits_once() {
    let factory = MockGrpcFactory::responses([response(RivaStreamingResult {
        transcript: "must disappear".into(),
        is_final: false,
        stability: Some(0.5),
        confidence: None,
        audio_processed_seconds: None,
    })]);
    let recognizer = NvidiaNimAsr::new(Arc::new(factory.clone()));
    let mut session = recognizer
        .start_session(
            config(),
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
        .expect("NVIDIA session starts");
    session.cancel().await.expect("cancel accepted");
    assert_eq!(
        session.next_event().await.expect("cancel event"),
        Some(RecognitionEvent::Cancelled)
    );
    assert_eq!(session.next_event().await.expect("terminal none"), None);
    assert_eq!(*factory.state.cancel_count.lock().expect("cancel lock"), 1);
}

#[tokio::test]
async fn development_api_429_is_sanitized_and_retryable() {
    let factory = MockGrpcFactory::connect_error(NvidiaGrpcError {
        code: GrpcStatusCode::ResourceExhausted,
        http_status: Some(429),
        retry_after: Some(Duration::from_secs(2)),
    });
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let error = match recognizer
        .start_session(
            config(),
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("rate-limited connection should fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind, SttErrorKind::RateLimited);
    assert!(error.retryable);
    assert_eq!(error.retry_after, Some(Duration::from_secs(2)));
    assert_eq!(error.provider_code.as_deref(), Some("rate_limited"));
}

#[tokio::test]
async fn authentication_failure_is_sanitized_and_non_retryable() {
    let factory = MockGrpcFactory::connect_error(NvidiaGrpcError {
        code: GrpcStatusCode::Unauthenticated,
        http_status: Some(401),
        retry_after: None,
    });
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let error = match recognizer
        .start_session(
            config(),
            SecretString::new("secret-that-must-never-escape"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("authentication failure should stop the session"),
        Err(error) => error,
    };
    assert_eq!(error.kind, SttErrorKind::Authentication);
    assert!(!error.retryable);
    assert!(!format!("{error:?}").contains("secret-that-must-never-escape"));
}

#[tokio::test]
async fn grpc_deadline_exceeded_maps_to_retryable_timeout() {
    let factory = MockGrpcFactory::connect_error(NvidiaGrpcError {
        code: GrpcStatusCode::DeadlineExceeded,
        http_status: Some(504),
        retry_after: None,
    });
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let error = match recognizer
        .start_session(
            config(),
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("deadline failure should stop the session"),
        Err(error) => error,
    };
    assert_eq!(error.kind, SttErrorKind::Timeout);
    assert!(error.retryable);
    assert_eq!(error.provider_code.as_deref(), Some("deadline_exceeded"));
}

#[tokio::test]
async fn unavailable_stream_error_is_sanitized() {
    let factory = MockGrpcFactory::responses([Err(NvidiaGrpcError {
        code: GrpcStatusCode::Unavailable,
        http_status: Some(503),
        retry_after: None,
    })]);
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let mut session = recognizer
        .start_session(
            config(),
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
        .expect("NVIDIA session starts");
    assert!(matches!(
        session.next_event().await.expect("session started"),
        Some(RecognitionEvent::SessionStarted { .. })
    ));
    let error = session.next_event().await.expect_err("unavailable error");
    assert_eq!(error.kind, SttErrorKind::Unavailable);
    assert!(error.retryable);
    assert!(!format!("{error:?}").contains("not-a-live-key"));
}

#[tokio::test]
async fn profile_cannot_replace_curated_model() {
    let factory = MockGrpcFactory::responses([]);
    let recognizer = NvidiaNimAsr::new(Arc::new(factory));
    let mut wrong = config();
    wrong.model = "profile-supplied-model".into();
    let error = match recognizer
        .start_session(
            wrong,
            SecretString::new("not-a-live-key"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("profile model override should fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind, SttErrorKind::InvalidRequest);
}

#[test]
fn response_debug_redacts_transcript_content() {
    let response = RivaStreamingResponse {
        results: vec![RivaStreamingResult {
            transcript: "sensitive spoken content".into(),
            is_final: false,
            stability: Some(0.5),
            confidence: None,
            audio_processed_seconds: None,
        }],
    };
    let debug = format!("{response:?}");
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("sensitive spoken content"));
}
