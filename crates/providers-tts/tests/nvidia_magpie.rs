use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use npc_providers_tts::*;

#[derive(Default)]
struct FixtureCredential {
    resolutions: AtomicUsize,
}

#[async_trait::async_trait]
impl ProviderCredentialResolver for FixtureCredential {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        assert_eq!(provider_id, HostedTtsProviderId::NvidiaNimMagpie);
        self.resolutions.fetch_add(1, Ordering::SeqCst);
        Ok(SensitiveString::new("fixture-nvidia-api-key"))
    }
}

fn aria() -> NvidiaStockVoice {
    NvidiaStockVoice {
        id: "Magpie-Multilingual.EN-US.Aria".into(),
        display_name: "Aria".into(),
        locale: "en-US".into(),
        origin: NvidiaVoiceOrigin::ProviderStock,
    }
}

fn binding(options: BTreeMap<String, String>) -> VoiceBinding {
    VoiceBinding {
        intent_id: "clear-companion".into(),
        provider_id: HostedTtsProviderId::NvidiaNimMagpie,
        voice_id: aria().id,
        model_id: NVIDIA_MAGPIE_MODEL_ID.into(),
        provider_options: options,
    }
}

fn request() -> TtsSessionRequest {
    TtsSessionRequest {
        identity: SessionIdentity {
            session_id: "session-nvidia".into(),
            turn_id: "turn-9".into(),
            cancellation_generation: 2,
        },
        locale: "en-US".into(),
        voice_intent_id: "clear-companion".into(),
        output: AudioFormat {
            encoding: PcmEncoding::PcmS16Le,
            sample_rate_hz: 22_050,
            channels: 1,
        },
        request_alignment: true,
        request_visemes: false,
        clause_policy: SemanticClausePolicy::default(),
    }
}

struct Fixture {
    provider: NvidiaNimMagpie,
    grpc: Arc<MockNvidiaGrpcTransport>,
    http: Arc<MockNvidiaHttpTransport>,
    credential: Arc<FixtureCredential>,
}

async fn fixture(scripts: impl IntoIterator<Item = MockNvidiaStreamScript>) -> Fixture {
    let grpc = Arc::new(MockNvidiaGrpcTransport::scripted(scripts));
    let http = Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![aria()])));
    let credential = Arc::new(FixtureCredential::default());
    let provider = NvidiaNimMagpie::new(
        Arc::clone(&grpc) as Arc<dyn NvidiaNimGrpcTransport>,
        Arc::clone(&http) as Arc<dyn NvidiaNimHttpTransport>,
        Arc::clone(&credential) as Arc<dyn ProviderCredentialResolver>,
        VoiceBindings::new([binding(BTreeMap::new())]).expect("valid binding"),
    );
    provider
        .discover_stock_voices()
        .await
        .expect("fixture stock discovery");
    Fixture {
        provider,
        grpc,
        http,
        credential,
    }
}

#[tokio::test]
async fn discovery_is_fixed_to_curated_function_subdomain_and_stock_ids() {
    let fixture = fixture([]).await;
    assert_eq!(
        fixture.provider.lifecycle(),
        NvidiaMagpieLifecycle::ExperimentalHttpSmokeQualifiedAwaitingGrpcStreamingQualification
    );
    assert_eq!(fixture.provider.discovered_stock_voices(), vec![aria()]);
    assert_eq!(fixture.credential.resolutions.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.http.requests(),
        vec![RecordedNvidiaVoiceListRequest {
            origin: NVIDIA_MAGPIE_HTTP_ORIGIN,
            path: NVIDIA_MAGPIE_LIST_VOICES_PATH,
            deadline: Duration::from_secs(30),
        }]
    );
    assert!(NVIDIA_MAGPIE_HTTP_ORIGIN.starts_with(&format!("https://{NVIDIA_MAGPIE_FUNCTION_ID}.")));
    assert_eq!(
        NVIDIA_MAGPIE_ENDPOINTS.function_id,
        NVIDIA_MAGPIE_FUNCTION_ID
    );
    assert_eq!(
        NVIDIA_MAGPIE_ENDPOINTS.http_origin,
        NVIDIA_MAGPIE_HTTP_ORIGIN
    );
    assert_eq!(
        NVIDIA_MAGPIE_ENDPOINTS.http_synthesize_path,
        NVIDIA_MAGPIE_SYNTHESIZE_PATH
    );
}

