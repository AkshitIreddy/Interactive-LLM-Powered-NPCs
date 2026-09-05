use std::{path::PathBuf, sync::Arc, time::Duration};

use npc_runtime_core::{DeliveryMode, TurnEvent, TurnLifecycle};
#[cfg(all(windows, feature = "dev-wasapi-audio", debug_assertions))]
use npc_runtime_host::simulation::{DevLiveTtsRequest, SimulationExecutionMode};
use npc_runtime_host::{
    simulation::SimulationSafetyContext, DeliveryCommitState, HostConfig, HostState,
    ManualFallbackActivation, ManualFallbackRoute, PrivateEvaluationModeV1,
    ProviderPrivateEvaluationAcknowledgementV1, PushToTalkCaptureState, RouteExecution,
    SelectedProviderRoute, SelectedRoleRoute, SelectedRouteRoles, SelectedRouteSnapshot,
    SelectedRouteState, SelectedSttRouteReceiptV1, SelectedSttTurnEvidenceV1, SimulationRequest,
    TurnDeliveryRequest, TurnDeliveryState, TurnExecutionDegradation, TurnInputMode,
    TurnInputSnapshot, NVIDIA_MAGPIE_PRIVATE_EVALUATION_TERMS_REVISION,
    PRIVATE_EVALUATION_ACKNOWLEDGEMENT_SCHEMA_VERSION,
    PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE,
    PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE, TURN_ROUTE_SCHEMA_VERSION,
};
use tokio_util::sync::CancellationToken;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("runtime-host is nested under apps")
        .to_path_buf()
}

async fn host() -> HostState {
    let app_data = tempfile::tempdir().expect("temporary app data");
    // HostState owns its open stores, so keep the directory for the process
    // lifetime instead of deleting files underneath SQLite.
    let app_data = app_data.keep();
    HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data,
    })
    .await
    .expect("initialize runtime host")
}

