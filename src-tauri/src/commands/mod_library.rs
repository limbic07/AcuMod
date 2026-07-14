use crate::operations::run_blocking_operation;
use crate::services::legacy_box::{self, LegacyBoxImportResult, LegacyBoxScan};
use crate::services::mod_library::{
    self, ApplyConflictOrderPlan, ApplyConflictOrderResult, InstalledModList,
    ModArchiveImportOutcome, ModCategory, ModCategoryDeleteResult, ModCategoryList,
    ModConflictMoveResult, ModConflictReport, ModDeploymentPlan, ModDeploymentResult,
    ModDisablePlan, ModImportPreview, ModInstallResult, ModLibraryStatus, ModMetadataPatch,
    ModMetadataUpdateResult, ModRemapApplyResult, ModRemapDetails, ModRemapPlan, ModUninstallPlan,
    ModUninstallResult, ModWorkspaceSnapshot, RestoreAllPlan, RestoreAllResult,
};

/// 扫描狩技 MOD 盒子记录与其游戏目录中的实际文件状态。
#[tauri::command]
pub async fn scan_legacy_box_mods(
    app: tauri::AppHandle,
    box_path: String,
) -> Result<LegacyBoxScan, String> {
    let acumod_game_path = crate::storage::config::load(&app)?.game_directory;
    run_blocking_operation(
        app,
        "scanLegacyBox",
        "正在扫描狩技 MOD 盒子",
        move |progress| {
            legacy_box::scan_legacy_box_with_progress(box_path, acumod_game_path, &progress)
        },
    )
    .await
}

/// 将狩技 MOD 盒子的所选 MOD 复制到 Acumod 本地库，初始不接管游戏目录。
#[tauri::command]
pub async fn import_legacy_box_mods(
    app: tauri::AppHandle,
    box_path: String,
    module_ids: Vec<String>,
) -> Result<LegacyBoxImportResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "importLegacyBox",
        "正在导入狩技 MOD 盒子",
        move |progress| {
            mod_library::import_legacy_box_mods_with_progress(
                &worker_app,
                box_path,
                module_ids,
                &progress,
            )
        },
    )
    .await
}

#[tauri::command]
pub fn get_mod_library_status(app: tauri::AppHandle) -> Result<ModLibraryStatus, String> {
    mod_library::get_mod_library_status(&app)
}

#[tauri::command]
pub async fn preview_mod_import(
    app: tauri::AppHandle,
    path: String,
    allow_game_root: bool,
) -> Result<ModImportPreview, String> {
    run_blocking_operation(app, "importPreview", "正在识别 MOD", move |progress| {
        mod_library::preview_mod_import_with_progress(path, allow_game_root, &progress)
    })
    .await
}

#[tauri::command]
pub async fn install_mod_from_folder(
    app: tauri::AppHandle,
    path: String,
    allow_game_root: bool,
) -> Result<ModInstallResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(app, "importFolder", "正在导入 MOD", move |progress| {
        mod_library::install_mod_from_folder_with_progress(
            &worker_app,
            path,
            allow_game_root,
            &progress,
        )
    })
    .await
}

#[tauri::command]
pub async fn install_mod_from_archive(
    app: tauri::AppHandle,
    path: String,
    allow_game_root: bool,
) -> Result<ModArchiveImportOutcome, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "importArchive",
        "正在解包并导入 MOD",
        move |progress| {
            mod_library::install_mod_from_archive_with_progress(
                &worker_app,
                path,
                allow_game_root,
                &progress,
            )
        },
    )
    .await
}

#[tauri::command]
pub async fn install_mod_from_candidate(
    app: tauri::AppHandle,
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
) -> Result<ModInstallResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "importCandidate",
        "正在导入所选 MOD",
        move |progress| {
            mod_library::install_mod_from_candidate_with_progress(
                &worker_app,
                source_path,
                candidate_root_path,
                original_archive_path,
                &progress,
            )
        },
    )
    .await
}

#[tauri::command]
pub async fn list_installed_mods(app: tauri::AppHandle) -> Result<InstalledModList, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "refreshLibrary",
        "正在刷新 MOD 库",
        move |progress| {
            progress.report("正在读取 MOD 清单", 0, None, None);
            mod_library::list_installed_mods(&worker_app)
        },
    )
    .await
}

#[tauri::command]
pub async fn get_mod_workspace_snapshot(
    app: tauri::AppHandle,
) -> Result<ModWorkspaceSnapshot, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "refreshWorkspace",
        "正在刷新 MOD 库",
        move |progress| {
            mod_library::get_mod_workspace_snapshot_with_progress(&worker_app, &progress)
        },
    )
    .await
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
pub fn list_mod_categories(app: tauri::AppHandle) -> Result<ModCategoryList, String> {
    mod_library::list_mod_categories(&app)
}

