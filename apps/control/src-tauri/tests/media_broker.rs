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
        .unwrap_or_else(|| {
            repository_root().join("artifacts/media-broker-build/Debug/npc-media-broker.exe")
        })
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
    assert_ne!(diagnostics.state, "unknown");
    supervisor.shutdown().await;
    assert!(!supervisor.health().connected);
}
