#![allow(clippy::unwrap_used)]

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

use interactive_npcs_game_discovery::{
    Confidence, DetectionSource, EditionEvidence, InstallationCandidate, InstallationEvidence,
    StoreKind,
};
use npc_runtime_core::TurnLifecycle;
use npc_runtime_host::profile_replays::ProfileReplayCorpus;
use npc_runtime_host::profiles::{GenericGameSelection, GENERIC_GAME_ID};
use npc_runtime_host::{
    simulation::{
        DevLiveTtsRequest, SimulationExecutionMode, SimulationProfilePolicy,
        SimulationSafetyContext, SimulationSafetyEvidenceState,
    },
    CatalogTrustState, HostConfig, HostState, SimulationRequest,
    REQUIRED_AUTHORED_GAME_PROFILE_COUNT, REQUIRED_PROFILE_COUNT,
    REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT, SYNTHETIC_REVIEW_PROFILE_ID,
};

#[cfg(feature = "test-fixture-vault")]
use interactive_npcs_credential_vault::MemoryCredentialVault;
#[cfg(windows)]
use interactive_npcs_credential_vault::{CredentialVault, WindowsCredentialVault};
#[cfg(windows)]
use npc_providers_tts::{
    AudioFormat, ElevenLabsConfig, ElevenLabsProvider, ElevenLabsWebSocketTransport,
    HostedTtsProviderId, SemanticClausePolicy, SessionIdentity, StreamingTtsProvider, TtsEvent,
    TtsSessionRequest, VoiceBinding, VoiceBindings,
};
#[cfg(windows)]
use npc_runtime_host::tts_bridge::VaultTtsCredentialResolver;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "explicit live ElevenLabs WebSocket qualification using the fixed Windows vault target"]
async fn direct_elevenlabs_websocket_stock_voice_qualification() {
    let vault: Arc<dyn CredentialVault> =
        Arc::new(WindowsCredentialVault::new("interactive-npcs/v2").unwrap());
    let resolver = Arc::new(VaultTtsCredentialResolver::new(vault));
    let bindings = VoiceBindings::new([VoiceBinding {
        intent_id: "dev.elevenlabs.stock".into(),
        provider_id: HostedTtsProviderId::ElevenLabs,
        voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
        model_id: "eleven_flash_v2_5".into(),
        provider_options: Default::default(),
    }])
    .unwrap();
    let provider = ElevenLabsProvider::new(
        Arc::new(ElevenLabsWebSocketTransport::default()),
        resolver,
        bindings,
        ElevenLabsConfig::default(),
    );
    let mut session = provider
        .start_session(TtsSessionRequest {
            identity: SessionIdentity {
                session_id: "direct-live-provider-qualification".into(),
                turn_id: "direct-live-provider-turn-1".into(),
                cancellation_generation: 0,
            },
            locale: "en-US".into(),
            voice_intent_id: "dev.elevenlabs.stock".into(),
            output: AudioFormat::default(),
            request_alignment: true,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
        })
        .await
        .expect("authenticated WebSocket session");
    session
        .push_text("Live route qualification.")
        .await
        .expect("text accepted");
    session.finish().await.expect("utterance finalized");

    let mut audio_bytes = 0usize;
    let mut completed = false;
    while let Some(event) = session.next_event().await {
        match event.expect("valid live provider event") {
            TtsEvent::Audio(chunk) => audio_bytes += chunk.data.len(),
            TtsEvent::Completed => {
                completed = true;
                break;
            }
            TtsEvent::Alignment(_) | TtsEvent::Viseme(_) | TtsEvent::Usage(_) => {}
            TtsEvent::Interrupted { .. } => panic!("live provider stream was interrupted"),
        }
    }
    assert!(completed);
    assert!(audio_bytes > 0);
}

