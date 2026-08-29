#![allow(clippy::unwrap_used)]

use std::{collections::BTreeSet, path::PathBuf};

use interactive_npcs_game_discovery::{
    Confidence, DetectionSource, EditionEvidence, InstallationCandidate, InstallationEvidence,
    StoreKind,
};
use npc_runtime_core::TurnLifecycle;
use npc_runtime_host::profile_replays::ProfileReplayCorpus;
use npc_runtime_host::profiles::{GenericGameSelection, GENERIC_GAME_ID};
use npc_runtime_host::{
    simulation::SimulationSafetyContext, CatalogTrustState, HostConfig, HostState,
    SimulationRequest, REQUIRED_PROFILE_COUNT,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
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
            generic_selection: None,
            safety_context: SimulationSafetyContext::default(),
            transcript: "Fixture input for a deterministic offline turn.".into(),
            locale: "en-US".into(),
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
            generic_selection: Some(GenericGameSelection {
                game_name: "Eclipse Harbor Test Window".into(),
                executable_name: "EclipseHarbor.exe".into(),
                character_name: "Mara Vale".into(),
                protected_online_detected: false,
                anti_cheat_detected: false,
            }),
            safety_context: SimulationSafetyContext::default(),
            transcript: "Can you hear me from offscreen?".into(),
            locale: "en-US".into(),
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
            game_id: GENERIC_GAME_ID.into(),
            character_id: None,
            generic_selection: Some(GenericGameSelection {
                game_name: "Eclipse Harbor".into(),
                executable_name: "interactive-npcs-synthetic-target.exe".into(),
                character_name: "Mara Venn".into(),
                protected_online_detected: false,
                anti_cheat_detected: false,
            }),
            safety_context: SimulationSafetyContext::default(),
            transcript: "Did you ever make it to the old lighthouse?".into(),
            locale: "en-US".into(),
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
    assert_eq!(result.integration_mode, "generic_experimental");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert_eq!(
        delivered,
        "I made it as far as the eastern lock. It jammed again, but I remembered your service-tunnel route. If the tide stays low, I can reach the old lighthouse before dark."
    );
    assert!(result.outcome.effects.action_proposals.is_empty());
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
                generic_selection: Some(GenericGameSelection {
                    game_name: "Blocked Test".into(),
                    executable_name: "BlockedGame.exe".into(),
                    character_name: "Selected NPC".into(),
                    protected_online_detected,
                    anti_cheat_detected,
                }),
                safety_context: SimulationSafetyContext::default(),
                transcript: "This must be refused.".into(),
                locale: "en-US".into(),
            })
            .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn every_authored_profile_has_hash_locked_runtime_replay_evidence() {
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
    assert_eq!(report.authored_profile_count, REQUIRED_PROFILE_COUNT);
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
