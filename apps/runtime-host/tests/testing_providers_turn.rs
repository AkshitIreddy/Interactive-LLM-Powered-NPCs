//! Hermetic zero-spend testing-provider turn qualification.
//!
//! No network calls, no credentials, and no ignored gate: the testing LLM
//! echoes the transcript and the testing voice synthesizes labeled sine
//! audio measured in memory. Nothing is submitted to a device here because
//! no broker playback leases are supplied, and the returned evidence must
//! preserve that distinction.

#![cfg(all(windows, feature = "test-fixture-vault"))]

use std::{path::PathBuf, sync::Arc};

use interactive_npcs_credential_vault::MemoryCredentialVault;
use npc_runtime_core::{DeliveryMode, TurnEvent, TurnLifecycle};
use npc_runtime_host::audio_output::broker::BrokerAudioPlaybackLease;
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

fn testing_request() -> SimulationRequest {
    SimulationRequest {
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
    }
}

fn testing_lease() -> BrokerAudioPlaybackLease {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "streamId": "pcm-000",
        "producerEndpoint": "\\\\.\\pipe\\npc-media-playback-test-000",
        "oneTimeToken": format!("{:064x}", 1),
        "sessionId": "testing-turn-session",
        "turnId": "testing-turn-1",
        "generation": 1,
        "sampleRate": 24_000,
        "channels": 1,
        "maxFrames": 1_440_000,
        "maxChunkBytes": 65_536,
        "expiresQpc": 10_000_000,
        "outputSelectionMode": "systemDefault",
        "outputEndpointId": "{0.0.0.00000000}.fixture-output",
        "outputEndpointGeneration": 17
    }))
    .expect("well-formed testing lease")
}

#[tokio::test]
async fn testing_turn_with_leases_passes_request_validation() {
    // Regression cover for the live failure where leased testing turns were
    // rejected by the runtime format gate before any provider ran.
    let mut request = testing_request();
    request.audio_playback_leases = vec![testing_lease()];
    request
        .validate()
        .expect("leased testing turn validates without network or credentials");
}

#[tokio::test]
async fn testing_turn_echoes_transcript_without_claiming_audio_submission() {
    let app_data = tempfile::tempdir().expect("temporary app data").keep();
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
    let request = testing_request();
    let result = host
        .simulate_turn(request)
        .await
        .expect("testing turn runs without network or credentials");
    assert_eq!(result.outcome.lifecycle, TurnLifecycle::Completed);
    assert!(
        result.fixture_only,
        "testing routes execute real code but remain explicit non-live dummy provenance"
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
        !evidence.success.audio_submitted,
        "in-memory synth measurement is not endpoint submission: {evidence:?}"
    );
    assert!(
        !evidence.success.audio_drained,
        "no endpoint existed to drain: {evidence:?}"
    );
    assert_eq!(evidence.success.audio_receipt_count, 0);
    assert!(
        evidence.audio_receipts.is_empty(),
        "in-memory measurement must not cross as delivery evidence: {evidence:?}"
    );
    assert!(
        result
            .outcome
            .delivered
            .iter()
            .all(|sentence| sentence.delivery == DeliveryMode::Subtitle),
        "without a broker lease, generated text must use subtitle fallback"
    );
    assert!(evidence.success.subtitle_delivered);
    assert_eq!(
        evidence.success.subtitle_receipt_count,
        result.outcome.delivered.len()
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
        result.events.iter().any(|event| matches!(
            event,
            TurnEvent::SpeechStarted { provider_id, .. } if provider_id == "testing"
        )),
        "testing synth ran before the no-endpoint subtitle fallback"
    );
    assert!(
        result.outcome.selected_tts_providers.is_empty(),
        "a provider without completed playback must not be reported as delivered"
    );
}
