//! One explicit, bounded hosted LLM turn through the normal runtime simulation
//! path. Audio is disabled and no native presenter is supplied, so the receipt
//! can prove schema, provider, supervisor, and subtitle fallback only.

#![cfg(all(windows, feature = "test-fixture-vault"))]

use std::{
    collections::BTreeMap,
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use futures_util::StreamExt;
use interactive_npcs_credential_vault::{CredentialVault, MemoryCredentialVault, SecretValue};
use npc_providers_tts::{
    CartesiaConfig, CartesiaProvider, CartesiaWebSocketTransport, HostedTtsProviderId,
    ProviderCapabilities, PushOutcome, SessionIdentity, SessionState, StreamingTtsProvider,
    StreamingTtsSession, TtsError, TtsEvent, TtsSessionRequest, VoiceBinding, VoiceBindings,
    CARTESIA_QUALIFIED_AUDIO_FORMAT, CARTESIA_QUALIFIED_MODEL_ID,
    CARTESIA_QUALIFIED_STOCK_VOICE_ID,
};
use npc_runtime_core::{
    CharacterIdentity, DataClass, DeliveryMode, GenerationRequest, LanguageModelProvider,
    MemoryContext, ProviderDescriptor, ProviderLocation, ProviderModality,
    ResponseValidationPolicy, SentenceSegmenterConfig, SpeechRequest, SpeechStreamItem,
    StreamingResponseAdapterV1, StreamingResponseFormatV1, TtsProvider, TurnIdentity,
    TurnLifecycle,
};
use npc_runtime_host::tts_bridge::{
    RuntimeTtsBridge, RuntimeTtsBridgeConfig, VaultTtsCredentialResolver,
};
use npc_runtime_host::{
    llm_bridge::selected_hosted_llm, simulation::SimulationSafetyContext, HostConfig, HostState,
    RouteExecution, SelectedProviderRoute, SelectedRoleRoute, SelectedRouteRoles,
    SelectedRouteSnapshot, SelectedRouteState, SimulationRequest, TurnDeliveryRequest,
    TurnInputSnapshot, TURN_ROUTE_SCHEMA_VERSION,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const KEY_ENV: &str = "MISTRAL_API_KEY";
const REPORT_ENV: &str = "STRUCTURED_NORMAL_ROUTE_REPORT";
const MODEL_ID: &str = "ministral-8b-2512";
const GROQ_KEY_ENV: &str = "GROQ_API_KEY";
const CARTESIA_KEY_ENV: &str = "CARTESIA_API_KEY";
const COMBINED_REPORT_ENV: &str = "GROQ_CARTESIA_STREAMING_REPORT";
const GROQ_QWEN_MODEL_ID: &str = "qwen/qwen3.6-27b";
const VOICE_INTENT_ID: &str = "selected.stock.voice";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("runtime-host is under apps")
        .to_path_buf()
}

fn disabled() -> SelectedRoleRoute {
    SelectedRoleRoute {
        state: SelectedRouteState::Disabled,
        primary: None,
        fallbacks: Vec::new(),
        degradation: None,
    }
}

#[tokio::test]
#[ignore = "explicit low-cost Mistral structured normal-route qualification"]
async fn selected_mistral_turn_uses_speech_first_schema_without_audio_claims() {
    let report_path = PathBuf::from(env::var_os(REPORT_ENV).expect("report path is required"));
    assert!(!report_path.exists(), "report must be create-new");
    let key = env::var(KEY_ENV).expect("Mistral credential is required");
    let vault = MemoryCredentialVault::default();
    vault
        .put(
            "providers/mistral",
            None,
            &SecretValue::new(key.into_bytes()).expect("bounded credential"),
        )
        .expect("seed isolated test vault");
    env::remove_var(KEY_ENV);
    let app_data = tempfile::tempdir().expect("temporary app data").keep();
    let host = HostState::initialize_with_test_vault(
        HostConfig {
            repo_root: repo_root(),
            app_data,
        },
        Arc::new(vault),
    )
    .await
    .expect("initialize isolated runtime host");
    let route = SelectedRouteSnapshot {
        schema_version: TURN_ROUTE_SCHEMA_VERSION,
        source_loadout_id: "live-structured-headless".into(),
        inheritance_chain: Vec::new(),
        generation: 1,
        roles: SelectedRouteRoles {
            llm: SelectedRoleRoute {
                state: SelectedRouteState::Ready,
                primary: Some(SelectedProviderRoute {
                    provider_id: "mistral".into(),
                    model_id: MODEL_ID.into(),
                    voice_id: None,
                    execution: RouteExecution::Cloud,
                    egress: "provider_cloud:transcript.game_context".into(),
                    credential_reference: Some("providers/mistral".into()),
                }),
                fallbacks: Vec::new(),
                degradation: None,
            },
            stt: disabled(),
            tts: disabled(),
            embeddings: disabled(),
            vision: disabled(),
            lip_sync: disabled(),
        },
    };
    let started = Instant::now();
    let result = host
        .simulate_turn(SimulationRequest {
            session_id: "live-structured-headless".into(),
            turn_id: "turn-1".into(),
            game_id: "eclipse-harbor".into(),
            character_id: Some("mara-venn".into()),
            effective_game_profile: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: SimulationSafetyContext::verified_safe(),
            application_namespace: None,
            transcript: "Give one short sentence confirming the harbor lantern is ready.".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
            route_snapshot: Some(route),
            input: TurnInputSnapshot::default(),
            delivery: TurnDeliveryRequest {
                audio: false,
                subtitles: true,
            },
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        })
        .await
        .expect("normal selected-route turn");
    let elapsed_ms = started.elapsed().as_millis() as u64;
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert_eq!(
        result.outcome.selected_llm_provider.as_deref(),
        Some("mistral")
    );
    assert!(result.outcome.structured_response.is_some());
    assert!(!result.outcome.full_response.trim().is_empty());
    assert!(!result.outcome.delivered.is_empty());
    assert!(result
        .outcome
        .delivered
        .iter()
        .all(|sentence| sentence.delivery == DeliveryMode::Subtitle));
    assert!(result
        .outcome
        .delivered
        .iter()
        .all(|sentence| sentence.audible_frames == 0));

    let response_hash = format!(
        "{:x}",
        Sha256::digest(result.outcome.full_response.as_bytes())
    );
    let receipt = json!({
        "schemaVersion": 1,
        "receiptType": "headlessStructuredNormalRouteQualification",
        "checkedAtEpochMs": SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_millis() as u64,
        "providerId": "mistral",
        "modelId": MODEL_ID,
        "routeFormat": "structured_speech_first_v1",
        "elapsedMs": elapsed_ms,
        "lifecycle": "completed",
        "structuredResponseValidated": true,
        "responseBytes": result.outcome.full_response.len(),
        "responseSha256": response_hash,
        "deliveredSubtitleSentences": result.outcome.delivered.len(),
        "containsCredential": false,
        "containsDialogueText": false,
        "audioRequested": false,
        "audioDeliveryClaimed": false,
        "physicalAudibilityClaimed": false,
        "nativeBrokerReceipt": false,
        "nativeSubtitlePresentationClaimed": false,
        "note": "The normal runtime selected-route path validated the speech-first JSON envelope and delivered only headless subtitle fallback receipts."
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)
        .expect("create report");
    file.write_all(&serde_json::to_vec_pretty(&receipt).expect("serialize report"))
        .expect("write report");
}

#[derive(Clone, Debug, Default)]
struct ProviderTiming {
    session_request_us: Option<u64>,
    session_ready_us: Option<u64>,
    push_started_us: Option<u64>,
    push_finished_us: Option<u64>,
    finish_started_us: Option<u64>,
    finish_finished_us: Option<u64>,
    first_pcm_us: Option<u64>,
    completed_us: Option<u64>,
}

struct ObservedProvider {
    inner: Arc<dyn StreamingTtsProvider>,
    origin: Arc<Instant>,
    timing: Arc<Mutex<ProviderTiming>>,
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
        self.timing
            .lock()
            .expect("provider timing mutex")
            .session_request_us = Some(elapsed_us(&self.origin));
        let inner = self.inner.start_session(request).await?;
        self.timing
            .lock()
            .expect("provider timing mutex")
            .session_ready_us = Some(elapsed_us(&self.origin));
        Ok(Box::new(ObservedSession {
            inner,
            origin: Arc::clone(&self.origin),
            timing: Arc::clone(&self.timing),
        }))
    }
}

struct ObservedSession {
    inner: Box<dyn StreamingTtsSession>,
    origin: Arc<Instant>,
    timing: Arc<Mutex<ProviderTiming>>,
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
        self.timing
            .lock()
            .expect("provider timing mutex")
            .push_started_us = Some(elapsed_us(&self.origin));
        let result = self.inner.push_text(text).await;
        self.timing
            .lock()
            .expect("provider timing mutex")
            .push_finished_us = Some(elapsed_us(&self.origin));
        result
    }

    async fn finish(&mut self) -> Result<(), TtsError> {
        self.timing
            .lock()
            .expect("provider timing mutex")
            .finish_started_us = Some(elapsed_us(&self.origin));
        let result = self.inner.finish().await;
        self.timing
            .lock()
            .expect("provider timing mutex")
            .finish_finished_us = Some(elapsed_us(&self.origin));
        result
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        let event = self.inner.next_event().await;
        if let Some(Ok(event)) = &event {
            let now = elapsed_us(&self.origin);
            let mut timing = self.timing.lock().expect("provider timing mutex");
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

#[derive(Debug)]
struct TtsOutcome {
    task_started_us: u64,
    runtime_session_ready_us: u64,
    synthesize_returned_us: u64,
    bridge_first_pcm_us: u64,
    bridge_complete_us: u64,
    pcm_bytes: usize,
    audio_chunks: usize,
    alignment_events: usize,
    pcm_sha256: String,
}

fn elapsed_us(origin: &Instant) -> u64 {
    origin.elapsed().as_micros().try_into().unwrap_or(u64::MAX)
}

fn cloud_descriptor(provider_id: &str, modality: ProviderModality) -> ProviderDescriptor {
    ProviderDescriptor {
        id: provider_id.into(),
        display_name: provider_id.into(),
        modality,
        location: ProviderLocation::Cloud {
            service: provider_id.into(),
        },
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::new(),
    }
}

async fn synthesize_to_memory(
    bridge: Arc<RuntimeTtsBridge>,
    identity: TurnIdentity,
    text: String,
    origin: Arc<Instant>,
) -> TtsOutcome {
    let task_started_us = elapsed_us(&origin);
    let mut session = bridge
        .start_session(&identity, "en-US", CancellationToken::new())
        .await
        .expect("start production runtime TTS session");
    let runtime_session_ready_us = elapsed_us(&origin);
    let mut speech = session
        .synthesize(
            SpeechRequest {
                identity,
                sentence_id: 1,
                text,
                locale: "en-US".into(),
                voice_hint: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("dispatch production Cartesia synthesis");
    let synthesize_returned_us = elapsed_us(&origin);
    let mut bridge_first_pcm_us = None;
    let mut pcm_bytes = 0_usize;
    let mut audio_chunks = 0_usize;
    let mut alignment_events = 0_usize;
    let mut hasher = Sha256::new();
    let mut end_of_stream = false;
    while let Some(item) = tokio::time::timeout(Duration::from_secs(20), speech.next())
        .await
        .expect("Cartesia bridge event deadline")
    {
        match item.expect("validated Cartesia bridge item") {
            SpeechStreamItem::Audio(chunk) => {
                bridge_first_pcm_us.get_or_insert_with(|| elapsed_us(&origin));
                assert_eq!(
                    chunk.sample_rate_hz,
                    CARTESIA_QUALIFIED_AUDIO_FORMAT.sample_rate_hz
                );
                assert_eq!(chunk.channels, CARTESIA_QUALIFIED_AUDIO_FORMAT.channels);
                pcm_bytes = pcm_bytes.saturating_add(chunk.pcm_s16le.len());
                audio_chunks = audio_chunks.saturating_add(1);
                hasher.update(&chunk.pcm_s16le);
                end_of_stream |= chunk.end_of_stream;
            }
            SpeechStreamItem::Alignment(_) => {
                alignment_events = alignment_events.saturating_add(1);
            }
        }
    }
    let bridge_complete_us = elapsed_us(&origin);
    session.close().await.expect("close runtime TTS session");
    assert!(end_of_stream && pcm_bytes > 0 && audio_chunks > 0);
    TtsOutcome {
        task_started_us,
        runtime_session_ready_us,
        synthesize_returned_us,
        bridge_first_pcm_us: bridge_first_pcm_us.expect("bridge first PCM"),
        bridge_complete_us,
        pcm_bytes,
        audio_chunks,
        alignment_events,
        pcm_sha256: format!("{:x}", hasher.finalize()),
    }
}

#[tokio::test]
#[ignore = "explicit one-request Groq Qwen 3.6 to Cartesia streaming qualification"]
async fn groq_qwen_speech_first_field_dispatches_one_cartesia_request_without_playback() {
    let report_path = PathBuf::from(
        env::var_os(COMBINED_REPORT_ENV).expect("combined qualification report path is required"),
    );
    assert!(!report_path.exists(), "report must be create-new");
    let groq_key = env::var(GROQ_KEY_ENV).expect("Groq credential is required");
    let cartesia_key = env::var(CARTESIA_KEY_ENV).expect("Cartesia credential is required");
    let vault = Arc::new(MemoryCredentialVault::default());
    vault
        .put(
            "providers/groq",
            None,
            &SecretValue::new(groq_key.into_bytes()).expect("bounded Groq credential"),
        )
        .expect("seed isolated Groq vault target");
    vault
        .put(
            "providers/cartesia",
            None,
            &SecretValue::new(cartesia_key.into_bytes()).expect("bounded Cartesia credential"),
        )
        .expect("seed isolated Cartesia vault target");
    env::remove_var(GROQ_KEY_ENV);
    env::remove_var(CARTESIA_KEY_ENV);

    let llm_route = SelectedProviderRoute {
        provider_id: "groq".into(),
        model_id: GROQ_QWEN_MODEL_ID.into(),
        voice_id: None,
        execution: RouteExecution::Cloud,
        egress: "provider_cloud:transcript.game_context".into(),
        credential_reference: Some("providers/groq".into()),
    };
    let llm: Arc<dyn LanguageModelProvider> = selected_hosted_llm(
        &llm_route,
        vault.clone(),
        "You are Mara Venn, a calm harbor keeper. Reply with exactly one natural spoken sentence containing 10 to 15 words. State only the answer in the required response schema."
            .into(),
    )
    .expect("construct exact selected Groq route");

    let bindings = VoiceBindings::new([VoiceBinding {
        intent_id: VOICE_INTENT_ID.into(),
        provider_id: HostedTtsProviderId::Cartesia,
        voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID.into(),
        model_id: CARTESIA_QUALIFIED_MODEL_ID.into(),
        provider_options: BTreeMap::new(),
    }])
    .expect("exact Cartesia voice binding");
    let cartesia_transport = Arc::new(CartesiaWebSocketTransport::default());
    let cartesia: Arc<dyn StreamingTtsProvider> = Arc::new(CartesiaProvider::new(
        cartesia_transport,
        Arc::new(VaultTtsCredentialResolver::new(vault)),
        bindings,
        CartesiaConfig::default(),
    ));
    let origin = Arc::new(Instant::now());
    let provider_timing = Arc::new(Mutex::new(ProviderTiming::default()));
    let observed: Arc<dyn StreamingTtsProvider> = Arc::new(ObservedProvider {
        inner: cartesia,
        origin: Arc::clone(&origin),
        timing: Arc::clone(&provider_timing),
    });
    let tts_bridge = Arc::new(
        RuntimeTtsBridge::new(
            observed,
            RuntimeTtsBridgeConfig::selected_stock(
                cloud_descriptor("cartesia", ProviderModality::Speech),
                VOICE_INTENT_ID,
                CARTESIA_QUALIFIED_MODEL_ID,
                CARTESIA_QUALIFIED_AUDIO_FORMAT,
            ),
        )
        .expect("construct production Cartesia runtime bridge"),
    );

    let identity = TurnIdentity {
        session_id: "live-groq-cartesia-chain".into(),
        turn_id: "turn-1".into(),
        cancellation_generation: 1,
    };
    let request = GenerationRequest {
        identity: identity.clone(),
        transcript: "Mara, is the harbor lantern ready, and is the eastern pier safe tonight?"
            .into(),
        character: CharacterIdentity {
            character_id: Some("mara-venn".into()),
            display_name: "Mara Venn".into(),
            confidence: 1.0,
            evidence: vec!["explicit live qualification character".into()],
            explicit_selection: true,
        },
        memory: MemoryContext {
            canon_facts: vec![
                "The harbor lantern has been lit.".into(),
                "The eastern pier is safe tonight.".into(),
            ],
            ..MemoryContext::default()
        },
        locale: "en-US".into(),
        metadata: BTreeMap::from([(
            "npc_response_format".into(),
            "structured_speech_first_v1".into(),
        )]),
    };
    let llm_request_started_us = elapsed_us(&origin);
    let mut llm_stream = llm
        .stream_response(request, CancellationToken::new())
        .await
        .expect("start exact selected Groq stream");
    let llm_stream_opened_us = elapsed_us(&origin);
    let mut adapter = StreamingResponseAdapterV1::new(
        StreamingResponseFormatV1::StructuredSpeechFirstV1,
        ResponseValidationPolicy::default(),
        SentenceSegmenterConfig::default(),
        identity.cancellation_generation,
    );
    let mut first_provider_delta_us = None;
    let mut spoken_field_validated_us = None;
    let mut tts_task = None;
    tokio::time::timeout(Duration::from_secs(70), async {
        while let Some(delta) = llm_stream.next().await {
            let delta = delta.expect("validated Groq runtime delta");
            first_provider_delta_us.get_or_insert_with(|| elapsed_us(&origin));
            let update = adapter
                .push_delta(
                    identity.cancellation_generation,
                    delta.sequence,
                    &delta.text,
                )
                .expect("speech-first streaming response validation");
            if !update.ready_sentences.is_empty() && tts_task.is_none() {
                let spoken = adapter
                    .released_spoken_text()
                    .expect("validated spoken field is retained")
                    .to_owned();
                let words = spoken.split_whitespace().count();
                assert!(
                    (10..=15).contains(&words),
                    "live response must remain a realistic 10-15 word sentence"
                );
                spoken_field_validated_us = Some(elapsed_us(&origin));
                tts_task = Some(tokio::spawn(synthesize_to_memory(
                    Arc::clone(&tts_bridge),
                    identity.clone(),
                    spoken,
                    Arc::clone(&origin),
                )));
            }
        }
    })
    .await
    .expect("bounded Groq stream deadline");
    let llm_stream_finished_us = elapsed_us(&origin);
    let finalized = adapter
        .finish(identity.cancellation_generation)
        .expect("strict complete response envelope validation");
    let llm_envelope_validated_us = elapsed_us(&origin);
    assert_eq!(finalized.all_sentences.len(), 1);
    let response_hash = format!(
        "{:x}",
        Sha256::digest(finalized.response.spoken_response.text.as_bytes())
    );
    let tts = tokio::time::timeout(
        Duration::from_secs(30),
        tts_task.expect("speech-first field must dispatch TTS"),
    )
    .await
    .expect("bounded Cartesia task deadline")
    .expect("Cartesia task joined");
    let provider = provider_timing
        .lock()
        .expect("provider timing mutex")
        .clone();
    let spoken_field_validated_us =
        spoken_field_validated_us.expect("spoken field validation timestamp");
    let first_provider_delta_us = first_provider_delta_us.expect("first LLM delta timestamp");
    let provider_session_request_us = provider.session_request_us.expect("provider request time");
    let provider_session_ready_us = provider.session_ready_us.expect("provider ready time");
    let provider_push_started_us = provider.push_started_us.expect("provider push start time");
    let provider_push_finished_us = provider
        .push_finished_us
        .expect("provider push finish time");
    let provider_finish_started_us = provider
        .finish_started_us
        .expect("provider finish start time");
    let provider_finish_finished_us = provider
        .finish_finished_us
        .expect("provider finish end time");
    let provider_first_pcm_us = provider.first_pcm_us.expect("provider first PCM time");
    let provider_completed_us = provider.completed_us.expect("provider completion time");
    assert!(tts.task_started_us >= spoken_field_validated_us);
    assert!(tts.bridge_first_pcm_us >= provider_first_pcm_us);
    assert!(llm_envelope_validated_us >= spoken_field_validated_us);

    let receipt = json!({
        "schemaVersion": 1,
        "receiptType": "groqQwenToCartesiaSpeechFirstComponentChain",
        "checkedAtEpochMs": SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_millis() as u64,
        "scope": "Production selected Groq RuntimeBridge and production Cartesia RuntimeTtsBridge connected by the runtime StreamingResponseAdapterV1 speech-first gate; this is a component-chain qualification and not a HostState, native broker, or physical playback claim.",
        "llm": {
            "providerId": "groq",
            "modelId": GROQ_QWEN_MODEL_ID,
            "reasoningEffort": "none",
            "routeFormat": "structured_speech_first_v1",
            "requestToStreamOpenUs": llm_stream_opened_us.saturating_sub(llm_request_started_us),
            "requestToFirstProviderDeltaUs": first_provider_delta_us.saturating_sub(llm_request_started_us),
            "requestToSpokenFieldValidatedUs": spoken_field_validated_us.saturating_sub(llm_request_started_us),
            "requestToStreamFinishedUs": llm_stream_finished_us.saturating_sub(llm_request_started_us),
            "requestToEnvelopeValidatedUs": llm_envelope_validated_us.saturating_sub(llm_request_started_us),
            "spokenFieldValidatedBeforeStreamFinished": spoken_field_validated_us < llm_stream_finished_us,
            "structuredResponseValidated": true,
            "spokenSentenceCount": finalized.all_sentences.len(),
            "spokenWordCount": finalized.response.spoken_response.text.split_whitespace().count(),
            "spokenResponseBytes": finalized.response.spoken_response.text.len(),
            "spokenResponseSha256": response_hash,
        },
        "tts": {
            "providerId": "cartesia",
            "modelId": CARTESIA_QUALIFIED_MODEL_ID,
            "voiceId": CARTESIA_QUALIFIED_STOCK_VOICE_ID,
            "sampleRateHz": CARTESIA_QUALIFIED_AUDIO_FORMAT.sample_rate_hz,
            "channels": CARTESIA_QUALIFIED_AUDIO_FORMAT.channels,
            "validatedFieldToTaskStartUs": tts.task_started_us.saturating_sub(spoken_field_validated_us),
            "taskStartToRuntimeSessionReadyUs": tts.runtime_session_ready_us.saturating_sub(tts.task_started_us),
            "providerSessionRequestUs": provider_session_request_us,
            "providerSessionReadyUs": provider_session_ready_us,
            "providerSessionHandshakeUs": provider_session_ready_us.saturating_sub(provider_session_request_us),
            "providerPushStartedUs": provider_push_started_us,
            "providerPushFinishedUs": provider_push_finished_us,
            "providerPushUs": provider_push_finished_us.saturating_sub(provider_push_started_us),
            "providerFinishStartedUs": provider_finish_started_us,
            "providerFinishFinishedUs": provider_finish_finished_us,
            "providerFinishUs": provider_finish_finished_us.saturating_sub(provider_finish_started_us),
            "synthesizeReturnedUs": tts.synthesize_returned_us,
            "providerFirstPcmUs": provider_first_pcm_us,
            "providerSessionReadyToFirstPcmUs": provider_first_pcm_us.saturating_sub(provider_session_ready_us),
            "bridgeFirstPcmUs": tts.bridge_first_pcm_us,
            "providerToBridgeFirstPcmUs": tts.bridge_first_pcm_us.saturating_sub(provider_first_pcm_us),
            "validatedFieldToProviderFirstPcmUs": provider_first_pcm_us.saturating_sub(spoken_field_validated_us),
            "validatedFieldToBridgeFirstPcmUs": tts.bridge_first_pcm_us.saturating_sub(spoken_field_validated_us),
            "providerCompletedUs": provider_completed_us,
            "bridgeCompleteUs": tts.bridge_complete_us,
            "pcmBytes": tts.pcm_bytes,
            "audioChunks": tts.audio_chunks,
            "alignmentEvents": tts.alignment_events,
            "pcmSha256": tts.pcm_sha256,
        },
        "combined": {
            "requestToProviderFirstPcmUs": provider_first_pcm_us.saturating_sub(llm_request_started_us),
            "requestToBridgeFirstPcmUs": tts.bridge_first_pcm_us.saturating_sub(llm_request_started_us),
            "requestToBridgeCompleteUs": tts.bridge_complete_us.saturating_sub(llm_request_started_us),
        },
        "containsCredential": false,
        "containsDialogueText": false,
        "audioDeliveryClaimed": false,
        "physicalAudibilityClaimed": false,
        "nativeBrokerReceipt": false,
        "wavWritten": false,
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)
        .expect("create combined report");
    file.write_all(&serde_json::to_vec_pretty(&receipt).expect("serialize combined report"))
        .expect("write combined report");
}
