use crate::operations::run_blocking_operation;
use crate::services::legacy_box::{self, LegacyBoxImportResult, LegacyBoxScan};
use crate::services::mod_library::{
    self, ApplyConflictOrderPlan, ApplyConflictOrderResult, BatchModAction,
    BatchModOperationResult, InstalledModList, ModArchiveImportOutcome, ModBranchGroup,
    ModBranchImportResult, ModBranchImportSelection, ModCategory, ModCategoryAssignment,
    ModCategoryBatchUpdateResult, ModCategoryDeleteResult, ModCategoryList, ModConflictMoveResult,
    ModConflictReport, ModDeploymentPlan, ModDeploymentResult, ModDisablePlan, ModImportPreview,
    ModInstallResult, ModLibraryOrderResult, ModLibraryStatus, ModMetadataPatch,
    ModMetadataUpdateResult, ModRemapApplyResult, ModRemapDetails, ModRemapPlan, ModUninstallPlan,
    ModUninstallResult, ModWorkspaceSnapshot, RestoreAllPlan, RestoreAllResult,
};
use crate::services::mod_state_sync::ModStateSyncResult;

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

/// 将狩技 MOD 盒子的所选 MOD 复制到 Acumod 本地库，并自动检测实际游戏状态。
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

/// 重新检测已关联狩技 MOD 盒子模块在当前游戏目录中的实际状态。
#[tauri::command]
pub async fn refresh_game_mod_states(app: tauri::AppHandle) -> Result<ModStateSyncResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "refreshGameModStates",
        "正在检测游戏实际 MOD 状态",
        move |progress| mod_library::refresh_game_mod_states_with_progress(&worker_app, &progress),
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
    let worker_app = app.clone();
    run_blocking_operation(app, "importPreview", "正在识别 MOD", move |progress| {
        mod_library::preview_mod_import_source_with_nested(
            &worker_app,
            path,
            allow_game_root,
            &progress,
        )
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

/// 安装所选的一个或多个候选版本，并可将它们组织为同一个分支组。
#[tauri::command]
pub async fn install_mod_branches(
    app: tauri::AppHandle,
    source_path: String,
    selections: Vec<ModBranchImportSelection>,
    original_source_path: Option<String>,
    group_name: Option<String>,
    as_branch_group: bool,
) -> Result<ModBranchImportResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "importBranches",
        "正在导入 MOD 分支",
        move |progress| {
            mod_library::install_mod_branches_with_progress(
                &worker_app,
                source_path,
                selections,
                original_source_path,
                group_name,
                as_branch_group,
                &progress,
            )
        },
    )
    .await
}

/// 将现有 MOD 组织为新的分支组。
#[tauri::command]
pub fn create_mod_branch_group(
    app: tauri::AppHandle,
    name: String,
    mod_ids: Vec<String>,
) -> Result<ModBranchGroup, String> {
    mod_library::create_mod_branch_group(&app, name, mod_ids)
}

/// 修改分支组名称。
#[tauri::command]
pub fn rename_mod_branch_group(
    app: tauri::AppHandle,
    group_id: String,
    name: String,
) -> Result<ModBranchGroup, String> {
    mod_library::rename_mod_branch_group(&app, group_id, name)
}

/// 将所选 MOD 移出其分支组。
#[tauri::command]
pub fn remove_mods_from_branch_group(
    app: tauri::AppHandle,
    mod_ids: Vec<String>,
) -> Result<Vec<ModBranchGroup>, String> {
    mod_library::remove_mods_from_branch_group(&app, mod_ids)
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
        "loadWorkspace",
        "正在加载 MOD 库",
        move |progress| {
            mod_library::get_mod_workspace_snapshot_with_progress(&worker_app, &progress)
        },
    )
    .await
}

#[tauri::command]
pub async fn refresh_mod_workspace_snapshot(
    app: tauri::AppHandle,
) -> Result<ModWorkspaceSnapshot, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "refreshWorkspace",
        "正在刷新 MOD 库",
        move |progress| {
            mod_library::refresh_mod_workspace_snapshot_with_progress(&worker_app, &progress)
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

/// 批量更新分支组成员分类，避免前端逐个命令刷新快照。
#[tauri::command]
pub fn update_mod_categories(
    app: tauri::AppHandle,
    assignments: Vec<ModCategoryAssignment>,
) -> Result<ModCategoryBatchUpdateResult, String> {
    mod_library::update_mod_categories(&app, assignments)
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

/// 将普通 MOD 或整个分支组作为一个连续块调整浏览顺序。
#[tauri::command]
pub fn move_mod_library_items(
    app: tauri::AppHandle,
    mod_ids: Vec<String>,
    target_mod_ids: Vec<String>,
    place_after: bool,
) -> Result<(), String> {
    mod_library::move_mod_library_items(&app, mod_ids, target_mod_ids, place_after)
}

/// 将当前完整浏览结果保存为新的手动顺序；后台执行以避免大量 MOD 时阻塞界面。
#[tauri::command]
pub async fn replace_mod_library_order(
    app: tauri::AppHandle,
    mod_ids: Vec<String>,
) -> Result<ModLibraryOrderResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "replaceModLibraryOrder",
        "正在保存 MOD 顺序",
        move |_| mod_library::replace_mod_library_order(&worker_app, mod_ids),
    )
    .await
}

/// 恢复最早导入在上、最新导入在下的原始顺序。
#[tauri::command]
pub async fn restore_mod_library_import_order(
    app: tauri::AppHandle,
) -> Result<ModLibraryOrderResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "restoreModLibraryOrder",
        "正在恢复导入顺序",
        move |_| mod_library::restore_mod_library_import_order(&worker_app),
    )
    .await
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

/// 打开 AI 清理候选在本地 MOD 库中的所在文件夹。
#[tauri::command]
pub fn open_mod_cleanup_candidate_folder(
    app: tauri::AppHandle,
    mod_id: String,
    candidate_id: String,
) -> Result<(), String> {
    mod_library::open_mod_cleanup_candidate_folder(&app, mod_id, candidate_id)
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

/// 在一个后台任务中顺序启用、禁用或卸载多个 MOD。
#[tauri::command]
pub async fn batch_update_mods(
    app: tauri::AppHandle,
    action: BatchModAction,
    mod_ids: Vec<String>,
) -> Result<BatchModOperationResult, String> {
    let title = match action {
        BatchModAction::Enable => "正在批量启用 MOD",
        BatchModAction::Disable => "正在批量禁用 MOD",
        BatchModAction::Uninstall => "正在批量卸载 MOD",
    };
    let worker_app = app.clone();
    run_blocking_operation(app, "batchUpdateMods", title, move |progress| {
        mod_library::batch_update_mods_with_progress(&worker_app, action, mod_ids, &progress)
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
pub async fn move_conflict_participant(
    app: tauri::AppHandle,
    group_id: String,
    mod_id: String,
    direction: String,
    participant_order: Vec<String>,
) -> Result<ModConflictMoveResult, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "moveConflict",
        "正在调整冲突优先级",
        move |progress| {
            progress.report("正在保存冲突顺序", 0, None, None);
            mod_library::move_conflict_participant(
                &worker_app,
                group_id,
                mod_id,
                direction,
                participant_order,
            )
        },
    )
    .await
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
