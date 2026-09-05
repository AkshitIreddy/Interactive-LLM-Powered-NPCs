#![cfg(windows)]

use interactive_npcs_control_lib::sidecar_protocol::{
    NativeDevLiveTtsRequest, NativeExecutionMode, NativeProfileSafetyPolicy,
    NativePushToTalkCaptureState, NativeRouteDegradation, NativeRouteExecution,
    NativeSafetyEvidenceState, NativeSelectedProviderRoute, NativeSelectedRoleRoute,
    NativeSelectedRouteRoles, NativeSelectedRouteSnapshot, NativeSelectedRouteState,
    NativeSimulationRequest, NativeSimulationSafetyContext, NativeTurnDeliveryRequest,
    NativeTurnInputMode, NativeTurnInputSnapshot,
};
use interactive_npcs_control_lib::sidecar_supervisor::{RuntimeLaunchConfig, RuntimeSupervisor};
use interactive_npcs_control_lib::{
    catalog::ResourceCatalog,
    character_workspace::{CharacterInspectionRequest, CharacterWorkspace},
    ProviderEntitlementModeV1, ProviderPrivateEvaluationAcknowledgementV1,
    PRIVATE_EVALUATION_TERMS_REVISION,
};
use npc_memory::{AuthorityScope, ContextQuery, MemoryStore, SpoilerPolicy};
use std::{collections::BTreeSet, path::PathBuf, sync::OnceLock};

fn runtime_sidecar_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root")
}

fn fixture_host() -> PathBuf {
    std::env::var_os("NPC_RUNTIME_FIXTURE_HOST")
        .map(PathBuf::from)
        .or_else(|| {
            // Cargo may redirect target-dir outside the checkout (the product
            // uses this for large artifacts on Windows). Integration test
            // executables live in <target>/<profile>/deps, so resolve the
            // sibling product binary from the executable that Cargo launched.
            std::env::current_exe().ok().and_then(|test_executable| {
                test_executable
                    .parent()
                    .and_then(std::path::Path::parent)
                    .map(|profile_dir| profile_dir.join("npc-runtime.exe"))
            })
        })
        .unwrap_or_else(|| repository_root().join("target/debug/npc-runtime.exe"))
}

fn verified_safety_context() -> NativeSimulationSafetyContext {
    NativeSimulationSafetyContext {
        evidence_state: NativeSafetyEvidenceState::VerifiedSafe,
        profile_policy: NativeProfileSafetyPolicy::SinglePlayerOnly,
        visuals_allowed: false,
        protected_online_detected: false,
        anti_cheat_detected: false,
    }
}