fn disabled_role() -> SelectedRoleRoute {
    SelectedRoleRoute {
        state: SelectedRouteState::Disabled,
        primary: None,
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn selected_route(llm_model: &str, tts: SelectedRoleRoute) -> SelectedRouteSnapshot {
    SelectedRouteSnapshot {
        schema_version: TURN_ROUTE_SCHEMA_VERSION,
        source_loadout_id: "integration-loadout".into(),
        inheritance_chain: vec!["base-loadout".into(), "integration-loadout".into()],
        generation: 41,
        roles: SelectedRouteRoles {
            llm: SelectedRoleRoute {
                state: SelectedRouteState::Ready,
                primary: Some(SelectedProviderRoute {
                    provider_id: "mock-llm".into(),
                    model_id: llm_model.into(),
                    voice_id: None,
                    execution: RouteExecution::Local,
                    egress: "conversation_text".into(),
                    credential_reference: None,
                }),
                fallbacks: Vec::new(),
                degradation: None,
            },
            stt: disabled_role(),
            tts,
            embeddings: disabled_role(),
            vision: disabled_role(),
            lip_sync: disabled_role(),
        },
    }
}

fn stock_tts() -> SelectedRoleRoute {
    SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(SelectedProviderRoute {
            provider_id: "mock-tts".into(),
            model_id: "mock-pcm-v1".into(),
            voice_id: Some("mock-stock-voice-1".into()),
            execution: RouteExecution::Local,
            egress: "conversation_audio".into(),
            credential_reference: None,
        }),
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn hosted_stock_tts(
    provider_id: &str,
    model_id: &str,
    voice_id: &str,
    credential_reference: &str,
) -> SelectedRoleRoute {
    SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(SelectedProviderRoute {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            voice_id: Some(voice_id.into()),
            execution: RouteExecution::Cloud,
            egress: "conversation_audio".into(),
            credential_reference: Some(credential_reference.into()),
        }),
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn request(route_snapshot: SelectedRouteSnapshot) -> SimulationRequest {
    SimulationRequest {
        session_id: "selected-route-session".into(),
        turn_id: "selected-route-turn".into(),
        game_id: "skyrim-special-edition".into(),
        character_id: Some("lydia".into()),
        effective_game_profile: None,
        native_identity_decision: None,
        enabled_spoiler_tiers: Vec::new(),
        generic_selection: None,
        safety_context: SimulationSafetyContext::verified_safe(),
        application_namespace: None,
        transcript: "Give me a short status report.".into(),
        locale: "en-US".into(),
        execution_mode: None,
        dev_live_tts: None,
        route_snapshot: Some(route_snapshot),
        input: TurnInputSnapshot::default(),
        delivery: TurnDeliveryRequest::default(),
        audio_playback_leases: Vec::new(),
        private_evaluation_acknowledgements: Vec::new(),
        subtitle_presentation_context: None,
    }
}

fn with_selected_stt(mut snapshot: SelectedRouteSnapshot) -> SelectedRouteSnapshot {
    snapshot.roles.stt = SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(SelectedProviderRoute {
            provider_id: "assemblyai".into(),
            model_id: "u3-rt-pro".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "microphone_audio_and_optional_non_secret_context".into(),
            credential_reference: Some("providers/assemblyai".into()),
        }),
        fallbacks: Vec::new(),
        degradation: None,
    };
    snapshot
}

fn selected_stt_receipt(transcript: &str) -> SelectedSttTurnEvidenceV1 {
    let mut receipt = SelectedSttTurnEvidenceV1 {
        schema_version: 1,
        receipt_id: "10000000-0000-4000-8000-000000000001".into(),
        receipt_sha256: String::new(),
        capture_session_id: "20000000-0000-4000-8000-000000000002".into(),
        capture_turn_id: "30000000-0000-4000-8000-000000000003".into(),
        // STT capture attempts and response turns have independent generation
        // counters; only the receipt's capture and route generations match.
        capture_generation: 9,
        game_id: "skyrim-special-edition".into(),
        character_id: Some("lydia".into()),
        source_loadout_id: "integration-loadout".into(),
        route: SelectedSttRouteReceiptV1 {
            provider_id: "assemblyai".into(),
            model_id: "u3-rt-pro".into(),
            credential_reference: "providers/assemblyai".into(),
            egress: "microphone_audio_and_optional_non_secret_context".into(),
            generation: 9,
            input_endpoint_id: "fixture-input-endpoint".into(),
            input_endpoint_generation: 7,
            manual_retry: false,
            automatic_fallback: false,
            captured_frames: 24_000,
            ptt_virtual_key: 86,
            ptt_press_transition_sequence: 10,
            ptt_pressed_qpc: 1_000,
            ptt_release_transition_sequence: 11,
            ptt_released_qpc: 2_000,
        },
        chunks_sent: 12,
        pcm_bytes_sent: 48_000,
        partial_events: 3,
    };
    receipt.receipt_sha256 = receipt
        .canonical_sha256(transcript)
        .expect("canonical selected STT digest");
    receipt
}

fn magpie_acknowledgement(catalog_revision: u64) -> ProviderPrivateEvaluationAcknowledgementV1 {
    ProviderPrivateEvaluationAcknowledgementV1 {
        schema_version: PRIVATE_EVALUATION_ACKNOWLEDGEMENT_SCHEMA_VERSION,
        provider_id: "nvidia-nim".into(),
        application_namespace: PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into(),
        mode: PrivateEvaluationModeV1::PrivateEvaluationOnly,
        terms_revision: NVIDIA_MAGPIE_PRIVATE_EVALUATION_TERMS_REVISION.into(),
        catalog_revision,
        acknowledged_at_epoch_ms: 1,
        promotion_supported: false,
        publication_supported: false,
    }
}

#[test]
fn old_magpie_only_acknowledgement_is_invalid_and_requires_reprompt() {
    let mut acknowledgement = magpie_acknowledgement(1);
    acknowledgement.provider_id = "nvidia-nim-magpie".into();
    acknowledgement.terms_revision =
        "nvidia-api-trial-terms-2025-09-19-magpie-private-evaluation-v1".into();
    assert!(acknowledgement.validate_shape().is_err());
}

#[tokio::test]
async fn nvidia_llm_and_embedding_routes_require_provider_wide_private_evaluation_authority() {
    let state = host().await;
    let mut nvidia_llm = selected_route("unused", stock_tts());
    nvidia_llm.roles.llm.primary = Some(SelectedProviderRoute {
        provider_id: "nvidia-nim".into(),
        model_id: "nvidia/nemotron-3-nano-30b-a3b".into(),
        voice_id: None,
        execution: RouteExecution::Cloud,
        egress: "conversation_text_and_derived_game_context".into(),
        credential_reference: Some("providers/nvidia-nim".into()),
    });
    let mut request_without_llm_ack = request(nvidia_llm);
    request_without_llm_ack.application_namespace =
        Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    assert!(matches!(
        state.simulate_turn(request_without_llm_ack).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut nvidia_embedding = selected_route("mock-stream-v1", stock_tts());
    nvidia_embedding.roles.embeddings = SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(SelectedProviderRoute {
            provider_id: "nvidia-nim".into(),
            model_id: "nvidia/nemotron-3-embed-1b".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "selected_memory_and_lore_text".into(),
            credential_reference: Some("providers/nvidia-nim".into()),
        }),
        fallbacks: Vec::new(),
        degradation: None,
    };
    let mut request_without_embedding_ack = request(nvidia_embedding);
    request_without_embedding_ack.application_namespace =
        Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    assert!(matches!(
        state.simulate_turn(request_without_embedding_ack).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut combined = selected_route(
        "unused",
        hosted_stock_tts(
            "nvidia-nim-magpie",
            "magpie-tts-multilingual",
            "Magpie-Multilingual.EN-US.Aria",
            "providers/nvidia-nim",
        ),
    );
    combined.roles.llm.primary = Some(SelectedProviderRoute {
        provider_id: "nvidia-nim".into(),
        model_id: "nvidia/nemotron-3-nano-30b-a3b".into(),
        voice_id: None,
        execution: RouteExecution::Cloud,
        egress: "conversation_text_and_derived_game_context".into(),
        credential_reference: Some("providers/nvidia-nim".into()),
    });
    combined.roles.embeddings = SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(SelectedProviderRoute {
            provider_id: "nvidia-nim".into(),
            model_id: "nvidia/nemotron-3-embed-1b".into(),
            voice_id: None,
            execution: RouteExecution::Cloud,
            egress: "selected_memory_and_lore_text".into(),
            credential_reference: Some("providers/nvidia-nim".into()),
        }),
        fallbacks: Vec::new(),
        degradation: None,
    };
    let mut admitted = request(combined);
    admitted.application_namespace = Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    admitted.private_evaluation_acknowledgements =
        vec![magpie_acknowledgement(state.catalog.catalog_revision)];
    let result = state
        .simulate_turn(admitted)
        .await
        .expect("one provider-wide authority admits all three selected NVIDIA modalities");
    let evidence = result.turn_execution.expect("provider-wide evidence");
    assert_eq!(
        serde_json::to_value(
            evidence
                .consumed_route
                .private_evaluation_acknowledgement
                .expect("consumed acknowledgement")
                .modalities
        )
        .expect("serialize modalities"),
        serde_json::json!(["llm", "embeddings", "tts"])
    );
}

#[tokio::test]
async fn typed_turn_streams_the_pinned_llm_and_commits_receipt_backed_stock_voice_audio() {
    let result = host()
        .await
        .simulate_turn(request(selected_route("mock-stream-v1", stock_tts())))
        .await
        .expect("execute selected-route turn");

    let evidence = result.turn_execution.expect("selected-route evidence");
    assert_eq!(result.integration_mode, "selected_route_turn");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert_eq!(
        result.outcome.selected_llm_provider.as_deref(),
        Some("mock-llm")
    );
    assert_eq!(result.outcome.selected_tts_providers, ["mock-tts"]);
    assert!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, TurnEvent::TextDelta { .. }))
            .count()
            >= 2
    );
    assert_eq!(evidence.consumed_route.generation, 41);
    assert_eq!(evidence.consumed_route.sha256.len(), 64);
    assert_eq!(evidence.delivery_state, TurnDeliveryState::Delivered);
    assert_eq!(evidence.commit_state, DeliveryCommitState::Committed);
    assert!(!evidence.success.llm_provider_live);
    assert!(!evidence.success.tts_provider_live);
    assert!(evidence.success.stt_skipped);
    assert!(evidence.success.subtitle_delivered);
    assert_eq!(
        evidence.success.subtitle_receipt_count,
        result.outcome.delivered.len()
    );
    assert_eq!(
        evidence.subtitle_presentation_receipts.len(),
        result.outcome.delivered.len()
    );
    assert!(evidence
        .subtitle_presentation_receipts
        .iter()
        .all(|receipt| receipt.committed));
    assert!(!evidence.success.audio_submitted);
    assert!(!evidence.success.audio_drained);
    assert_eq!(evidence.success.audio_receipt_count, 0);
    assert_eq!(evidence.subtitles.len(), result.outcome.delivered.len());
    assert!(evidence.audio_receipts.is_empty());
    assert!(result
        .outcome
        .delivered
        .iter()
        .all(|sentence| sentence.delivery == DeliveryMode::Audio));
    // Deterministic PCM measurement is provider-format evidence only. It never
    // crosses the native boundary as an output-device receipt.
}

#[tokio::test]
async fn unknown_safety_evidence_fails_closed_before_selected_provider_execution() {
    let mut turn = request(selected_route("mock-stream-v1", stock_tts()));
    turn.safety_context = SimulationSafetyContext::default();

    let result = host().await.simulate_turn(turn).await;

    assert!(matches!(
        result,
        Err(npc_runtime_host::simulation::SimulationError::SafetyEvidenceUnverified)
    ));
}

#[tokio::test]
async fn ordinary_hosted_tts_without_control_issued_broker_leases_requires_manual_retry() {
    for tts in [hosted_stock_tts(
        "elevenlabs",
        "eleven_flash_v2_5",
        "EXAVITQu4vr4xnSDxMaL",
        "providers/elevenlabs",
    )] {
        let result = host()
            .await
            .simulate_turn(request(selected_route("mock-stream-v1", tts)))
            .await
            .expect("return typed manual retry without starting a provider");
        let evidence = result.turn_execution.expect("selected-route evidence");

        assert_eq!(result.outcome.lifecycle, TurnLifecycle::Failed);
        assert_eq!(
            evidence.delivery_state,
            TurnDeliveryState::ManualRetryRequired
        );
        assert!(evidence.audio_receipts.is_empty());
        assert!(!evidence.success.tts_provider_live);
        assert!(!evidence.success.audio_submitted);
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, TurnEvent::TextDelta { .. })));
        assert!(evidence.degradations.iter().any(|degradation| matches!(
            degradation,
            TurnExecutionDegradation::ManualRetryRequired {
                failed_role,
                reason,
                ..
            } if failed_role == "tts" && reason.contains("no one-time playback leases")
        )));
    }
}

