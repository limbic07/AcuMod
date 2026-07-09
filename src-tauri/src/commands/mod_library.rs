use crate::services::mod_library::{
    self, InstalledModList, ModImportPreview, ModInstallResult, ModLibraryStatus,
};

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

#[tauri::command]
pub fn install_mod_from_archive(
    app: tauri::AppHandle,
    path: String,
    allow_game_root: bool,
) -> Result<ModInstallResult, String> {
    mod_library::install_mod_from_archive(&app, path, allow_game_root)
}

#[tauri::command]
pub fn list_installed_mods(app: tauri::AppHandle) -> Result<InstalledModList, String> {
    mod_library::list_installed_mods(&app)
}
