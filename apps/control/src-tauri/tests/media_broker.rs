#![cfg(windows)]

use interactive_npcs_control_lib::media_broker::{MediaBrokerLaunchConfig, MediaBrokerSupervisor};
use interactive_npcs_control_lib::sidecar_supervisor::{RuntimeLaunchConfig, RuntimeSupervisor};
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repository root")
}

fn fixture_broker() -> PathBuf {
    std::env::var_os("NPC_MEDIA_BROKER_FIXTURE")
        .map(PathBuf::from)
        .expect(
            "NPC_MEDIA_BROKER_FIXTURE is required; run scripts/dev.ps1 test or set the fixed-name native broker path explicitly",
        )
}

#[tokio::test]
async fn fixed_broker_authenticates_reports_health_and_shuts_down() {
    let executable = fixture_broker();
    assert!(
        executable.is_file(),
        "build the media broker fixture first: {}",
        executable.display()
    );
    let app_data = tempfile::tempdir().expect("app data");
    let parent_job = RuntimeSupervisor::try_new(RuntimeLaunchConfig {
        application_namespace: "io.github.akshitireddy.interactive-npcs".into(),
        executable: PathBuf::from("C:/missing/npc-runtime.exe"),
        resource_root: repository_root(),
        app_data: app_data.path().to_path_buf(),
        development_fixture_allowed: true,
    })
    .expect("parent-death job");
    let supervisor = MediaBrokerSupervisor::new(
        MediaBrokerLaunchConfig {
            executable,
            development_fixture_allowed: false,
            audio_output_selection_path: app_data.path().join("audio-output-selection-v1.json"),
            debug_synthetic_metadata_path: app_data
                .path()
                .join("debug-synthetic-replay-target.json"),
        },
        parent_job,
    );

    let health = supervisor.refresh_health().await.expect("broker health");
    assert!(health.connected);
    assert!(!health.fixture_only);
    assert!(health.process_id.is_some());
    assert_eq!(health.protocol_version, Some(1));

    let diagnostics = supervisor.diagnostics().await.expect("broker diagnostics");
    let outputs = supervisor
        .enumerate_audio_outputs()
        .await
        .expect("real Windows audio outputs");
    assert!(outputs.catalog_generation > 0);
    assert!(!outputs.endpoints.is_empty());
    let selected = supervisor
        .select_audio_output(
            interactive_npcs_control_lib::media_broker::AudioOutputSelection::SystemDefault,
        )
        .await
        .expect("explicitly persist system-default output");
    assert!(selected.resolved.system_default);
    assert!(selected.resolved.generation > 0);
    assert_eq!(
        supervisor
            .selected_audio_output()
            .await
            .expect("selected output query")
            .expect("persisted selection")
            .resolved
            .endpoint_id,
        selected.resolved.endpoint_id
    );
    assert_ne!(diagnostics.state, "unknown");
    supervisor.shutdown().await;
    assert!(!supervisor.health().connected);
}