fn ready_route(
    provider_id: &str,
    model_id: &str,
    voice_id: Option<&str>,
) -> NativeSelectedRoleRoute {
    NativeSelectedRoleRoute {
        state: NativeSelectedRouteState::Ready,
        primary: Some(NativeSelectedProviderRoute {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            voice_id: voice_id.map(str::to_owned),
            execution: NativeRouteExecution::Cloud,
            egress: "providerCloud".into(),
            credential_reference: Some(format!("providers/{provider_id}")),
        }),
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn ready_local_route(
    provider_id: &str,
    model_id: &str,
    voice_id: Option<&str>,
) -> NativeSelectedRoleRoute {
    NativeSelectedRoleRoute {
        state: NativeSelectedRouteState::Ready,
        primary: Some(NativeSelectedProviderRoute {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            voice_id: voice_id.map(str::to_owned),
            execution: NativeRouteExecution::Local,
            egress: "none".into(),
            credential_reference: None,
        }),
        fallbacks: Vec::new(),
        degradation: None,
    }
}

fn disabled_route() -> NativeSelectedRoleRoute {
    NativeSelectedRoleRoute {
        state: NativeSelectedRouteState::Disabled,
        primary: None,
        fallbacks: Vec::new(),
        degradation: Some(NativeRouteDegradation {
            code: "not_configured".into(),
            detail: "This optional route is disabled in the selected loadout.".into(),
            retryable: false,
        }),
    }
}

fn route_snapshot(generation: u64) -> NativeSelectedRouteSnapshot {
    NativeSelectedRouteSnapshot {
        schema_version: 1,
        source_loadout_id: "integration-routes".into(),
        inheritance_chain: vec!["integration-routes".into()],
        generation,
        roles: NativeSelectedRouteRoles {
            llm: ready_local_route("mock-llm", "mock-stream-v1", None),
            stt: disabled_route(),
            tts: ready_local_route("mock-tts", "mock-pcm-v1", Some("mock-stock-voice-1")),
            embeddings: disabled_route(),
            vision: disabled_route(),
            lip_sync: disabled_route(),
        },
    }
}

fn live_route_snapshot(generation: u64) -> NativeSelectedRouteSnapshot {
    let mut snapshot = route_snapshot(generation);
    snapshot.roles.tts = ready_route(
        "elevenlabs",
        "eleven_flash_v2_5",
        Some("EXAVITQu4vr4xnSDxMaL"),
    );
    snapshot
}

fn magpie_private_evaluation_route_snapshot(generation: u64) -> NativeSelectedRouteSnapshot {
    let mut snapshot = route_snapshot(generation);
    snapshot.roles.tts = ready_route(
        "nvidia-nim-magpie",
        "magpie-tts-multilingual",
        Some("Magpie-Multilingual.EN-US.Aria"),
    );
    snapshot
        .roles
        .tts
        .primary
        .as_mut()
        .expect("Magpie primary")
        .credential_reference = Some("providers/nvidia-nim".into());
    snapshot
        .roles
        .tts
        .primary
        .as_mut()
        .expect("Magpie primary")
        .egress = "conversation_audio".into();
    snapshot
}

fn nvidia_provider_wide_private_evaluation_route_snapshot(
    generation: u64,
) -> NativeSelectedRouteSnapshot {
    let mut snapshot = magpie_private_evaluation_route_snapshot(generation);
    snapshot.roles.llm = ready_route("nvidia-nim", "nvidia/nemotron-3-nano-30b-a3b", None);
    snapshot.roles.embeddings = ready_route("nvidia-nim", "nvidia/nemotron-3-embed-1b", None);
    snapshot
        .roles
        .llm
        .primary
        .as_mut()
        .expect("NVIDIA LLM primary")
        .egress = "conversation_text_and_derived_game_context".into();
    snapshot
        .roles
        .embeddings
        .primary
        .as_mut()
        .expect("NVIDIA embedding primary")
        .egress = "selected_memory_and_lore_text".into();
    snapshot
}

fn private_evaluation_acknowledgement(
    application_namespace: &str,
) -> ProviderPrivateEvaluationAcknowledgementV1 {
    let catalog = npc_provider_catalog::CatalogDocument::load(
        repository_root().join("catalog/v1/catalog.json"),
    )
    .expect("load current provider catalog");
    ProviderPrivateEvaluationAcknowledgementV1 {
        schema_version: 1,
        provider_id: "nvidia-nim".into(),
        mode: ProviderEntitlementModeV1::PrivateEvaluationOnly,
        terms_revision: PRIVATE_EVALUATION_TERMS_REVISION.into(),
        catalog_revision: catalog.catalog_revision,
        application_namespace: application_namespace.into(),
        acknowledged_at_epoch_ms: 1,
        promotion_supported: false,
        publication_supported: false,
    }
}

fn typed_input() -> NativeTurnInputSnapshot {
    NativeTurnInputSnapshot {
        mode: NativeTurnInputMode::Typed,
        push_to_talk_state: NativePushToTalkCaptureState::NotRequested,
        selected_stt_receipt: None,
    }
}

fn delivery() -> NativeTurnDeliveryRequest {
    NativeTurnDeliveryRequest {
        audio: true,
        subtitles: true,
    }
}

#[tokio::test]
async fn fixed_fixture_host_supports_authenticated_control_lifecycle() {
    let _process_guard = runtime_sidecar_test_lock().lock().await;
    let executable = fixture_host();
    assert!(
        executable.is_file(),
        "build npc-runtime-host before this integration test: {}",
        executable.display()
    );
    let config_root = tempfile::tempdir().expect("temporary config root");
    let app_data = config_root.path().join("runtime-host-data");
    let supervisor = RuntimeSupervisor::try_new(RuntimeLaunchConfig {
        executable,
        resource_root: repository_root(),
        app_data: app_data.clone(),
        development_fixture_allowed: false,
    })
    .expect("parent-death supervision");

    let health = supervisor.ping().await.expect("authenticated ping");
    assert!(health.connected);
    assert!(!health.fixture_only);

    let doctor = supervisor.doctor().await.expect("doctor response");
    assert_eq!(doctor.profile_count, 21);
    assert!(!doctor.performance_measurements_captured);
    assert!(!doctor.power_profile_changed);

    let profiles = supervisor.profiles().await.expect("profile response");
    assert_eq!(profiles.len(), doctor.profile_count);
    let synthetic = profiles
        .iter()
        .filter(|profile| profile.id == "eclipse-harbor")
        .collect::<Vec<_>>();
    assert_eq!(synthetic.len(), 1);
    assert_eq!(
        synthetic[0].display_name,
        "Eclipse Harbor (Synthetic Review Game)"
    );
    let commercial_ids = profiles
        .iter()
        .filter(|profile| profile.id != "eclipse-harbor")
        .map(|profile| profile.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        commercial_ids,
        BTreeSet::from([
            "baldurs-gate-3",
            "cyberpunk-2077",
            "divinity-original-sin-2",
            "dragon-age-inquisition",
            "elden-ring-offline",
            "fallout-4",
            "fallout-new-vegas",
            "gta-v-story",
            "kenshi",
            "kingdom-come-deliverance-2",
            "mass-effect-legendary-edition",
            "minecraft-java",
            "mount-and-blade-2-bannerlord",
            "oblivion-remastered",
            "red-dead-redemption-2-story",
            "skyrim-special-edition",
            "stardew-valley",
            "starfield",
            "the-sims-4",
            "the-witcher-3",
        ])
    );

    let simulation = supervisor
        .simulate(NativeSimulationRequest {
            session_id: "response-console-simulation".into(),
            turn_id: "control-turn-1".into(),
            game_id: "skyrim-special-edition".into(),
            effective_game_profile: None,
            character_id: Some("lydia".into()),
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: verified_safety_context(),
            transcript: "Bounded fixture input for the authenticated sidecar test.".into(),
            locale: "en-US".into(),
            route_snapshot: route_snapshot(1),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
            subtitle_presentation_context: None,
            execution_mode: None,
            dev_live_tts: None,
        })
        .await
        .expect("simulation response");
    assert!(simulation.fixture_only);
    assert_eq!(simulation.integration_mode, "selected_route_turn");
    assert!(!simulation.events.is_empty());
    assert!(simulation
        .events
        .iter()
        .any(|event| { event["type"] == "lifecycle" && event["stage"] == "completed" }));
    let turn_execution = simulation
        .turn_execution
        .as_deref()
        .expect("selected-route turn execution evidence");
    assert_eq!(turn_execution.consumed_route.generation, 1);
    assert_eq!(
        turn_execution.consumed_route.source_loadout_id,
        "integration-routes"
    );
    let memory = MemoryStore::open(app_data.join("runtime").join("memory.sqlite3"))
        .await
        .expect("runtime host canonical memory database");
    let remembered = memory
        .retrieve_context(ContextQuery {
            scope: AuthorityScope {
                user_id: "local-user".into(),
                profile_id: "skyrim-special-edition".into(),
                game_id: "skyrim-special-edition".into(),
                character_id: Some("lydia".into()),
                encounter_id: None,
                session_id: Some("response-console-simulation".into()),
                save_id: None,
            },
            text: None,
            spoiler_policy: SpoilerPolicy::default(),
            recent_turn_limit: 20,
            per_class_limit: 0,
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(i64::MAX as u128) as i64,
        })
        .await
        .expect("retrieve runtime-delivered turn");
    assert!(remembered
        .recent_dialogue
        .iter()
        .any(|turn| turn.provenance.source_id.as_deref() == Some("control-turn-1")));
    let workspace = CharacterWorkspace::new(config_root.path()).expect("character workspace");
    let inspection = workspace
        .inspect(
            &ResourceCatalog::new(Some(repository_root())),
            CharacterInspectionRequest {
                game_profile_id: "skyrim-special-edition".into(),
                character_id: Some("lydia".into()),
                recent_turn_limit: 20,
            },
        )
        .await
        .expect("inspect the sidecar-delivered turn through the product workspace");
    assert!(inspection
        .delivered_memory
        .iter()
        .any(|turn| turn.provenance_source_id.as_deref() == Some("control-turn-1")));

    let generation = supervisor
        .cancel()
        .await
        .expect("idempotent cancellation request");
    assert!(generation > 0);

    let live_route = supervisor
        .simulate(NativeSimulationRequest {
            session_id: "control-integration".into(),
            turn_id: "control-turn-2".into(),
            game_id: "skyrim-special-edition".into(),
            effective_game_profile: None,
            character_id: Some("lydia".into()),
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: verified_safety_context(),
            transcript: "Use the explicitly authorized stock voice.".into(),
            locale: "en-US".into(),
            route_snapshot: live_route_snapshot(2),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
            subtitle_presentation_context: None,
            execution_mode: None,
            dev_live_tts: None,
        })
        .await
        .expect("ordinary hosted route without broker leases must return typed manual retry");
    let evidence = live_route
        .turn_execution
        .as_deref()
        .expect("live route execution evidence");
    assert!(live_route.fixture_only);
    let evidence_json = serde_json::to_value(evidence).expect("serialize turn evidence");
    assert_eq!(evidence_json["deliveryState"], "manualRetryRequired");
    assert_eq!(evidence_json["commitState"], "notCommitted");
    let degradations = evidence_json["degradations"]
        .as_array()
        .expect("typed degradation list");
    assert_eq!(degradations.len(), 1);
    assert_eq!(degradations[0]["type"], "manualRetryRequired");
    assert_eq!(degradations[0]["failedRole"], "tts");
    assert_eq!(degradations[0]["providerId"], "elevenlabs");
    assert_eq!(degradations[0]["retryable"], true);
    assert!(!evidence.success.tts_provider_live);
    assert!(!evidence.success.audio_submitted);
    assert!(!evidence.success.audio_drained);
    assert_eq!(evidence.success.audio_receipt_count, 0);
    assert!(evidence.audio_receipts.is_empty());

    for (turn_id, safety_context) in [
        (
            "control-turn-unknown-safety",
            NativeSimulationSafetyContext {
                evidence_state: NativeSafetyEvidenceState::Unknown,
                profile_policy: NativeProfileSafetyPolicy::SinglePlayerOnly,
                visuals_allowed: false,
                protected_online_detected: false,
                anti_cheat_detected: false,
            },
        ),
        (
            "control-turn-blocked-safety",
            NativeSimulationSafetyContext {
                evidence_state: NativeSafetyEvidenceState::Blocked,
                profile_policy: NativeProfileSafetyPolicy::OfflineOnly,
                visuals_allowed: false,
                protected_online_detected: false,
                anti_cheat_detected: true,
            },
        ),
    ] {
        let refused = supervisor
            .simulate(NativeSimulationRequest {
                session_id: "control-integration".into(),
                turn_id: turn_id.into(),
                game_id: "skyrim-special-edition".into(),
                effective_game_profile: None,
                character_id: Some("lydia".into()),
                native_identity_decision: None,
                enabled_spoiler_tiers: Vec::new(),
                generic_selection: None,
                safety_context,
                transcript: "This turn must fail closed before provider execution.".into(),
                locale: "en-US".into(),
                route_snapshot: route_snapshot(3),
                input: typed_input(),
                delivery: delivery(),
                audio_playback_leases: Vec::new(),
                private_evaluation_acknowledgements: Vec::new(),
                application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
                subtitle_presentation_context: None,
                execution_mode: None,
                dev_live_tts: None,
            })
            .await;
        assert!(refused.is_err(), "{turn_id} must fail closed");
    }

    supervisor.shutdown().await;
    assert!(!supervisor.health().connected);
}

#[tokio::test]
async fn private_evaluation_namespace_is_bound_across_the_authenticated_sidecar() {
    let _process_guard = runtime_sidecar_test_lock().lock().await;
    let executable = fixture_host();
    assert!(executable.is_file(), "build the runtime sidecar first");
    let app_data = tempfile::tempdir().expect("temporary app data");
    let supervisor = RuntimeSupervisor::try_new(RuntimeLaunchConfig {
        executable,
        resource_root: repository_root(),
        app_data: app_data.path().to_path_buf(),
        development_fixture_allowed: true,
    })
    .expect("parent-death supervision");

    for (ordinal, application_namespace) in [
        (1_u64, "io.github.akshitireddy.interactive-npcs.review"),
        (2_u64, "io.github.akshitireddy.interactive-npcs.debug"),
    ] {
        let request = NativeSimulationRequest {
            session_id: "private-evaluation-sidecar".into(),
            turn_id: format!("private-evaluation-{ordinal}"),
            game_id: "skyrim-special-edition".into(),
            effective_game_profile: None,
            character_id: Some("lydia".into()),
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: verified_safety_context(),
            transcript: "Do not contact the provider without a playback lease.".into(),
            locale: "en-US".into(),
            route_snapshot: nvidia_provider_wide_private_evaluation_route_snapshot(ordinal),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: vec![private_evaluation_acknowledgement(
                application_namespace,
            )],
            application_namespace: application_namespace.into(),
            subtitle_presentation_context: None,
            execution_mode: None,
            dev_live_tts: None,
        };
        let request_wire = serde_json::to_value(&request).expect("serialize native request");
        assert_eq!(request_wire["applicationNamespace"], application_namespace);
        assert_eq!(
            request_wire["privateEvaluationAcknowledgements"][0]["providerId"],
            "nvidia-nim"
        );
        assert_eq!(
            request_wire["privateEvaluationAcknowledgements"][0]["applicationNamespace"],
            application_namespace
        );
        assert_eq!(
            request_wire["routeSnapshot"]["roles"]["tts"]["primary"]["credentialReference"],
            "providers/nvidia-nim"
        );
        let result = supervisor
            .simulate(request)
            .await
            .expect("matching isolated namespace reaches typed no-lease retry");
        let evidence = result
            .turn_execution
            .as_deref()
            .expect("private-evaluation evidence");
        let wire = serde_json::to_value(evidence).expect("serialize evidence");
        assert_eq!(wire["deliveryState"], "manualRetryRequired");
        assert_eq!(
            wire["consumedRoute"]["privateEvaluationAcknowledgement"]["applicationNamespace"],
            application_namespace
        );
        assert_eq!(
            wire["consumedRoute"]["privateEvaluationAcknowledgement"]["providerId"],
            "nvidia-nim"
        );
        assert_eq!(
            wire["consumedRoute"]["privateEvaluationAcknowledgement"]["modalities"],
            serde_json::json!(["llm", "embeddings", "tts"])
        );
        assert!(!evidence.success.audio_submitted);
        assert!(evidence.audio_receipts.is_empty());
    }

    for (ordinal, acknowledgement_namespace, request_namespace) in [
        (
            3_u64,
            "io.github.akshitireddy.interactive-npcs.debug",
            "io.github.akshitireddy.interactive-npcs.review",
        ),
        (
            4_u64,
            "io.github.akshitireddy.interactive-npcs.review",
            "io.github.akshitireddy.interactive-npcs.debug",
        ),
    ] {
        let replay = supervisor
            .simulate(NativeSimulationRequest {
                session_id: "private-evaluation-sidecar".into(),
                turn_id: format!("private-evaluation-cross-namespace-replay-{ordinal}"),
                game_id: "skyrim-special-edition".into(),
                effective_game_profile: None,
                character_id: Some("lydia".into()),
                native_identity_decision: None,
                enabled_spoiler_tiers: Vec::new(),
                generic_selection: None,
                safety_context: verified_safety_context(),
                transcript: "Reject this replay before provider execution.".into(),
                locale: "en-US".into(),
                route_snapshot: nvidia_provider_wide_private_evaluation_route_snapshot(ordinal),
                input: typed_input(),
                delivery: delivery(),
                audio_playback_leases: Vec::new(),
                private_evaluation_acknowledgements: vec![private_evaluation_acknowledgement(
                    acknowledgement_namespace,
                )],
                application_namespace: request_namespace.into(),
                subtitle_presentation_context: None,
                execution_mode: None,
                dev_live_tts: None,
            })
            .await;
        assert!(replay.is_err(), "cross-namespace replay must fail closed");
    }

    let production = supervisor
        .simulate(NativeSimulationRequest {
            session_id: "private-evaluation-sidecar".into(),
            turn_id: "private-evaluation-production-denied".into(),
            game_id: "skyrim-special-edition".into(),
            effective_game_profile: None,
            character_id: Some("lydia".into()),
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: None,
            safety_context: verified_safety_context(),
            transcript: "Production must not use Developer API trial authority.".into(),
            locale: "en-US".into(),
            route_snapshot: nvidia_provider_wide_private_evaluation_route_snapshot(5),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
            subtitle_presentation_context: None,
            execution_mode: None,
            dev_live_tts: None,
        })
        .await;
    assert!(production.is_err(), "production namespace must fail closed");

    supervisor.shutdown().await;
}

#[tokio::test]
#[ignore = "explicit live-provider and Windows audio qualification; requires a saved ElevenLabs credential and a Debug sidecar built with dev-wasapi-audio"]
async fn qualified_debug_host_streams_stock_voice_to_wasapi() {
    let _process_guard = runtime_sidecar_test_lock().lock().await;
    let executable = fixture_host();
    assert!(
        executable.is_file(),
        "build the qualified Debug runtime sidecar before this test: {}",
        executable.display()
    );
    let app_data = tempfile::tempdir().expect("temporary app data");
    let supervisor = RuntimeSupervisor::try_new(RuntimeLaunchConfig {
        executable,
        resource_root: repository_root(),
        app_data: app_data.path().to_path_buf(),
        development_fixture_allowed: false,
    })
    .expect("parent-death supervision");

    let result = supervisor
        .simulate(NativeSimulationRequest {
            session_id: "live-audio-qualification".into(),
            turn_id: "live-audio-qualification-turn-1".into(),
            game_id: "generic-game".into(),
            effective_game_profile: None,
            character_id: None,
            native_identity_decision: None,
            enabled_spoiler_tiers: Vec::new(),
            generic_selection: Some(
                interactive_npcs_control_lib::sidecar_protocol::NativeGenericGameSelection {
                    game_name: "Synthetic Eclipse Harbor".into(),
                    executable_name: "interactive-npcs-synthetic-target.exe".into(),
                    character_name: "Mara Venn".into(),
                    protected_online_detected: false,
                    anti_cheat_detected: false,
                },
            ),
            safety_context: verified_safety_context(),
            transcript: "Give me the shortest safe route to the eastern lock.".into(),
            locale: "en-US".into(),
            route_snapshot: route_snapshot(1),
            input: typed_input(),
            delivery: delivery(),
            audio_playback_leases: Vec::new(),
            private_evaluation_acknowledgements: Vec::new(),
            application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
            subtitle_presentation_context: None,
            execution_mode: Some(NativeExecutionMode::Cloud),
            dev_live_tts: Some(NativeDevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
        })
        .await
        .expect("qualified hosted TTS and WASAPI submission");

    assert!(!result.fixture_only);
    assert_eq!(
        result.integration_mode,
        "debug_hosted_tts_wasapi_submission"
    );
    assert!(result.capability_notices.iter().any(|notice| {
        notice.contains("developer WASAPI sink accepted") && notice.contains("device frames")
    }));
    assert!(result
        .capability_notices
        .iter()
        .any(|notice| notice.contains("lip_sync_unavailable")));

    supervisor.shutdown().await;
}