#[tokio::test]
async fn dormant_magpie_manual_fallback_does_not_consume_private_evaluation_authority() {
    let mut snapshot = selected_route(
        "mock-stream-v1",
        hosted_stock_tts(
            "elevenlabs",
            "eleven_flash_v2_5",
            "EXAVITQu4vr4xnSDxMaL",
            "providers/elevenlabs",
        ),
    );
    let magpie = hosted_stock_tts(
        "nvidia-nim-magpie",
        "magpie-tts-multilingual",
        "Magpie-Multilingual.EN-US.Aria",
        "providers/nvidia-nim",
    )
    .primary
    .expect("Magpie fallback route");
    snapshot.roles.tts.fallbacks.push(ManualFallbackRoute {
        route: magpie,
        activation: ManualFallbackActivation::ManualOnly,
        user_authorized: true,
    });

    let result = host()
        .await
        .simulate_turn(request(snapshot))
        .await
        .expect("dormant fallback does not require private-evaluation authority");
    let evidence = result.turn_execution.expect("selected-route evidence");
    assert_eq!(
        evidence
            .consumed_route
            .tts
            .as_ref()
            .map(|route| route.provider_id.as_str()),
        Some("elevenlabs")
    );
    assert!(evidence
        .consumed_route
        .private_evaluation_acknowledgement
        .is_none());
}

