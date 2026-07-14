mod commands;
mod operations;
mod services;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(operations::OperationCoordinator::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::game::detect_game_directory,
            commands::game::get_game_directory_status,
            commands::game::save_game_directory,
            commands::mod_library::apply_conflict_order,
            commands::mod_library::apply_mod_remap,
            commands::mod_library::create_mod_category,
            commands::mod_library::delete_mod_category,
            commands::mod_library::disable_mod,
            commands::mod_library::enable_mod,
            commands::mod_library::get_mod_conflict_report,
            commands::mod_library::get_mod_library_status,
            commands::mod_library::get_mod_remap_details,
            commands::mod_library::get_mod_workspace_snapshot,
            commands::mod_library::install_mod_from_archive,
            commands::mod_library::install_mod_from_candidate,
            commands::mod_library::install_mod_from_folder,
            commands::mod_library::import_legacy_box_mods,
            commands::mod_library::list_installed_mods,
            commands::mod_library::list_mod_categories,
            commands::mod_library::move_conflict_participant,
            commands::mod_library::move_mod_library_item,
            commands::mod_library::open_installed_mod_folder,
            commands::mod_library::preview_apply_conflict_order,
            commands::mod_library::preview_disable_mod,
            commands::mod_library::preview_enable_mod,
            commands::mod_library::preview_mod_import,
            commands::mod_library::preview_mod_remap,
            commands::mod_library::preview_restore_all_mods,
            commands::mod_library::preview_uninstall_mod,
            commands::mod_library::rename_mod_category,
            commands::mod_library::refresh_game_mod_states,
            commands::mod_library::restore_all_mods,
            commands::mod_library::scan_legacy_box_mods,
            commands::mod_library::uninstall_mod,
            commands::mod_library::update_mod_metadata,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
