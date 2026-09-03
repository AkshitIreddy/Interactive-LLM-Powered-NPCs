use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    future::pending,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use npc_providers_tts::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;
use zeroize::Zeroizing;

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

async fn http_fixture(
    status: &'static str,
    response_headers: &'static str,
    body: &'static str,
    response_delay: Duration,
) -> (Url, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let request = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("fixture accept");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            let count = socket.read(&mut buffer).await.expect("fixture read");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        tokio::time::sleep(response_delay).await;
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{response_headers}Connection: close\r\n\r\n{body}",
            body.len()
        );
        let _ignored = socket.write_all(response.as_bytes()).await;
        String::from_utf8(bytes).expect("request UTF-8")
    });
    (
        Url::parse(&format!("http://{address}/")).expect("fixture URL"),
        request,
    )
}

#[tokio::test]
async fn discovery_is_fixed_to_curated_function_subdomain_and_stock_ids() {
    let fixture = fixture([]).await;
    assert_eq!(
        fixture.provider.lifecycle(),
        NvidiaMagpieLifecycle::ExperimentalGrpcStreamingQualified
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
async fn concrete_discovery_uses_header_auth_filters_custom_records_and_caches() {
    const CREDENTIAL: &str = "fixture-nvidia-credential-canary";
    let body = r#"{"region":{"voices":[{"voice_id":"Magpie-Multilingual.EN-US.Aria","display_name":"Aria","language_code":"en-US"},{"voice_id":"user-custom-clone","display_name":"Clone","language_code":"en-US"}]}}"#;
    let (origin, request_capture) = http_fixture("200 OK", "", body, Duration::ZERO).await;
    let transport = Arc::new(
        ReqwestNvidiaNimHttpTransport::with_loopback_fixture(origin).expect("loopback transport"),
    );
    #[derive(Default)]
    struct CanaryCredential(AtomicUsize);
    #[async_trait::async_trait]
    impl ProviderCredentialResolver for CanaryCredential {
        async fn resolve(
            &self,
            _: HostedTtsProviderId,
        ) -> Result<SensitiveString, CredentialResolveError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(SensitiveString::new(CREDENTIAL))
        }
    }
    let credential = Arc::new(CanaryCredential::default());
    let provider = NvidiaNimMagpie::new(
        Arc::new(MockNvidiaGrpcTransport::default()),
        Arc::clone(&transport) as Arc<dyn NvidiaNimHttpTransport>,
        Arc::clone(&credential) as Arc<dyn ProviderCredentialResolver>,
        VoiceBindings::new([binding(BTreeMap::new())]).expect("binding"),
    );

    let first = provider.discover_stock_voices().await.expect("discovery");
    let second = provider
        .discover_stock_voices()
        .await
        .expect("cached discovery");
    assert_eq!(first, vec![aria()]);
    assert_eq!(second, first);
    assert_eq!(credential.0.load(Ordering::SeqCst), 1);

    let request = request_capture.await.expect("captured request");
    assert!(request.starts_with("GET /v1/audio/list_voices HTTP/1.1\r\n"));
    assert!(request
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {CREDENTIAL}").to_ascii_lowercase()));
    let request_line = request.lines().next().expect("request line");
    assert!(!request_line.contains(CREDENTIAL));
    assert!(!format!("{transport:?}").contains(CREDENTIAL));
}

#[tokio::test]
async fn concrete_discovery_redacts_provider_bodies_and_enforces_deadlines() {
    const CANARY: &str = "private-provider-body-and-credential";
    let (unauthorized_origin, unauthorized_request) =
        http_fixture("401 Unauthorized", "", CANARY, Duration::ZERO).await;
    let unauthorized = NvidiaNimMagpie::new(
        Arc::new(MockNvidiaGrpcTransport::default()),
        Arc::new(
            ReqwestNvidiaNimHttpTransport::with_loopback_fixture(unauthorized_origin)
                .expect("loopback transport"),
        ),
        Arc::new(FixtureCredential::default()),
        VoiceBindings::new([binding(BTreeMap::new())]).expect("binding"),
    );
    let error = unauthorized
        .discover_stock_voices()
        .await
        .expect_err("401 is rejected");
    assert_eq!(error.kind, TtsErrorKind::Authentication);
    assert_eq!(error.code, "nvidia_authentication_failed");
    assert!(!format!("{error:?}").contains(CANARY));
    unauthorized_request.await.expect("401 request captured");

    let (stalled_origin, stalled_request) =
        http_fixture("200 OK", "", "{}", Duration::from_secs(2)).await;
    let mut stalled = NvidiaNimMagpie::new(
        Arc::new(MockNvidiaGrpcTransport::default()),
        Arc::new(
            ReqwestNvidiaNimHttpTransport::with_loopback_fixture(stalled_origin)
                .expect("loopback transport"),
        ),
        Arc::new(FixtureCredential::default()),
        VoiceBindings::new([binding(BTreeMap::new())]).expect("binding"),
    );
    stalled
        .set_request_deadline(Duration::from_millis(100))
        .expect("bounded deadline");
    let error = tokio::time::timeout(Duration::from_secs(1), stalled.discover_stock_voices())
        .await
        .expect("outer bound")
        .expect_err("stalled discovery times out");
    assert_eq!(error.kind, TtsErrorKind::Timeout);
    assert_eq!(error.code, "nvidia_deadline_exceeded");
    stalled_request.await.expect("stalled request captured");
}

struct StalledGrpc;

#[async_trait::async_trait]
impl NvidiaNimGrpcTransport for StalledGrpc {
    async fn synthesize_online(
        &self,
        _: NvidiaSynthesizeOnlineRequest,
    ) -> Result<Box<dyn NvidiaNimSynthesisStream>, NvidiaNvcfError> {
        pending().await
    }
}

#[tokio::test]
async fn synthesis_stream_start_is_deadline_bounded() {
    let http = Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![aria()])));
    let mut provider = NvidiaNimMagpie::new(
        Arc::new(StalledGrpc),
        http,
        Arc::new(FixtureCredential::default()),
        VoiceBindings::new([binding(BTreeMap::new())]).expect("binding"),
    );
    provider
        .set_request_deadline(Duration::from_millis(100))
        .expect("bounded deadline");
    provider
        .discover_stock_voices()
        .await
        .expect("stock discovery");
    let mut session = provider.start_session(request()).await.expect("session");
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        session.push_text("The perimeter is secure."),
    )
    .await
    .expect("outer bound")
    .expect_err("stalled gRPC start times out");
    assert_eq!(error.kind, TtsErrorKind::Timeout);
    assert_eq!(error.code, "nvidia_deadline_exceeded");
}

