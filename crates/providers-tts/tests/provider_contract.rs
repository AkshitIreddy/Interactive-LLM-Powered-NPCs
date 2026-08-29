use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use npc_providers_tts::*;

struct OneShotCredential {
    value: std::sync::Mutex<Option<SensitiveString>>,
    resolutions: std::sync::atomic::AtomicUsize,
}

impl OneShotCredential {
    fn new(value: &str) -> Self {
        Self {
            value: std::sync::Mutex::new(Some(SensitiveString::new(value))),
            resolutions: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn resolution_count(&self) -> usize {
        self.resolutions.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl ProviderCredentialResolver for OneShotCredential {
    async fn resolve(
        &self,
        _provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        self.resolutions
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.value
            .lock()
            .expect("credential fixture mutex poisoned")
            .take()
            .ok_or(CredentialResolveError::Missing)
    }
}

#[tokio::test]
async fn credential_is_resolved_once_at_connect_time_and_never_cloned() {
    let transport = Arc::new(MockTransport::default());
    let resolver = Arc::new(OneShotCredential::new("one-shot-secret"));
    let provider = CartesiaProvider::new(
        Arc::clone(&transport) as Arc<dyn TtsTransport>,
        Arc::clone(&resolver) as Arc<dyn ProviderCredentialResolver>,
        bindings(),
        CartesiaConfig::default(),
    );
    assert_eq!(resolver.resolution_count(), 0);

    let _session = provider
        .start_session(request())
        .await
        .expect("first connect consumes credential");
    assert_eq!(resolver.resolution_count(), 1);

    let error = match provider.start_session(request()).await {
        Ok(_) => panic!("one-shot secret must not have been cloned"),
        Err(error) => error,
    };
    assert_eq!(error.code, "credential_missing");
    assert_eq!(resolver.resolution_count(), 2);
}

fn identity() -> SessionIdentity {
    SessionIdentity {
        session_id: "session-7".into(),
        turn_id: "turn-11".into(),
        cancellation_generation: 3,
    }
}

fn request() -> TtsSessionRequest {
    TtsSessionRequest {
        identity: identity(),
        locale: "en-US".into(),
        voice_intent_id: "steady-guide".into(),
        output: AudioFormat::default(),
        request_alignment: true,
        request_visemes: true,
        clause_policy: SemanticClausePolicy::default(),
    }
}

fn binding(provider_id: HostedTtsProviderId) -> VoiceBinding {
    VoiceBinding {
        intent_id: "steady-guide".into(),
        provider_id,
        voice_id: format!("voice-{}", provider_id.as_str()),
        model_id: format!("model-{}", provider_id.as_str()),
        provider_options: Default::default(),
    }
}

fn bindings() -> VoiceBindings {
    VoiceBindings::new([
        binding(HostedTtsProviderId::Cartesia),
        binding(HostedTtsProviderId::ElevenLabs),
        binding(HostedTtsProviderId::Inworld),
        binding(HostedTtsProviderId::Deepgram),
    ])
    .expect("valid fixture bindings")
}

fn provider(
    id: HostedTtsProviderId,
    transport: Arc<MockTransport>,
) -> Box<dyn StreamingTtsProvider> {
    let credential_resolver: Arc<dyn ProviderCredentialResolver> =
        Arc::new(OneShotCredential::new("super-secret-api-key"));
    match id {
        HostedTtsProviderId::Cartesia => Box::new(CartesiaProvider::new(
            transport,
            credential_resolver,
            bindings(),
            CartesiaConfig::default(),
        )),
        HostedTtsProviderId::ElevenLabs => Box::new(ElevenLabsProvider::new(
            transport,
            credential_resolver,
            bindings(),
            ElevenLabsConfig::default(),
        )),
        HostedTtsProviderId::Inworld => Box::new(InworldProvider::new(
            transport,
            credential_resolver,
            bindings(),
            InworldConfig::default(),
        )),
        HostedTtsProviderId::Deepgram => Box::new(DeepgramProvider::new(
            transport,
            credential_resolver,
            bindings(),
            DeepgramConfig::default(),
        )),
        HostedTtsProviderId::NvidiaNimMagpie => {
            panic!("NVIDIA Magpie uses its dedicated NVCF fixture")
        }
    }
}

#[tokio::test]
async fn every_adapter_uses_documented_streaming_shape() {
    for id in [
        HostedTtsProviderId::Cartesia,
        HostedTtsProviderId::ElevenLabs,
        HostedTtsProviderId::Inworld,
        HostedTtsProviderId::Deepgram,
    ] {
        let transport = Arc::new(MockTransport::default());
        let provider = provider(id, Arc::clone(&transport));
        let mut session = provider
            .start_session(request())
            .await
            .expect("session starts");
        session
            .push_text("The perimeter is secure. Continue to the rendezvous point.")
            .await
            .expect("text accepted");
        session.finish().await.expect("finish accepted");

        let commands = transport.commands();
        assert!(!commands.is_empty(), "{id:?} emitted no commands");
        match id {
            HostedTtsProviderId::Cartesia => {
                assert!(matches!(
                    commands.last(),
                    Some(RecordedCommand::CartesiaGenerate {
                        continue_generation: false,
                        ..
                    })
                ));
            }
            HostedTtsProviderId::ElevenLabs => {
                assert!(matches!(
                    commands.first(),
                    Some(RecordedCommand::ElevenLabsInitialize)
                ));
                assert!(matches!(
                    commands.last(),
                    Some(RecordedCommand::ElevenLabsFinish)
                ));
            }
            HostedTtsProviderId::Inworld => {
                assert!(matches!(
                    commands.first(),
                    Some(RecordedCommand::InworldCreateContext)
                ));
                assert!(matches!(
                    commands.last(),
                    Some(RecordedCommand::InworldFlushContext)
                ));
            }
            HostedTtsProviderId::Deepgram => {
                assert!(matches!(
                    commands.last(),
                    Some(RecordedCommand::DeepgramFlush)
                ));
            }
            HostedTtsProviderId::NvidiaNimMagpie => unreachable!("not in hosted WebSocket loop"),
        }
    }
}

#[tokio::test]
async fn semantic_chunking_ignores_network_chunk_boundaries() {
    let transport = Arc::new(MockTransport::default());
    let provider = provider(HostedTtsProviderId::Deepgram, Arc::clone(&transport));
    let mut session = provider.start_session(request()).await.expect("starts");

    let first = session
        .push_text("Captain, the reac")
        .await
        .expect("accepted");
    assert_eq!(first.clauses_submitted, 0);
    let second = session
        .push_text("tor is stable. We can proceed, but keep watch.")
        .await
        .expect("accepted");
    assert_eq!(second.clauses_submitted, 2);
    session.finish().await.expect("finished");

    let spoken: Vec<(usize, bool)> = transport
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            RecordedCommand::DeepgramSpeak {
                text_chars,
                text_ends_with_space,
            } => Some((text_chars, text_ends_with_space)),
            _ => None,
        })
        .collect();
    assert_eq!(
        spoken,
        [
            ("Captain, the reactor is stable. ".chars().count(), true),
            ("We can proceed, but keep watch. ".chars().count(), true)
        ]
    );
}

#[tokio::test]
async fn audio_alignment_visemes_usage_and_completion_stay_ordered() {
    let alignment = vec![WordAlignment {
        word: "Ready".into(),
        start_ms: 10,
        end_ms: 180,
        source_text_start: Some(0),
        source_text_length: Some(5),
    }];
    let visemes = vec![VisemeEvent {
        symbol: "R".into(),
        start_ms: 10,
        duration_ms: 40,
    }];
    let usage = UsageEvent {
        processed_characters: 6,
        provider_request_id: Some("safe-request-id".into()),
    };
    let transport = Arc::new(MockTransport::scripted(MockScript {
        incoming: VecDeque::from([
            Ok(WireEvent::Audio(vec![1, 2, 3, 4])),
            Ok(WireEvent::Alignment(alignment.clone())),
            Ok(WireEvent::Viseme(visemes.clone())),
            Ok(WireEvent::Usage(usage.clone())),
            Ok(WireEvent::Complete),
        ]),
        ..MockScript::default()
    }));
    let provider = provider(HostedTtsProviderId::Cartesia, transport);
    let mut session = provider.start_session(request()).await.expect("starts");
    session
        .push_text("Ready when you are.")
        .await
        .expect("accepted");
    session.finish().await.expect("finished");

    assert!(matches!(
        session.next_event().await,
        Some(Ok(TtsEvent::Audio(PcmChunk { sequence: 0, .. })))
    ));
    assert_eq!(
        session.next_event().await,
        Some(Ok(TtsEvent::Alignment(alignment)))
    );
    assert_eq!(
        session.next_event().await,
        Some(Ok(TtsEvent::Viseme(visemes)))
    );
    assert_eq!(session.next_event().await, Some(Ok(TtsEvent::Usage(usage))));
    assert_eq!(session.next_event().await, Some(Ok(TtsEvent::Completed)));
    assert_eq!(session.state(), SessionState::Completed);
    assert!(session.next_event().await.is_none());
}

#[tokio::test]
async fn cancellation_is_idempotent_and_emits_interrupted_once() {
    let transport = Arc::new(MockTransport::default());
    let provider = provider(HostedTtsProviderId::Deepgram, Arc::clone(&transport));
    let mut session = provider.start_session(request()).await.expect("starts");
    session
        .push_text("Stop me after this complete clause.")
        .await
        .expect("accepted");
    assert!(session.has_started_utterance());

    session.cancel().await.expect("cancelled");
    session.cancel().await.expect("second cancel is idempotent");
    assert_eq!(session.state(), SessionState::Cancelled);
    assert_eq!(
        session.next_event().await,
        Some(Ok(TtsEvent::Interrupted { reason: "barge_in" }))
    );
    assert!(session.next_event().await.is_none());
    assert_eq!(transport.close_count(), 1);
    assert!(transport
        .commands()
        .iter()
        .any(|command| matches!(command, RecordedCommand::DeepgramClear)));
}

#[tokio::test]
async fn transport_faults_become_content_free_typed_errors() {
    let transport = Arc::new(MockTransport::scripted(MockScript {
        fail_send_at: Some(1), // ElevenLabs initialization is send zero.
        ..MockScript::default()
    }));
    let provider = provider(HostedTtsProviderId::ElevenLabs, transport);
    let mut session = provider.start_session(request()).await.expect("starts");
    let error = session
        .push_text("A complete sentence that triggers the transport.")
        .await
        .expect_err("scripted failure");
    assert_eq!(error.kind, TtsErrorKind::Unavailable);
    assert_eq!(error.code, "transport_unavailable");
    assert!(error.retryable);
    assert_eq!(session.state(), SessionState::Faulted);
    assert!(!format!("{error:?}").contains("complete sentence"));
}

#[tokio::test]
async fn premature_stream_close_and_invalid_alignment_are_protocol_faults() {
    let closed_transport = Arc::new(MockTransport::default());
    let deepgram_provider = provider(HostedTtsProviderId::Deepgram, Arc::clone(&closed_transport));
    let mut session = deepgram_provider
        .start_session(request())
        .await
        .expect("starts");
    session
        .push_text("A complete line to synthesize safely.")
        .await
        .expect("accepted");
    session.finish().await.expect("finished");
    let error = session
        .next_event()
        .await
        .expect("typed terminal event")
        .expect_err("premature close");
    assert_eq!(error.code, "stream_closed_before_completion");
    assert_eq!(session.state(), SessionState::Faulted);

    let invalid_transport = Arc::new(MockTransport::scripted(MockScript {
        incoming: VecDeque::from([Ok(WireEvent::Alignment(vec![WordAlignment {
            word: "invalid".into(),
            start_ms: 100,
            end_ms: 20,
            source_text_start: None,
            source_text_length: None,
        }]))]),
        ..MockScript::default()
    }));
    let provider = provider(HostedTtsProviderId::Cartesia, invalid_transport);
    let mut session = provider.start_session(request()).await.expect("starts");
    let error = session
        .next_event()
        .await
        .expect("typed event")
        .expect_err("invalid alignment rejected");
    assert_eq!(error.code, "invalid_alignment");
}

#[tokio::test]
async fn empty_utterances_are_rejected_without_provider_work() {
    let transport = Arc::new(MockTransport::default());
    let provider = provider(HostedTtsProviderId::Deepgram, Arc::clone(&transport));
    let mut session = provider.start_session(request()).await.expect("starts");
    let error = session
        .finish()
        .await
        .expect_err("empty utterance rejected");
    assert_eq!(error.code, "empty_utterance");
    assert!(transport.commands().is_empty());
}

#[test]
fn fallback_requires_both_egress_authorization_and_explicit_voice_mapping() {
    let all_bindings = bindings();
    let only_cartesia = FallbackAuthorization {
        authorized_providers: BTreeSet::from([HostedTtsProviderId::Cartesia]),
        allow_provider_change: true,
    };
    assert_eq!(
        select_voice_route(
            "steady-guide",
            HostedTtsProviderId::Cartesia,
            &[HostedTtsProviderId::Deepgram],
            &all_bindings,
            &only_cartesia,
        ),
        Err(FallbackError::NoAuthorizedMappedVoice)
    );

    let authorized = FallbackAuthorization {
        authorized_providers: BTreeSet::from([
            HostedTtsProviderId::Cartesia,
            HostedTtsProviderId::Deepgram,
        ]),
        allow_provider_change: true,
    };
    let decision = select_voice_route(
        "steady-guide",
        HostedTtsProviderId::Cartesia,
        &[HostedTtsProviderId::Deepgram],
        &all_bindings,
        &authorized,
    )
    .expect("authorized route");
    assert!(matches!(
        decision,
        FallbackDecision::Fallback { binding, .. }
            if binding.provider_id == HostedTtsProviderId::Deepgram
    ));
}

#[test]
fn provider_pin_forbids_mid_utterance_switching() {
    let authorization = FallbackAuthorization {
        authorized_providers: BTreeSet::from([
            HostedTtsProviderId::Cartesia,
            HostedTtsProviderId::Deepgram,
        ]),
        allow_provider_change: true,
    };
    let mut pin = UtteranceProviderPin::new(HostedTtsProviderId::Cartesia);
    pin.switch_before_start(HostedTtsProviderId::Deepgram, &authorization)
        .expect("pre-start switch allowed");
    pin.mark_started();
    assert_eq!(
        pin.switch_before_start(HostedTtsProviderId::Cartesia, &authorization),
        Err(FallbackError::UtteranceAlreadyStarted)
    );
}

#[tokio::test]
async fn debug_and_mock_recordings_never_reveal_credentials_or_dialogue() {
    let secret = "api-key-that-must-not-appear";
    let dialogue = "private player dialogue must not appear";
    let transport = Arc::new(MockTransport::default());
    let provider = CartesiaProvider::new(
        Arc::clone(&transport) as Arc<dyn TtsTransport>,
        Arc::new(OneShotCredential::new(secret)),
        bindings(),
        CartesiaConfig::default(),
    );
    let mut session = provider.start_session(request()).await.expect("starts");
    session.push_text(dialogue).await.expect("accepted");
    session.finish().await.expect("finished");

    let debug_commands = format!("{:?}", transport.commands());
    let debug_opens = format!("{:?}", transport.opens());
    assert!(!debug_commands.contains(dialogue));
    assert!(!debug_commands.contains(secret));
    assert!(!debug_opens.contains(secret));
    assert_eq!(format!("{:?}", SensitiveString::new(secret)), "[REDACTED]");
    assert_eq!(
        format!(
            "{:?}",
            SensitiveHeaderValue::with_scheme("Bearer", SensitiveString::new(secret))
        ),
        "[REDACTED]"
    );
}
