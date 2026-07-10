mod commands;
mod services;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::game::detect_game_directory,
            commands::game::get_game_directory_status,
            commands::game::save_game_directory,
            commands::mod_library::disable_mod,
            commands::mod_library::enable_mod,
            commands::mod_library::get_mod_library_status,
            commands::mod_library::install_mod_from_archive,
            commands::mod_library::install_mod_from_folder,
            commands::mod_library::list_installed_mods,
            commands::mod_library::preview_enable_mod,
            commands::mod_library::preview_mod_import,
            commands::mod_library::preview_restore_all_mods,
            commands::mod_library::preview_uninstall_mod,
            commands::mod_library::restore_all_mods,
            commands::mod_library::uninstall_mod,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