#[tokio::test]
async fn initializes_every_contract_and_runs_offline_turn() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();

    assert_eq!(state.profiles.profiles().len(), REQUIRED_PROFILE_COUNT);
    assert!(!state.catalog.content.providers.is_empty());
    assert!(state.config.database_path().is_file());

    let installation = InstallationCandidate {
        store: StoreKind::Steam,
        display_name: "Cyberpunk 2077".into(),
        install_dir: PathBuf::from(r"C:\Steam\steamapps\common\Cyberpunk 2077"),
        executable: None,
        edition: EditionEvidence {
            edition_id: None,
            store_id: Some("1091500".into()),
            build_id: Some("fixture-build".into()),
        },
        evidence: vec![InstallationEvidence {
            source: DetectionSource::StoreManifest,
            confidence: Confidence::High,
            detail: "fixture appmanifest".into(),
        }],
        warnings: BTreeSet::new(),
    };
    let matches = state.profiles.match_installations(&[installation]);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].profile_id, "cyberpunk-2077");

    let result = state
        .simulate_turn(SimulationRequest {
            session_id: "integration-session".into(),
            turn_id: "integration-turn".into(),
            game_id: "skyrim-special-edition".into(),
            character_id: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: SimulationSafetyContext::verified_safe(),
            application_namespace: None,
            transcript: "Fixture input for a deterministic offline turn.".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
            route_snapshot: None,
            input: Default::default(),
            delivery: Default::default(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        })
        .await
        .unwrap();

    assert!(result.fixture_only);
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert_eq!(
        result.outcome.selected_llm_provider.as_deref(),
        Some("fixture-llm")
    );
    assert!(result.outcome.effects.action_proposals.is_empty());
    assert!(result.capability_notices.iter().any(|notice| {
        notice.contains("cannot load modules, inject code, install hooks, or execute adapters")
    }));
    assert!(!result.outcome.delivered.is_empty());
}

#[tokio::test]
async fn doctor_never_pretends_current_machine_metrics_are_release_evidence() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();

    let report = state.doctor().await;
    assert_eq!(report.profile_count, REQUIRED_PROFILE_COUNT);
    assert_eq!(
        report.authored_game_profile_count,
        REQUIRED_AUTHORED_GAME_PROFILE_COUNT
    );
    assert_eq!(
        report.synthetic_review_profile_count,
        REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT
    );
    assert_eq!(
        report.synthetic_review_profile_ids,
        vec![SYNTHETIC_REVIEW_PROFILE_ID.to_owned()]
    );
    assert!(!report.performance_measurements_captured);
    assert!(!report.power_profile_changed);
    assert_eq!(
        report.catalog_trust,
        CatalogTrustState::DevelopmentUnsignedAllowed
    );
    let catalog_trust = report
        .diagnostics
        .facts
        .iter()
        .find(|fact| fact.check_id == "providers.catalog_trust")
        .expect("catalog trust diagnostic");
    assert_eq!(
        catalog_trust.status,
        interactive_npcs_diagnostics::DiagnosticStatus::Degraded
    );
    assert!(catalog_trust.summary.contains("debug/development build"));
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains(app_data.path().to_string_lossy().as_ref()));
}

#[tokio::test]
async fn generic_game_is_simulatable_but_not_counted_as_an_authored_profile() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();
    let contract = state.profiles.generic_contract();
    assert_eq!(state.profiles.profiles().len(), REQUIRED_PROFILE_COUNT);
    assert!(!contract.counted_as_authored_profile);
    assert!(!contract.executable_adapters_allowed);
    assert!(!contract.action_proposals_allowed);

    let result = state
        .simulate_turn(SimulationRequest {
            session_id: "generic-session".into(),
            turn_id: "generic-turn".into(),
            game_id: GENERIC_GAME_ID.into(),
            character_id: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: Some(GenericGameSelection {
                game_name: "Eclipse Harbor Test Window".into(),
                executable_name: "EclipseHarbor.exe".into(),
                character_name: "Mara Vale".into(),
                protected_online_detected: false,
                anti_cheat_detected: false,
            }),
            safety_context: SimulationSafetyContext::verified_synthetic_fixture(),
            application_namespace: None,
            transcript: "Can you hear me from offscreen?".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
            route_snapshot: None,
            input: Default::default(),
            delivery: Default::default(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        })
        .await
        .unwrap();
    assert_eq!(result.integration_mode, "generic_experimental");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert!(result.outcome.effects.action_proposals.is_empty());
    assert!(!result.outcome.delivered.is_empty());
    assert!(result
        .capability_notices
        .iter()
        .any(|notice| notice.contains("action proposals are disabled")));
}

#[tokio::test]
async fn eclipse_harbor_fixture_reply_matches_its_authored_turn() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();

    let result = state
        .simulate_turn(SimulationRequest {
            session_id: "eclipse-harbor-session".into(),
            turn_id: "eclipse-harbor-turn".into(),
            game_id: "eclipse-harbor".into(),
            character_id: Some("mara-venn".into()),
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: SimulationSafetyContext::verified_safe(),
            application_namespace: None,
            transcript: "Did you ever make it to the old lighthouse?".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
            route_snapshot: None,
            input: Default::default(),
            delivery: Default::default(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        })
        .await
        .unwrap();

    let delivered = result
        .outcome
        .delivered
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(result.integration_mode, "authored_profile");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert_eq!(
        delivered,
        "I made it as far as the eastern lock. It jammed again, but I remembered your service-tunnel route. If the tide stays low, I can reach the old lighthouse before dark."
    );
    assert!(result.outcome.effects.action_proposals.is_empty());
}

