use crate::{
    services::game::{self, GameDirectoryStatus, GameTextSettings},
    storage::config::GameTextLanguage,
};

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

/// 读取武器、防具等游戏内容名称使用的语言。
#[tauri::command]
pub fn get_game_text_settings(app: tauri::AppHandle) -> Result<GameTextSettings, String> {
    game::get_game_text_settings(&app)
}

/// 保存游戏内容名称语言；该设置不改变 Acumod 自身界面语言。
#[tauri::command]
pub fn save_game_text_language(
    app: tauri::AppHandle,
    language: GameTextLanguage,
) -> Result<GameTextSettings, String> {
    game::save_game_text_language(&app, language)
}