#[test]
fn endpoint_and_cache_configuration_reject_arbitrary_or_unbounded_values() {
    assert!(ReqwestNvidiaNimHttpTransport::with_loopback_fixture(
        Url::parse("https://example.com/").expect("URL")
    )
    .is_err());
    let mut provider = NvidiaNimMagpie::new(
        Arc::new(MockNvidiaGrpcTransport::default()),
        Arc::new(MockNvidiaHttpTransport::returning(Ok(vec![aria()]))),
        Arc::new(FixtureCredential::default()),
        VoiceBindings::new([binding(BTreeMap::new())]).expect("binding"),
    );
    assert!(provider.set_voice_cache_ttl(Duration::ZERO).is_err());
    assert!(provider
        .set_voice_cache_ttl(Duration::from_secs(24 * 60 * 60 + 1))
        .is_err());
}

struct LiveFileCredential {
    path: PathBuf,
}

#[async_trait::async_trait]
impl ProviderCredentialResolver for LiveFileCredential {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        if provider_id != HostedTtsProviderId::NvidiaNimMagpie {
            return Err(CredentialResolveError::Missing);
        }
        let contents = Zeroizing::new(
            fs::read_to_string(&self.path).map_err(|_| CredentialResolveError::Unavailable)?,
        );
        extract_nvidia_credential(&contents)
            .map(SensitiveString::new)
            .ok_or(CredentialResolveError::Missing)
    }
}

fn extract_nvidia_credential(contents: &str) -> Option<String> {
    let mut nvidia_label_seen = false;
    for raw_line in contents.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let lowered = line.to_ascii_lowercase();
        if lowered.contains("nvidia") || lowered.contains("nim") {
            nvidia_label_seen = true;
            if let Some((_, value)) = line.split_once([':', '=']) {
                let candidate = value.trim().trim_matches(['\"', '\'']);
                if candidate.len() >= 16 && !candidate.chars().any(char::is_whitespace) {
                    return Some(candidate.to_owned());
                }
            }
            continue;
        }
        if nvidia_label_seen {
            let candidate = line.trim_matches(['\"', '\'']);
            if candidate.len() >= 16 && !candidate.chars().any(char::is_whitespace) {
                return Some(candidate.to_owned());
            }
            nvidia_label_seen = false;
        }
    }
    None
}