#[tokio::test]
async fn dev_live_tts_requires_the_qualified_build_and_refuses_trusted_risk() {
    let app_data = tempfile::tempdir().unwrap();
    let config = HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    };
    #[cfg(feature = "test-fixture-vault")]
    let state =
        HostState::initialize_with_test_vault(config, Arc::new(MemoryCredentialVault::default()))
            .await
            .unwrap();
    #[cfg(not(feature = "test-fixture-vault"))]
    let state = HostState::initialize(config).await.unwrap();

    let result = state
        .simulate_turn(SimulationRequest {
            session_id: "dev-live-tts-session".into(),
            turn_id: "dev-live-tts-turn".into(),
            game_id: "skyrim-special-edition".into(),
            character_id: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: SimulationSafetyContext::verified_safe(),
            application_namespace: None,
            transcript: "Use the explicitly authorized stock voice.".into(),
            locale: "en-US".into(),
            execution_mode: Some(SimulationExecutionMode::Hybrid),
            dev_live_tts: Some(DevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
            route_snapshot: None,
            input: Default::default(),
            delivery: Default::default(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            subtitle_presentation_context: None,
        })
        .await;

    #[cfg(not(feature = "dev-wasapi-audio"))]
    assert!(matches!(
        result,
        Err(npc_runtime_host::simulation::SimulationError::DevLiveTtsUnavailable)
    ));
    #[cfg(feature = "dev-wasapi-audio")]
    assert!(matches!(
        result,
        Err(npc_runtime_host::simulation::SimulationError::DevLiveTtsProviderNotSelected)
    ));

    for safety_context in [
        SimulationSafetyContext {
            evidence_state: SimulationSafetyEvidenceState::Blocked,
            profile_policy: SimulationProfilePolicy::SinglePlayerOnly,
            visuals_allowed: false,
            protected_online_detected: true,
            anti_cheat_detected: false,
        },
        SimulationSafetyContext {
            evidence_state: SimulationSafetyEvidenceState::Blocked,
            profile_policy: SimulationProfilePolicy::SinglePlayerOnly,
            visuals_allowed: false,
            protected_online_detected: false,
            anti_cheat_detected: true,
        },
    ] {
        let refusal = state
            .simulate_turn(SimulationRequest {
                session_id: "blocked-live-tts-session".into(),
                turn_id: "blocked-live-tts-turn".into(),
                game_id: "skyrim-special-edition".into(),
                character_id: None,
                native_identity_decision: None,
                enabled_spoiler_tiers: Vec::new(),
                generic_selection: None,
                safety_context,
                application_namespace: None,
                transcript: "This provider route must not begin.".into(),
                locale: "en-US".into(),
                execution_mode: Some(SimulationExecutionMode::Hybrid),
                dev_live_tts: Some(DevLiveTtsRequest {
                    provider_id: "elevenlabs".into(),
                    model_id: "eleven_flash_v2_5".into(),
                    voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                    explicit_user_authorization: true,
                }),
                route_snapshot: None,
                input: Default::default(),
                delivery: Default::default(),
                audio_playback_leases: Vec::new(),
                private_evaluation_acknowledgements: Vec::new(),
                subtitle_presentation_context: None,
            })
            .await;
        if safety_context.protected_online_detected {
            assert!(matches!(
                refusal,
                Err(npc_runtime_host::simulation::SimulationError::ProtectedOnlineBlocked)
            ));
        } else {
            assert!(matches!(
                refusal,
                Err(npc_runtime_host::simulation::SimulationError::AntiCheatBlocked)
            ));
        }
    }
}

#[tokio::test]
async fn every_game_is_hard_limited_to_the_generic_external_capture_boundary() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();

    for game_id in state
        .profiles
        .profiles()
        .iter()
        .map(|loaded| loaded.profile.id.as_str())
        .chain(std::iter::once(GENERIC_GAME_ID))
    {
        let policy = state
            .profiles
            .runtime_integration_policy(game_id)
            .expect("known games always have an integration policy");
        assert_eq!(policy.mode, "external_generic_capture");
        assert_eq!(
            policy.capture_routes,
            [
                "windows_graphics_capture",
                "desktop_duplication",
                "audio_subtitles"
            ]
        );
        assert!(policy.explicit_character_selection_supported);
        assert!(policy.screen_space_animation_experimental);
        assert!(!policy.native_rig_animation_allowed);
        assert!(!policy.executable_adapters_allowed);
        assert!(!policy.process_injection_allowed);
        assert!(!policy.game_hooks_allowed);
        assert!(!policy.game_module_loading_allowed);
        assert!(!policy.action_proposals_allowed);
        assert_eq!(policy.protected_online_policy, "blocked");
        assert_eq!(policy.anti_cheat_policy, "block_when_detected");
    }

    assert!(state
        .profiles
        .runtime_integration_policy("not-a-real-game")
        .is_none());
}