#[tauri::command]
pub fn create_mod_category(
    app: tauri::AppHandle,
    name: String,
    parent_id: Option<String>,
) -> Result<ModCategory, String> {
    mod_library::create_mod_category(&app, name, parent_id)
}

#[tauri::command]
pub fn move_mod_library_item(
    app: tauri::AppHandle,
    mod_id: String,
    target_mod_id: String,
    place_after: bool,
) -> Result<(), String> {
    mod_library::move_mod_library_item(&app, mod_id, target_mod_id, place_after)
}

#[tauri::command]
pub fn rename_mod_category(
    app: tauri::AppHandle,
    category_id: String,
    name: String,
) -> Result<ModCategory, String> {
    mod_library::rename_mod_category(&app, category_id, name)
}

#[tauri::command]
pub fn delete_mod_category(
    app: tauri::AppHandle,
    category_id: String,
) -> Result<ModCategoryDeleteResult, String> {
    mod_library::delete_mod_category(&app, category_id)
}

#[tauri::command]
pub fn open_installed_mod_folder(app: tauri::AppHandle, mod_id: String) -> Result<(), String> {
    mod_library::open_installed_mod_folder(&app, mod_id)
}

#[tauri::command]
pub async fn get_mod_remap_details(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModRemapDetails, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "remapDetails",
        "正在读取模型替换信息",
        move |progress| {
            progress.report("正在读取 MOD 文件", 0, None, None);
            mod_library::get_mod_remap_details(&worker_app, mod_id)
        },
    )
    .await
}

#[tauri::command]
pub async fn preview_mod_remap(
    app: tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapPlan, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "remapPreview",
        "正在检查模型替换",
        move |progress| {
            progress.report("正在检查模型文件", 0, None, None);
            mod_library::preview_mod_remap(&worker_app, mod_id, group_key, target_id)
        },
    )
    .await
}

#[tauri::command]
pub async fn apply_mod_remap(
    app: tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapApplyResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "remapApply",
        "正在保存模型替换",
        move |progress| {
            mod_library::apply_mod_remap_with_progress(
                &worker_app,
                mod_id,
                group_key,
                target_id,
                &progress,
            )
        },
    )
    .await
}

#[tauri::command]
pub async fn preview_enable_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModDeploymentPlan, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "enablePreview",
        "正在检查启用方案",
        move |progress| {
            progress.report("正在检查部署文件", 0, None, None);
            mod_library::preview_enable_mod(&worker_app, mod_id)
        },
    )
    .await
}

#[tauri::command]
pub async fn enable_mod(
    app: tauri::AppHandle,
    mod_id: String,
    confirm_overwrite: bool,
) -> Result<ModDeploymentResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(app, "enableMod", "正在启用 MOD", move |progress| {
        mod_library::enable_mod_with_progress(&worker_app, mod_id, confirm_overwrite, &progress)
    })
    .await
}

#[tauri::command]
pub async fn disable_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModDeploymentResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(app, "disableMod", "正在禁用 MOD", move |progress| {
        mod_library::disable_mod_with_progress(&worker_app, mod_id, &progress)
    })
    .await
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
pub async fn uninstall_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModUninstallResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(app, "uninstallMod", "正在卸载 MOD", move |progress| {
        mod_library::uninstall_mod_with_progress(&worker_app, mod_id, &progress)
    })
    .await
}

#[tauri::command]
pub fn preview_restore_all_mods(app: tauri::AppHandle) -> Result<RestoreAllPlan, String> {
    mod_library::preview_restore_all_mods(&app)
}

#[tauri::command]
pub async fn restore_all_mods(app: tauri::AppHandle) -> Result<RestoreAllResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "restoreAll",
        "正在还原游戏目录",
        move |progress| mod_library::restore_all_mods_with_progress(&worker_app, &progress),
    )
    .await
}

#[tauri::command]
pub async fn get_mod_conflict_report(app: tauri::AppHandle) -> Result<ModConflictReport, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "refreshConflicts",
        "正在读取冲突信息",
        move |progress| {
            progress.report("正在分析启用中的 MOD", 0, None, None);
            mod_library::get_mod_conflict_report(&worker_app)
        },
    )
    .await
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
pub async fn preview_apply_conflict_order(
    app: tauri::AppHandle,
    group_id: String,
) -> Result<ApplyConflictOrderPlan, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "conflictPreview",
        "正在检查冲突优先级",
        move |progress| {
            progress.report("正在分析冲突文件", 0, None, None);
            mod_library::preview_apply_conflict_order(&worker_app, group_id)
        },
    )
    .await
}

#[tauri::command]
pub async fn apply_conflict_order(
    app: tauri::AppHandle,
    group_id: String,
    confirm_overwrite: bool,
) -> Result<ApplyConflictOrderResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "conflictApply",
        "正在应用冲突优先级",
        move |progress| {
            mod_library::apply_conflict_order_with_progress(
                &worker_app,
                group_id,
                confirm_overwrite,
                &progress,
            )
        },
    )
    .await
}