#[tokio::test]
async fn synthesize_online_emits_ordered_pcm_then_supplied_alignment() {
    let fixture = fixture([MockNvidiaStreamScript {
        frames: VecDeque::from([
            Ok(NvidiaSynthesizeFrame {
                pcm: vec![1, 2, 3, 4],
                word_offsets: vec![NvidiaWordOffset {
                    word: "Perimeter".into(),
                    start_ms: 12,
                    end_ms: 230,
                    source_text_start: Some(4),
                    source_text_length: Some(9),
                }],
            }),
            Ok(NvidiaSynthesizeFrame {
                pcm: vec![5, 6],
                word_offsets: Vec::new(),
            }),
        ]),
        ..MockNvidiaStreamScript::default()
    }])
    .await;
    let mut session = fixture
        .provider
        .start_session(request())
        .await
        .expect("stock voice session");
    session
        .push_text("The perimeter is secure.")
        .await
        .expect("clause starts SynthesizeOnline");
    session.finish().await.expect("finish");

    assert_eq!(
        session.next_event().await,
        Some(Ok(TtsEvent::Audio(PcmChunk {
            sequence: 0,
            format: request().output,
            data: vec![1, 2, 3, 4],
        })))
    );
    assert!(matches!(
        session.next_event().await,
        Some(Ok(TtsEvent::Alignment(words))) if words[0].word == "Perimeter"
            && words[0].start_ms == 12 && words[0].end_ms == 230
    ));
    assert!(matches!(
        session.next_event().await,
        Some(Ok(TtsEvent::Audio(PcmChunk { sequence: 1, data, .. }))) if data == vec![5, 6]
    ));
    assert_eq!(session.next_event().await, Some(Ok(TtsEvent::Completed)));

    let requests = fixture.grpc.requests();
    assert_eq!(requests.len(), 1);
    let sent = &requests[0];
    assert_eq!(sent.authority, NVIDIA_MAGPIE_GRPC_AUTHORITY);
    assert!(sent.tls_required);
    assert_eq!(sent.function_id, NVIDIA_MAGPIE_FUNCTION_ID);
    assert_eq!(sent.method, RIVA_TTS_SYNTHESIZE_ONLINE_METHOD);
    assert_eq!(sent.voice_name, aria().id);
    assert_eq!(sent.encoding, NvidiaRivaAudioEncoding::LinearPcm);
    assert_eq!(sent.sample_rate_hz, 22_050);
    assert_eq!(sent.deadline, Duration::from_secs(30));
    assert_eq!(sent.text_chars, "The perimeter is secure.".chars().count());
    assert_eq!(fixture.credential.resolutions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancellation_stops_every_active_stream_and_emits_interrupted() {
    let fixture = fixture([MockNvidiaStreamScript::default()]).await;
    let mut session = fixture
        .provider
        .start_session(request())
        .await
        .expect("session");
    session
        .push_text("The perimeter is secure.")
        .await
        .expect("stream starts");
    session.cancel().await.expect("cancel");
    assert_eq!(fixture.grpc.cancellation_count(), 1);
    assert_eq!(
        session.next_event().await,
        Some(Ok(TtsEvent::Interrupted { reason: "barge_in" }))
    );
    assert!(session.next_event().await.is_none());
}

#[tokio::test]
async fn rate_limit_and_bad_function_are_sanitized_typed_failures() {
    for (failure, expected_kind, expected_code) in [
        (
            NvidiaNvcfError::RateLimited {
                retry_after: Some(Duration::from_secs(4)),
            },
            TtsErrorKind::RateLimited,
            "nvidia_rate_limited",
        ),
        (
            NvidiaNvcfError::BadFunction,
            TtsErrorKind::InvalidRequest,
            "nvidia_function_rejected",
        ),
        (
            NvidiaNvcfError::Unavailable,
            TtsErrorKind::Unavailable,
            "nvidia_unavailable",
        ),
    ] {
        let fixture = fixture([MockNvidiaStreamScript {
            start_error: Some(failure),
            ..MockNvidiaStreamScript::default()
        }])
        .await;
        let mut session = fixture
            .provider
            .start_session(request())
            .await
            .expect("session");
        let error = session
            .push_text("The perimeter is secure.")
            .await
            .expect_err("scripted NVCF failure");
        assert_eq!(error.kind, expected_kind);
        assert_eq!(error.code, expected_code);
        assert!(!format!("{error:?}").contains("perimeter"));
    }
}

#[tokio::test]
async fn zero_shot_clone_options_and_non_stock_voices_are_rejected() {
    let mut clone_options = BTreeMap::new();
    clone_options.insert("audio_prompt".into(), "forbidden bytes".into());
    let grpc = Arc::new(MockNvidiaGrpcTransport::default());
    let http = Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![aria()])));
    let credential = Arc::new(FixtureCredential::default());
    let provider = NvidiaNimMagpie::new(
        grpc as Arc<dyn NvidiaNimGrpcTransport>,
        http as Arc<dyn NvidiaNimHttpTransport>,
        credential as Arc<dyn ProviderCredentialResolver>,
        VoiceBindings::new([binding(clone_options)]).expect("structurally valid mapping"),
    );
    provider
        .discover_stock_voices()
        .await
        .expect("stock discovery");
    let error = match provider.start_session(request()).await {
        Ok(_) => panic!("clone option must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, "nvidia_provider_options_forbidden");

    let invalid_voice = NvidiaStockVoice {
        id: "Magpie-Multilingual.EN-US.UserClone".into(),
        display_name: "Untrusted".into(),
        locale: "en-US".into(),
        origin: NvidiaVoiceOrigin::UserOrCustom,
    };
    let invalid_http = Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![invalid_voice])));
    let invalid_provider = NvidiaNimMagpie::new(
        Arc::new(MockNvidiaGrpcTransport::default()),
        Arc::clone(&invalid_http) as Arc<dyn NvidiaNimHttpTransport>,
        Arc::new(FixtureCredential::default()),
        VoiceBindings::new([binding(BTreeMap::new())]).expect("valid mapping"),
    );
    let invalid_error = invalid_provider
        .discover_stock_voices()
        .await
        .expect_err("non-stock discovery response rejected");
    assert_eq!(invalid_error.code, "invalid_nvidia_voice_list");
    let debug = &invalid_http.request_debugs()[0];
    assert!(!debug.contains("fixture-nvidia-api-key"));
    assert!(!debug.contains("audio_prompt"));
    assert!(debug.contains("[REDACTED]"));
}

#[tokio::test]
async fn synthesis_request_debug_retains_no_dialogue_auth_or_clone_surface() {
    let fixture = fixture([MockNvidiaStreamScript::default()]).await;
    let mut session = fixture
        .provider
        .start_session(request())
        .await
        .expect("session");
    session
        .push_text("The private NPC dialogue is complete.")
        .await
        .expect("synthesis starts");
    let debug = &fixture.grpc.request_debugs()[0];
    assert!(!debug.contains("fixture-nvidia-api-key"));
    assert!(!debug.contains("private NPC dialogue"));
    assert!(!debug.contains("audio_prompt"));
    assert!(!debug.contains("zero_shot"));
    assert!(debug.matches("[REDACTED]").count() >= 2);
}
