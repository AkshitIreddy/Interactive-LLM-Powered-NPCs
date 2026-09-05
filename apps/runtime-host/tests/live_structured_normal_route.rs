//! One explicit, bounded hosted LLM turn through the normal runtime simulation
//! path. Audio is disabled and no native presenter is supplied, so the receipt
//! can prove schema, provider, supervisor, and subtitle fallback only.

#![cfg(all(windows, feature = "test-fixture-vault"))]

use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use interactive_npcs_credential_vault::{CredentialVault, MemoryCredentialVault, SecretValue};
use npc_runtime_core::{DeliveryMode, TurnLifecycle};
use npc_runtime_host::{
    simulation::SimulationSafetyContext, HostConfig, HostState, RouteExecution,
    SelectedProviderRoute, SelectedRoleRoute, SelectedRouteRoles, SelectedRouteSnapshot,
    SelectedRouteState, SimulationRequest, TurnDeliveryRequest, TurnInputSnapshot,
    TURN_ROUTE_SCHEMA_VERSION,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KEY_ENV: &str = "MISTRAL_API_KEY";
const REPORT_ENV: &str = "STRUCTURED_NORMAL_ROUTE_REPORT";
const MODEL_ID: &str = "ministral-8b-2512";

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
