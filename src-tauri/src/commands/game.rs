use crate::services::game::{self, GameDirectoryStatus};

#[tauri::command]
pub fn get_game_directory_status(app: tauri::AppHandle) -> Result<GameDirectoryStatus, String> {
    game::get_game_directory_status(&app)
}

#[tauri::command]
pub fn detect_game_directory(app: tauri::AppHandle) -> Result<GameDirectoryStatus, String> {
    game::detect_game_directory(&app)
}

#[tauri::command]
pub fn save_game_directory(
    app: tauri::AppHandle,
    path: String,
) -> Result<GameDirectoryStatus, String> {
    game::save_game_directory(&app, path)
}
