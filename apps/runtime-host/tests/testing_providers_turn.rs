//! Hermetic zero-spend testing-provider turn qualification.
//!
//! No network calls, no credentials, and no ignored gate: the testing LLM
//! echoes the transcript and the testing voice synthesizes labeled sine
//! audio measured by the local receipt ledger. Nothing is played to a device
//! here because no broker playback leases are supplied.

#![cfg(all(windows, feature = "test-fixture-vault"))]

use std::{path::PathBuf, sync::Arc};

use interactive_npcs_credential_vault::MemoryCredentialVault;
use npc_runtime_core::TurnLifecycle;
use npc_runtime_host::simulation::SimulationSafetyContext;
use npc_runtime_host::{
    HostConfig, HostState, RouteExecution, SelectedProviderRoute, SelectedRoleRoute,
    SelectedRouteRoles, SelectedRouteSnapshot, SelectedRouteState, SimulationRequest,
    TurnDeliveryRequest, TurnInputSnapshot, TURN_ROUTE_SCHEMA_VERSION,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
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

fn testing_snapshot() -> SelectedRouteSnapshot {
    let llm = SelectedProviderRoute {
        provider_id: "testing".into(),
        model_id: "testing-echo-v1".into(),
        voice_id: None,
        execution: RouteExecution::Local,
        egress: "offline".into(),
        credential_reference: None,
    };
    let tts = SelectedProviderRoute {
        provider_id: "testing".into(),
        model_id: "testing-synth-v1".into(),
        voice_id: Some("testing-voice-1".into()),
        execution: RouteExecution::Local,
        egress: "offline".into(),
        credential_reference: None,
    };
    let ready = |primary: SelectedProviderRoute| SelectedRoleRoute {
        state: SelectedRouteState::Ready,
        primary: Some(primary),
        fallbacks: Vec::new(),
        degradation: None,
    };
    SelectedRouteSnapshot {
        schema_version: TURN_ROUTE_SCHEMA_VERSION,
        source_loadout_id: "testing-dummy".into(),
        inheritance_chain: Vec::new(),
        generation: 1,
        roles: SelectedRouteRoles {
            llm: ready(llm),
            stt: disabled(),
            tts: ready(tts),
            embeddings: disabled(),
            vision: disabled(),
            lip_sync: disabled(),
        },
    }
}

#[tokio::test]
async fn testing_turn_echoes_transcript_and_measures_synth_audio() {
    let app_data = tempfile::tempdir()
        .expect("temporary app data")
        .keep();
    let vault = Arc::new(MemoryCredentialVault::default());
    let host = HostState::initialize_with_test_vault(
        HostConfig {
            repo_root: repo_root(),
            app_data,
        },
        vault,
    )
    .await
    .expect("hermetic host state");
    let request = SimulationRequest {
        session_id: "testing-turn-session".into(),
        turn_id: "testing-turn-1".into(),
        game_id: "eclipse-harbor".into(),
        character_id: Some("mara-venn".into()),
        effective_game_profile: None,
        native_identity_decision: None,
        enabled_spoiler_tiers: Vec::new(),
        generic_selection: None,
        safety_context: SimulationSafetyContext::verified_safe(),
        application_namespace: None,
        transcript: "harbor lantern check".into(),
        locale: "en".into(),
        execution_mode: None,
        dev_live_tts: None,
        route_snapshot: Some(testing_snapshot()),
        input: TurnInputSnapshot::default(),
        delivery: TurnDeliveryRequest {
            audio: true,
            subtitles: false,
        },
        audio_playback_leases: Vec::new(),
        private_evaluation_acknowledgements: Vec::new(),
        subtitle_presentation_context: None,
    };
    let result = host
        .simulate_turn(request)
        .await
        .expect("testing turn runs without network or credentials");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert!(
        !result.fixture_only,
        "testing routes execute; they are not fixtures"
    );
    let delivered: Vec<&str> = result
        .outcome
        .delivered
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    assert!(
        delivered
            .iter()
            .any(|text| text.starts_with("Testing echo:") && text.contains("harbor lantern check")),
        "echo reply carries the transcript: {delivered:?}"
    );
    let evidence = result
        .turn_execution
        .as_ref()
        .expect("selected-route turn carries execution evidence");
    assert!(!evidence.success.llm_provider_live);
    assert!(!evidence.success.tts_provider_live);
    assert!(
        evidence.success.audio_submitted,
        "synth audio measured nonzero: {evidence:?}"
    );
    assert!(
        result
            .capability_notices
            .iter()
            .any(|notice| notice.contains("Testing routes produced local dummy output")),
        "testing provenance is explicit: {:?}",
        result.capability_notices
    );
    assert!(
        result
            .outcome
            .selected_tts_providers
            .iter()
            .any(|provider| provider == "testing"),
        "tts provenance names testing"
    );
}