#[tokio::test]
async fn magpie_requires_current_private_evaluation_ack_and_emits_nonpromotion_evidence() {
    let state = host().await;
    let snapshot = selected_route(
        "mock-stream-v1",
        hosted_stock_tts(
            "nvidia-nim-magpie",
            "magpie-tts-multilingual",
            "Magpie-Multilingual.EN-US.Aria",
            "providers/nvidia-nim",
        ),
    );

    let mut missing_request = request(snapshot.clone());
    missing_request.application_namespace =
        Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    let missing = state.simulate_turn(missing_request).await;
    assert!(matches!(
        missing,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut stale = request(snapshot.clone());
    stale.application_namespace = Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    stale.private_evaluation_acknowledgements = vec![magpie_acknowledgement(
        state.catalog.catalog_revision.saturating_add(1),
    )];
    assert!(matches!(
        state.simulate_turn(stale).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut cross_namespace_replay = request(snapshot.clone());
    cross_namespace_replay.application_namespace =
        Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    let mut invalid_authority = magpie_acknowledgement(state.catalog.catalog_revision);
    invalid_authority.application_namespace = PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE.into();
    cross_namespace_replay.private_evaluation_acknowledgements = vec![invalid_authority];
    assert!(matches!(
        state.simulate_turn(cross_namespace_replay).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut debug_namespace = magpie_acknowledgement(state.catalog.catalog_revision);
    debug_namespace.application_namespace = PRIVATE_EVALUATION_DEBUG_APPLICATION_NAMESPACE.into();
    assert!(debug_namespace.validate_shape().is_ok());

    let mut admitted = request(snapshot);
    admitted.application_namespace = Some(PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE.into());
    admitted.private_evaluation_acknowledgements =
        vec![magpie_acknowledgement(state.catalog.catalog_revision)];
    let result = state
        .simulate_turn(admitted)
        .await
        .expect("current native acknowledgement reaches lease admission");
    let evidence = result.turn_execution.expect("selected-route evidence");
    assert_eq!(
        evidence.delivery_state,
        TurnDeliveryState::ManualRetryRequired
    );
    let consumed = evidence
        .consumed_route
        .private_evaluation_acknowledgement
        .expect("nonsecret acknowledgement evidence");
    assert_eq!(
        consumed.terms_revision,
        NVIDIA_MAGPIE_PRIVATE_EVALUATION_TERMS_REVISION
    );
    assert_eq!(consumed.catalog_revision, state.catalog.catalog_revision);
    assert_eq!(
        consumed.application_namespace,
        PRIVATE_EVALUATION_REVIEW_APPLICATION_NAMESPACE
    );
    assert_eq!(consumed.acknowledgement_sha256.len(), 64);
    assert!(!consumed.promotion_supported);
    assert!(!consumed.publication_supported);
}

#[tokio::test]
async fn unavailable_push_to_talk_capture_uses_typed_input_and_subtitle_only_delivery() {
    let mut turn = request(selected_route("mock-stream-v1", stock_tts()));
    turn.input = TurnInputSnapshot {
        mode: TurnInputMode::PushToTalk,
        push_to_talk_state: PushToTalkCaptureState::CaptureUnavailable,
        selected_stt_receipt: None,
    };
    turn.delivery.audio = false;

    let result = host()
        .await
        .simulate_turn(turn)
        .await
        .expect("execute typed PTT fallback");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert!(evidence.audio_receipts.is_empty());
    assert!(!evidence.subtitles.is_empty());
    assert!(result
        .outcome
        .delivered
        .iter()
        .all(|sentence| sentence.delivery == DeliveryMode::Subtitle));
    assert!(evidence
        .degradations
        .iter()
        .any(|degradation| matches!(degradation, TurnExecutionDegradation::TypedInput { .. })));
    assert!(evidence
        .degradations
        .iter()
        .any(|degradation| matches!(degradation, TurnExecutionDegradation::SubtitleOnly { .. })));
}

#[tokio::test]
async fn receipt_backed_push_to_talk_is_preserved_and_does_not_claim_stt_skipped() {
    let state = host().await;
    let mut prior_typed_snapshot = with_selected_stt(selected_route("mock-stream-v1", stock_tts()));
    prior_typed_snapshot.generation = 40;
    let mut prior_typed = request(prior_typed_snapshot);
    prior_typed.turn_id = "typed-before-ptt".into();
    prior_typed.delivery.audio = false;
    state
        .simulate_turn(prior_typed)
        .await
        .expect("typed turn before selected STT receipt");

    let mut turn = request(with_selected_stt(selected_route(
        "mock-stream-v1",
        stock_tts(),
    )));
    let mut receipt = selected_stt_receipt(&turn.transcript);
    receipt.route.manual_retry = true;
    receipt.receipt_sha256 = receipt
        .canonical_sha256(&turn.transcript)
        .expect("manual-retry selected STT digest");
    assert_ne!(receipt.capture_generation, 41);
    turn.input = TurnInputSnapshot {
        mode: TurnInputMode::PushToTalk,
        push_to_talk_state: PushToTalkCaptureState::TranscriptReady,
        selected_stt_receipt: Some(receipt.clone()),
    };
    turn.delivery.audio = false;

    let result = state
        .simulate_turn(turn)
        .await
        .expect("execute receipt-backed PTT turn");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert!(!evidence.success.stt_skipped);
    assert_eq!(evidence.input.mode, TurnInputMode::PushToTalk);
    assert_eq!(
        evidence.input.push_to_talk_state,
        PushToTalkCaptureState::TranscriptReady
    );
    assert_eq!(evidence.input.selected_stt_receipt, Some(receipt));
    assert!(!evidence.subtitles.is_empty());
}

#[tokio::test]
async fn typed_and_mismatched_push_to_talk_receipts_fail_closed() {
    let state = host().await;
    let snapshot = with_selected_stt(selected_route("mock-stream-v1", stock_tts()));

    let mut typed = request(snapshot.clone());
    typed.input.selected_stt_receipt = Some(selected_stt_receipt(&typed.transcript));
    assert!(matches!(
        state.simulate_turn(typed).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut digest_mismatch = request(snapshot.clone());
    let mut receipt = selected_stt_receipt(&digest_mismatch.transcript);
    receipt.receipt_sha256.replace_range(0..1, "f");
    digest_mismatch.input = TurnInputSnapshot {
        mode: TurnInputMode::PushToTalk,
        push_to_talk_state: PushToTalkCaptureState::TranscriptReady,
        selected_stt_receipt: Some(receipt),
    };
    assert!(matches!(
        state.simulate_turn(digest_mismatch).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));

    let mut endpoint_mismatch = request(snapshot);
    let mut receipt = selected_stt_receipt(&endpoint_mismatch.transcript);
    receipt.route.generation += 1;
    receipt.receipt_sha256 = receipt
        .canonical_sha256(&endpoint_mismatch.transcript)
        .expect("recompute mismatch digest");
    endpoint_mismatch.input = TurnInputSnapshot {
        mode: TurnInputMode::PushToTalk,
        push_to_talk_state: PushToTalkCaptureState::TranscriptReady,
        selected_stt_receipt: Some(receipt),
    };
    assert!(matches!(
        state.simulate_turn(endpoint_mismatch).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));
}

#[tokio::test]
async fn selected_stt_receipt_is_one_use_within_the_runtime_process() {
    let state = host().await;
    let mut turn = request(with_selected_stt(selected_route(
        "mock-stream-v1",
        stock_tts(),
    )));
    turn.input = TurnInputSnapshot {
        mode: TurnInputMode::PushToTalk,
        push_to_talk_state: PushToTalkCaptureState::TranscriptReady,
        selected_stt_receipt: Some(selected_stt_receipt(&turn.transcript)),
    };
    turn.delivery.audio = false;

    state
        .simulate_turn(turn.clone())
        .await
        .expect("first receipt consumption");
    assert!(matches!(
        state.simulate_turn(turn).await,
        Err(npc_runtime_host::simulation::SimulationError::InvalidRequest)
    ));
}

#[tokio::test]
async fn audio_only_fixture_requires_manual_retry_without_a_native_sink_receipt() {
    let mut turn = request(selected_route("mock-stream-v1", stock_tts()));
    turn.delivery.subtitles = false;

    let result = host()
        .await
        .simulate_turn(turn)
        .await
        .expect("execute audio-only turn");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert!(evidence.subtitles.is_empty());
    assert!(evidence.audio_receipts.is_empty());
    assert!(!evidence.success.audio_submitted);
    assert!(!evidence.success.audio_drained);
    assert_eq!(
        evidence.delivery_state,
        TurnDeliveryState::ManualRetryRequired
    );
    assert_eq!(evidence.commit_state, DeliveryCommitState::NotCommitted);
    assert!(evidence
        .degradations
        .iter()
        .any(|degradation| matches!(degradation, TurnExecutionDegradation::ManualRetryRequired { failed_role, .. } if failed_role == "tts")));
}

#[tokio::test]
async fn disabled_tts_degrades_to_subtitles_without_claiming_an_audio_receipt() {
    let result = host()
        .await
        .simulate_turn(request(selected_route("mock-stream-v1", disabled_role())))
        .await
        .expect("execute subtitle fallback");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert_eq!(evidence.delivery_state, TurnDeliveryState::Delivered);
    assert_eq!(evidence.commit_state, DeliveryCommitState::CommitDeferred);
    assert!(evidence.audio_receipts.is_empty());
    assert!(!evidence.success.audio_submitted);
    assert!(!evidence.success.audio_drained);
    assert_eq!(evidence.success.audio_receipt_count, 0);
    assert!(!evidence.subtitles.is_empty());
    assert!(result.outcome.selected_tts_providers.is_empty());
    assert!(result
        .outcome
        .delivered
        .iter()
        .all(|sentence| sentence.delivery == DeliveryMode::Subtitle));
    assert!(evidence
        .degradations
        .iter()
        .any(|degradation| matches!(degradation, TurnExecutionDegradation::SubtitleOnly { .. })));
}

#[tokio::test]
async fn unavailable_llm_requires_explicit_manual_retry_and_never_activates_its_fallback() {
    let mut route = selected_route("mock-stream-v1", stock_tts());
    route.roles.llm.state = SelectedRouteState::Degraded;
    route.roles.llm.fallbacks.push(ManualFallbackRoute {
        route: SelectedProviderRoute {
            provider_id: "mock-llm-fallback".into(),
            model_id: "mock-stream-v1".into(),
            voice_id: None,
            execution: RouteExecution::Local,
            egress: "conversation_text".into(),
            credential_reference: None,
        },
        activation: ManualFallbackActivation::ManualOnly,
        user_authorized: true,
    });

    let result = host()
        .await
        .simulate_turn(request(route))
        .await
        .expect("return explicit manual-retry state");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Failed);
    assert_eq!(
        evidence.delivery_state,
        TurnDeliveryState::ManualRetryRequired
    );
    assert_eq!(evidence.commit_state, DeliveryCommitState::NotCommitted);
    assert!(result.outcome.selected_llm_provider.is_none());
    assert!(result.outcome.selected_tts_providers.is_empty());
    assert!(evidence.subtitles.is_empty());
    assert!(evidence.audio_receipts.is_empty());
    assert!(!result
        .events
        .iter()
        .any(|event| matches!(event, TurnEvent::TextDelta { .. })));
    assert!(evidence.degradations.iter().any(|degradation| matches!(
        degradation,
        TurnExecutionDegradation::ManualRetryRequired {
            failed_role,
            ..
        } if failed_role == "llm"
    )));
}

#[tokio::test]
async fn cancellation_closes_delivery_without_committing_queued_output() {
    let state = Arc::new(host().await);
    let cancellation = CancellationToken::new();
    let cancel_turn = cancellation.clone();
    let task = {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            state
                .simulate_turn_cancellable(
                    request(selected_route("mock-stream-cancellable-v1", stock_tts())),
                    cancel_turn,
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(10)).await;
    cancellation.cancel();

    let result = task
        .await
        .expect("join turn")
        .expect("return cancellation evidence");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Cancelled);
    assert_eq!(evidence.delivery_state, TurnDeliveryState::Cancelled);
    assert_eq!(evidence.commit_state, DeliveryCommitState::NotCommitted);
    assert!(evidence.subtitles.is_empty());
    assert!(evidence.audio_receipts.is_empty());
}

#[cfg(all(windows, feature = "dev-wasapi-audio"))]
#[tokio::test]
#[ignore = "explicit live ElevenLabs stock-voice and WASAPI receipt qualification"]
async fn explicit_debug_elevenlabs_route_requires_live_provider_and_wasapi_receipts() {
    let mut turn = request(selected_route(
        "mock-stream-v1",
        hosted_stock_tts(
            "elevenlabs",
            "eleven_flash_v2_5",
            "EXAVITQu4vr4xnSDxMaL",
            "providers/elevenlabs",
        ),
    ));
    turn.execution_mode = Some(SimulationExecutionMode::Hybrid);
    turn.dev_live_tts = Some(DevLiveTtsRequest {
        provider_id: "elevenlabs".into(),
        model_id: "eleven_flash_v2_5".into(),
        voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
        explicit_user_authorization: true,
    });
    let result = host()
        .await
        .simulate_turn(turn)
        .await
        .expect("qualified explicit debug ElevenLabs turn");
    let evidence = result.turn_execution.expect("selected-route evidence");

    assert!(!result.fixture_only);
    assert!(!evidence.success.llm_provider_live);
    assert!(evidence.success.tts_provider_live);
    assert!(evidence.success.audio_submitted);
    assert!(evidence.success.audio_drained);
    assert!(evidence.success.audio_receipt_count > 0);
    assert!(evidence.audio_receipts.iter().all(|receipt| {
        receipt.sink == "devWasapiSubmission"
            && receipt.source_frames > 0
            && receipt.device_frames > 0
            && receipt.submitted
            && receipt.drained
            && receipt.completed
    }));
}