#[tokio::test]
async fn generic_game_refuses_protected_online_and_anti_cheat_states() {
    let app_data = tempfile::tempdir().unwrap();
    let state = HostState::initialize(HostConfig {
        repo_root: repo_root(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();
    for (protected_online_detected, anti_cheat_detected) in [(true, false), (false, true)] {
        let result = state
            .simulate_turn(SimulationRequest {
                session_id: "generic-refusal".into(),
                turn_id: "generic-refusal-turn".into(),
                game_id: GENERIC_GAME_ID.into(),
                character_id: None,
                native_identity_decision: None,
                enabled_spoiler_tiers: Vec::new(),
                generic_selection: Some(GenericGameSelection {
                    game_name: "Blocked Test".into(),
                    executable_name: "BlockedGame.exe".into(),
                    character_name: "Selected NPC".into(),
                    protected_online_detected,
                    anti_cheat_detected,
                }),
                safety_context: SimulationSafetyContext {
                    evidence_state: SimulationSafetyEvidenceState::Blocked,
                    profile_policy: SimulationProfilePolicy::Unknown,
                    visuals_allowed: false,
                    protected_online_detected,
                    anti_cheat_detected,
                },
                application_namespace: None,
                transcript: "This must be refused.".into(),
                locale: "en-US".into(),
                execution_mode: None,
                dev_live_tts: None,
                route_snapshot: None,
                input: Default::default(),
                delivery: Default::default(),
                audio_playback_leases: Vec::new(),
                private_evaluation_acknowledgements: Vec::new(),
                subtitle_presentation_context: None,
            })
            .await;
        if protected_online_detected {
            assert!(matches!(
                result,
                Err(npc_runtime_host::simulation::SimulationError::ProtectedOnlineBlocked)
            ));
        } else {
            assert!(matches!(
                result,
                Err(npc_runtime_host::simulation::SimulationError::AntiCheatBlocked)
            ));
        }
    }
}

#[tokio::test]
async fn authored_games_and_synthetic_review_profile_have_hash_locked_runtime_replays() {
    let app_data = tempfile::tempdir().unwrap();
    let root = repo_root();
    let state = HostState::initialize(HostConfig {
        repo_root: root.clone(),
        app_data: app_data.path().to_path_buf(),
    })
    .await
    .unwrap();
    let corpus = ProfileReplayCorpus::load(&root, &state).unwrap();
    let report = corpus.validate_runtime(&state).await.unwrap();
    assert!(report.valid);
    assert_eq!(
        report.authored_profile_count,
        REQUIRED_AUTHORED_GAME_PROFILE_COUNT
    );
    assert_eq!(
        report.synthetic_review_profile_count,
        REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT
    );
    assert_eq!(report.profile_count, REQUIRED_PROFILE_COUNT);
    assert_eq!(
        state
            .profiles
            .profiles()
            .iter()
            .filter(|loaded| loaded.profile.id == SYNTHETIC_REVIEW_PROFILE_ID)
            .count(),
        REQUIRED_SYNTHETIC_REVIEW_PROFILE_COUNT
    );
    assert_eq!(report.replay_count, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.integrity_entries_verified, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.detection_assertions_passed, REQUIRED_PROFILE_COUNT);
    assert_eq!(
        report.explicit_offscreen_selection_assertions_passed,
        REQUIRED_PROFILE_COUNT
    );
    assert_eq!(report.conversation_routes_passed, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.subtitle_routes_passed, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.memory_routes_passed, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.audio_routes_passed, REQUIRED_PROFILE_COUNT);
    assert_eq!(report.risk_refusals_passed, 6);
    assert_eq!(report.live_certified_claims, 0);
}
