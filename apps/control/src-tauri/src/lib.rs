mod catalog;
mod commands;
mod credential_prompt;
mod domain;
mod lifecycle;
pub mod media_broker;
mod persistence;
mod private_directory;
mod provider_loadouts;
mod runtime_bridge;
mod runtime_router;
pub mod sidecar_protocol;
pub mod sidecar_supervisor;

use commands::AppState;
use lifecycle::LifecycleState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(LifecycleState::default())
        .setup(|app| {
            let config_directory = app.path().app_config_dir().map_err(|error| {
                format!("application configuration path is unavailable: {error}")
            })?;
            let resource_directory = app.path().resource_dir().ok();
            let state = AppState::new(config_directory.clone(), resource_directory)
                .map_err(|error| format!("runtime supervisor setup failed: {error}"))?;
            state.runtime.supervisor().start_background();
            state.media_broker.start_background();
            #[cfg(debug_assertions)]
            state.start_installer_smoke_probe(config_directory);
            app.manage(state);
            lifecycle::install_tray(app)?;
            Ok(())
        })
        .on_window_event(lifecycle::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap_snapshot,
            commands::save_onboarding,
            commands::start_simulation,
            commands::cancel_simulation,
            commands::provider_credential_status,
            commands::prompt_and_save_provider_credential,
            commands::delete_provider_credential,
            commands::test_provider_connection,
            commands::game_profile_summaries,
            commands::model_pack_summaries,
            commands::diagnostic_summary,
            commands::runtime_health,
            commands::runtime_doctor,
            commands::runtime_profile_summaries,
            commands::media_broker_health,
            commands::media_broker_diagnostics,
            #[cfg(debug_assertions)]
            commands::debug_select_synthetic_replay_capture_target,
            #[cfg(debug_assertions)]
            commands::debug_clear_synthetic_replay_capture_target,
            #[cfg(debug_assertions)]
            commands::debug_synthetic_replay_capture_diagnostics,
            provider_loadouts::provider_loadout_snapshot,
            provider_loadouts::create_provider_loadout,
            provider_loadouts::clone_provider_loadout,
            provider_loadouts::rename_provider_loadout,
            provider_loadouts::update_provider_loadout,
            provider_loadouts::delete_provider_loadout,
            provider_loadouts::activate_provider_loadout,
            provider_loadouts::deactivate_provider_loadout_scope,
            provider_loadouts::review_provider_loadout,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Interactive NPCs Response Console");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            tauri::async_runtime::block_on(handle.state::<AppState>().shutdown_services());
        }
    });
}
