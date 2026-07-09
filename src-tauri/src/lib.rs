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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
