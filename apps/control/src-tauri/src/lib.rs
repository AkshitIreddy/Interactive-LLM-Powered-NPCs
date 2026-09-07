pub mod catalog;
mod character_content_overrides;
mod character_mouth_packs;
pub mod character_workspace;
mod commands;
mod content_packs;
mod credential_prompt;
mod diagnostics_v2;
mod domain;
mod effective_configuration;
mod game_targets;
pub(crate) mod identity_runtime;
pub(crate) mod identity_worker_transport;
mod lifecycle;
mod local_resources;
pub mod media_broker;
mod optional_pack_activation;
mod packaged_privacy_receipt;
mod persistence;
mod private_directory;
mod product_benchmark;
mod product_preferences;
mod provider_loadouts;
mod runtime_bridge;
mod runtime_router;
mod selected_stt;
pub mod sidecar_protocol;
pub mod sidecar_supervisor;
#[cfg(debug_assertions)]
mod synthetic_review_target;
pub mod visual_runtime;

use commands::AppState;
use lifecycle::LifecycleState;
use tauri::Manager;

pub use provider_loadouts::{
    ProviderEntitlementModeV1, ProviderPrivateEvaluationAcknowledgementV1,
    PRIVATE_EVALUATION_TERMS_REVISION,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(LifecycleState::default())
        .setup(|app| {
            let config_directory = app.path().app_config_dir().map_err(|error| {
                format!("application configuration path is unavailable: {error}")
            })?;
            let resource_directory = app.path().resource_dir().ok();
            let state = AppState::new_for_distribution(
                config_directory.clone(),
                resource_directory,
                app.config().identifier.clone(),
            )
            .map_err(|error| format!("runtime supervisor setup failed: {error}"))?;
            state.runtime.supervisor().start_background();
            state.media_broker.start_background();
            state.identity_runtime.invalidate_and_reconcile_background();
            state.start_packaged_privacy_receipt_probe();
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
            commands::diagnostic_summary,
            commands::discover_tts_stock_voices,
            commands::enumerate_audio_outputs,
            commands::selected_audio_output,
            commands::select_audio_output,
            commands::enumerate_audio_inputs,
            commands::selected_audio_input,
            commands::select_audio_input,
            commands::start_selected_stt_push_to_talk,
            commands::selected_stt_push_to_talk_status,
            commands::cancel_selected_stt_push_to_talk,
            commands::identity_reference_enrollment_status,
            commands::enroll_identity_reference,
            commands::discover_game_targets,
            commands::select_game_target,
            commands::selected_game_target,
            commands::clear_game_target,
            commands::verify_selected_game_capture,
            commands::start_manual_actor_picker,
            commands::manual_actor_picker_status,
            commands::cancel_manual_actor_picker,
            commands::character_database_inspection,
            commands::character_database_catalog,
            commands::select_game_character,
            commands::character_content_override,
            commands::save_character_content_override,
            commands::reset_character_content_override,
            commands::inspect_character_mouth_pack,
            commands::import_character_mouth_pack,
            commands::enable_character_mouth_pack,
            commands::disable_character_mouth_pack,
            commands::character_mouth_pack_state,
            commands::inspect_content_pack,
            commands::activate_content_pack,
            commands::content_pack_state,
            commands::character_memory_status,
            commands::backup_all_local_memory,
            commands::list_local_memory_backups,
            commands::delete_local_memory_backup,
            commands::erase_character_memory,
            commands::restore_local_memory_backup,
            commands::remove_all_local_memory,
            commands::correct_encounter_to_authored_character,
            commands::merge_unknown_encounters,
            commands::local_resource_settings,
            commands::save_local_resource_settings,
            commands::local_resource_telemetry,
            commands::trusted_local_pack_catalog,
            commands::trusted_optional_pack_lifecycle,
            commands::install_trusted_optional_pack,
            commands::activate_trusted_optional_pack,
            commands::cancel_trusted_optional_pack_download,
            commands::repair_trusted_optional_pack,
            commands::remove_trusted_optional_pack,
            commands::selected_local_loadout_planner,
            commands::admit_selected_local_loadout,
            commands::experimental_visual_pack_status,
            commands::install_experimental_model_pack,
            commands::activate_experimental_model_pack,
            commands::repair_model_pack,
            commands::remove_model_pack,
            commands::diagnostics_v2_snapshot,
            commands::diagnostics_v2_settings,
            commands::save_diagnostics_v2_settings,
            commands::diagnostics_v2_matrix,
            commands::export_diagnostics_v2,
            commands::start_this_pc_benchmark,
            commands::cancel_this_pc_benchmark,
            commands::this_pc_benchmark_status,
            commands::this_pc_benchmark_report,
            commands::product_preferences_snapshot,
            commands::effective_configuration_snapshot,
            commands::read_subtitle_preferences,
            commands::save_subtitle_preferences,
            commands::reset_subtitle_preferences,
            commands::save_product_preferences,
            commands::reset_product_preferences,
            #[cfg(debug_assertions)]
            commands::prepare_synthetic_review_target,
            #[cfg(debug_assertions)]
            commands::debug_select_synthetic_replay_capture_target,
            #[cfg(debug_assertions)]
            commands::debug_latest_visual_presentation_receipt,
            #[cfg(debug_assertions)]
            commands::debug_visual_presentation_receipt_history,
            provider_loadouts::provider_loadout_snapshot,
            provider_loadouts::provider_private_evaluation_policy,
            provider_loadouts::provider_private_evaluation_acknowledgement,
            provider_loadouts::acknowledge_provider_private_evaluation,
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