/// Explicitly ignored because it requires the user's authorized local key
/// source and makes bounded calls to NVIDIA's production HTTP and gRPC routes.
#[tokio::test]
#[ignore = "requires NVIDIA_TEST_KEYS_FILE and NVIDIA_LIVE_METRICS_PATH"]
async fn live_nvidia_magpie_grpc_stream_is_non_silent_and_bounded() {
    let credential_path = env::var_os("NVIDIA_TEST_KEYS_FILE")
        .map(PathBuf::from)
        .expect("NVIDIA_TEST_KEYS_FILE is required");
    let metrics_path = env::var_os("NVIDIA_LIVE_METRICS_PATH")
        .map(PathBuf::from)
        .expect("NVIDIA_LIVE_METRICS_PATH is required");
    assert!(credential_path.is_file());
    assert!(metrics_path.parent().is_some_and(Path::is_dir));

    let grpc_started = Instant::now();
    let grpc = TonicNvidiaNimGrpcTransport::connect()
        .await
        .expect("connect curated NVIDIA gRPC endpoint");
    let grpc_connect_ms = grpc_started.elapsed().as_millis();
    let http = ReqwestNvidiaNimHttpTransport::new().expect("curated HTTP transport");
    let credential = Arc::new(LiveFileCredential {
        path: credential_path,
    });
    let mut provider = NvidiaNimMagpie::new(
        Arc::new(grpc),
        Arc::new(http),
        credential,
        VoiceBindings::new([binding(BTreeMap::new())]).expect("curated binding"),
    );
    provider
        .set_request_deadline(Duration::from_secs(60))
        .expect("bounded live deadline");

    let discovery_started = Instant::now();
    let voices = provider
        .discover_stock_voices()
        .await
        .expect("live stock voice discovery");
    let discovery_ms = discovery_started.elapsed().as_millis();
    assert!(voices.iter().any(|voice| voice.id == aria().id));

    let mut session = provider
        .start_session(request())
        .await
        .expect("live session");
    let synthesis_started = Instant::now();
    session
        .push_text("The north beacon is ready.")
        .await
        .expect("start live gRPC stream");
    let response_headers_ms = synthesis_started.elapsed().as_millis();
    session.finish().await.expect("finish live text");

    let mut pcm = Vec::new();
    let mut first_audio_ms = None;
    for _ in 0..512 {
        let event = tokio::time::timeout(Duration::from_secs(60), session.next_event())
            .await
            .expect("bounded live frame")
            .expect("live stream completed event")
            .expect("live stream event");
        match event {
            TtsEvent::Audio(chunk) => {
                first_audio_ms.get_or_insert_with(|| synthesis_started.elapsed().as_millis());
                assert!(pcm.len().saturating_add(chunk.data.len()) <= 16 * 1_048_576);
                pcm.extend_from_slice(&chunk.data);
            }
            TtsEvent::Completed => break,
            TtsEvent::Alignment(_) | TtsEvent::Viseme(_) | TtsEvent::Usage(_) => {}
            TtsEvent::Interrupted { .. } => panic!("live stream was interrupted"),
        }
    }
    let total_ms = synthesis_started.elapsed().as_millis();
    assert!(!pcm.is_empty());
    assert_eq!(pcm.len() % 2, 0);
    let samples = pcm
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect::<Vec<_>>();
    let peak_sample = samples
        .iter()
        .map(|sample| i32::from(*sample).abs())
        .max()
        .unwrap_or_default();
    let sum_squares = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>();
    let rms = (sum_squares / samples.len() as f64).sqrt() / 32_768.0;
    let clipped_samples = samples
        .iter()
        .filter(|sample| sample.unsigned_abs() >= 32_767)
        .count();
    assert!(rms >= 0.0001);
    assert_eq!(clipped_samples, 0);

    let metrics = serde_json::json!({
        "grpcConnectMs": grpc_connect_ms,
        "voiceDiscoveryMs": discovery_ms,
        "voiceRecords": voices.len(),
        "responseHeadersMs": response_headers_ms,
        "firstAudioMs": first_audio_ms.expect("first audio"),
        "totalSynthesisMs": total_ms,
        "audioBytes": pcm.len(),
        "peak": f64::from(peak_sample) / 32_768.0,
        "rms": rms,
        "clippedSamples": clipped_samples,
    });
    fs::write(
        metrics_path,
        serde_json::to_vec(&metrics).expect("serialize sanitized metrics"),
    )
    .expect("write sanitized live metrics");
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
