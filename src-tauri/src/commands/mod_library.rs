use crate::services::mod_library::{self, ModImportPreview, ModInstallResult, ModLibraryStatus};

#[tauri::command]
pub fn get_mod_library_status(app: tauri::AppHandle) -> Result<ModLibraryStatus, String> {
    mod_library::get_mod_library_status(&app)
}

#[tauri::command]
pub fn preview_mod_import(path: String, allow_game_root: bool) -> Result<ModImportPreview, String> {
    mod_library::preview_mod_import(path, allow_game_root)
}

#[tauri::command]
pub fn install_mod_from_folder(
    app: tauri::AppHandle,
    path: String,
    allow_game_root: bool,
) -> Result<ModInstallResult, String> {
    mod_library::install_mod_from_folder(&app, path, allow_game_root)
}
