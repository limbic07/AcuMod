use crate::services::mod_library::{
    self, ApplyConflictOrderPlan, ApplyConflictOrderResult, InstalledModList,
    ModArchiveImportOutcome, ModConflictMoveResult, ModConflictReport, ModDeploymentPlan,
    ModDeploymentResult, ModDisablePlan, ModImportPreview, ModInstallResult, ModLibraryStatus,
    ModMetadataPatch, ModMetadataUpdateResult, ModRemapApplyResult, ModRemapDetails, ModRemapPlan,
    ModUninstallPlan, ModUninstallResult, RestoreAllPlan, RestoreAllResult, UserModCategory,
    UserModCategoryDeleteResult, UserModCategoryList,
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
) -> Result<ModArchiveImportOutcome, String> {
    mod_library::install_mod_from_archive(&app, path, allow_game_root)
}

#[tauri::command]
pub fn install_mod_from_candidate(
    app: tauri::AppHandle,
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
) -> Result<ModInstallResult, String> {
    mod_library::install_mod_from_candidate(
        &app,
        source_path,
        candidate_root_path,
        original_archive_path,
    )
}

#[tauri::command]
pub fn list_installed_mods(app: tauri::AppHandle) -> Result<InstalledModList, String> {
    mod_library::list_installed_mods(&app)
}

#[tauri::command]
pub fn update_mod_metadata(
    app: tauri::AppHandle,
    mod_id: String,
    patch: ModMetadataPatch,
) -> Result<ModMetadataUpdateResult, String> {
    mod_library::update_mod_metadata(&app, mod_id, patch)
}

#[tauri::command]
pub fn list_user_mod_categories(app: tauri::AppHandle) -> Result<UserModCategoryList, String> {
    mod_library::list_user_mod_categories(&app)
}

#[tauri::command]
pub fn create_user_mod_category(
    app: tauri::AppHandle,
    name: String,
) -> Result<UserModCategory, String> {
    mod_library::create_user_mod_category(&app, name)
}

#[tauri::command]
pub fn rename_user_mod_category(
    app: tauri::AppHandle,
    category_id: String,
    name: String,
) -> Result<UserModCategory, String> {
    mod_library::rename_user_mod_category(&app, category_id, name)
}

#[tauri::command]
pub fn delete_user_mod_category(
    app: tauri::AppHandle,
    category_id: String,
) -> Result<UserModCategoryDeleteResult, String> {
    mod_library::delete_user_mod_category(&app, category_id)
}

#[tauri::command]
pub fn open_installed_mod_folder(app: tauri::AppHandle, mod_id: String) -> Result<(), String> {
    mod_library::open_installed_mod_folder(&app, mod_id)
}

#[tauri::command]
pub fn get_mod_remap_details(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModRemapDetails, String> {
    mod_library::get_mod_remap_details(&app, mod_id)
}

#[tauri::command]
pub fn preview_mod_remap(
    app: tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapPlan, String> {
    mod_library::preview_mod_remap(&app, mod_id, group_key, target_id)
}

#[tauri::command]
pub fn apply_mod_remap(
    app: tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapApplyResult, String> {
    mod_library::apply_mod_remap(&app, mod_id, group_key, target_id)
}

#[tauri::command]
pub fn preview_enable_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModDeploymentPlan, String> {
    mod_library::preview_enable_mod(&app, mod_id)
}

#[tauri::command]
pub fn enable_mod(
    app: tauri::AppHandle,
    mod_id: String,
    confirm_overwrite: bool,
) -> Result<ModDeploymentResult, String> {
    mod_library::enable_mod(&app, mod_id, confirm_overwrite)
}

#[tauri::command]
pub fn disable_mod(app: tauri::AppHandle, mod_id: String) -> Result<ModDeploymentResult, String> {
    mod_library::disable_mod(&app, mod_id)
}

#[tauri::command]
pub fn preview_disable_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModDisablePlan, String> {
    mod_library::preview_disable_mod(&app, mod_id)
}

#[tauri::command]
pub fn preview_uninstall_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModUninstallPlan, String> {
    mod_library::preview_uninstall_mod(&app, mod_id)
}

#[tauri::command]
pub fn uninstall_mod(app: tauri::AppHandle, mod_id: String) -> Result<ModUninstallResult, String> {
    mod_library::uninstall_mod(&app, mod_id)
}

#[tauri::command]
pub fn preview_restore_all_mods(app: tauri::AppHandle) -> Result<RestoreAllPlan, String> {
    mod_library::preview_restore_all_mods(&app)
}

#[tauri::command]
pub fn restore_all_mods(app: tauri::AppHandle) -> Result<RestoreAllResult, String> {
    mod_library::restore_all_mods(&app)
}

#[tauri::command]
pub fn get_mod_conflict_report(app: tauri::AppHandle) -> Result<ModConflictReport, String> {
    mod_library::get_mod_conflict_report(&app)
}

#[tauri::command]
pub fn move_conflict_participant(
    app: tauri::AppHandle,
    group_id: String,
    mod_id: String,
    direction: String,
) -> Result<ModConflictMoveResult, String> {
    mod_library::move_conflict_participant(&app, group_id, mod_id, direction)
}

#[tauri::command]
pub fn preview_apply_conflict_order(
    app: tauri::AppHandle,
    group_id: String,
) -> Result<ApplyConflictOrderPlan, String> {
    mod_library::preview_apply_conflict_order(&app, group_id)
}

#[tauri::command]
pub fn apply_conflict_order(
    app: tauri::AppHandle,
    group_id: String,
    confirm_overwrite: bool,
) -> Result<ApplyConflictOrderResult, String> {
    mod_library::apply_conflict_order(&app, group_id, confirm_overwrite)
}
