#![cfg(windows)]

use interactive_npcs_control_lib::sidecar_protocol::{
    NativeDevLiveTtsRequest, NativeExecutionMode, NativeSimulationRequest,
    NativeSimulationSafetyContext,
};
use interactive_npcs_control_lib::sidecar_supervisor::{RuntimeLaunchConfig, RuntimeSupervisor};
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root")
}

fn fixture_host() -> PathBuf {
    std::env::var_os("NPC_RUNTIME_FIXTURE_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository_root().join("target/debug/npc-runtime.exe"))
}

#[tokio::test]
async fn fixed_fixture_host_supports_authenticated_control_lifecycle() {
    let executable = fixture_host();
    assert!(
        executable.is_file(),
        "build npc-runtime-host before this integration test: {}",
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

    let health = supervisor.ping().await.expect("authenticated ping");
    assert!(health.connected);
    assert!(!health.fixture_only);

    let doctor = supervisor.doctor().await.expect("doctor response");
    assert_eq!(doctor.profile_count, 20);
    assert!(!doctor.performance_measurements_captured);
    assert!(!doctor.power_profile_changed);

    let profiles = supervisor.profiles().await.expect("profile response");
    assert_eq!(profiles.len(), 20);

    let simulation = supervisor
        .simulate(NativeSimulationRequest {
            session_id: "control-integration".into(),
            turn_id: "control-turn-1".into(),
            game_id: "skyrim-special-edition".into(),
            character_id: Some("lydia".into()),
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Bounded fixture input for the authenticated sidecar test.".into(),
            locale: "en-US".into(),
            execution_mode: None,
            dev_live_tts: None,
        })
        .await
        .expect("simulation response");
    assert!(simulation.fixture_only);
    assert_eq!(simulation.integration_mode, "authored_profile");
    assert!(!simulation.events.is_empty());

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
            character_id: Some("lydia".into()),
            generic_selection: None,
            safety_context: NativeSimulationSafetyContext::default(),
            transcript: "Use the explicitly authorized stock voice.".into(),
            locale: "en-US".into(),
            execution_mode: Some(NativeExecutionMode::Hybrid),
            dev_live_tts: Some(NativeDevLiveTtsRequest {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: "EXAVITQu4vr4xnSDxMaL".into(),
                explicit_user_authorization: true,
            }),
        })
        .await
        .expect("authorized dev live TTS route response");
    assert!(live_route.fixture_only);
    assert_eq!(
        live_route.integration_mode,
        "hosted_tts_request_shaping_only"
    );
    assert!(live_route
        .capability_notices
        .iter()
        .any(|notice| notice.contains("never credential values")));

    supervisor.shutdown().await;
    assert!(!supervisor.health().connected);
}
