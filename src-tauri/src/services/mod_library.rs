use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env, fs,
    io::{BufReader, ErrorKind, Read},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::operations::OperationReporter;
use crate::storage::config;

use super::effect_remap::{
    build_effect_remap_groups, build_effective_effect_remap_paths, updated_effect_remap_selections,
    EffectRemapGroup, EffectRemapSelection,
};
use super::legacy_box::{self, LegacyBoxImportItem, LegacyBoxImportResult};
use super::mod_analysis::{summarize_effect_paths, EffectRecognitionSummary};
use super::mod_state_sync::{
    self, ModStateSyncFile, ModStateSyncInput, ModStateSyncPlan, ModStateSyncPriorityEdge,
    ModStateSyncResult,
};
use super::model_recognition::{
    recognize_model_replacements, recognize_model_replacements_with_evam, EvamRecognitionFile,
    ModelReplacement,
};
use super::model_remap::{
    build_effective_remap_files, build_effective_remap_files_with_armor_dat,
    build_model_remap_groups, is_armor_epv_deploy_path, rewrite_evam_slinger_id,
    rewrite_mrl3_texture_paths, special_character_armor_target_warning, EffectiveRemapFile,
    EvamSlingerIdRewrite, ModelRemapGroup, ModelRemapSelection,
};

const PREVIEW_FILE_LIMIT: usize = 200;
const CURRENT_MOD_MANIFEST_SCHEMA_VERSION: u32 = 18;
const CURRENT_MODEL_RECOGNITION_SCHEMA_VERSION: u32 = 16;
const WORKSPACE_SNAPSHOT_SCHEMA_VERSION: u32 = 4;
const MOD_CATEGORY_STORE_SCHEMA_VERSION: u32 = 3;
const MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION: u32 = 2;
const MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION: u32 = 1;
const MOD_BRANCH_GROUP_NAME_LIMIT: usize = 120;
const MAX_ARMOR_DAT_SIZE_BYTES: u64 = 4 * 1024 * 1024;
const NESTED_ARCHIVE_MAX_DEPTH: usize = 2;
const NESTED_ARCHIVE_MAX_COUNT: usize = 32;
const MOD_CATEGORY_NAME_LIMIT: usize = 40;
const COMMON_NATIVE_PC_CHILDREN: &[&str] = &[
    "weapon", "wp", "pl", "armor", "common", "npc", "em", "quest", "stage", "sound", "vfx",
    "effect", "ui", "otomo", "charm", "mus", "plugins",
];
const MOD_CLEANUP_RULES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/mod-cleanup-rules.json"
));

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLibraryStatus {
    pub software_data_path: String,
    pub mods_path: String,
    pub installed_path: String,
    pub staging_path: String,
    pub import_staging_path: String,
    pub is_ready: bool,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModImportPreview {
    pub source_path: String,
    pub original_source_path: String,
    pub status: String,
    pub detection_method: String,
    pub deploy_root: String,
    pub content_root_path: Option<String>,
    pub requires_game_root_confirmation: bool,
    pub message: String,
    pub file_count: usize,
    pub files: Vec<ModImportFilePreview>,
    pub candidates: Vec<ModImportCandidate>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModImportFilePreview {
    pub source_path: String,
    pub source_relative_path: String,
    pub deploy_relative_path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModImportCandidate {
    pub root_path: String,
    pub source_root_path: String,
    pub relative_path: String,
    pub suggested_name: String,
    pub archive_chain: Vec<String>,
    pub requires_game_root_confirmation: bool,
    pub detection_method: String,
    pub deploy_root: String,
    pub file_count: usize,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModBranchImportSelection {
    pub candidate_root_path: String,
    pub branch_name: String,
    pub allow_game_root: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModBranchImportResult {
    pub group: Option<ModBranchGroup>,
    pub install_results: Vec<ModInstallResult>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModFile {
    pub source_relative_path: String,
    pub deploy_relative_path: String,
    pub library_relative_path: String,
}

/// 本地库文件的部署排除记录。原始文件仍保留，只是不再进入游戏目录部署计划。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentExclusion {
    pub candidate_id: String,
    pub library_relative_path: String,
    pub deploy_relative_path: String,
    pub reason: String,
    pub batch_id: String,
    pub excluded_at_unix_seconds: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupCandidate {
    pub candidate_id: String,
    pub mod_id: String,
    pub mod_name: String,
    pub library_relative_path: String,
    pub deploy_relative_path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub local_kind: String,
    pub local_hint: String,
    pub currently_deployed: bool,
    pub review_source: String,
    pub risk_level: String,
    pub keep_signals: Vec<String>,
    pub exclude_signals: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupScan {
    pub installed_mod_count: usize,
    pub scanned_file_count: usize,
    pub local_keep_count: usize,
    pub local_remove_count: usize,
    pub ai_review_count: usize,
    pub rule_version: u32,
    pub candidate_count: usize,
    pub candidates: Vec<ModCleanupCandidate>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupTextPreview {
    pub candidate_id: String,
    pub library_relative_path: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModCleanupRules {
    schema_version: u32,
    runtime_extensions: HashSet<String>,
    plugin_runtime_extensions: HashSet<String>,
    game_root_runtime_extensions: HashSet<String>,
    exact_junk_names: HashSet<String>,
    junk_path_components: HashSet<String>,
    backup_suffixes: Vec<String>,
    known_authoring_tool_prefixes: Vec<String>,
    documentation_extensions: HashSet<String>,
    documentation_keywords: Vec<String>,
    preview_extensions: HashSet<String>,
    preview_keywords: Vec<String>,
    archive_extensions: HashSet<String>,
    safe_text_extensions: HashSet<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupSelection {
    pub candidate_id: String,
    pub mod_id: String,
    pub library_relative_path: String,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupApplyResult {
    pub batch_id: String,
    pub affected_mod_count: usize,
    pub exclusion_count: usize,
    pub removed_deployed_file_count: usize,
    pub restored_conflict_file_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupExclusionGroup {
    pub mod_id: String,
    pub mod_name: String,
    pub exclusions: Vec<ModDeploymentExclusion>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupExclusionList {
    pub exclusion_count: usize,
    pub latest_batch_id: Option<String>,
    pub groups: Vec<ModCleanupExclusionGroup>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCleanupRestoreResult {
    pub affected_mod_count: usize,
    pub restored_exclusion_count: usize,
    pub deployed_file_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInstallResult {
    pub mod_id: String,
    pub name: String,
    pub already_installed: bool,
    pub mod_path: String,
    pub content_path: String,
    pub manifest_path: String,
    pub file_count: usize,
    pub files: Vec<InstalledModFile>,
    pub model_replacements: Vec<ModelReplacement>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModArchiveImportOutcome {
    pub status: String,
    pub source_path: String,
    pub original_archive_path: String,
    pub preview: Option<ModImportPreview>,
    pub install_result: Option<ModInstallResult>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModBranchGroup {
    pub id: String,
    pub name: String,
    pub mod_ids: Vec<String>,
    pub created_at_unix_seconds: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModSummary {
    pub id: String,
    pub name: String,
    pub original_name: String,
    pub note: String,
    pub category_ids: Vec<String>,
    pub categories: Vec<ModCategory>,
    pub mod_path: String,
    pub content_path: String,
    pub manifest_path: String,
    pub source_path: String,
    pub file_count: usize,
    pub files: Vec<InstalledModFile>,
    pub enabled: bool,
    /// 已启用时的附加提示，不构成第三种 MOD 状态。
    pub partially_overridden: bool,
    pub deploy_root: String,
    pub detection_method: String,
    pub installed_at_unix_seconds: u64,
    pub model_replacements: Vec<ModelReplacement>,
    pub original_model_replacements: Vec<ModelReplacement>,
    pub model_remap_count: usize,
    pub effect_remap_count: usize,
    pub effect_recognition: EffectRecognitionSummary,
}

/// MOD 分析服务的受控输入。绝对路径只在 Rust 内部使用，不会序列化给前端或模型。
pub(crate) struct ModAnalysisInput {
    pub mod_id: String,
    pub name: String,
    pub files: Vec<ModAnalysisInputFile>,
    pub model_replacements: Vec<ModelReplacement>,
}

pub(crate) struct ModAnalysisInputFile {
    pub source_path: PathBuf,
    pub library_relative_path: String,
    pub source_deploy_relative_path: String,
    pub effective_deploy_relative_path: String,
    pub size_bytes: u64,
    pub excluded_from_deployment: bool,
}

/// MOD 库浏览顺序更新结果；只描述界面顺序，不参与部署或冲突优先级。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLibraryOrderResult {
    pub manual_mod_ids: Vec<String>,
    pub import_mod_ids: Vec<String>,
    pub applied_source: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRemapDetails {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub groups: Vec<ModelRemapGroup>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRemapPlanFile {
    pub source_deploy_relative_path: String,
    pub effective_deploy_relative_path: String,
    pub path_changed: bool,
    pub mrl3_rewrite_count: usize,
    pub evam_rewrite_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRemapPlan {
    pub mod_id: String,
    pub name: String,
    pub group_key: String,
    pub source_label: String,
    pub target_id: Option<String>,
    pub target_label: String,
    pub changed_file_count: usize,
    pub mrl3_rewrite_count: usize,
    pub evam_rewrite_count: usize,
    pub files: Vec<ModRemapPlanFile>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRemapApplyResult {
    pub mod_id: String,
    pub name: String,
    pub group_key: String,
    pub target_id: Option<String>,
    pub selection_count: usize,
    pub changed_file_count: usize,
    pub mrl3_rewrite_count: usize,
    pub evam_rewrite_count: usize,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEffectRemapDetails {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub groups: Vec<EffectRemapGroup>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEffectRemapPlan {
    pub mod_id: String,
    pub name: String,
    pub group_key: String,
    pub source_label: String,
    pub target_id: Option<String>,
    pub target_label: String,
    pub changed_file_count: usize,
    pub files: Vec<ModRemapPlanFile>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEffectRemapApplyResult {
    pub mod_id: String,
    pub name: String,
    pub group_key: String,
    pub target_id: Option<String>,
    pub selection_count: usize,
    pub changed_file_count: usize,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModList {
    pub mods: Vec<InstalledModSummary>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModWorkspaceSnapshot {
    pub installed_mods: InstalledModList,
    pub categories: ModCategoryList,
    pub conflict_report: ModConflictReport,
    #[serde(default)]
    pub branch_groups: Vec<ModBranchGroup>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentPlanFile {
    pub deploy_relative_path: String,
    pub source_path: String,
    pub target_path: String,
    pub target_exists: bool,
    pub target_managed_by_current_mod: bool,
    pub target_managed_by_other_mod: bool,
    pub target_managed_mod_id: Option<String>,
}

/// 启用前按已启用 MOD 汇总的真实路径冲突，供前端确认框分组展示。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentConflict {
    pub mod_id: String,
    pub name: String,
    pub conflict_files: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentPlan {
    pub mod_id: String,
    pub name: String,
    pub status: String,
    pub message: String,
    pub file_count: usize,
    pub files: Vec<ModDeploymentPlanFile>,
    pub conflicts: Vec<ModDeploymentConflict>,
    pub warnings: Vec<String>,
    pub requires_overwrite_confirmation: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployedModFile {
    pub deploy_relative_path: String,
    pub deployed_path: String,
    pub deployed_at_unix_seconds: u64,
    /// `observed` 表示由状态检测记录，删除前必须重新比对游戏目录内容。
    #[serde(default)]
    pub deployment_origin: DeploymentOrigin,
    /// 兼容上一版接管原型写出的字段。保存新清单时会迁移为 `deploymentOrigin`。
    #[serde(default, skip_serializing)]
    pub is_adopted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeploymentOrigin {
    Copied,
    Observed,
}

impl Default for DeploymentOrigin {
    fn default() -> Self {
        Self::Copied
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentResult {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub affected_file_count: usize,
    pub files: Vec<DeployedModFile>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDisablePlan {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub file_count: usize,
    pub files: Vec<DeployedModFile>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUninstallPlan {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub deployed_file_count: usize,
    pub library_file_count: usize,
    pub deployed_files: Vec<DeployedModFile>,
    pub library_files: Vec<InstalledModFile>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUninstallResult {
    pub mod_id: String,
    pub name: String,
    pub removed_deployed_file_count: usize,
    pub removed_library_file_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

/// MOD 库支持的批量操作类型。
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BatchModAction {
    Enable,
    Disable,
    Uninstall,
}

/// 批量操作中单个 MOD 的执行结果；失败不会阻止后续项目继续执行。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchModOperationItem {
    pub mod_id: String,
    pub name: String,
    pub status: String,
    pub affected_file_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

/// 一次批量启用、禁用或卸载的汇总结果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchModOperationResult {
    pub action: BatchModAction,
    pub requested_count: usize,
    pub succeeded_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub affected_file_count: usize,
    pub items: Vec<BatchModOperationItem>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreModPlanItem {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub deployed_file_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreAllPlan {
    pub affected_mod_count: usize,
    pub deployed_file_count: usize,
    pub mods: Vec<RestoreModPlanItem>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreAllResult {
    pub affected_mod_count: usize,
    pub removed_deployed_file_count: usize,
    pub mods: Vec<RestoreModPlanItem>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictParticipant {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub order: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedModelTarget {
    pub model_kind: String,
    pub sub_kind: String,
    pub model_id: String,
    pub display_names: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictGroup {
    pub group_id: String,
    pub participant_count: usize,
    pub conflict_file_count: usize,
    pub conflict_files: Vec<String>,
    pub enabled_participant_count: usize,
    pub participants: Vec<ModConflictParticipant>,
    pub shared_model_targets: Vec<SharedModelTarget>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictReport {
    pub conflict_count: usize,
    pub conflict_file_count: usize,
    pub groups: Vec<ModConflictGroup>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictMoveResult {
    pub group_id: String,
    pub mod_id: String,
    pub direction: String,
    pub moved: bool,
    pub participant_order: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyConflictOrderPlan {
    pub group_id: String,
    pub conflict_file_count: usize,
    pub applicable_file_count: usize,
    pub enabled_participant_count: usize,
    pub requires_overwrite_confirmation: bool,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyConflictOrderResult {
    pub group_id: String,
    pub applied_file_count: usize,
    pub skipped_file_count: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadataUpdateResult {
    pub mod_id: String,
    pub name: String,
    pub original_name: String,
    pub note: String,
    pub category_ids: Vec<String>,
    pub categories: Vec<ModCategory>,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCategoryAssignment {
    pub mod_id: String,
    pub category_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCategoryBatchUpdateResult {
    pub mods: Vec<ModMetadataUpdateResult>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModCategory {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at_unix_seconds: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCategoryList {
    pub categories: Vec<ModCategory>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCategoryDeleteResult {
    pub category_id: String,
    pub affected_mod_count: usize,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadataPatch {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub category_ids: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledModManifest {
    schema_version: u32,
    id: String,
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    category_ids: Vec<String>,
    // schema 13 and earlier stored one optional user override. It is read only
    // so existing libraries can be migrated into the unified category list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category_override: Option<String>,
    source_path: String,
    /// 第三方盒子模块的稳定来源别名，不参与用户可见名称或普通同名判断。
    #[serde(default)]
    legacy_sources: Vec<LegacyBoxSourceRef>,
    content_root_path: String,
    detection_method: String,
    deploy_root: String,
    installed_at_unix_seconds: u64,
    enabled: bool,
    /// 已启用 MOD 的辅助提示；仅由游戏目录状态同步维护。
    #[serde(default)]
    partially_overridden: bool,
    file_count: usize,
    files: Vec<InstalledModFile>,
    #[serde(default)]
    model_replacements: Vec<ModelReplacement>,
    #[serde(default)]
    model_remaps: Vec<ModelRemapSelection>,
    /// 特效改绑仅保存被兼容索引验证过的本地武器槽位选择。
    #[serde(default)]
    effect_remaps: Vec<EffectRemapSelection>,
    /// AI 清理只改变部署选择；本地库中的原始文件永不删除。
    #[serde(default)]
    deployment_exclusions: Vec<ModDeploymentExclusion>,
    #[serde(default)]
    deployed_files: Vec<DeployedModFile>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LegacyBoxSourceRef {
    box_path: String,
    module_id: String,
    /// 盒子中的原始开关，用于保留导入时的初始状态，不替代后续手动实际状态检测。
    #[serde(default)]
    box_enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModCategoryStore {
    #[serde(default = "default_mod_category_store_schema_version")]
    schema_version: u32,
    #[serde(default)]
    categories: Vec<StoredModCategory>,
    #[serde(default)]
    suppressed_recognition_keys: Vec<String>,
}

impl Default for ModCategoryStore {
    fn default() -> Self {
        Self {
            schema_version: MOD_CATEGORY_STORE_SCHEMA_VERSION,
            categories: Vec::new(),
            suppressed_recognition_keys: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredModCategory {
    id: String,
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    created_at_unix_seconds: u64,
    #[serde(default)]
    recognition_keys: Vec<String>,
}

impl From<&StoredModCategory> for ModCategory {
    fn from(category: &StoredModCategory) -> Self {
        Self {
            id: category.id.clone(),
            name: category.name.clone(),
            parent_id: category.parent_id.clone(),
            created_at_unix_seconds: category.created_at_unix_seconds,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConflictOrderStore {
    #[serde(default = "default_conflict_order_schema_version")]
    schema_version: u32,
    #[serde(default)]
    orders: HashMap<String, Vec<String>>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModLibraryOrderStore {
    #[serde(default = "default_mod_library_order_store_schema_version")]
    schema_version: u32,
    /// `modIds` 是第一版字段名，反序列化后保留用户已有的手动顺序。
    #[serde(default, alias = "modIds")]
    manual_mod_ids: Vec<String>,
    #[serde(default)]
    import_mod_ids: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModBranchGroupStore {
    #[serde(default = "default_mod_branch_group_store_schema_version")]
    schema_version: u32,
    #[serde(default)]
    groups: Vec<ModBranchGroup>,
}

struct ConflictPathGroup {
    deploy_relative_path: String,
    participant_ids: Vec<String>,
}

fn default_conflict_order_schema_version() -> u32 {
    2
}

fn default_mod_library_order_store_schema_version() -> u32 {
    MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION
}

fn default_mod_branch_group_store_schema_version() -> u32 {
    MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION
}

fn default_mod_category_store_schema_version() -> u32 {
    MOD_CATEGORY_STORE_SCHEMA_VERSION
}

#[derive(Clone)]
struct Candidate {
    root_path: PathBuf,
    detection_method: &'static str,
    deploy_root: DeployRoot,
    depth: usize,
}

#[derive(Clone)]
enum DeployRoot {
    NativePc,
    NativePcChild(String),
    GameRoot,
}

struct ScanResult {
    directories: Vec<PathBuf>,
    warnings: Vec<String>,
}

#[derive(Clone)]
struct InstalledManifestContext {
    mod_path: PathBuf,
    content_path: PathBuf,
    manifest_path: PathBuf,
    manifest: InstalledModManifest,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredWorkspaceSnapshot {
    schema_version: u32,
    manifest_schema_version: u32,
    snapshot: ModWorkspaceSnapshot,
    /// 快照内保存有效部署路径索引，启停后可只重读受影响的 manifest 并重算冲突。
    mod_index: Vec<WorkspaceModIndexEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceModIndexEntry {
    mod_id: String,
    name: String,
    enabled: bool,
    effective_files: Vec<String>,
    model_replacements: Vec<ModelReplacement>,
}

#[derive(Clone)]
struct EffectiveInstalledModFile {
    installed_file: InstalledModFile,
    deploy_relative_path: String,
    texture_path_rewrites: BTreeMap<String, String>,
    evam_slinger_rewrite: Option<EvamSlingerIdRewrite>,
}

pub fn get_mod_library_status(app: &tauri::AppHandle) -> Result<ModLibraryStatus, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    initialize_import_staging(&paths)?;

    Ok(ModLibraryStatus {
        software_data_path: path_to_string(&paths.software_data_path),
        mods_path: path_to_string(&paths.mods_path),
        installed_path: path_to_string(&paths.installed_path),
        staging_path: path_to_string(&paths.staging_path),
        import_staging_path: path_to_string(&paths.import_staging_path),
        is_ready: true,
        message: "MOD library directories are ready.".to_string(),
    })
}

#[cfg(test)]
pub fn preview_mod_import(
    raw_path: String,
    allow_game_root: bool,
) -> Result<ModImportPreview, String> {
    preview_mod_import_with_progress(raw_path, allow_game_root, &OperationReporter::default())
}

pub fn preview_mod_import_with_progress(
    raw_path: String,
    allow_game_root: bool,
    progress: &OperationReporter,
) -> Result<ModImportPreview, String> {
    progress.report("正在扫描目录", 0, None, None);
    let source_path = normalize_user_path(&raw_path);

    if source_path.as_os_str().is_empty() {
        return Ok(invalid_preview(
            source_path,
            "Choose a MOD folder before previewing import rules.",
        ));
    }

    if !source_path.exists() {
        return Ok(invalid_preview(
            source_path,
            "Source directory does not exist.",
        ));
    }

    if !source_path.is_dir() {
        return Ok(invalid_preview(
            source_path,
            "Source path is not a directory.",
        ));
    }

    let source_path = source_path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve source directory {}: {error}",
            source_path.display()
        )
    })?;
    let scan = scan_directories(&source_path, progress)?;
    let candidates = detect_candidates(&source_path, &scan.directories);

    if let Some(preview) =
        preview_from_candidates(&source_path, candidates, scan.warnings.clone(), progress)?
    {
        return Ok(preview);
    }

    preview_game_root_fallback(&source_path, allow_game_root, scan.warnings, progress)
}

/// 文件夹只有在包含内嵌压缩包时才进入隔离暂存；普通文件夹继续使用原有快速预览。
pub fn preview_mod_import_source_with_nested(
    app: &tauri::AppHandle,
    raw_path: String,
    allow_game_root: bool,
    progress: &OperationReporter,
) -> Result<ModImportPreview, String> {
    let direct_preview =
        preview_mod_import_with_progress(raw_path.clone(), allow_game_root, progress)?;
    if direct_preview.status == "invalid" {
        return Ok(direct_preview);
    }

    let source = canonical_directory(&normalize_user_path(&raw_path), "MOD source")?;
    if collect_nested_archives(&source)?.is_empty() {
        return Ok(direct_preview);
    }

    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let import_staging_root = paths.import_staging_path.canonicalize().map_err(|error| {
        format!(
            "无法确认导入暂存目录 {}：{error}",
            paths.import_staging_path.display()
        )
    })?;
    if source.starts_with(&import_staging_root) || import_staging_root.starts_with(&source) {
        return Err("不能选择 Acumod 导入暂存目录或包含该目录的上级文件夹。".to_string());
    }
    initialize_import_staging(&paths)?;
    clear_import_staging(&paths.import_staging_path)?;
    let staging_path = paths
        .import_staging_path
        .join(unique_mod_id(&derive_mod_name(&source))?);
    fs::create_dir_all(&staging_path).map_err(|error| {
        format!(
            "无法创建文件夹导入暂存目录 {}：{error}",
            staging_path.display()
        )
    })?;

    if let Err(error) = copy_import_source_directory(&source, &staging_path, progress) {
        let _ = fs::remove_dir_all(&staging_path);
        return Err(error);
    }
    let preview =
        preview_archive_staging_with_nested(app, &staging_path, allow_game_root, progress);
    match preview {
        Ok(mut preview) => {
            preview.original_source_path = path_to_string(&source);
            Ok(preview)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_path);
            Err(error)
        }
    }
}

pub fn install_mod_from_folder_with_progress(
    app: &tauri::AppHandle,
    raw_path: String,
    allow_game_root: bool,
    preferred_name: Option<String>,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut result = install_mod_from_folder_into_with_options_and_progress(
        raw_path,
        allow_game_root,
        &paths.installed_path,
        preferred_name,
        None,
        progress,
    )?;
    append_workspace_snapshot_warning(
        &mut result.message,
        update_workspace_snapshot_after_import(&paths, &result.mod_id),
    );
    Ok(result)
}

pub fn install_mod_from_archive_with_progress(
    app: &tauri::AppHandle,
    raw_path: String,
    allow_game_root: bool,
    preferred_name: Option<String>,
    progress: &OperationReporter,
) -> Result<ModArchiveImportOutcome, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    progress.report("正在准备解包", 0, None, None);
    initialize_import_staging(&paths)?;
    clear_import_staging(&paths.import_staging_path)?;
    let archive_path = normalize_user_path(&raw_path);
    let archive_path = validate_archive_path(&archive_path)?;
    let archive_name = derive_mod_name(&archive_path);

    let staging_path = paths
        .import_staging_path
        .join(unique_mod_id(&archive_name)?);

    fs::create_dir_all(&staging_path).map_err(|error| {
        format!(
            "Could not create archive staging directory {}: {error}",
            staging_path.display()
        )
    })?;

    if let Err(error) =
        extract_archive_with_bundled_7zip(app, &archive_path, &staging_path, progress)
    {
        let _ = fs::remove_dir_all(&staging_path);
        return Err(error);
    }
    let preview =
        preview_archive_staging_with_nested(app, &staging_path, allow_game_root, progress)?;

    if preview.status == "ambiguous" {
        return Ok(ModArchiveImportOutcome {
            status: "ambiguous".to_string(),
            source_path: path_to_string(&staging_path),
            original_archive_path: path_to_string(&archive_path),
            message: preview.message.clone(),
            preview: Some(preview),
            install_result: None,
        });
    }

    let result = install_mod_from_folder_into_with_options_and_progress(
        path_to_string(&staging_path),
        allow_game_root,
        &paths.installed_path,
        Some(preferred_name.unwrap_or(archive_name)),
        Some(path_to_string(&archive_path)),
        progress,
    );

    match result {
        Ok(mut install_result) => {
            let cleanup_message = match fs::remove_dir_all(&staging_path) {
                Ok(()) => "Archive MOD import completed.".to_string(),
                Err(error) => format!(
                    "Archive MOD import completed, but staging cleanup failed at {}: {error}",
                    staging_path.display()
                ),
            };

            append_workspace_snapshot_warning(
                &mut install_result.message,
                update_workspace_snapshot_after_import(&paths, &install_result.mod_id),
            );
            Ok(ModArchiveImportOutcome {
                status: if install_result.already_installed {
                    "alreadyInstalled".to_string()
                } else {
                    "installed".to_string()
                },
                source_path: String::new(),
                original_archive_path: path_to_string(&archive_path),
                preview: None,
                install_result: Some(install_result),
                message: cleanup_message,
            })
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_path);
            Err(error)
        }
    }
}

fn preview_archive_staging_with_nested(
    app: &tauri::AppHandle,
    staging_path: &Path,
    allow_game_root: bool,
    progress: &OperationReporter,
) -> Result<ModImportPreview, String> {
    let outer_preview =
        preview_mod_import_with_progress(path_to_string(staging_path), allow_game_root, progress)?;
    let mut candidates = candidates_from_preview(&outer_preview, Vec::new());
    let mut warnings = outer_preview.warnings.clone();
    let mut queue = collect_nested_archives(staging_path)?
        .into_iter()
        .map(|archive_path| (archive_path, 1_usize, Vec::<String>::new()))
        .collect::<Vec<_>>();
    let has_nested_archives = !queue.is_empty();
    if has_nested_archives
        && outer_preview.status == "needsGameRootConfirmation"
        && !contains_non_archive_file(staging_path)?
    {
        // 只有内嵌压缩包时，外层目录中的压缩包文件不是可部署的游戏根目录内容。
        candidates.clear();
    }
    let nested_staging_root = staging_path.join(".acumod-nested");
    let mut processed_count = 0_usize;

    while !queue.is_empty() && processed_count < NESTED_ARCHIVE_MAX_COUNT {
        let (archive_path, depth, parent_chain) = queue.remove(0);
        processed_count += 1;
        let archive_label = archive_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_to_string(&archive_path));
        let mut archive_chain = parent_chain;
        archive_chain.push(archive_label.clone());
        let destination = nested_staging_root.join(format!(
            "{:02}-{}",
            processed_count,
            unique_mod_id(&derive_mod_name(&archive_path))?
        ));
        fs::create_dir_all(&destination).map_err(|error| {
            format!(
                "无法创建内嵌压缩包暂存目录 {}：{error}",
                destination.display()
            )
        })?;

        progress.report(
            "正在识别内嵌压缩包",
            processed_count,
            Some(NESTED_ARCHIVE_MAX_COUNT),
            Some(archive_chain.join(" > ")),
        );
        if let Err(error) =
            extract_archive_with_bundled_7zip(app, &archive_path, &destination, progress)
        {
            warnings.push(format!(
                "无法解开内嵌压缩包 {}，已跳过这个分支：{}",
                archive_chain.join(" > "),
                concise_archive_error(&error)
            ));
            let _ = fs::remove_dir_all(&destination);
            continue;
        }

        let preview =
            preview_mod_import_with_progress(path_to_string(&destination), false, progress)?;
        warnings.extend(preview.warnings.clone());
        candidates.extend(candidates_from_preview(&preview, archive_chain.clone()));

        if depth < NESTED_ARCHIVE_MAX_DEPTH {
            queue.extend(
                collect_nested_archives(&destination)?
                    .into_iter()
                    .map(|nested| (nested, depth + 1, archive_chain.clone())),
            );
        } else if !collect_nested_archives(&destination)?.is_empty() {
            warnings.push(format!(
                "{} 内仍包含更深层压缩包，已按两层递归上限停止。",
                archive_chain.join(" > ")
            ));
        }
    }

    if !queue.is_empty() {
        warnings.push(format!(
            "内嵌压缩包超过 {NESTED_ARCHIVE_MAX_COUNT} 个，仅识别了前 {NESTED_ARCHIVE_MAX_COUNT} 个。"
        ));
    }
    if !has_nested_archives && outer_preview.status != "needsGameRootConfirmation" {
        return Ok(outer_preview);
    }

    candidates.sort_by(|left, right| {
        left.archive_chain
            .cmp(&right.archive_chain)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    candidates.dedup_by(|left, right| left.root_path.eq_ignore_ascii_case(&right.root_path));
    if candidates.is_empty() {
        let mut preview = outer_preview;
        preview.warnings = warnings;
        return Ok(preview);
    }

    Ok(ModImportPreview {
        source_path: path_to_string(staging_path),
        original_source_path: path_to_string(staging_path),
        status: "ambiguous".to_string(),
        detection_method: "multipleCandidates".to_string(),
        deploy_root: "unknown".to_string(),
        content_root_path: None,
        requires_game_root_confirmation: false,
        message: format!(
            "识别到 {} 个可导入分支，请选择一个或多个。",
            candidates.len()
        ),
        file_count: 0,
        files: Vec::new(),
        candidates,
        warnings,
    })
}

fn candidates_from_preview(
    preview: &ModImportPreview,
    archive_chain: Vec<String>,
) -> Vec<ModImportCandidate> {
    if preview.status == "ambiguous" {
        return preview
            .candidates
            .iter()
            .cloned()
            .map(|mut candidate| {
                candidate.source_root_path = preview.source_path.clone();
                candidate.archive_chain = archive_chain.clone();
                candidate.suggested_name = suggested_candidate_name(
                    Path::new(&preview.source_path),
                    Path::new(&candidate.root_path),
                );
                candidate.relative_path =
                    candidate_display_path(&candidate.archive_chain, &candidate.relative_path);
                candidate
            })
            .collect();
    }
    if preview.status != "ready" && preview.status != "needsGameRootConfirmation" {
        return Vec::new();
    }
    let Some(root_path) = preview.content_root_path.clone() else {
        return Vec::new();
    };
    let root = PathBuf::from(&root_path);
    let relative_path = root
        .strip_prefix(Path::new(&preview.source_path))
        .map(path_to_string)
        .unwrap_or_default();
    let fallback_name = archive_chain
        .last()
        .map(|name| derive_mod_name(Path::new(name)))
        .unwrap_or_else(|| derive_mod_name(Path::new(&preview.source_path)));
    vec![ModImportCandidate {
        root_path,
        source_root_path: preview.source_path.clone(),
        relative_path: candidate_display_path(&archive_chain, &relative_path),
        suggested_name: if !archive_chain.is_empty() || relative_path.is_empty() {
            fallback_name
        } else {
            derive_mod_name(&root)
        },
        archive_chain,
        requires_game_root_confirmation: preview.requires_game_root_confirmation,
        detection_method: preview.detection_method.clone(),
        deploy_root: preview.deploy_root.clone(),
        file_count: preview.file_count,
    }]
}

fn candidate_display_path(archive_chain: &[String], relative_path: &str) -> String {
    let mut parts = archive_chain.to_vec();
    if !relative_path.is_empty() {
        parts.push(relative_path.to_string());
    }
    parts.join(" > ")
}

fn suggested_candidate_name(source_root: &Path, candidate_root: &Path) -> String {
    if file_name_equals(candidate_root, "nativepc") {
        if let Some(parent) = candidate_root.parent() {
            if parent != source_root {
                return derive_mod_name(parent);
            }
        }
    }
    derive_mod_name(candidate_root)
}

fn collect_nested_archives(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut archives = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("无法扫描内嵌压缩包目录 {}：{error}", directory.display()))?
        {
            let entry = entry.map_err(|error| {
                format!("无法读取内嵌压缩包目录项 {}：{error}", directory.display())
            })?;
            let path = entry.path();
            if path.is_dir() {
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".acumod-nested")
                {
                    stack.push(path);
                }
            } else if is_supported_archive_path(&path) {
                archives.push(path);
            }
        }
    }
    archives.sort_by_key(|path| path_to_string(path).to_lowercase());
    Ok(archives)
}

fn copy_import_source_directory(
    source_root: &Path,
    destination_root: &Path,
    progress: &OperationReporter,
) -> Result<(), String> {
    let mut source_files = Vec::new();
    let mut stack = vec![source_root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("无法读取导入目录 {}：{error}", directory.display()))?
        {
            let entry = entry
                .map_err(|error| format!("无法读取导入目录项 {}：{error}", directory.display()))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("无法读取导入文件信息 {}：{error}", path.display()))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                source_files.push(path);
            }
        }
    }
    source_files.sort_by_key(|path| path_to_string(path).to_lowercase());
    let total = source_files.len();
    progress.report("正在复制文件夹到导入暂存", 0, Some(total), None);
    for (index, source_path) in source_files.into_iter().enumerate() {
        let relative_path = source_path.strip_prefix(source_root).map_err(|error| {
            format!(
                "无法计算导入文件 {} 的相对路径：{error}",
                source_path.display()
            )
        })?;
        let destination_path = destination_root.join(relative_path);
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建导入暂存子目录 {}：{error}", parent.display()))?;
        }
        fs::copy(&source_path, &destination_path).map_err(|error| {
            format!(
                "无法复制 {} 到导入暂存 {}：{error}",
                source_path.display(),
                destination_path.display()
            )
        })?;
        progress.report(
            "正在复制文件夹到导入暂存",
            index + 1,
            Some(total),
            source_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
        );
    }
    Ok(())
}

fn contains_non_archive_file(root: &Path) -> Result<bool, String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("无法扫描压缩包外层目录 {}：{error}", directory.display()))?
        {
            let path = entry
                .map_err(|error| format!("无法读取压缩包外层目录项：{error}"))?
                .path();
            if path.is_dir() {
                if !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".acumod-nested")
                {
                    stack.push(path);
                }
            } else if path.is_file() && !is_supported_archive_path(&path) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn is_supported_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| matches!(extension.as_str(), "zip" | "7z" | "rar"))
}

fn concise_archive_error(error: &str) -> String {
    error.lines().next().unwrap_or(error).trim().to_string()
}

pub fn install_mod_from_candidate_with_progress(
    app: &tauri::AppHandle,
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let normalized_source_path = normalize_user_path(&source_path);
    let should_cleanup_staging = original_archive_path.is_some()
        && normalized_source_path
            .canonicalize()
            .ok()
            .zip(paths.import_staging_path.canonicalize().ok())
            .map(|(source, staging_root)| source.starts_with(staging_root))
            .unwrap_or(false);
    let result = install_mod_from_candidate_into_with_progress(
        source_path,
        candidate_root_path,
        original_archive_path,
        &paths.installed_path,
        progress,
    );

    match result {
        Ok(mut install_result) => {
            if should_cleanup_staging {
                match fs::remove_dir_all(&normalized_source_path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => {
                        install_result.message = format!(
                            "MOD was installed, but archive staging cleanup failed at {}: {error}",
                            normalized_source_path.display()
                        );
                    }
                }
            }

            append_workspace_snapshot_warning(
                &mut install_result.message,
                update_workspace_snapshot_after_import(&paths, &install_result.mod_id),
            );

            Ok(install_result)
        }
        Err(error) => Err(error),
    }
}

/// 一次安装一个或多个候选分支；每个分支仍生成独立 manifest，并可独立启停。
pub fn install_mod_branches_with_progress(
    app: &tauri::AppHandle,
    source_path: String,
    selections: Vec<ModBranchImportSelection>,
    original_source_path: Option<String>,
    group_name: Option<String>,
    as_branch_group: bool,
    progress: &OperationReporter,
) -> Result<ModBranchImportResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    if selections.is_empty() {
        return Err("请至少选择一个要导入的 MOD 分支。".to_string());
    }

    let source = canonical_directory(&normalize_user_path(&source_path), "candidate source")?;
    let staging_root = paths.import_staging_path.canonicalize().map_err(|error| {
        format!(
            "无法确认导入暂存目录 {}：{error}",
            paths.import_staging_path.display()
        )
    })?;
    let should_cleanup_staging =
        original_source_path.is_some() && source.starts_with(&staging_root);

    let original_source = original_source_path
        .as_deref()
        .map(normalize_user_path)
        .unwrap_or_else(|| source.clone());
    let mut contexts = load_all_installed_manifests(&paths.installed_path)?;
    let mut results = Vec::new();
    let mut newly_installed_ids = Vec::new();

    let import_result = (|| -> Result<(), String> {
        for (index, selection) in selections.into_iter().enumerate() {
            let branch_name = validate_mod_branch_group_name(&selection.branch_name)?;
            let candidate = canonical_directory(
                &normalize_user_path(&selection.candidate_root_path),
                "candidate content root",
            )?;
            if !candidate.starts_with(&source) {
                return Err("所选分支不属于当前导入来源目录。".to_string());
            }
            let mut preview =
                preview_mod_import_with_progress(path_to_string(&candidate), false, progress)?;
            if preview.requires_game_root_confirmation && selection.allow_game_root {
                preview =
                    preview_mod_import_with_progress(path_to_string(&candidate), true, progress)?;
            }
            if preview.status != "ready" {
                return Err(format!(
                    "分支“{branch_name}”当前无法导入：{}",
                    preview.message
                ));
            }
            progress.report("正在导入 MOD 分支", index, None, Some(branch_name.clone()));

            let result = match find_installed_mod_by_content_with_options(
                &contexts,
                &candidate,
                selection.allow_game_root,
                progress,
            )? {
                Some(existing) => existing,
                None => {
                    let installed =
                        install_mod_from_folder_into_with_options_and_progress_allow_same_name(
                            path_to_string(&candidate),
                            selection.allow_game_root,
                            &paths.installed_path,
                            Some(branch_name),
                            Some(path_to_string(&original_source)),
                            progress,
                        )?;
                    newly_installed_ids.push(installed.mod_id.clone());
                    contexts.push(load_installed_manifest(
                        &paths.installed_path,
                        &installed.mod_id,
                    )?);
                    installed
                }
            };
            results.push(result);
        }
        Ok(())
    })();
    if let Err(error) = import_result {
        for mod_id in newly_installed_ids {
            let _ = fs::remove_dir_all(paths.installed_path.join(mod_id));
        }
        invalidate_workspace_snapshot(&paths.installed_path);
        return Err(error);
    }

    let result = (|| {
        let mut unique_mod_ids = Vec::new();
        let mut seen = HashSet::new();
        for install_result in &results {
            if seen.insert(install_result.mod_id.clone()) {
                unique_mod_ids.push(install_result.mod_id.clone());
            }
        }
        let group = if as_branch_group && unique_mod_ids.len() >= 2 {
            let fallback_group_name = derive_mod_name(&original_source);
            let name = validate_mod_branch_group_name(
                group_name.as_deref().unwrap_or(&fallback_group_name),
            )?;
            let (group, groups) =
                create_mod_branch_group_from(&paths.installed_path, name, unique_mod_ids)?;
            update_workspace_branch_groups_snapshot(&paths, &groups)?;
            Some(group)
        } else {
            None
        };
        for install_result in &results {
            update_workspace_snapshot_after_import(&paths, &install_result.mod_id)?;
        }
        if group.is_some() {
            let store = read_mod_branch_group_store(&paths.installed_path)?;
            update_workspace_branch_groups_snapshot(&paths, &store.groups)?;
        }
        Ok(ModBranchImportResult {
            group,
            message: format!("已处理 {} 个 MOD 分支。", results.len()),
            install_results: results,
        })
    })();

    if result.is_err() {
        // 分支批量导入失败时只回滚本次新建且尚未部署的本地副本，不碰既有 MOD。
        for mod_id in newly_installed_ids {
            let _ = fs::remove_dir_all(paths.installed_path.join(mod_id));
        }
        invalidate_workspace_snapshot(&paths.installed_path);
        return result;
    }

    let mut result = result?;
    if should_cleanup_staging {
        if let Err(error) = fs::remove_dir_all(&source) {
            if error.kind() != ErrorKind::NotFound {
                result
                    .message
                    .push_str(&format!(" 导入已完成，但暂存目录清理失败：{error}"));
            }
        }
    }
    Ok(result)
}

/// 将狩技 MOD 盒子中的所选模块复制到 Acumod 本地库，并自动检测游戏目录实际状态。
pub fn import_legacy_box_mods_with_progress(
    app: &tauri::AppHandle,
    box_path: String,
    module_ids: Vec<String>,
    progress: &OperationReporter,
) -> Result<LegacyBoxImportResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let sources = legacy_box::load_legacy_box_import_sources(&box_path, &module_ids)?;
    let total = sources.len();
    let mut items = Vec::with_capacity(total);
    let mut imported_count = 0;
    let mut already_installed_count = 0;
    // 批量导入期间复用同一份清单索引，避免每个盒子模块都重新扫描整个本地库。
    let mut installed_contexts = load_all_installed_manifests_with_progress_phase(
        &paths.installed_path,
        progress,
        "正在建立本地 MOD 索引",
    )?;

    for (index, source) in sources.into_iter().enumerate() {
        progress.report(
            "正在导入狩技 MOD 盒子内容",
            index,
            Some(total),
            Some(legacy_box::import_source_name(&source).to_string()),
        );
        let module_id = legacy_box::import_source_module_id(&source).to_string();
        let name = legacy_box::import_source_name(&source).to_string();
        let source_ref = LegacyBoxSourceRef {
            box_path: path_to_string(legacy_box::import_source_box_path(&source)),
            module_id: module_id.clone(),
            box_enabled: legacy_box::import_source_box_enabled(&source),
        };
        // 盒子模块先按已保存来源、再按完整内容关联。这样同名但内容不同的 MOD 不会被误吞掉。
        let existing = match find_installed_mod_by_legacy_source(&installed_contexts, &source_ref)?
        {
            Some(existing) => Some(existing),
            // 狩技盒子的 files/ 已经是相对游戏根目录的部署结构；其中 DLL 等加载器
            // 不会位于 nativePC，内容关联时也必须使用同一映射，避免整批导入中断。
            None => find_installed_mod_by_content_with_options(
                &installed_contexts,
                legacy_box::import_source_files_path(&source),
                true,
                progress,
            )?,
        };
        let result = match existing {
            Some(result) => Ok(result),
            None => install_mod_from_folder_into_with_options_and_progress_allow_same_name(
                path_to_string(legacy_box::import_source_files_path(&source)),
                true,
                &paths.installed_path,
                Some(name.clone()),
                Some(path_to_string(legacy_box::import_source_module_path(
                    &source,
                ))),
                progress,
            ),
        };

        match result {
            Ok(result) => {
                let associated_context = match associate_legacy_box_source(
                    &paths.installed_path,
                    &result.mod_id,
                    source_ref,
                    legacy_box::import_source_box_enabled(&source),
                ) {
                    Ok(context) => context,
                    Err(error) => {
                        items.push(LegacyBoxImportItem {
                            module_id,
                            name,
                            status: "failed".to_string(),
                            mod_id: Some(result.mod_id),
                            message: format!("本地副本已存在，但无法保存盒子来源关联：{error}"),
                        });
                        progress.report("正在导入狩技 MOD 盒子内容", index + 1, Some(total), None);
                        continue;
                    }
                };
                upsert_installed_manifest_context(&mut installed_contexts, associated_context);

                if result.already_installed {
                    already_installed_count += 1;
                    items.push(LegacyBoxImportItem {
                        module_id,
                        name,
                        status: "alreadyInstalled".to_string(),
                        mod_id: Some(result.mod_id),
                        message: "已关联到内容相同的本地 MOD，未重复保存文件。".to_string(),
                    });
                } else {
                    imported_count += 1;
                    items.push(LegacyBoxImportItem {
                        module_id,
                        name,
                        status: "imported".to_string(),
                        mod_id: Some(result.mod_id),
                        message: "已复制到 Acumod 本地 MOD 库，已沿用盒子记录的启用状态。"
                            .to_string(),
                    });
                }
            }
            Err(error) => items.push(LegacyBoxImportItem {
                module_id,
                name,
                status: "failed".to_string(),
                mod_id: None,
                message: error,
            }),
        }
        progress.report("正在导入狩技 MOD 盒子内容", index + 1, Some(total), None);
    }

    // 导入时优先保留盒子记录的开关；实际文件比对改由用户显式触发，避免外部部署状态覆盖来源意图。
    let mut state_sync = ModStateSyncResult::unavailable(
        "已沿用狩技 MOD 盒子的启用状态；如需以游戏目录实际文件为准，请手动点击“检测游戏实际状态”。"
            .to_string(),
    );
    if let Err(error) =
        save_workspace_snapshot_from_contexts(&paths.installed_path, &installed_contexts, progress)
    {
        state_sync
            .warnings
            .push(format!("工作区快照更新失败，下次刷新时会重新生成：{error}"));
    }

    let failed_count = items.iter().filter(|item| item.status == "failed").count();
    let message = format!(
        "狩技 MOD 盒子导入完成：新增 {imported_count} 个，已关联 {already_installed_count} 个，失败 {failed_count} 个。{}",
        state_sync.message
    );
    Ok(LegacyBoxImportResult {
        items,
        imported_count,
        already_installed_count,
        failed_count,
        state_sync,
        message,
    })
}

/// 重新比较已关联狩技 MOD 盒子的本地 MOD 与当前游戏目录；不会写入游戏文件。
pub fn refresh_game_mod_states_with_progress(
    app: &tauri::AppHandle,
    progress: &OperationReporter,
) -> Result<ModStateSyncResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    synchronize_legacy_mod_states_with_progress(&paths.installed_path, &game_root, progress)
}

fn synchronize_legacy_mod_states_with_progress(
    installed_root: &Path,
    game_root: &Path,
    progress: &OperationReporter,
) -> Result<ModStateSyncResult, String> {
    progress.report("正在读取状态检测清单", 0, None, None);
    let mut contexts = load_all_installed_manifests_with_progress_phase(
        installed_root,
        progress,
        "正在读取状态检测清单",
    )?;
    let mut warnings = Vec::new();
    let mut inputs = Vec::new();
    let detectable_context_count = contexts
        .iter()
        .filter(|context| {
            let is_trusted_enabled = context.manifest.enabled
                && context
                    .manifest
                    .deployed_files
                    .iter()
                    .any(is_copied_deployment_file);
            is_trusted_enabled || !context.manifest.legacy_sources.is_empty()
        })
        .count();
    let mut prepared_mods = Vec::with_capacity(detectable_context_count);
    let mut total_file_count = 0;
    let mut prepared_context_count = 0;

    // 先生成全部有效文件，得到准确总数后再进入逐文件比对阶段。
    for (context_index, context) in contexts.iter().enumerate() {
        let is_trusted_enabled = context.manifest.enabled
            && context
                .manifest
                .deployed_files
                .iter()
                .any(is_copied_deployment_file);
        let is_candidate = !context.manifest.legacy_sources.is_empty() && !is_trusted_enabled;
        if !is_trusted_enabled && !is_candidate {
            continue;
        }

        let effective_files = match effective_installed_files_for_context(context) {
            Ok(files) => files,
            Err(error) => {
                warnings.push(format!(
                    "无法读取 {} 的有效部署文件，状态保持未启用：{error}",
                    manifest_display_name(&context.manifest)
                ));
                Vec::new()
            }
        };
        let mut seen_path_keys = HashSet::new();
        let mut unique_effective_files = Vec::with_capacity(effective_files.len());
        for effective_file in effective_files {
            let path_key = conflict_path_key(&effective_file.deploy_relative_path);
            if !seen_path_keys.insert(path_key.clone()) {
                warnings.push(format!(
                    "{} 的有效部署路径重复，状态检测只保留第一个：{}",
                    manifest_display_name(&context.manifest),
                    effective_file.deploy_relative_path
                ));
                continue;
            }
            unique_effective_files.push(effective_file);
        }
        total_file_count += unique_effective_files.len();
        prepared_context_count += 1;
        progress.report(
            "正在准备游戏状态检测",
            prepared_context_count,
            Some(detectable_context_count),
            Some(manifest_display_name(&context.manifest)),
        );
        prepared_mods.push((
            context_index,
            is_candidate,
            is_trusted_enabled,
            unique_effective_files,
        ));
    }

    let mut compared_file_count = 0;
    progress.report("正在检测游戏实际状态", 0, Some(total_file_count), None);

    for (context_index, is_candidate, is_trusted_enabled, effective_files) in prepared_mods {
        let context = &contexts[context_index];
        let mut files = Vec::with_capacity(effective_files.len());

        for effective_file in effective_files {
            let path_key = conflict_path_key(&effective_file.deploy_relative_path);
            let matches_game_directory =
                match relative_string_to_path(&effective_file.deploy_relative_path) {
                    Ok(relative_path) => {
                        let target_path = game_root.join(relative_path);
                        if !target_path.is_file() {
                            false
                        } else {
                            match effective_file_matches_target(
                                context,
                                &effective_file,
                                &target_path,
                            ) {
                                Ok(matches) => matches,
                                Err(error) => {
                                    warnings.push(format!(
                                        "无法比较 {} 的游戏文件 {}：{error}",
                                        manifest_display_name(&context.manifest),
                                        target_path.display()
                                    ));
                                    false
                                }
                            }
                        }
                    }
                    Err(error) => {
                        warnings.push(format!(
                            "{} 的部署路径无效，无法检测：{error}",
                            manifest_display_name(&context.manifest)
                        ));
                        false
                    }
                };
            compared_file_count += 1;
            progress.report(
                "正在检测游戏实际状态",
                compared_file_count,
                Some(total_file_count),
                Some(format!(
                    "{} · {}",
                    manifest_display_name(&context.manifest),
                    effective_file.deploy_relative_path
                )),
            );
            let has_existing_record = context
                .manifest
                .deployed_files
                .iter()
                .any(|record| conflict_path_key(&record.deploy_relative_path) == path_key);
            files.push(ModStateSyncFile {
                path_key,
                matches_game_directory,
                has_existing_record,
            });
        }

        if is_trusted_enabled && files.iter().any(|file| !file.matches_game_directory) {
            // 主动部署状态不在这里被降级，但内容已经漂移时不能用它解释不一致路径。
            // 记录警告让用户知道后续应通过正常启停或冲突应用重新部署。
            warnings.push(format!(
                "{} 的部分主动部署记录与游戏目录不一致，已保留当前状态，但不会作为不一致文件的覆盖依据。",
                manifest_display_name(&context.manifest)
            ));
        }

        inputs.push(ModStateSyncInput {
            mod_id: context.manifest.id.clone(),
            is_candidate,
            is_trusted_enabled,
            files,
        });
    }

    progress.report("正在分析实际覆盖关系", 0, None, None);
    let plan = mod_state_sync::analyze_mod_states(&inputs);
    for group in &plan.mixed_conflict_groups {
        warnings.push(format!(
            "检测到无法还原为整体优先级的混合覆盖：{}。相关 MOD 已保持未启用。",
            group.join("、")
        ));
    }

    apply_mod_state_sync_plan(installed_root, game_root, &mut contexts, &plan, progress)?;
    progress.report("正在更新工作区快照", 0, None, None);
    if let Err(error) = save_workspace_snapshot_from_contexts(installed_root, &contexts, progress) {
        warnings.push(format!("工作区快照更新失败，下次刷新时会重新生成：{error}"));
    }
    Ok(plan.to_result(warnings))
}

fn apply_mod_state_sync_plan(
    installed_root: &Path,
    game_root: &Path,
    contexts: &mut [InstalledManifestContext],
    plan: &ModStateSyncPlan,
    progress: &OperationReporter,
) -> Result<(), String> {
    if plan.mod_states.is_empty() {
        return Ok(());
    }

    let states_by_mod_id = plan
        .mod_states
        .iter()
        .map(|state| (state.mod_id.as_str(), state))
        .collect::<HashMap<_, _>>();
    let deployed_at = unix_seconds_now()?;
    let mut changed_context_ids = HashSet::new();

    progress.report(
        "正在整理检测到的部署记录",
        0,
        Some(plan.mod_states.len()),
        None,
    );
    let mut processed_state_count = 0;
    for context in contexts.iter_mut() {
        let Some(state) = states_by_mod_id.get(context.manifest.id.as_str()) else {
            continue;
        };

        // 同步只管理观察所得记录。主动复制留下的记录永远不在这里被猜测或删除。
        context
            .manifest
            .deployed_files
            .retain(is_copied_deployment_file);
        context.manifest.enabled = state.enabled;
        context.manifest.partially_overridden = state.enabled && state.partially_overridden;

        if state.enabled {
            let effective_files = effective_installed_files_for_context(context)?;
            let files_by_path = effective_files
                .into_iter()
                .map(|file| (conflict_path_key(&file.deploy_relative_path), file))
                .collect::<HashMap<_, _>>();
            for path_key in &state.observed_path_keys {
                let file = files_by_path
                    .get(path_key)
                    .ok_or_else(|| format!("状态同步记录缺少有效部署文件：{path_key}"))?;
                let target_path =
                    game_root.join(relative_string_to_path(&file.deploy_relative_path)?);
                context.manifest.deployed_files.push(DeployedModFile {
                    deploy_relative_path: file.deploy_relative_path.clone(),
                    deployed_path: path_to_string(&target_path),
                    deployed_at_unix_seconds: deployed_at,
                    deployment_origin: DeploymentOrigin::Observed,
                    is_adopted: false,
                });
            }
        }

        context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
        changed_context_ids.insert(context.manifest.id.clone());
        processed_state_count += 1;
        progress.report(
            "正在整理检测到的部署记录",
            processed_state_count,
            Some(plan.mod_states.len()),
            Some(manifest_display_name(&context.manifest)),
        );
    }

    let mut conflict_store = read_conflict_order_store(installed_root)?;
    update_conflict_orders_from_state_sync(&mut conflict_store, contexts, plan)?;

    progress.report("正在保存游戏状态", 0, Some(changed_context_ids.len()), None);
    let mut saved_count = 0;
    for context in contexts.iter() {
        if !changed_context_ids.contains(&context.manifest.id) {
            continue;
        }
        save_manifest(&context.manifest_path, &context.manifest)?;
        saved_count += 1;
        progress.report(
            "正在保存游戏状态",
            saved_count,
            Some(changed_context_ids.len()),
            Some(manifest_display_name(&context.manifest)),
        );
    }
    save_conflict_order_store(installed_root, &conflict_store)?;
    Ok(())
}

fn update_conflict_orders_from_state_sync(
    store: &mut ConflictOrderStore,
    contexts: &[InstalledManifestContext],
    plan: &ModStateSyncPlan,
) -> Result<(), String> {
    let observed_enabled_ids = plan
        .mod_states
        .iter()
        .filter(|state| state.enabled)
        .map(|state| state.mod_id.as_str())
        .collect::<HashSet<_>>();
    if observed_enabled_ids.is_empty() {
        return Ok(());
    }

    let report = build_mod_conflict_report(contexts, store)?;
    for group in report.groups {
        let participant_ids = group
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();
        if !participant_ids
            .iter()
            .any(|mod_id| observed_enabled_ids.contains(mod_id.as_str()))
        {
            continue;
        }
        let participant_set = participant_ids.iter().cloned().collect::<HashSet<_>>();
        let edges = plan
            .priority_edges
            .iter()
            .filter(|edge| {
                participant_set.contains(&edge.winner_mod_id)
                    && participant_set.contains(&edge.covered_mod_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let existing_order = store.orders.get(&group.group_id);
        let order = stable_priority_order(&participant_ids, existing_order, &edges)?;
        store.orders.insert(group.group_id, order);
    }
    Ok(())
}

fn stable_priority_order(
    participant_ids: &[String],
    existing_order: Option<&Vec<String>>,
    edges: &[ModStateSyncPriorityEdge],
) -> Result<Vec<String>, String> {
    let participant_set = participant_ids.iter().cloned().collect::<HashSet<_>>();
    let mut rank_by_id = HashMap::new();
    if let Some(existing_order) = existing_order {
        for (index, mod_id) in existing_order.iter().enumerate() {
            if participant_set.contains(mod_id) {
                rank_by_id.insert(mod_id.clone(), index);
            }
        }
    }
    let fallback_rank = rank_by_id.len();
    let mut indegree = participant_ids
        .iter()
        .map(|mod_id| (mod_id.clone(), 0usize))
        .collect::<HashMap<_, _>>();
    let mut targets_by_winner = HashMap::<String, BTreeSet<String>>::new();
    for edge in edges {
        if edge.winner_mod_id == edge.covered_mod_id
            || !participant_set.contains(&edge.winner_mod_id)
            || !participant_set.contains(&edge.covered_mod_id)
        {
            continue;
        }
        if targets_by_winner
            .entry(edge.winner_mod_id.clone())
            .or_default()
            .insert(edge.covered_mod_id.clone())
        {
            *indegree
                .get_mut(&edge.covered_mod_id)
                .expect("participant indegree must exist") += 1;
        }
    }

    let mut available = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(mod_id, _)| mod_id.clone())
        .collect::<Vec<_>>();
    let mut order = Vec::with_capacity(participant_ids.len());
    while !available.is_empty() {
        available.sort_by(|left, right| {
            rank_by_id
                .get(left)
                .copied()
                .unwrap_or(fallback_rank)
                .cmp(&rank_by_id.get(right).copied().unwrap_or(fallback_rank))
                .then_with(|| left.cmp(right))
        });
        let current = available.remove(0);
        order.push(current.clone());
        if let Some(targets) = targets_by_winner.get(&current) {
            for target in targets {
                let degree = indegree
                    .get_mut(target)
                    .expect("participant indegree must exist");
                *degree -= 1;
                if *degree == 0 {
                    available.push(target.clone());
                }
            }
        }
    }

    if order.len() != participant_ids.len() {
        return Err("状态同步得到的覆盖关系存在循环，未写入冲突顺序。".to_string());
    }
    Ok(order)
}

fn is_copied_deployment_file(file: &DeployedModFile) -> bool {
    file.deployment_origin == DeploymentOrigin::Copied && !file.is_adopted
}

/// 测试时通过固定安装根目录执行候选分支导入，避免依赖应用目录。
#[cfg(test)]
fn install_mod_from_candidate_into(
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
    installed_root: &Path,
) -> Result<ModInstallResult, String> {
    install_mod_from_candidate_into_with_progress(
        source_path,
        candidate_root_path,
        original_archive_path,
        installed_root,
        &OperationReporter::default(),
    )
}

fn install_mod_from_candidate_into_with_progress(
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
    installed_root: &Path,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    let source = canonical_directory(&normalize_user_path(&source_path), "candidate source")?;
    let candidate = canonical_directory(
        &normalize_user_path(&candidate_root_path),
        "candidate content root",
    )?;
    let preview = preview_mod_import_with_progress(path_to_string(&source), false, progress)?;
    let selected = preview.candidates.iter().any(|entry| {
        PathBuf::from(&entry.root_path)
            .canonicalize()
            .map(|path| path == candidate)
            .unwrap_or(false)
    });

    if preview.status != "ambiguous" || !selected {
        return Err(
            "The selected content root is not a current candidate of this MOD source.".to_string(),
        );
    }

    let original_source = original_archive_path
        .as_deref()
        .map(normalize_user_path)
        .unwrap_or_else(|| source.clone());
    let preferred_name = derive_mod_name(&original_source);
    install_mod_from_folder_into_with_options_and_progress(
        path_to_string(&candidate),
        false,
        installed_root,
        Some(preferred_name),
        Some(path_to_string(&original_source)),
        progress,
    )
}

pub fn list_installed_mods(app: &tauri::AppHandle) -> Result<InstalledModList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    list_installed_mods_from(&paths.installed_path)
}

/// 只读取一个已安装 MOD 的 manifest，并把每个清单文件解析为受控本地路径。
/// Agent 和前端均只传稳定 MOD ID，不能提交任意文件系统路径。
pub(crate) fn load_mod_analysis_input(
    app: &tauri::AppHandle,
    mod_id: &str,
) -> Result<ModAnalysisInput, String> {
    let paths = library_paths(app)?;
    load_mod_analysis_input_from(&paths.installed_path, mod_id)
}

/// 测试和离线审计可直接指定受控的 installed 根目录，不需要构造 Tauri 窗口。
pub(crate) fn load_mod_analysis_input_from(
    installed_root: &Path,
    mod_id: &str,
) -> Result<ModAnalysisInput, String> {
    let context = load_installed_manifest(installed_root, mod_id.trim())?;
    let original_replacements =
        model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    let model_replacements = effective_model_replacements_for_context(
        &context,
        &context.manifest,
        &original_replacements,
    )?;
    let effective_files = effective_remap_files_for_context_with_manifest(
        &context,
        &context.manifest,
        &original_replacements,
    )?;
    if effective_files.len() != context.manifest.files.len() {
        return Err("模型改绑后的文件索引与 MOD 清单不一致。".to_string());
    }
    let excluded_paths = context
        .manifest
        .deployment_exclusions
        .iter()
        .map(|item| conflict_path_key(&item.library_relative_path))
        .collect::<HashSet<_>>();
    let mut files = Vec::with_capacity(context.manifest.files.len());
    for (index, installed_file) in context.manifest.files.iter().enumerate() {
        let source_path = source_path_for_installed_file(&context, installed_file)?;
        let size_bytes = fs::metadata(&source_path)
            .map_err(|error| format!("无法读取 MOD 文件大小：{error}"))?
            .len();
        files.push(ModAnalysisInputFile {
            source_path,
            library_relative_path: installed_file.library_relative_path.clone(),
            source_deploy_relative_path: installed_file.deploy_relative_path.clone(),
            effective_deploy_relative_path: effective_files[index].deploy_relative_path.clone(),
            size_bytes,
            excluded_from_deployment: excluded_paths
                .contains(&conflict_path_key(&installed_file.library_relative_path))
                || effective_files[index].automatic_exclusion_reason.is_some(),
        });
    }
    files.sort_by(|left, right| {
        left.effective_deploy_relative_path
            .to_lowercase()
            .cmp(&right.effective_deploy_relative_path.to_lowercase())
    });
    Ok(ModAnalysisInput {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        files,
        model_replacements,
    })
}

pub fn get_mod_workspace_snapshot_with_progress(
    app: &tauri::AppHandle,
    progress: &OperationReporter,
) -> Result<ModWorkspaceSnapshot, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;

    progress.report("正在读取工作区快照", 0, None, None);
    if let Some(stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? {
        return Ok(stored.snapshot);
    }

    refresh_mod_workspace_snapshot_with_progress(app, progress)
}

/// 扫描本地库中的非游戏资源候选。扫描只读取元数据，不修改本地库或游戏目录。
pub fn scan_mod_cleanup_candidates_with_progress(
    app: &tauri::AppHandle,
    progress: &OperationReporter,
) -> Result<ModCleanupScan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    progress.report("正在读取 MOD 清单", 0, None, None);
    let contexts = load_all_installed_manifests_with_progress(&paths.installed_path, progress)?;
    scan_mod_cleanup_candidates_from(&contexts, progress)
}

/// 只重新盘点指定 MOD，用于清理计划的状态版本校验，避免确认阶段再次扫描整个库。
pub fn scan_mod_cleanup_candidates_for_mod_ids(
    app: &tauri::AppHandle,
    mod_ids: &[String],
) -> Result<ModCleanupScan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut seen = HashSet::new();
    let contexts = mod_ids
        .iter()
        .filter(|mod_id| seen.insert(mod_id.as_str()))
        .map(|mod_id| load_installed_manifest(&paths.installed_path, mod_id))
        .collect::<Result<Vec<_>, _>>()?;
    scan_mod_cleanup_candidates_from(&contexts, &OperationReporter::default())
}

/// 读取清理审查中的安全纯文本片段；不会返回绝对路径或读取二进制内容。
pub fn read_mod_cleanup_text_preview(
    app: &tauri::AppHandle,
    mod_id: String,
    candidate_id: String,
) -> Result<ModCleanupTextPreview, String> {
    const MAX_TEXT_BYTES: u64 = 32 * 1024;

    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let context = load_installed_manifest(&paths.installed_path, mod_id.trim())?;
    let file = context
        .manifest
        .files
        .iter()
        .find(|file| {
            cleanup_candidate_id(&context.manifest.id, &file.library_relative_path)
                == candidate_id.trim()
        })
        .ok_or_else(|| "清理候选已经变化，请重新扫描。".to_string())?;
    let extension = cleanup_file_extension(&file.library_relative_path);
    if !mod_cleanup_rules()?
        .safe_text_extensions
        .contains(&extension)
    {
        return Err("该文件不是允许读取的安全文本类型。".to_string());
    }

    let source_path = source_path_for_installed_file(&context, file)?;
    let mut bytes = Vec::new();
    fs::File::open(&source_path)
        .map_err(|error| format!("无法打开清理候选文本：{error}"))?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取清理候选文本：{error}"))?;
    let truncated = bytes.len() as u64 > MAX_TEXT_BYTES;
    bytes.truncate(MAX_TEXT_BYTES as usize);
    let content = redact_cleanup_text(decode_cleanup_text(&bytes)?);
    Ok(ModCleanupTextPreview {
        candidate_id,
        library_relative_path: file.library_relative_path.clone(),
        content,
        truncated,
    })
}

/// 返回当前全部部署排除项，用于生成恢复计划。
pub fn list_mod_cleanup_exclusions(
    app: &tauri::AppHandle,
) -> Result<ModCleanupExclusionList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let contexts = load_all_installed_manifests(&paths.installed_path)?;
    Ok(mod_cleanup_exclusion_list_from(&contexts))
}

/// 保存部署排除项，并协调当前游戏目录中由这些路径产生的文件。
pub fn apply_mod_cleanup_exclusions_with_progress(
    app: &tauri::AppHandle,
    batch_id: String,
    selections: Vec<ModCleanupSelection>,
    progress: &OperationReporter,
) -> Result<ModCleanupApplyResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    apply_mod_cleanup_exclusions_from_with_progress(
        &paths, &game_root, batch_id, selections, progress,
    )
}

/// 移除指定部署排除项；已启用 MOD 的对应路径会按当前冲突优先级重新协调。
pub fn restore_mod_cleanup_exclusions_with_progress(
    app: &tauri::AppHandle,
    candidate_ids: Vec<String>,
    progress: &OperationReporter,
) -> Result<ModCleanupRestoreResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    restore_mod_cleanup_exclusions_from_with_progress(&paths, &game_root, candidate_ids, progress)
}

pub fn refresh_mod_workspace_snapshot_with_progress(
    app: &tauri::AppHandle,
    progress: &OperationReporter,
) -> Result<ModWorkspaceSnapshot, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;

    progress.report("正在读取 MOD 分类", 0, None, None);
    let category_store = load_or_initialize_mod_category_store(&paths)?;
    progress.report("正在读取 MOD 清单", 0, None, None);
    let contexts = load_all_installed_manifests_with_progress(&paths.installed_path, progress)?;
    let snapshot = build_workspace_snapshot_from_contexts(
        &paths.installed_path,
        &contexts,
        &category_store,
        progress,
    )?;
    let mod_index = build_workspace_mod_index(&contexts)?;
    save_workspace_snapshot(&paths.workspace_snapshot_path, &snapshot, mod_index)?;
    Ok(snapshot)
}

fn build_workspace_snapshot_from_contexts(
    installed_root: &Path,
    contexts: &[InstalledManifestContext],
    category_store: &ModCategoryStore,
    progress: &OperationReporter,
) -> Result<ModWorkspaceSnapshot, String> {
    let installed_mods =
        installed_mod_list_from_contexts(installed_root, contexts, category_store, progress, true)?;
    progress.report("正在分析冲突信息", 0, None, None);
    let conflict_store = read_conflict_order_store(installed_root)?;
    let conflict_report = build_mod_conflict_report(contexts, &conflict_store)?;
    let categories = sorted_mod_categories(&category_store.categories);
    let installed_ids = contexts
        .iter()
        .map(|context| context.manifest.id.clone())
        .collect::<HashSet<_>>();
    let branch_groups = load_normalized_mod_branch_groups(installed_root, &installed_ids)?;

    Ok(ModWorkspaceSnapshot {
        installed_mods,
        categories: ModCategoryList {
            message: format!("共有 {} 个分类。", categories.len()),
            categories,
        },
        conflict_report,
        branch_groups,
    })
}

fn save_workspace_snapshot_from_contexts(
    installed_root: &Path,
    contexts: &[InstalledManifestContext],
    progress: &OperationReporter,
) -> Result<(), String> {
    let categories_path = mod_category_store_path(installed_root)
        .ok_or_else(|| "无法确定分类数据文件位置。".to_string())?;
    // 状态同步已经持有最新 manifest，上下文不能再通过分类迁移入口重复全库读取。
    let category_store = load_mod_category_store(&categories_path)?;
    let snapshot = build_workspace_snapshot_from_contexts(
        installed_root,
        contexts,
        &category_store,
        progress,
    )?;
    let mod_index = build_workspace_mod_index(contexts)?;
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    save_workspace_snapshot(&snapshot_path, &snapshot, mod_index)
}

fn update_workspace_snapshot_after_import(
    paths: &LibraryPaths,
    mod_id: &str,
) -> Result<(), String> {
    let Some(mut stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? else {
        return Ok(());
    };
    let category_store = load_mod_category_store(&paths.categories_path)?;
    let context = load_installed_manifest(&paths.installed_path, mod_id)?;
    let mut imported = installed_mod_list_from_contexts(
        &paths.installed_path,
        std::slice::from_ref(&context),
        &category_store,
        &OperationReporter::default(),
        false,
    )?;
    let Some(imported_mod) = imported.mods.pop() else {
        return Err("导入完成，但无法生成 MOD 快照条目。".to_string());
    };

    stored
        .snapshot
        .installed_mods
        .mods
        .retain(|installed| installed.id != imported_mod.id);
    stored.snapshot.installed_mods.mods.push(imported_mod);
    stored.snapshot.installed_mods.mods.sort_by(|left, right| {
        right
            .installed_at_unix_seconds
            .cmp(&left.installed_at_unix_seconds)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    apply_mod_library_order(
        &paths.installed_path,
        &mut stored.snapshot.installed_mods.mods,
    )?;
    stored.snapshot.installed_mods.message = format!(
        "本地 MOD 库共有 {} 个 MOD。",
        stored.snapshot.installed_mods.mods.len()
    );
    let categories = sorted_mod_categories(&category_store.categories);
    stored.snapshot.categories = ModCategoryList {
        message: format!("共有 {} 个分类。", categories.len()),
        categories,
    };
    upsert_workspace_mod_index_entry(&mut stored.mod_index, &context)?;
    save_stored_workspace_snapshot(&paths.workspace_snapshot_path, &stored)
}

fn append_workspace_snapshot_warning(message: &mut String, result: Result<(), String>) {
    if let Err(error) = result {
        message.push_str(&format!(" 工作区快照更新失败，请点击刷新重建：{error}"));
    }
}

fn read_stored_workspace_snapshot(path: &Path) -> Result<Option<StoredWorkspaceSnapshot>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let json = fs::read_to_string(path)
        .map_err(|error| format!("无法读取工作区快照 {}：{error}", path.display()))?;
    let Ok(stored) = serde_json::from_str::<StoredWorkspaceSnapshot>(&json) else {
        return Ok(None);
    };
    if stored.schema_version != WORKSPACE_SNAPSHOT_SCHEMA_VERSION
        || stored.manifest_schema_version != CURRENT_MOD_MANIFEST_SCHEMA_VERSION
    {
        return Ok(None);
    }
    Ok(Some(stored))
}

fn save_workspace_snapshot(
    path: &Path,
    snapshot: &ModWorkspaceSnapshot,
    mod_index: Vec<WorkspaceModIndexEntry>,
) -> Result<(), String> {
    let stored = StoredWorkspaceSnapshot {
        schema_version: WORKSPACE_SNAPSHOT_SCHEMA_VERSION,
        manifest_schema_version: CURRENT_MOD_MANIFEST_SCHEMA_VERSION,
        snapshot: snapshot.clone(),
        mod_index,
    };
    save_stored_workspace_snapshot(path, &stored)
}

fn save_stored_workspace_snapshot(
    path: &Path,
    stored: &StoredWorkspaceSnapshot,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|error| format!("无法序列化工作区快照：{error}"))?;
    fs::write(path, json).map_err(|error| format!("无法保存工作区快照 {}：{error}", path.display()))
}

fn build_workspace_mod_index(
    contexts: &[InstalledManifestContext],
) -> Result<Vec<WorkspaceModIndexEntry>, String> {
    contexts
        .iter()
        .map(workspace_mod_index_entry)
        .collect::<Result<Vec<_>, _>>()
}

fn workspace_mod_index_entry(
    context: &InstalledManifestContext,
) -> Result<WorkspaceModIndexEntry, String> {
    let effective_files = effective_installed_files_for_context(context)?
        .into_iter()
        .map(|file| file.deploy_relative_path)
        .collect();
    let original_replacements =
        model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    let model_replacements = effective_model_replacements_for_context(
        context,
        &context.manifest,
        &original_replacements,
    )?;

    Ok(WorkspaceModIndexEntry {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        enabled: context.manifest.enabled,
        effective_files,
        model_replacements,
    })
}

fn upsert_workspace_mod_index_entry(
    mod_index: &mut Vec<WorkspaceModIndexEntry>,
    context: &InstalledManifestContext,
) -> Result<(), String> {
    let entry = workspace_mod_index_entry(context)?;
    mod_index.retain(|candidate| candidate.mod_id != entry.mod_id);
    mod_index.push(entry);
    mod_index.sort_by(|left, right| left.mod_id.cmp(&right.mod_id));
    Ok(())
}

fn update_workspace_snapshot_after_mod_changes(
    paths: &LibraryPaths,
    changed_mod_ids: &[String],
    removed_mod_ids: &[String],
) -> Result<(), String> {
    let result =
        update_workspace_snapshot_after_mod_changes_inner(paths, changed_mod_ids, removed_mod_ids);
    if result.is_err() {
        invalidate_workspace_snapshot(&paths.installed_path);
    }
    result
}

fn update_workspace_snapshot_after_mod_changes_inner(
    paths: &LibraryPaths,
    changed_mod_ids: &[String],
    removed_mod_ids: &[String],
) -> Result<(), String> {
    let Some(mut stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? else {
        return Ok(());
    };
    let removed_ids = removed_mod_ids.iter().collect::<HashSet<_>>();
    stored
        .snapshot
        .installed_mods
        .mods
        .retain(|installed| !removed_ids.contains(&installed.id));
    stored
        .mod_index
        .retain(|entry| !removed_ids.contains(&entry.mod_id));

    let category_store = load_mod_category_store(&paths.categories_path)?;
    for mod_id in changed_mod_ids {
        if removed_ids.contains(mod_id) {
            continue;
        }
        let context = load_installed_manifest(&paths.installed_path, mod_id)?;
        let mut list = installed_mod_list_from_contexts(
            &paths.installed_path,
            std::slice::from_ref(&context),
            &category_store,
            &OperationReporter::default(),
            false,
        )?;
        let Some(summary) = list.mods.pop() else {
            continue;
        };
        if let Some(position) = stored
            .snapshot
            .installed_mods
            .mods
            .iter()
            .position(|installed| installed.id == summary.id)
        {
            // 启停、改名等状态更新只替换快照内容，不能把该 MOD 当作新导入项追加到列表末尾。
            stored.snapshot.installed_mods.mods[position] = summary;
        } else {
            stored.snapshot.installed_mods.mods.push(summary);
        }
        upsert_workspace_mod_index_entry(&mut stored.mod_index, &context)?;
    }

    for installed in &mut stored.snapshot.installed_mods.mods {
        installed.categories = resolve_mod_categories(&category_store, &installed.category_ids);
        installed.category_ids = installed
            .categories
            .iter()
            .map(|category| category.id.clone())
            .collect();
    }

    apply_mod_library_order(
        &paths.installed_path,
        &mut stored.snapshot.installed_mods.mods,
    )?;
    stored.snapshot.installed_mods.message = format!(
        "本地 MOD 库共有 {} 个 MOD。",
        stored.snapshot.installed_mods.mods.len()
    );
    let categories = sorted_mod_categories(&category_store.categories);
    stored.snapshot.categories = ModCategoryList {
        message: format!("共有 {} 个分类。", categories.len()),
        categories,
    };
    let installed_ids = stored
        .snapshot
        .installed_mods
        .mods
        .iter()
        .map(|mod_summary| mod_summary.id.clone())
        .collect::<HashSet<_>>();
    stored.snapshot.branch_groups =
        load_normalized_mod_branch_groups(&paths.installed_path, &installed_ids)?;
    let conflict_store = read_conflict_order_store(&paths.installed_path)?;
    stored.snapshot.conflict_report =
        build_mod_conflict_report_from_workspace_index(&stored.mod_index, &conflict_store);
    update_snapshot_partial_override_flags(&mut stored);
    save_stored_workspace_snapshot(&paths.workspace_snapshot_path, &stored)
}

fn update_snapshot_partial_override_flags(stored: &mut StoredWorkspaceSnapshot) {
    for installed in &mut stored.snapshot.installed_mods.mods {
        installed.partially_overridden = false;
    }

    let entries_by_id = stored
        .mod_index
        .iter()
        .map(|entry| (entry.mod_id.as_str(), entry))
        .collect::<HashMap<_, _>>();
    let mut overridden_ids = HashSet::new();
    for group in &stored.snapshot.conflict_report.groups {
        for conflict_file in &group.conflict_files {
            let path_key = conflict_path_key(conflict_file);
            let providers = group
                .participants
                .iter()
                .filter(|participant| {
                    entries_by_id
                        .get(participant.mod_id.as_str())
                        .is_some_and(|entry| {
                            entry.enabled
                                && entry
                                    .effective_files
                                    .iter()
                                    .any(|path| conflict_path_key(path) == path_key)
                        })
                })
                .map(|participant| participant.mod_id.as_str())
                .collect::<Vec<_>>();
            overridden_ids.extend(providers.into_iter().skip(1).map(str::to_string));
        }
    }

    for installed in &mut stored.snapshot.installed_mods.mods {
        installed.partially_overridden =
            installed.enabled && overridden_ids.contains(&installed.id);
    }
}

fn mark_all_workspace_mods_disabled(paths: &LibraryPaths) -> Result<(), String> {
    let Some(mut stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? else {
        return Ok(());
    };
    for installed in &mut stored.snapshot.installed_mods.mods {
        installed.enabled = false;
        installed.partially_overridden = false;
    }
    for entry in &mut stored.mod_index {
        entry.enabled = false;
    }
    stored.snapshot.conflict_report = ModConflictReport {
        conflict_count: 0,
        conflict_file_count: 0,
        groups: Vec::new(),
        warnings: Vec::new(),
        message: "未发现已启用 MOD 之间的文件冲突。".to_string(),
    };
    save_stored_workspace_snapshot(&paths.workspace_snapshot_path, &stored)
}

fn update_workspace_mod_index_only(installed_root: &Path, mod_id: &str) -> Result<(), String> {
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let Some(mut stored) = read_stored_workspace_snapshot(&snapshot_path)? else {
        return Ok(());
    };
    let context = load_installed_manifest(installed_root, mod_id)?;
    upsert_workspace_mod_index_entry(&mut stored.mod_index, &context)?;
    save_stored_workspace_snapshot(&snapshot_path, &stored)
}

fn invalidate_workspace_snapshot(installed_root: &Path) {
    if let Ok(snapshot_path) = workspace_snapshot_path_for_installed_root(installed_root) {
        let _ = fs::remove_file(snapshot_path);
    }
}

fn workspace_snapshot_path_for_installed_root(installed_root: &Path) -> Result<PathBuf, String> {
    let is_standard_installed_root = installed_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("installed"));
    if is_standard_installed_root {
        return installed_root
            .parent()
            .map(|mods_path| mods_path.join("workspace-snapshot.json"))
            .ok_or_else(|| "无法确定工作区快照目录。".to_string());
    }
    Ok(installed_root.join(".workspace-snapshot.json"))
}

pub fn move_mod_library_item(
    app: &tauri::AppHandle,
    mod_id: String,
    target_mod_id: String,
    place_after: bool,
) -> Result<(), String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    move_mod_library_item_from(&paths.installed_path, &mod_id, &target_mod_id, place_after)?;
    update_workspace_snapshot_after_mod_changes(&paths, &[], &[])
}

/// 将一个普通 MOD 或整个分支组作为连续块移动，避免组内成员在持久化顺序中被拆散。
pub fn move_mod_library_items(
    app: &tauri::AppHandle,
    mod_ids: Vec<String>,
    target_mod_ids: Vec<String>,
    place_after: bool,
) -> Result<(), String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    move_mod_library_items_from(
        &paths.installed_path,
        &mod_ids,
        &target_mod_ids,
        place_after,
    )?;
    update_workspace_snapshot_after_mod_changes(&paths, &[], &[])
}

/// 使用完整 MOD ID 列表替换手动浏览顺序，不修改原始导入顺序。
pub fn replace_mod_library_order(
    app: &tauri::AppHandle,
    mod_ids: Vec<String>,
) -> Result<ModLibraryOrderResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let installation_order = current_mod_library_installation_order(&paths)?;
    replace_mod_library_order_from(&paths.installed_path, mod_ids, &installation_order)?;
    let snapshot_warning = update_workspace_snapshot_after_mod_changes(&paths, &[], &[]).err();
    let store = read_mod_library_order_store(&paths.installed_path)?;
    Ok(ModLibraryOrderResult {
        manual_mod_ids: store.manual_mod_ids,
        import_mod_ids: store.import_mod_ids,
        applied_source: "browseOrder".to_string(),
        message: snapshot_warning.map_or_else(
            || "已将当前排序保存为手动顺序。".to_string(),
            |_| "手动顺序已保存，工作区快照将在下次读取时重建。".to_string(),
        ),
    })
}

/// 将手动浏览顺序恢复为最早导入在上、最新导入在下。
pub fn restore_mod_library_import_order(
    app: &tauri::AppHandle,
) -> Result<ModLibraryOrderResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let installation_order = current_mod_library_installation_order(&paths)?;
    restore_mod_library_import_order_from(&paths.installed_path, &installation_order)?;
    let snapshot_warning = update_workspace_snapshot_after_mod_changes(&paths, &[], &[]).err();
    let store = read_mod_library_order_store(&paths.installed_path)?;
    Ok(ModLibraryOrderResult {
        manual_mod_ids: store.manual_mod_ids,
        import_mod_ids: store.import_mod_ids,
        applied_source: "importOrder".to_string(),
        message: snapshot_warning.map_or_else(
            || "已恢复为原始导入顺序。".to_string(),
            |_| "导入顺序已恢复，工作区快照将在下次读取时重建。".to_string(),
        ),
    })
}

fn current_mod_library_installation_order(paths: &LibraryPaths) -> Result<Vec<String>, String> {
    if let Some(stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? {
        return Ok(installation_mod_library_order(
            &stored.snapshot.installed_mods.mods,
        ));
    }

    let mut entries = load_all_installed_manifests(&paths.installed_path)?
        .into_iter()
        .map(|context| {
            (
                context.manifest.id,
                context.manifest.installed_at_unix_seconds,
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    Ok(entries.into_iter().map(|entry| entry.0).collect())
}

/// 将至少两个现有 MOD 组织为一个分支组；MOD 本身仍保持独立部署和冲突记录。
pub fn create_mod_branch_group(
    app: &tauri::AppHandle,
    name: String,
    mod_ids: Vec<String>,
) -> Result<ModBranchGroup, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let name = validate_mod_branch_group_name(&name)?;
    let mod_ids = validate_branch_group_members(&paths.installed_path, mod_ids)?;

    let (group, groups) = create_mod_branch_group_from(&paths.installed_path, name, mod_ids)?;
    update_workspace_branch_groups_snapshot(&paths, &groups)?;
    Ok(group)
}

/// 修改分支组显示名称，不改动任何分支 manifest 或本地 MOD 文件。
pub fn rename_mod_branch_group(
    app: &tauri::AppHandle,
    group_id: String,
    name: String,
) -> Result<ModBranchGroup, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let name = validate_mod_branch_group_name(&name)?;
    let mut store = read_mod_branch_group_store(&paths.installed_path)?;
    let group = store
        .groups
        .iter_mut()
        .find(|group| group.id == group_id)
        .ok_or_else(|| "未找到要重命名的 MOD 分支组。".to_string())?;
    group.name = name;
    let updated = group.clone();
    save_mod_branch_group_store(&paths.installed_path, &store)?;
    update_workspace_branch_groups_snapshot(&paths, &store.groups)?;
    Ok(updated)
}

/// 将所选 MOD 移出分支组；只剩一个成员时自动拆散该组。
pub fn remove_mods_from_branch_group(
    app: &tauri::AppHandle,
    mod_ids: Vec<String>,
) -> Result<Vec<ModBranchGroup>, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let selected = mod_ids
        .into_iter()
        .map(|mod_id| {
            validate_mod_id(&mod_id)?;
            Ok(mod_id)
        })
        .collect::<Result<HashSet<_>, String>>()?;
    if selected.is_empty() {
        return Err("请至少选择一个要移出分支组的 MOD。".to_string());
    }

    let mut store = read_mod_branch_group_store(&paths.installed_path)?;
    for group in &mut store.groups {
        group.mod_ids.retain(|mod_id| !selected.contains(mod_id));
    }
    store.groups.retain(|group| group.mod_ids.len() >= 2);
    save_mod_branch_group_store(&paths.installed_path, &store)?;
    update_workspace_branch_groups_snapshot(&paths, &store.groups)?;
    Ok(store.groups)
}

fn validate_branch_group_members(
    installed_root: &Path,
    mod_ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut unique_ids = Vec::new();
    let mut seen = HashSet::new();
    for mod_id in mod_ids {
        validate_mod_id(&mod_id)?;
        if seen.insert(mod_id.clone()) {
            load_installed_manifest(installed_root, &mod_id)?;
            unique_ids.push(mod_id);
        }
    }
    if unique_ids.len() < 2 {
        return Err("一个分支组至少需要两个 MOD。".to_string());
    }
    Ok(unique_ids)
}

fn create_mod_branch_group_from(
    installed_root: &Path,
    name: String,
    mod_ids: Vec<String>,
) -> Result<(ModBranchGroup, Vec<ModBranchGroup>), String> {
    let mut store = read_mod_branch_group_store(installed_root)?;
    let selected = mod_ids.iter().cloned().collect::<HashSet<_>>();
    for group in &mut store.groups {
        group.mod_ids.retain(|mod_id| !selected.contains(mod_id));
    }
    store.groups.retain(|group| group.mod_ids.len() >= 2);
    let group = ModBranchGroup {
        id: unique_mod_id(&format!("branch-{name}"))?,
        name,
        mod_ids,
        created_at_unix_seconds: unix_seconds_now()?,
    };
    store.groups.push(group.clone());
    save_mod_branch_group_store(installed_root, &store)?;
    Ok((group, store.groups))
}

fn update_workspace_branch_groups_snapshot(
    paths: &LibraryPaths,
    groups: &[ModBranchGroup],
) -> Result<(), String> {
    let Some(mut stored) = read_stored_workspace_snapshot(&paths.workspace_snapshot_path)? else {
        return Ok(());
    };
    stored.snapshot.branch_groups = groups.to_vec();
    save_stored_workspace_snapshot(&paths.workspace_snapshot_path, &stored)
}

pub fn update_mod_metadata(
    app: &tauri::AppHandle,
    mod_id: String,
    patch: ModMetadataPatch,
) -> Result<ModMetadataUpdateResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_mod_id(&mod_id)?;
    if patch.display_name.is_none() && patch.note.is_none() && patch.category_ids.is_none() {
        return Err("至少需要修改一项 MOD 信息。".to_string());
    }

    let category_store = load_or_initialize_mod_category_store(&paths)?;
    let mut context = load_installed_manifest(&paths.installed_path, &mod_id)?;

    if let Some(display_name) = patch.display_name {
        let display_name = validate_mod_display_name(&display_name)?;
        context.manifest.display_name = (!display_name.is_empty()).then_some(display_name);
    }

    if let Some(note) = patch.note {
        context.manifest.note = validate_mod_note(&note)?;
    }

    if let Some(category_ids) = patch.category_ids {
        context.manifest.category_ids = resolve_category_ids(&category_store, &category_ids)?;
        context.manifest.category_override = None;
    }

    refresh_manifest_model_replacements(&mut context)?;
    context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    save_manifest(&context.manifest_path, &context.manifest)?;

    let categories = resolve_mod_categories(&category_store, &context.manifest.category_ids);
    let category_ids = categories
        .iter()
        .map(|category| category.id.clone())
        .collect();

    let mut result = ModMetadataUpdateResult {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        original_name: context.manifest.name.clone(),
        note: context.manifest.note.clone(),
        category_ids,
        categories,
        message: "MOD 信息已保存。".to_string(),
    };
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        std::slice::from_ref(&result.mod_id),
        &[],
    ) {
        result
            .message
            .push_str(&format!(" 工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

/// 一次保存多个分支的分类，并只在全部参数验证通过后开始写入清单。
pub fn update_mod_categories(
    app: &tauri::AppHandle,
    assignments: Vec<ModCategoryAssignment>,
) -> Result<ModCategoryBatchUpdateResult, String> {
    if assignments.is_empty() {
        return Err("至少需要选择一个 MOD。".to_string());
    }

    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let category_store = load_or_initialize_mod_category_store(&paths)?;
    let mut seen_mod_ids = HashSet::new();
    let mut pending_updates = Vec::with_capacity(assignments.len());

    for assignment in assignments {
        validate_mod_id(&assignment.mod_id)?;
        if !seen_mod_ids.insert(assignment.mod_id.clone()) {
            return Err(format!("MOD 分类更新中包含重复项目：{}", assignment.mod_id));
        }
        let category_ids = resolve_category_ids(&category_store, &assignment.category_ids)?;
        let context = load_installed_manifest(&paths.installed_path, &assignment.mod_id)?;
        pending_updates.push((context, category_ids));
    }

    let mut results = Vec::with_capacity(pending_updates.len());
    for (mut context, category_ids) in pending_updates {
        context.manifest.category_ids = category_ids;
        context.manifest.category_override = None;
        context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
        save_manifest(&context.manifest_path, &context.manifest)?;

        let categories = resolve_mod_categories(&category_store, &context.manifest.category_ids);
        results.push(ModMetadataUpdateResult {
            mod_id: context.manifest.id.clone(),
            name: manifest_display_name(&context.manifest),
            original_name: context.manifest.name.clone(),
            note: context.manifest.note.clone(),
            category_ids: categories
                .iter()
                .map(|category| category.id.clone())
                .collect(),
            categories,
            message: "MOD 分类已保存。".to_string(),
        });
    }

    let changed_mod_ids = results
        .iter()
        .map(|result| result.mod_id.clone())
        .collect::<Vec<_>>();
    let mut message = format!("已更新 {} 个 MOD 的分类。", results.len());
    if let Err(error) = update_workspace_snapshot_after_mod_changes(&paths, &changed_mod_ids, &[]) {
        message.push_str(&format!(" 工作区快照更新失败，请手动刷新：{error}"));
    }

    Ok(ModCategoryBatchUpdateResult {
        mods: results,
        message,
    })
}

pub fn list_mod_categories(app: &tauri::AppHandle) -> Result<ModCategoryList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let store = load_or_initialize_mod_category_store(&paths)?;
    let categories = sorted_mod_categories(&store.categories);

    Ok(ModCategoryList {
        message: format!("共有 {} 个分类。", categories.len()),
        categories,
    })
}

pub fn create_mod_category(
    app: &tauri::AppHandle,
    name: String,
    parent_id: Option<String>,
) -> Result<ModCategory, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut store = load_or_initialize_mod_category_store(&paths)?;
    let name = validate_mod_category_name(&name)?;
    let parent_id = resolve_category_parent_id(&store, parent_id.as_deref())?;

    ensure_mod_category_name_is_available(&store.categories, &name, parent_id.as_deref(), None)?;
    let category = StoredModCategory {
        id: unique_mod_category_id(&store.categories, &name)?,
        name,
        parent_id,
        created_at_unix_seconds: unix_seconds_now()?,
        recognition_keys: Vec::new(),
    };
    store.categories.push(category.clone());
    save_mod_category_store(&paths.categories_path, &store)?;

    update_workspace_snapshot_after_mod_changes(&paths, &[], &[])?;

    Ok(ModCategory::from(&category))
}

pub fn rename_mod_category(
    app: &tauri::AppHandle,
    category_id: String,
    name: String,
) -> Result<ModCategory, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_mod_category_id(&category_id)?;
    let mut store = load_or_initialize_mod_category_store(&paths)?;
    let name = validate_mod_category_name(&name)?;

    let existing_category = store
        .categories
        .iter()
        .find(|category| category.id == category_id)
        .ok_or_else(|| format!("未找到分类：{category_id}"))?;
    ensure_mod_category_name_is_available(
        &store.categories,
        &name,
        existing_category.parent_id.as_deref(),
        Some(&category_id),
    )?;
    let category = store
        .categories
        .iter_mut()
        .find(|category| category.id == category_id)
        .ok_or_else(|| format!("未找到分类：{category_id}"))?;
    category.name = name;
    let category = category.clone();
    save_mod_category_store(&paths.categories_path, &store)?;

    update_workspace_snapshot_after_mod_changes(&paths, &[], &[])?;

    Ok(ModCategory::from(&category))
}

pub fn delete_mod_category(
    app: &tauri::AppHandle,
    category_id: String,
) -> Result<ModCategoryDeleteResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_mod_category_id(&category_id)?;
    let mut store = load_or_initialize_mod_category_store(&paths)?;
    let category_index = store
        .categories
        .iter()
        .position(|category| category.id == category_id)
        .ok_or_else(|| format!("未找到分类：{category_id}"))?;

    let removed_category = store.categories.remove(category_index);
    for category in &mut store.categories {
        if category.parent_id.as_deref() == Some(&category_id) {
            category.parent_id = None;
        }
    }
    for recognition_key in removed_category.recognition_keys {
        if !store.suppressed_recognition_keys.contains(&recognition_key) {
            store.suppressed_recognition_keys.push(recognition_key);
        }
    }
    store.suppressed_recognition_keys.sort();
    store.suppressed_recognition_keys.dedup();

    let affected_mod_count = remove_category_from_manifests(&paths.installed_path, &category_id)?;
    save_mod_category_store(&paths.categories_path, &store)?;

    update_workspace_snapshot_after_mod_changes(&paths, &[], &[])?;

    Ok(ModCategoryDeleteResult {
        category_id,
        affected_mod_count,
        message: "分类已删除。".to_string(),
    })
}

pub fn open_installed_mod_folder(app: &tauri::AppHandle, mod_id: String) -> Result<(), String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let content_path = installed_mod_content_path(&paths.installed_path, &mod_id)?;

    app.opener()
        .open_path(path_to_string(&content_path), None::<String>)
        .map_err(|error| {
            format!(
                "Could not open installed MOD folder {}: {error}",
                content_path.display()
            )
        })
}

/// 打开清理候选在本地 MOD 库中的所在目录；候选 ID 会在 manifest 内重新校验。
pub fn open_mod_cleanup_candidate_folder(
    app: &tauri::AppHandle,
    mod_id: String,
    candidate_id: String,
) -> Result<(), String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let folder = mod_cleanup_candidate_folder_from(
        &paths.installed_path,
        mod_id.trim(),
        candidate_id.trim(),
    )?;

    app.opener()
        .open_path(path_to_string(&folder), None::<String>)
        .map_err(|error| format!("无法打开清理候选所在文件夹 {}：{error}", folder.display()))
}

fn mod_cleanup_candidate_folder_from(
    installed_root: &Path,
    mod_id: &str,
    candidate_id: &str,
) -> Result<PathBuf, String> {
    if mod_id.is_empty() || candidate_id.is_empty() {
        return Err("清理候选标识无效。".to_string());
    }
    let context = load_installed_manifest(installed_root, mod_id)?;
    let file = context
        .manifest
        .files
        .iter()
        .find(|file| cleanup_candidate_id(mod_id, &file.library_relative_path) == candidate_id)
        .ok_or_else(|| "清理候选已经变化，请重新扫描。".to_string())?;
    let source_path = source_path_for_installed_file(&context, file)?;
    source_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法确定清理候选所在文件夹。".to_string())
}

fn installed_mod_content_path(installed_root: &Path, mod_id: &str) -> Result<PathBuf, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;

    if !context.content_path.is_dir() {
        return Err(format!(
            "Installed MOD content directory does not exist: {}",
            context.content_path.display()
        ));
    }

    Ok(context.content_path)
}

pub fn get_mod_remap_details(
    app: &tauri::AppHandle,
    mod_id: String,
) -> Result<ModRemapDetails, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    get_mod_remap_details_from(&paths.installed_path, &mod_id)
}

pub fn preview_mod_remap(
    app: &tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_mod_remap_from(&paths.installed_path, &mod_id, &group_key, target_id)
}

pub fn apply_mod_remap_with_progress(
    app: &tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
    progress: &OperationReporter,
) -> Result<ModRemapApplyResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut result = apply_mod_remap_from_with_progress(
        &paths.installed_path,
        &mod_id,
        &group_key,
        target_id,
        progress,
    )?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        std::slice::from_ref(&result.mod_id),
        &[],
    ) {
        result
            .message
            .push_str(&format!(" 工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

/// 读取受兼容索引约束的本地武器特效改绑项。
pub fn get_mod_effect_remap_details(
    app: &tauri::AppHandle,
    mod_id: String,
) -> Result<ModEffectRemapDetails, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    get_mod_effect_remap_details_from(&paths.installed_path, &mod_id)
}

pub fn preview_mod_effect_remap(
    app: &tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModEffectRemapPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_mod_effect_remap_from(&paths.installed_path, &mod_id, &group_key, target_id)
}

pub fn apply_mod_effect_remap_with_progress(
    app: &tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
    progress: &OperationReporter,
) -> Result<ModEffectRemapApplyResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut result = apply_mod_effect_remap_from_with_progress(
        &paths.installed_path,
        &mod_id,
        &group_key,
        target_id,
        progress,
    )?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        std::slice::from_ref(&result.mod_id),
        &[],
    ) {
        result
            .message
            .push_str(&format!(" 工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

pub fn preview_enable_mod(
    app: &tauri::AppHandle,
    mod_id: String,
) -> Result<ModDeploymentPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    preview_enable_mod_from(&paths.installed_path, &game_root, &mod_id)
}

pub fn enable_mod_with_progress(
    app: &tauri::AppHandle,
    mod_id: String,
    confirm_overwrite: bool,
    progress: &OperationReporter,
) -> Result<ModDeploymentResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result = enable_mod_from_with_progress(
        &paths.installed_path,
        &game_root,
        &mod_id,
        confirm_overwrite,
        progress,
    )?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        std::slice::from_ref(&result.mod_id),
        &[],
    ) {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

pub fn disable_mod_with_progress(
    app: &tauri::AppHandle,
    mod_id: String,
    progress: &OperationReporter,
) -> Result<ModDeploymentResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result =
        disable_mod_from_with_progress(&paths.installed_path, &game_root, &mod_id, progress)?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        std::slice::from_ref(&result.mod_id),
        &[],
    ) {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

/// 在一个后台任务中顺序处理多个 MOD；单项失败会记录在结果中并继续后续项目。
pub fn batch_update_mods_with_progress(
    app: &tauri::AppHandle,
    action: BatchModAction,
    mod_ids: Vec<String>,
    progress: &OperationReporter,
) -> Result<BatchModOperationResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result = batch_update_mods_from_with_progress(
        &paths.installed_path,
        &game_root,
        action,
        mod_ids,
        progress,
    )?;
    let succeeded_ids = result
        .items
        .iter()
        .filter(|item| item.status == "succeeded")
        .map(|item| item.mod_id.clone())
        .collect::<Vec<_>>();
    let (changed_ids, removed_ids) = match action {
        BatchModAction::Uninstall => (Vec::new(), succeeded_ids),
        BatchModAction::Enable | BatchModAction::Disable => (succeeded_ids, Vec::new()),
    };
    if let Err(error) =
        update_workspace_snapshot_after_mod_changes(&paths, &changed_ids, &removed_ids)
    {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

pub fn preview_disable_mod(
    app: &tauri::AppHandle,
    mod_id: String,
) -> Result<ModDisablePlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_disable_mod_from(&paths.installed_path, &mod_id)
}

pub fn preview_uninstall_mod(
    app: &tauri::AppHandle,
    mod_id: String,
) -> Result<ModUninstallPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_uninstall_mod_from(&paths.installed_path, &mod_id)
}

pub fn uninstall_mod_with_progress(
    app: &tauri::AppHandle,
    mod_id: String,
    progress: &OperationReporter,
) -> Result<ModUninstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result = uninstall_mod_with_paths_and_progress(
        &paths.installed_path,
        &game_root,
        &mod_id,
        progress,
    )?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(
        &paths,
        &[],
        std::slice::from_ref(&result.mod_id),
    ) {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

fn uninstall_mod_with_paths_and_progress(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    progress: &OperationReporter,
) -> Result<ModUninstallResult, String> {
    let mut result = uninstall_mod_from_with_progress(installed_root, game_root, mod_id, progress)?;
    if let Err(error) = remove_mod_from_conflict_orders(installed_root, mod_id) {
        result.warnings.push(format!(
            "MOD was uninstalled, but conflict order entries could not be cleaned: {error}"
        ));
    }
    if let Err(error) = remove_mod_from_library_order(installed_root, mod_id) {
        result.warnings.push(format!(
            "MOD was uninstalled, but library order entries could not be cleaned: {error}"
        ));
    }
    Ok(result)
}

pub fn preview_restore_all_mods(app: &tauri::AppHandle) -> Result<RestoreAllPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_restore_all_mods_from(&paths.installed_path)
}

pub fn restore_all_mods_with_progress(
    app: &tauri::AppHandle,
    progress: &OperationReporter,
) -> Result<RestoreAllResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result =
        restore_all_mods_from_with_progress(&paths.installed_path, &game_root, progress)?;
    if let Err(error) = mark_all_workspace_mods_disabled(&paths) {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

pub fn get_mod_conflict_report(app: &tauri::AppHandle) -> Result<ModConflictReport, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    get_mod_conflict_report_from(&paths.installed_path)
}

pub fn move_conflict_participant(
    app: &tauri::AppHandle,
    group_id: String,
    mod_id: String,
    direction: String,
    participant_order: Vec<String>,
) -> Result<ModConflictMoveResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    move_conflict_participant_from(
        &paths.installed_path,
        &group_id,
        &mod_id,
        &direction,
        participant_order,
    )
}

/// 按界面展示的“上方优先”规则保存一个冲突组的完整顺序。
///
/// Agent 使用完整列表而不是连续模拟上下移动，避免批量意图在中途留下半套顺序。
pub fn set_conflict_participant_order(
    app: &tauri::AppHandle,
    group_id: String,
    participant_order: Vec<String>,
) -> Result<(), String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    set_conflict_participant_order_from(&paths.installed_path, &group_id, participant_order)
}

pub fn preview_apply_conflict_order(
    app: &tauri::AppHandle,
    group_id: String,
) -> Result<ApplyConflictOrderPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    preview_apply_conflict_order_from(&paths.installed_path, &game_root, &group_id)
}

pub fn apply_conflict_order_with_progress(
    app: &tauri::AppHandle,
    group_id: String,
    confirm_overwrite: bool,
    progress: &OperationReporter,
) -> Result<ApplyConflictOrderResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result = apply_conflict_order_from_with_progress(
        &paths.installed_path,
        &game_root,
        &group_id,
        confirm_overwrite,
        progress,
    )?;
    if let Err(error) = update_workspace_snapshot_after_mod_changes(&paths, &[], &[]) {
        result
            .warnings
            .push(format!("工作区快照更新失败，请手动刷新：{error}"));
    }
    Ok(result)
}

#[cfg(test)]
fn install_mod_from_folder_into(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
) -> Result<ModInstallResult, String> {
    install_mod_from_folder_into_with_options_and_progress(
        raw_path,
        allow_game_root,
        installed_root,
        None,
        None,
        &OperationReporter::default(),
    )
}

fn install_mod_from_folder_into_with_options_and_progress(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
    preferred_name: Option<String>,
    original_source_path: Option<String>,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    install_mod_from_folder_into_with_duplicate_name_check(
        raw_path,
        allow_game_root,
        installed_root,
        preferred_name,
        original_source_path,
        true,
        progress,
    )
}

/// 盒子导入以内容关联为准，因此同名但内容不同的模块需要允许独立保存。
fn install_mod_from_folder_into_with_options_and_progress_allow_same_name(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
    preferred_name: Option<String>,
    original_source_path: Option<String>,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    install_mod_from_folder_into_with_duplicate_name_check(
        raw_path,
        allow_game_root,
        installed_root,
        preferred_name,
        original_source_path,
        false,
        progress,
    )
}

fn install_mod_from_folder_into_with_duplicate_name_check(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
    preferred_name: Option<String>,
    original_source_path: Option<String>,
    check_name_duplicate: bool,
    progress: &OperationReporter,
) -> Result<ModInstallResult, String> {
    let preview = preview_mod_import_with_progress(raw_path, allow_game_root, progress)?;

    if preview.status != "ready" {
        return Err(format!(
            "MOD is not ready to import. Current status: {}. {}",
            preview.status, preview.message
        ));
    }

    let content_root_path = preview
        .content_root_path
        .clone()
        .ok_or_else(|| "MOD content root was not resolved.".to_string())?;
    let source_root = PathBuf::from(&content_root_path);
    let deploy_root = deploy_root_from_preview(&preview, &source_root)?;
    let files = build_file_previews(&source_root, &deploy_root, progress)?;

    if files.is_empty() {
        return Err("MOD import has no files to copy.".to_string());
    }

    let source_path = PathBuf::from(&preview.source_path);
    let mod_name = preferred_name
        .map(|name| validate_import_mod_name(&name))
        .transpose()?
        .unwrap_or_else(|| derive_mod_name(&source_path));

    if check_name_duplicate {
        if let Some(existing) = find_installed_mod_by_name(installed_root, &mod_name)? {
            return Ok(existing);
        }
    }

    let mod_id = unique_mod_id(&mod_name)?;
    let final_mod_path = installed_root.join(&mod_id);
    let temp_mod_path = installed_root.join(format!(".{mod_id}.tmp"));
    let result = (|| {
        if temp_mod_path.exists() {
            fs::remove_dir_all(&temp_mod_path).map_err(|error| {
                format!(
                    "Could not clean temporary MOD import directory {}: {error}",
                    temp_mod_path.display()
                )
            })?;
        }

        fs::create_dir_all(installed_root).map_err(|error| {
            format!(
                "Could not create installed MOD directory {}: {error}",
                installed_root.display()
            )
        })?;

        let content_path = temp_mod_path.join("content");
        fs::create_dir_all(&content_path).map_err(|error| {
            format!(
                "Could not create MOD content directory {}: {error}",
                content_path.display()
            )
        })?;

        let mut installed_files = Vec::new();

        progress.report("正在复制到本地 MOD 库", 0, Some(files.len()), None);
        for (index, file) in files.iter().enumerate() {
            let destination_relative_path = relative_string_to_path(&file.deploy_relative_path)?;
            let destination_path = content_path.join(&destination_relative_path);

            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "Could not create destination directory {}: {error}",
                        parent.display()
                    )
                })?;
            }

            fs::copy(&file.source_path, &destination_path).map_err(|error| {
                format!(
                    "Could not copy {} to {}: {error}",
                    file.source_path,
                    destination_path.display()
                )
            })?;

            installed_files.push(InstalledModFile {
                source_relative_path: file.source_relative_path.clone(),
                deploy_relative_path: file.deploy_relative_path.clone(),
                library_relative_path: format!("content/{}", file.deploy_relative_path),
            });
            progress.report(
                "正在复制到本地 MOD 库",
                index + 1,
                Some(files.len()),
                Some(file.deploy_relative_path.clone()),
            );
        }

        progress.report("正在识别 MOD 内容", 0, None, None);
        let model_replacements =
            recognize_model_replacements_for_library_files(&installed_files, &content_path)?;
        let mut category_store =
            load_or_initialize_mod_category_store_for_installed_root(installed_root)?;
        let (category_ids, category_store_changed) =
            ensure_recognition_categories(&mut category_store, &model_replacements)?;
        if category_store_changed {
            let categories_path = mod_category_store_path(installed_root)
                .ok_or_else(|| "无法确定分类数据目录。".to_string())?;
            save_mod_category_store(&categories_path, &category_store)?;
        }
        let manifest_path = temp_mod_path.join("manifest.json");
        let manifest = InstalledModManifest {
            schema_version: CURRENT_MOD_MANIFEST_SCHEMA_VERSION,
            id: mod_id.clone(),
            name: mod_name.clone(),
            display_name: None,
            note: String::new(),
            category_ids,
            category_override: None,
            source_path: original_source_path.unwrap_or_else(|| preview.source_path.clone()),
            legacy_sources: Vec::new(),
            content_root_path,
            detection_method: preview.detection_method.clone(),
            deploy_root: preview.deploy_root.clone(),
            installed_at_unix_seconds: unix_seconds_now()?,
            enabled: false,
            partially_overridden: false,
            file_count: installed_files.len(),
            files: installed_files.clone(),
            model_replacements: model_replacements.clone(),
            model_remaps: Vec::new(),
            effect_remaps: Vec::new(),
            deployment_exclusions: Vec::new(),
            deployed_files: Vec::new(),
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|error| format!("Could not serialize MOD manifest: {error}"))?;
        fs::write(&manifest_path, manifest_json).map_err(|error| {
            format!(
                "Could not write MOD manifest {}: {error}",
                manifest_path.display()
            )
        })?;

        if final_mod_path.exists() {
            return Err(format!(
                "Installed MOD directory already exists: {}",
                final_mod_path.display()
            ));
        }

        fs::rename(&temp_mod_path, &final_mod_path).map_err(|error| {
            format!(
                "Could not finalize MOD import {} -> {}: {error}",
                temp_mod_path.display(),
                final_mod_path.display()
            )
        })?;
        progress.report("正在完成导入", files.len(), Some(files.len()), None);

        Ok(ModInstallResult {
            mod_id,
            name: mod_name,
            already_installed: false,
            mod_path: path_to_string(&final_mod_path),
            content_path: path_to_string(&final_mod_path.join("content")),
            manifest_path: path_to_string(&final_mod_path.join("manifest.json")),
            file_count: installed_files.len(),
            files: installed_files,
            model_replacements,
            message: "MOD was imported into the local Acumod library.".to_string(),
        })
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&temp_mod_path);
    }

    result
}

fn find_installed_mod_by_name(
    installed_root: &Path,
    mod_name: &str,
) -> Result<Option<ModInstallResult>, String> {
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let existing_id = stored
            .snapshot
            .installed_mods
            .mods
            .iter()
            .find(|installed| {
                installed
                    .original_name
                    .trim()
                    .eq_ignore_ascii_case(mod_name.trim())
            })
            .map(|installed| installed.id.clone());
        return existing_id
            .map(|mod_id| load_installed_manifest(installed_root, &mod_id))
            .transpose()?
            .map(existing_mod_install_result)
            .transpose();
    }

    let contexts = load_all_installed_manifests(installed_root)?;
    let existing = contexts.into_iter().find(|context| {
        context
            .manifest
            .name
            .trim()
            .eq_ignore_ascii_case(mod_name.trim())
    });

    let Some(context) = existing else {
        return Ok(None);
    };
    Ok(Some(existing_mod_install_result(context)?))
}

fn find_installed_mod_by_legacy_source(
    contexts: &[InstalledManifestContext],
    source_ref: &LegacyBoxSourceRef,
) -> Result<Option<ModInstallResult>, String> {
    let context = contexts
        .iter()
        .find(|context| {
            context
                .manifest
                .legacy_sources
                .iter()
                .any(|stored| legacy_box_source_matches(stored, source_ref))
        })
        .cloned();

    context.map(existing_mod_install_result).transpose()
}

fn find_installed_mod_by_content_with_options(
    contexts: &[InstalledManifestContext],
    source_path: &Path,
    allow_game_root: bool,
    progress: &OperationReporter,
) -> Result<Option<ModInstallResult>, String> {
    let preview =
        preview_mod_import_with_progress(path_to_string(source_path), allow_game_root, progress)?;
    if preview.status != "ready" {
        return Err(format!("导入来源的文件结构无法导入：{}", preview.message));
    }
    let content_root = preview
        .content_root_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定导入来源的内容根目录。".to_string())?;
    let deploy_root = deploy_root_from_preview(&preview, &content_root)?;
    let source_files = build_file_previews(&content_root, &deploy_root, progress)?;
    let source_files_by_path = file_previews_by_deploy_path(&source_files)?;

    for context in contexts {
        if context.manifest.files.len() != source_files_by_path.len() {
            continue;
        }
        let installed_files_by_path = installed_files_by_deploy_path(&context.manifest.files)?;
        if installed_files_by_path.len() != source_files_by_path.len()
            || !installed_files_by_path
                .keys()
                .all(|path_key| source_files_by_path.contains_key(path_key))
        {
            continue;
        }

        let mut same_content = true;
        for (path_key, source_file) in &source_files_by_path {
            let Some(installed_file) = installed_files_by_path.get(path_key) else {
                same_content = false;
                break;
            };
            let installed_path = source_path_for_installed_file(&context, installed_file)?;
            if !regular_files_are_equal(Path::new(&source_file.source_path), &installed_path)? {
                same_content = false;
                break;
            }
        }

        if same_content {
            return Ok(Some(existing_mod_install_result(context.clone())?));
        }
    }

    Ok(None)
}

fn existing_mod_install_result(
    context: InstalledManifestContext,
) -> Result<ModInstallResult, String> {
    let model_replacements =
        model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    let display_name = manifest_display_name(&context.manifest);
    Ok(ModInstallResult {
        mod_id: context.manifest.id.clone(),
        name: display_name,
        already_installed: true,
        mod_path: path_to_string(&context.mod_path),
        content_path: path_to_string(&context.content_path),
        manifest_path: path_to_string(&context.manifest_path),
        file_count: context.manifest.file_count,
        files: context.manifest.files.clone(),
        model_replacements,
        message: "已关联到本地 MOD 库中的相同内容。".to_string(),
    })
}

fn file_previews_by_deploy_path<'a>(
    files: &'a [ModImportFilePreview],
) -> Result<BTreeMap<String, &'a ModImportFilePreview>, String> {
    let mut files_by_path = BTreeMap::new();
    for file in files {
        let path_key = conflict_path_key(&file.deploy_relative_path);
        if files_by_path.insert(path_key, file).is_some() {
            return Err(format!(
                "导入来源包含重复的部署路径：{}",
                file.deploy_relative_path
            ));
        }
    }
    Ok(files_by_path)
}

fn installed_files_by_deploy_path<'a>(
    files: &'a [InstalledModFile],
) -> Result<BTreeMap<String, &'a InstalledModFile>, String> {
    let mut files_by_path = BTreeMap::new();
    for file in files {
        let path_key = conflict_path_key(&file.deploy_relative_path);
        if files_by_path.insert(path_key, file).is_some() {
            return Err(format!(
                "本地 MOD 清单包含重复的部署路径：{}",
                file.deploy_relative_path
            ));
        }
    }
    Ok(files_by_path)
}

fn associate_legacy_box_source(
    installed_root: &Path,
    mod_id: &str,
    source_ref: LegacyBoxSourceRef,
    box_enabled: bool,
) -> Result<InstalledManifestContext, String> {
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    let source_exists = context
        .manifest
        .legacy_sources
        .iter()
        .any(|stored| legacy_box_source_matches(stored, &source_ref));
    if !source_exists {
        context.manifest.legacy_sources.push(source_ref);
    } else if let Some(stored) = context
        .manifest
        .legacy_sources
        .iter_mut()
        .find(|stored| legacy_box_source_matches(stored, &source_ref))
    {
        stored.box_enabled = source_ref.box_enabled;
    }
    // 来源关联成功后立刻以盒子记录初始化状态；导入不写游戏目录，也不伪造 deployed_files。
    context.manifest.enabled = box_enabled;
    context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    save_manifest(&context.manifest_path, &context.manifest)?;
    Ok(context)
}

fn upsert_installed_manifest_context(
    contexts: &mut Vec<InstalledManifestContext>,
    updated: InstalledManifestContext,
) {
    if let Some(existing) = contexts
        .iter_mut()
        .find(|context| context.manifest.id == updated.manifest.id)
    {
        *existing = updated;
    } else {
        contexts.push(updated);
        sort_contexts_by_installation(contexts);
    }
}

fn legacy_box_source_matches(left: &LegacyBoxSourceRef, right: &LegacyBoxSourceRef) -> bool {
    left.module_id == right.module_id && same_normalized_path(&left.box_path, &right.box_path)
}

fn same_normalized_path(left: &str, right: &str) -> bool {
    let left = PathBuf::from(left)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(left));
    let right = PathBuf::from(right)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(right));
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn preview_from_candidates(
    source_path: &Path,
    candidates: Vec<Candidate>,
    warnings: Vec<String>,
    progress: &OperationReporter,
) -> Result<Option<ModImportPreview>, String> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut shallowest = candidates;
    shallowest.sort_by_key(|candidate| candidate.depth);
    let selected_depth = shallowest[0].depth;
    shallowest.retain(|candidate| candidate.depth == selected_depth);

    if shallowest.len() > 1 {
        let candidate_dtos = build_candidate_dtos(source_path, &shallowest, progress)?;

        return Ok(Some(ModImportPreview {
            source_path: path_to_string(source_path),
            original_source_path: path_to_string(source_path),
            status: "ambiguous".to_string(),
            detection_method: "multipleCandidates".to_string(),
            deploy_root: "unknown".to_string(),
            content_root_path: None,
            requires_game_root_confirmation: false,
            message: "Multiple possible MOD content roots were found. Choose one before importing."
                .to_string(),
            file_count: 0,
            files: Vec::new(),
            candidates: candidate_dtos,
            warnings,
        }));
    }

    let candidate = shallowest
        .into_iter()
        .next()
        .ok_or_else(|| "Could not select MOD import candidate.".to_string())?;
    let files = build_file_previews(&candidate.root_path, &candidate.deploy_root, progress)?;
    let file_count = files.len();
    let mut preview_files = files;

    if preview_files.len() > PREVIEW_FILE_LIMIT {
        preview_files.truncate(PREVIEW_FILE_LIMIT);
    }

    Ok(Some(ModImportPreview {
        source_path: path_to_string(source_path),
        original_source_path: path_to_string(source_path),
        status: "ready".to_string(),
        detection_method: candidate.detection_method.to_string(),
        deploy_root: deploy_root_label(&candidate.deploy_root).to_string(),
        content_root_path: Some(path_to_string(&candidate.root_path)),
        requires_game_root_confirmation: false,
        message: "MOD content root was recognized.".to_string(),
        file_count,
        files: preview_files,
        candidates: Vec::new(),
        warnings,
    }))
}

fn preview_game_root_fallback(
    source_path: &Path,
    allow_game_root: bool,
    warnings: Vec<String>,
    progress: &OperationReporter,
) -> Result<ModImportPreview, String> {
    let files = build_file_previews(source_path, &DeployRoot::GameRoot, progress)?;
    let file_count = files.len();

    if file_count == 0 {
        return Ok(ModImportPreview {
            source_path: path_to_string(source_path),
            original_source_path: path_to_string(source_path),
            status: "invalid".to_string(),
            detection_method: "emptyDirectory".to_string(),
            deploy_root: "unknown".to_string(),
            content_root_path: None,
            requires_game_root_confirmation: false,
            message: "No files were found in this directory.".to_string(),
            file_count,
            files: Vec::new(),
            candidates: Vec::new(),
            warnings,
        });
    }

    let mut preview_files = files;

    if preview_files.len() > PREVIEW_FILE_LIMIT {
        preview_files.truncate(PREVIEW_FILE_LIMIT);
    }

    if allow_game_root {
        Ok(ModImportPreview {
            source_path: path_to_string(source_path),
            original_source_path: path_to_string(source_path),
            status: "ready".to_string(),
            detection_method: "userConfirmedGameRoot".to_string(),
            deploy_root: deploy_root_label(&DeployRoot::GameRoot).to_string(),
            content_root_path: Some(path_to_string(source_path)),
            requires_game_root_confirmation: false,
            message: "User confirmed this MOD should be installed relative to the game root."
                .to_string(),
            file_count,
            files: preview_files,
            candidates: Vec::new(),
            warnings,
        })
    } else {
        Ok(ModImportPreview {
            source_path: path_to_string(source_path),
            original_source_path: path_to_string(source_path),
            status: "needsGameRootConfirmation".to_string(),
            detection_method: "unrecognizedRoot".to_string(),
            deploy_root: deploy_root_label(&DeployRoot::GameRoot).to_string(),
            content_root_path: Some(path_to_string(source_path)),
            requires_game_root_confirmation: true,
            message: "Could not find nativePC or known nativePC child folders. Some MODs install at the game root; confirm before using this mapping.".to_string(),
            file_count,
            files: preview_files,
            candidates: Vec::new(),
            warnings,
        })
    }
}

fn invalid_preview(source_path: PathBuf, message: &str) -> ModImportPreview {
    ModImportPreview {
        source_path: path_to_string(&source_path),
        original_source_path: path_to_string(&source_path),
        status: "invalid".to_string(),
        detection_method: "invalidSource".to_string(),
        deploy_root: "unknown".to_string(),
        content_root_path: None,
        requires_game_root_confirmation: false,
        message: message.to_string(),
        file_count: 0,
        files: Vec::new(),
        candidates: Vec::new(),
        warnings: Vec::new(),
    }
}

fn detect_candidates(source_path: &Path, directories: &[PathBuf]) -> Vec<Candidate> {
    let native_pc_candidates = directories
        .iter()
        .filter(|path| file_name_equals(path, "nativepc"))
        .map(|path| Candidate {
            root_path: path.clone(),
            detection_method: "nativePcDirectory",
            deploy_root: DeployRoot::NativePc,
            depth: depth_from(source_path, path),
        })
        .collect::<Vec<_>>();

    if !native_pc_candidates.is_empty() {
        return native_pc_candidates;
    }

    if let Some(child_name) = common_native_pc_child_name(source_path) {
        return vec![Candidate {
            root_path: source_path.to_path_buf(),
            detection_method: "selectedNativePcChildDirectory",
            deploy_root: DeployRoot::NativePcChild(child_name),
            depth: 0,
        }];
    }

    directories
        .iter()
        .filter(|path| has_common_native_pc_child(path))
        .map(|path| Candidate {
            root_path: path.clone(),
            detection_method: "nativePcChildDirectory",
            deploy_root: DeployRoot::NativePc,
            depth: depth_from(source_path, path),
        })
        .collect()
}

fn scan_directories(root: &Path, progress: &OperationReporter) -> Result<ScanResult, String> {
    let mut directories = vec![root.to_path_buf()];
    let mut warnings = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        progress.report(
            "正在扫描目录",
            directories.len(),
            None,
            directory
                .file_name()
                .map(|name| name.to_string_lossy().to_string()),
        );
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!("Could not read directory {}: {error}", directory.display())
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "Could not read entry under {}: {error}",
                    directory.display()
                )
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("Could not read metadata {}: {error}", path.display()))?;

            if metadata.file_type().is_symlink() {
                warnings.push(format!("已跳过符号链接：{}", path.display()));
                continue;
            }

            if metadata.is_dir() {
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".acumod-nested")
                {
                    continue;
                }
                directories.push(path.clone());
                stack.push(path);
            }
        }
    }

    directories.sort_by_key(|path| (depth_from(root, path), path_to_string(path).to_lowercase()));

    Ok(ScanResult {
        directories,
        warnings,
    })
}

fn build_candidate_dtos(
    source_path: &Path,
    candidates: &[Candidate],
    progress: &OperationReporter,
) -> Result<Vec<ModImportCandidate>, String> {
    let mut dtos = Vec::new();

    for candidate in candidates {
        dtos.push(ModImportCandidate {
            root_path: path_to_string(&candidate.root_path),
            source_root_path: path_to_string(source_path),
            relative_path: candidate
                .root_path
                .strip_prefix(source_path)
                .map(path_to_string)
                .unwrap_or_else(|_| path_to_string(&candidate.root_path)),
            suggested_name: suggested_candidate_name(source_path, &candidate.root_path),
            archive_chain: Vec::new(),
            requires_game_root_confirmation: false,
            detection_method: candidate.detection_method.to_string(),
            deploy_root: deploy_root_label(&candidate.deploy_root).to_string(),
            file_count: build_file_previews(
                &candidate.root_path,
                &candidate.deploy_root,
                progress,
            )?
            .len(),
        });
    }

    dtos.sort_by_key(|candidate| Reverse(candidate.file_count));

    Ok(dtos)
}

fn build_file_previews(
    root: &Path,
    deploy_root: &DeployRoot,
    progress: &OperationReporter,
) -> Result<Vec<ModImportFilePreview>, String> {
    let mut files = Vec::new();
    collect_file_previews(root, root, deploy_root, &mut files, progress)?;
    files.sort_by_key(|file| file.deploy_relative_path.to_lowercase());
    progress.report("正在整理文件清单", files.len(), Some(files.len()), None);
    Ok(files)
}

fn collect_file_previews(
    root: &Path,
    directory: &Path,
    deploy_root: &DeployRoot,
    files: &mut Vec<ModImportFilePreview>,
    progress: &OperationReporter,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Could not read directory {}: {error}", directory.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not read entry under {}: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Could not read metadata {}: {error}", path.display()))?;

        if metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == ".acumod-nested")
            {
                continue;
            }
            collect_file_previews(root, &path, deploy_root, files, progress)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let relative_path = path.strip_prefix(root).map_err(|error| {
            format!(
                "Could not make {} relative to {}: {error}",
                path.display(),
                root.display()
            )
        })?;
        let source_relative_path = safe_relative_path(relative_path)?;
        let deploy_relative_path = match deploy_root {
            DeployRoot::NativePc => format!("nativePC/{source_relative_path}"),
            DeployRoot::NativePcChild(child_name) => {
                format!("nativePC/{child_name}/{source_relative_path}")
            }
            DeployRoot::GameRoot => source_relative_path.clone(),
        };

        files.push(ModImportFilePreview {
            source_path: path_to_string(&path),
            source_relative_path,
            deploy_relative_path,
        });
        progress.report(
            "正在读取 MOD 文件",
            files.len(),
            None,
            path.file_name()
                .map(|name| name.to_string_lossy().to_string()),
        );
    }

    Ok(())
}

fn has_common_native_pc_child(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };

    let common_names = COMMON_NATIVE_PC_CHILDREN
        .iter()
        .copied()
        .collect::<HashSet<_>>();

    for entry in entries.flatten() {
        let child_path = entry.path();

        if !child_path.is_dir() {
            continue;
        }

        let Some(name) = child_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if common_names.contains(name.to_ascii_lowercase().as_str()) {
            return true;
        }
    }

    false
}

fn common_native_pc_child_name(path: &Path) -> Option<String> {
    let name = path.file_name().and_then(|name| name.to_str())?;
    let normalized_name = name.to_ascii_lowercase();

    COMMON_NATIVE_PC_CHILDREN
        .contains(&normalized_name.as_str())
        .then(|| normalized_name)
}

fn list_installed_mods_from(installed_root: &Path) -> Result<InstalledModList, String> {
    let mut mods = Vec::new();
    let mut warnings = Vec::new();
    let category_store = load_or_initialize_mod_category_store_for_installed_root(installed_root)?;

    if !installed_root.exists() {
        return Ok(InstalledModList {
            mods,
            warnings,
            message: "尚未创建本地 MOD 目录。".to_string(),
        });
    }

    let entries = fs::read_dir(installed_root).map_err(|error| {
        format!(
            "Could not read installed MOD directory {}: {error}",
            installed_root.display()
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not read entry under {}: {error}",
                installed_root.display()
            )
        })?;
        let mod_path = entry.path();

        if !mod_path.is_dir()
            || mod_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
        {
            continue;
        }

        let manifest_path = mod_path.join("manifest.json");

        if !manifest_path.is_file() {
            warnings.push(format!("已跳过缺少清单文件的 MOD：{}", mod_path.display()));
            continue;
        }

        let manifest_json = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "Could not read MOD manifest {}: {error}",
                manifest_path.display()
            )
        })?;
        let manifest =
            serde_json::from_str::<InstalledModManifest>(&manifest_json).map_err(|error| {
                format!(
                    "Could not parse MOD manifest {}: {error}",
                    manifest_path.display()
                )
            })?;

        let content_path = mod_path.join("content");
        let manifest_context = InstalledManifestContext {
            mod_path: mod_path.clone(),
            content_path: content_path.clone(),
            manifest_path: manifest_path.clone(),
            manifest: manifest.clone(),
        };
        let original_model_replacements =
            match model_replacements_for_manifest(&manifest, &content_path) {
                Ok(model_replacements) => model_replacements,
                Err(_error) => {
                    warnings.push(format!(
                        "无法识别 {} 的模型替换信息，请检查 MOD 文件结构。",
                        manifest.name
                    ));
                    Vec::new()
                }
            };
        let model_replacements = match effective_model_replacements_for_context(
            &manifest_context,
            &manifest,
            &original_model_replacements,
        ) {
            Ok(model_replacements) => model_replacements,
            Err(_error) => {
                warnings.push(format!(
                    "无法应用 {} 已保存的模型修改，请重新设置替换模型。",
                    manifest.name
                ));
                original_model_replacements.clone()
            }
        };

        let categories = resolve_mod_categories(&category_store, &manifest.category_ids);
        let category_ids = categories
            .iter()
            .map(|category| category.id.clone())
            .collect();
        let effect_recognition = summarize_effect_paths(
            manifest
                .files
                .iter()
                .map(|file| file.deploy_relative_path.as_str()),
        );

        mods.push(InstalledModSummary {
            id: manifest.id.clone(),
            name: manifest_display_name(&manifest),
            original_name: manifest.name.clone(),
            note: manifest.note.clone(),
            category_ids,
            categories,
            mod_path: path_to_string(&mod_path),
            content_path: path_to_string(&mod_path.join("content")),
            manifest_path: path_to_string(&manifest_path),
            source_path: manifest.source_path,
            file_count: manifest.file_count,
            files: manifest.files,
            enabled: manifest.enabled,
            partially_overridden: manifest.partially_overridden,
            deploy_root: manifest.deploy_root,
            detection_method: manifest.detection_method,
            installed_at_unix_seconds: manifest.installed_at_unix_seconds,
            model_replacements,
            original_model_replacements,
            model_remap_count: manifest.model_remaps.len(),
            effect_remap_count: manifest.effect_remaps.len(),
            effect_recognition,
        });
    }

    mods.sort_by(|left, right| {
        right
            .installed_at_unix_seconds
            .cmp(&left.installed_at_unix_seconds)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    apply_mod_library_order(installed_root, &mut mods)?;

    let message = if mods.is_empty() {
        "本地 MOD 库为空。".to_string()
    } else {
        format!("本地 MOD 库共有 {} 个 MOD。", mods.len())
    };

    Ok(InstalledModList {
        mods,
        warnings,
        message,
    })
}

fn installed_mod_list_from_contexts(
    installed_root: &Path,
    contexts: &[InstalledManifestContext],
    category_store: &ModCategoryStore,
    progress: &OperationReporter,
    apply_saved_order: bool,
) -> Result<InstalledModList, String> {
    let mut mods = Vec::new();
    let mut warnings = Vec::new();
    let total = contexts.len();

    for (index, context) in contexts.iter().enumerate() {
        let manifest = &context.manifest;
        let original_model_replacements =
            match model_replacements_for_manifest(manifest, &context.content_path) {
                Ok(model_replacements) => model_replacements,
                Err(_error) => {
                    warnings.push(format!(
                        "无法识别 {} 的模型替换信息，请检查 MOD 文件结构。",
                        manifest.name
                    ));
                    Vec::new()
                }
            };
        let model_replacements = match effective_model_replacements_for_context(
            context,
            manifest,
            &original_model_replacements,
        ) {
            Ok(model_replacements) => model_replacements,
            Err(_error) => {
                warnings.push(format!(
                    "无法应用 {} 已保存的模型修改，请重新设置替换模型。",
                    manifest.name
                ));
                original_model_replacements.clone()
            }
        };
        let categories = resolve_mod_categories(category_store, &manifest.category_ids);
        let category_ids = categories
            .iter()
            .map(|category| category.id.clone())
            .collect();

        mods.push(InstalledModSummary {
            id: manifest.id.clone(),
            name: manifest_display_name(manifest),
            original_name: manifest.name.clone(),
            note: manifest.note.clone(),
            category_ids,
            categories,
            mod_path: path_to_string(&context.mod_path),
            content_path: path_to_string(&context.content_path),
            manifest_path: path_to_string(&context.manifest_path),
            source_path: manifest.source_path.clone(),
            file_count: manifest.file_count,
            files: manifest.files.clone(),
            enabled: manifest.enabled,
            partially_overridden: manifest.partially_overridden,
            deploy_root: manifest.deploy_root.clone(),
            detection_method: manifest.detection_method.clone(),
            installed_at_unix_seconds: manifest.installed_at_unix_seconds,
            model_replacements,
            original_model_replacements,
            model_remap_count: manifest.model_remaps.len(),
            effect_remap_count: manifest.effect_remaps.len(),
            effect_recognition: summarize_effect_paths(
                manifest
                    .files
                    .iter()
                    .map(|file| file.deploy_relative_path.as_str()),
            ),
        });
        progress.report(
            "正在整理 MOD 列表",
            index + 1,
            Some(total),
            Some(manifest_display_name(manifest)),
        );
    }

    mods.sort_by(|left, right| {
        right
            .installed_at_unix_seconds
            .cmp(&left.installed_at_unix_seconds)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    // 只有完整列表才能归一化并持久化全局顺序；局部快照若传入单个 MOD，
    // 绝不能把其余 ID 当作已卸载项从 manualModIds 中删除。
    if apply_saved_order {
        apply_mod_library_order(installed_root, &mut mods)?;
    }

    let message = if mods.is_empty() {
        "本地 MOD 库为空。".to_string()
    } else {
        format!("本地 MOD 库共有 {} 个 MOD。", mods.len())
    };

    Ok(InstalledModList {
        mods,
        warnings,
        message,
    })
}

fn model_replacements_for_manifest(
    manifest: &InstalledModManifest,
    content_path: &Path,
) -> Result<Vec<ModelReplacement>, String> {
    if manifest.schema_version >= CURRENT_MODEL_RECOGNITION_SCHEMA_VERSION {
        return Ok(manifest.model_replacements.clone());
    }

    recognize_model_replacements_for_library_files(&manifest.files, content_path)
}

fn recognize_model_replacements_for_library_files(
    files: &[InstalledModFile],
    content_path: &Path,
) -> Result<Vec<ModelReplacement>, String> {
    let deploy_relative_paths = files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let mut evam_files = Vec::new();

    if files.iter().any(|file| {
        Path::new(&file.deploy_relative_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("evam"))
    }) {
        let resolved_content_path = content_path.canonicalize().map_err(|error| {
            format!(
                "无法读取 MOD 库内容目录 {}：{error}",
                content_path.display()
            )
        })?;

        for file in files {
            if !Path::new(&file.deploy_relative_path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("evam"))
            {
                continue;
            }

            let relative_path = relative_string_to_path(&file.deploy_relative_path)?;
            let source_path = content_path.join(relative_path);
            let Ok(metadata) = source_path.metadata() else {
                continue;
            };
            if metadata.len() != 26 {
                continue;
            }
            let resolved_source_path = source_path.canonicalize().map_err(|error| {
                format!("无法解析 EVAM 文件 {}：{error}", source_path.display())
            })?;
            if !resolved_source_path.starts_with(&resolved_content_path) {
                return Err(format!(
                    "EVAM 文件越过了 MOD 库内容目录：{}",
                    resolved_source_path.display()
                ));
            }
            let bytes = fs::read(&resolved_source_path).map_err(|error| {
                format!(
                    "无法读取 EVAM 文件 {}：{error}",
                    resolved_source_path.display()
                )
            })?;
            evam_files.push(EvamRecognitionFile {
                deploy_relative_path: file.deploy_relative_path.clone(),
                bytes,
            });
        }
    }

    recognize_model_replacements_with_evam(&deploy_relative_paths, &evam_files)
}

fn effective_model_replacements_for_context(
    context: &InstalledManifestContext,
    manifest: &InstalledModManifest,
    original_replacements: &[ModelReplacement],
) -> Result<Vec<ModelReplacement>, String> {
    let effective_files =
        effective_remap_files_for_context_with_manifest(context, manifest, original_replacements)?;
    let paths = effective_files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let mut effective_replacements = recognize_model_replacements(&paths)?;
    let (groups, _) = build_model_remap_groups(original_replacements, &manifest.model_remaps)?;
    copy_effective_slinger_associations(
        original_replacements,
        &groups,
        &mut effective_replacements,
    );
    Ok(effective_replacements)
}

fn copy_effective_slinger_associations(
    original_replacements: &[ModelReplacement],
    groups: &[ModelRemapGroup],
    effective_replacements: &mut [ModelReplacement],
) {
    for original in original_replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "slinger")
    {
        if original.associations.is_empty() {
            continue;
        }
        let group_key = format!("slinger:{}", original.model_id);
        let effective_model_id = groups
            .iter()
            .find(|group| group.group_key == group_key)
            .and_then(|group| group.selected_target_id.as_deref())
            .and_then(|target_id| target_id.strip_prefix("slinger:"))
            .unwrap_or(&original.model_id);
        let associations = original
            .associations
            .iter()
            .map(|association| {
                let group_key = format!("armor:{}", association.model_id);
                let effective_armor_id = groups
                    .iter()
                    .find(|group| group.group_key == group_key)
                    .and_then(|group| group.selected_target_id.as_deref())
                    .and_then(|target_id| target_id.strip_prefix("armor:"))
                    .unwrap_or(&association.model_id);
                let mut effective_association = association.clone();
                effective_association.model_id = effective_armor_id.to_string();
                let armor_names = effective_replacements
                    .iter()
                    .filter(|replacement| {
                        replacement.model_kind == "armor"
                            && replacement.model_id == effective_armor_id
                    })
                    .flat_map(|replacement| replacement.display_names.iter().cloned())
                    .collect::<Vec<_>>();
                if !armor_names.is_empty() {
                    effective_association.display_names =
                        vec![armor_set_label(&armor_names, effective_armor_id)];
                }
                effective_association
            })
            .collect::<Vec<_>>();
        if let Some(effective) = effective_replacements.iter_mut().find(|replacement| {
            replacement.model_kind == "slinger" && replacement.model_id == effective_model_id
        }) {
            effective.associations = associations;
        }
    }
}

fn refresh_manifest_model_replacements(
    context: &mut InstalledManifestContext,
) -> Result<(), String> {
    if context.manifest.schema_version < CURRENT_MODEL_RECOGNITION_SCHEMA_VERSION {
        context.manifest.model_replacements =
            model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    }

    Ok(())
}

fn manifest_display_name(manifest: &InstalledModManifest) -> String {
    manifest
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&manifest.name)
        .to_string()
}

fn model_kind_label(model_kind: &str) -> &str {
    match model_kind {
        "weapon" => "武器",
        "armor" => "防具",
        "hair" => "发型",
        "palicoWeapon" => "随从武器",
        "palicoArmor" => "随从防具",
        "kinsect" => "猎虫",
        "pendant" => "挂件",
        "npc" => "NPC",
        "slinger" => "投射器",
        "voice" => "人物语音",
        "weaponVoice" => "武器语音",
        "plugin" => "插件",
        "face" => "脸型",
        "monster" => "怪物",
        "poogie" => "噗吱猪服装",
        "furniture" => "家具",
        "playerAccessory" => "玩家附件",
        "palicoAccessory" => "随从附件",
        _ => "未识别",
    }
}

fn get_mod_effect_remap_details_from(
    installed_root: &Path,
    mod_id: &str,
) -> Result<ModEffectRemapDetails, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let paths = context
        .manifest
        .files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let (groups, warnings) = build_effect_remap_groups(&paths, &context.manifest.effect_remaps)?;
    let message = if groups.is_empty() {
        "未发现已验证可改绑的独立武器特效槽。通用命中、会心、全局 EPV/EVWP 与未知特效仅支持识别。"
            .to_string()
    } else if context.manifest.enabled || !context.manifest.deployed_files.is_empty() {
        "请先禁用并清理该 MOD 的部署记录，再修改特效目标。".to_string()
    } else {
        format!("可安全改绑 {} 个已验证的独立武器特效槽。", groups.len())
    };
    Ok(ModEffectRemapDetails {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        enabled: context.manifest.enabled,
        groups,
        warnings,
        message,
    })
}

fn preview_mod_effect_remap_from(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
) -> Result<ModEffectRemapPlan, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    ensure_manifest_can_remap(&context.manifest)?;
    let paths = context
        .manifest
        .files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let (groups, _) = build_effect_remap_groups(&paths, &context.manifest.effect_remaps)?;
    let group = groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| format!("未找到可改绑特效分组：{group_key}"))?;
    let normalized_target_id = normalize_effect_target_id(group, target_id)?;
    let selections = updated_effect_remap_selections(
        &context.manifest.effect_remaps,
        group_key,
        normalized_target_id.clone(),
    );
    let effective_paths = build_effective_effect_remap_paths(&paths, &selections)?;
    let files = paths
        .iter()
        .zip(&effective_paths)
        .filter_map(|(source, effective)| {
            (!source.eq_ignore_ascii_case(effective)).then(|| ModRemapPlanFile {
                source_deploy_relative_path: source.clone(),
                effective_deploy_relative_path: effective.clone(),
                path_changed: true,
                mrl3_rewrite_count: 0,
                evam_rewrite_count: 0,
            })
        })
        .collect::<Vec<_>>();
    let target_label = normalized_target_id
        .as_deref()
        .and_then(|target_id| {
            group
                .targets
                .iter()
                .find(|target| target.target_id == target_id)
        })
        .map(|target| target.target_label.clone())
        .unwrap_or_else(|| "恢复导入时的原始槽位".to_string());
    let changed_file_count = files.len();
    Ok(ModEffectRemapPlan {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        group_key: group_key.to_string(),
        source_label: group.source_label.clone(),
        target_id: normalized_target_id,
        target_label,
        changed_file_count,
        files,
        warnings: vec!["只会重定向已验证的本地槽位部署路径；不会改写 MOD 原文件、EFX/EPV 二进制内容，也不会处理全局会心或通用命中特效。".to_string()],
        message: format!("本次特效改绑会改变 {changed_file_count} 个部署文件路径。"),
    })
}

fn apply_mod_effect_remap_from_with_progress(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
    progress: &OperationReporter,
) -> Result<ModEffectRemapApplyResult, String> {
    progress.report("正在检查特效替换", 0, None, None);
    let plan = preview_mod_effect_remap_from(installed_root, mod_id, group_key, target_id)?;
    progress.report("正在保存特效替换设置", 0, Some(1), None);
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    ensure_manifest_can_remap(&context.manifest)?;
    let paths = context
        .manifest
        .files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let (groups, _) = build_effect_remap_groups(&paths, &context.manifest.effect_remaps)?;
    let group = groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| format!("未找到可改绑特效分组：{group_key}"))?;
    let normalized_target_id = normalize_effect_target_id(group, plan.target_id.clone())?;
    context.manifest.effect_remaps = updated_effect_remap_selections(
        &context.manifest.effect_remaps,
        group_key,
        normalized_target_id.clone(),
    );
    context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    save_manifest(&context.manifest_path, &context.manifest)?;
    progress.report("正在保存特效替换设置", 1, Some(1), None);
    Ok(ModEffectRemapApplyResult {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        group_key: group_key.to_string(),
        target_id: normalized_target_id,
        selection_count: context.manifest.effect_remaps.len(),
        changed_file_count: plan.changed_file_count,
        message: "特效替换目标已保存，本地 MOD 原始副本未被修改。".to_string(),
    })
}

fn normalize_effect_target_id(
    group: &EffectRemapGroup,
    target_id: Option<String>,
) -> Result<Option<String>, String> {
    let Some(target_id) = target_id else {
        return Ok(None);
    };
    let target_id = target_id.trim();
    if target_id.is_empty() {
        return Ok(None);
    }
    if group
        .targets
        .iter()
        .any(|target| target.target_id == target_id)
    {
        Ok(Some(target_id.to_string()))
    } else {
        Err(format!("不支持的特效替换目标：{target_id}"))
    }
}

fn get_mod_remap_details_from(
    installed_root: &Path,
    mod_id: &str,
) -> Result<ModRemapDetails, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let replacements = model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    let (groups, warnings) =
        build_model_remap_groups(&replacements, &context.manifest.model_remaps)?;
    let message = if groups.is_empty() {
        "该 MOD 没有支持改绑的模型目标；人物语音仅保留识别。".to_string()
    } else if context.manifest.enabled || !context.manifest.deployed_files.is_empty() {
        "请先禁用并清理该 MOD 的部署记录，再修改替换目标。".to_string()
    } else {
        format!("可修改 {} 个模型替换分组。", groups.len())
    };

    Ok(ModRemapDetails {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        enabled: context.manifest.enabled,
        groups,
        warnings,
        message,
    })
}

fn preview_mod_remap_from(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
) -> Result<ModRemapPlan, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    ensure_manifest_can_remap(&context.manifest)?;
    let replacements = model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    let (current_groups, _) =
        build_model_remap_groups(&replacements, &context.manifest.model_remaps)?;
    let current_group = current_groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| format!("未找到可改绑模型分组：{group_key}"))?;
    let normalized_target_id = normalize_requested_target_id(current_group, target_id)?;
    let selections = updated_model_remap_selections(
        &context.manifest.model_remaps,
        current_group,
        normalized_target_id.clone(),
    );
    let (preview_groups, mut warnings) = build_model_remap_groups(&replacements, &selections)?;
    let preview_group = preview_groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| format!("未找到可改绑模型分组：{group_key}"))?;

    let mut preview_manifest = context.manifest.clone();
    preview_manifest.model_remaps = selections;
    let current_remap_files = effective_remap_files_for_context_with_manifest(
        &context,
        &context.manifest,
        &replacements,
    )?;
    let preview_remap_files = effective_remap_files_for_context_with_manifest(
        &context,
        &preview_manifest,
        &replacements,
    )?;
    if current_remap_files.len() != context.manifest.files.len()
        || preview_remap_files.len() != preview_manifest.files.len()
    {
        return Err("模型改绑后的文件索引与 MOD 清单不一致。".to_string());
    }
    let current_excluded_paths = context
        .manifest
        .deployment_exclusions
        .iter()
        .map(|exclusion| conflict_path_key(&exclusion.library_relative_path))
        .collect::<HashSet<_>>();
    let preview_excluded_paths = preview_manifest
        .deployment_exclusions
        .iter()
        .map(|exclusion| conflict_path_key(&exclusion.library_relative_path))
        .collect::<HashSet<_>>();
    let mut files = Vec::new();
    let mut total_mrl3_rewrite_count = 0;
    let mut total_evam_rewrite_count = 0;
    let mut changed_armor_epv_file_count = 0;
    let current_automatic_exclusion_count = current_remap_files
        .iter()
        .filter(|file| file.automatic_exclusion_reason.is_some())
        .count();
    let automatic_exclusion_count = preview_remap_files
        .iter()
        .filter(|file| file.automatic_exclusion_reason.is_some())
        .count();

    for index in 0..preview_remap_files.len() {
        let current_effective_file = effective_installed_file_from_remap(
            &context.manifest,
            &current_excluded_paths,
            current_remap_files[index].clone(),
        )
        .transpose()?;
        let effective_file = effective_installed_file_from_remap(
            &preview_manifest,
            &preview_excluded_paths,
            preview_remap_files[index].clone(),
        )
        .transpose()?;
        let (Some(current_effective_file), Some(effective_file)) =
            (current_effective_file, effective_file)
        else {
            // 用户手动排除和 DAT 自动排除均不产生可部署文件，无须检查二进制改写。
            continue;
        };
        if !current_effective_file
            .deploy_relative_path
            .eq_ignore_ascii_case(&effective_file.deploy_relative_path)
            && (is_armor_epv_deploy_path(&current_effective_file.deploy_relative_path)
                || is_armor_epv_deploy_path(&effective_file.deploy_relative_path))
        {
            changed_armor_epv_file_count += 1;
        }

        let source_path = source_path_for_installed_file(&context, &effective_file.installed_file)?;
        let mrl3_rewrite_count = preview_mrl3_rewrite_count(&source_path, &effective_file)?;
        let evam_rewrite_count = preview_evam_rewrite_count(&source_path, &effective_file)?;
        let path_changed = !effective_file
            .installed_file
            .deploy_relative_path
            .eq_ignore_ascii_case(&effective_file.deploy_relative_path);
        if path_changed || mrl3_rewrite_count > 0 || evam_rewrite_count > 0 {
            files.push(ModRemapPlanFile {
                source_deploy_relative_path: effective_file
                    .installed_file
                    .deploy_relative_path
                    .clone(),
                effective_deploy_relative_path: effective_file.deploy_relative_path.clone(),
                path_changed,
                mrl3_rewrite_count,
                evam_rewrite_count,
            });
        }
        total_mrl3_rewrite_count += mrl3_rewrite_count;
        total_evam_rewrite_count += evam_rewrite_count;
    }

    if files.is_empty() {
        warnings.push("当前选择不会改变原始部署路径。".to_string());
    }
    if changed_armor_epv_file_count > 0 {
        warnings.push(format!(
            "检测到 {changed_armor_epv_file_count} 个防具特效触发文件。改绑后会同步调整其部署位置；启用后请在游戏中确认装备特效是否正常。"
        ));
    }
    if automatic_exclusion_count > 0 {
        warnings.push(format!(
            "已验证 DAT 型防具资源：部署时将自动排除 {automatic_exclusion_count} 个 armor.am_dat，并按命中的部位标准化核心文件名。MOD 库原文件不会被修改。"
        ));
    } else if current_automatic_exclusion_count > 0 {
        warnings.push(format!(
            "恢复后会重新部署 {current_automatic_exclusion_count} 个原始 armor.am_dat，使 MOD 回到导入时的来源防具映射。"
        ));
    }
    if let Some(target_id) = normalized_target_id.as_deref() {
        if let Some(warning) = special_character_armor_target_warning(target_id) {
            warnings.push(warning);
        }
    }
    let target_label = normalized_target_id
        .as_deref()
        .map(|target_id| remap_target_label(preview_group, target_id))
        .transpose()?
        .unwrap_or_else(|| "恢复导入时的原始目标".to_string());
    let source_label = remap_source_label(current_group);
    let changed_file_count = files.len();

    Ok(ModRemapPlan {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        group_key: group_key.to_string(),
        source_label,
        target_id: normalized_target_id,
        target_label,
        changed_file_count,
        mrl3_rewrite_count: total_mrl3_rewrite_count,
        evam_rewrite_count: total_evam_rewrite_count,
        files,
        warnings,
        message: format!(
            "本次改绑会改变 {changed_file_count} 个部署文件，精确修正 {total_mrl3_rewrite_count} 条 MRL3 贴图路径和 {total_evam_rewrite_count} 个 EVAM 飞翔爪绑定。"
        ),
    })
}

#[cfg(test)]
fn apply_mod_remap_from(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
) -> Result<ModRemapApplyResult, String> {
    apply_mod_remap_from_with_progress(
        installed_root,
        mod_id,
        group_key,
        target_id,
        &OperationReporter::default(),
    )
}

fn apply_mod_remap_from_with_progress(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
    progress: &OperationReporter,
) -> Result<ModRemapApplyResult, String> {
    progress.report("正在检查模型替换", 0, None, None);
    let plan = preview_mod_remap_from(installed_root, mod_id, group_key, target_id.clone())?;
    progress.report("正在保存模型替换设置", 0, Some(1), None);
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    ensure_manifest_can_remap(&context.manifest)?;
    refresh_manifest_model_replacements(&mut context)?;
    let replacements = context.manifest.model_replacements.clone();
    let (groups, _) = build_model_remap_groups(&replacements, &context.manifest.model_remaps)?;
    let group = groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| format!("未找到可改绑模型分组：{group_key}"))?;
    let normalized_target_id = normalize_requested_target_id(group, target_id)?;
    context.manifest.model_remaps = updated_model_remap_selections(
        &context.manifest.model_remaps,
        group,
        normalized_target_id.clone(),
    );
    context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    save_manifest(&context.manifest_path, &context.manifest)?;
    progress.report("正在保存模型替换设置", 1, Some(1), None);

    Ok(ModRemapApplyResult {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        group_key: group_key.to_string(),
        target_id: normalized_target_id,
        selection_count: context.manifest.model_remaps.len(),
        changed_file_count: plan.changed_file_count,
        mrl3_rewrite_count: plan.mrl3_rewrite_count,
        evam_rewrite_count: plan.evam_rewrite_count,
        message: "模型替换目标已保存，本地 MOD 原始副本未被修改。".to_string(),
    })
}

fn ensure_manifest_can_remap(manifest: &InstalledModManifest) -> Result<(), String> {
    if manifest.enabled {
        return Err("请先禁用该 MOD，再修改模型替换目标。".to_string());
    }
    if !manifest.deployed_files.is_empty() {
        return Err("该 MOD 仍有部署记录，请先完成禁用清理再修改模型替换目标。".to_string());
    }
    Ok(())
}

fn normalize_requested_target_id(
    group: &ModelRemapGroup,
    target_id: Option<String>,
) -> Result<Option<String>, String> {
    let Some(target_id) = target_id else {
        return Ok(None);
    };
    let target_id = target_id.trim();
    if target_id.is_empty() {
        return Ok(None);
    }
    let normalized = if group.model_kind == "slinger" && target_id.starts_with("slg") {
        format!("slinger:{target_id}")
    } else {
        target_id.to_string()
    };
    if group.original_target_id.as_deref() == Some(normalized.as_str()) {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn updated_model_remap_selections(
    current: &[ModelRemapSelection],
    group: &ModelRemapGroup,
    target_id: Option<String>,
) -> Vec<ModelRemapSelection> {
    let mut selections = current
        .iter()
        .filter(|selection| selection.group_key != group.group_key)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(target_id) = target_id {
        selections.push(ModelRemapSelection {
            group_key: group.group_key.clone(),
            target_id,
        });
    }
    selections.sort_by(|left, right| left.group_key.cmp(&right.group_key));
    selections
}

fn remap_source_label(group: &ModelRemapGroup) -> String {
    if group.model_kind == "armor" {
        return armor_set_label(
            &group.source_display_names,
            group
                .source_model_ids
                .first()
                .map(String::as_str)
                .unwrap_or("未知 ID"),
        );
    }

    group
        .source_display_names
        .first()
        .cloned()
        .unwrap_or_else(|| group.source_model_ids.join(" + "))
}

fn remap_target_label(group: &ModelRemapGroup, target_id: &str) -> Result<String, String> {
    let target = group
        .targets
        .iter()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| format!("目标不适用于当前分组：{target_id}"))?;
    if group.model_kind == "armor" {
        return Ok(armor_set_label(&target.display_names, &target.model_id));
    }

    Ok(target
        .display_names
        .first()
        .cloned()
        .unwrap_or_else(|| target.model_id.clone()))
}

fn armor_set_label(display_names: &[String], model_id: &str) -> String {
    for display_name in display_names {
        for suffix in ["·头部", "·身体", "·腕部", "·腰部", "·脚部"] {
            if let Some(set_name) = display_name.strip_suffix(suffix) {
                if !set_name.is_empty() {
                    return set_name.to_string();
                }
            }
        }
    }

    model_id.to_string()
}

fn scan_mod_cleanup_candidates_from(
    contexts: &[InstalledManifestContext],
    progress: &OperationReporter,
) -> Result<ModCleanupScan, String> {
    let rules = mod_cleanup_rules()?;
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();
    let mut scanned_file_count = 0;
    let mut local_keep_count = 0;
    let mut local_remove_count = 0;
    let mut ai_review_count = 0;
    let total_file_count = contexts
        .iter()
        .map(|context| context.manifest.files.len())
        .sum::<usize>();
    progress.report("正在盘点 MOD 文件", 0, Some(total_file_count), None);

    for context in contexts {
        let excluded_paths = context
            .manifest
            .deployment_exclusions
            .iter()
            .map(|exclusion| conflict_path_key(&exclusion.library_relative_path))
            .collect::<HashSet<_>>();
        let effective_by_library_path = effective_installed_files_for_context(context)?
            .into_iter()
            .map(|file| {
                (
                    conflict_path_key(&file.installed_file.library_relative_path),
                    file.deploy_relative_path,
                )
            })
            .collect::<HashMap<_, _>>();
        let deployed_path_keys = context
            .manifest
            .deployed_files
            .iter()
            .map(|file| conflict_path_key(&file.deploy_relative_path))
            .collect::<HashSet<_>>();
        let recognized_path_keys = recognized_cleanup_path_keys(&context.manifest);

        for file in &context.manifest.files {
            // 无论文件最终保留、排除还是需要 AI 判断，都计入进度，避免大 MOD 长时间没有反馈。
            let inspected_file_count = scanned_file_count + 1;
            let library_key = conflict_path_key(&file.library_relative_path);
            if excluded_paths.contains(&library_key) {
                scanned_file_count = inspected_file_count;
                progress.report(
                    "正在盘点 MOD 文件",
                    scanned_file_count,
                    Some(total_file_count),
                    Some(manifest_display_name(&context.manifest)),
                );
                continue;
            }
            scanned_file_count = inspected_file_count;
            progress.report(
                "正在盘点 MOD 文件",
                scanned_file_count,
                Some(total_file_count),
                Some(manifest_display_name(&context.manifest)),
            );
            let source_path = match source_path_for_installed_file(context, file) {
                Ok(path) => path,
                Err(error) => {
                    warnings.push(error);
                    continue;
                }
            };
            let size_bytes = match fs::metadata(&source_path) {
                Ok(metadata) if metadata.is_file() => metadata.len(),
                Ok(_) => continue,
                Err(error) => {
                    warnings.push(format!(
                        "无法读取候选文件元数据 {}：{error}",
                        file.library_relative_path
                    ));
                    continue;
                }
            };
            let effective_deploy_path = effective_by_library_path
                .get(&library_key)
                .cloned()
                .unwrap_or_else(|| file.deploy_relative_path.clone());
            let evidence = cleanup_rule_evidence(
                context,
                file,
                &effective_deploy_path,
                &recognized_path_keys,
                rules,
                &mut warnings,
            );
            if !evidence.keep_signals.is_empty() && evidence.exclude_signals.is_empty() {
                local_keep_count += 1;
                continue;
            }
            let (review_source, risk_level, local_kind, local_hint) =
                if evidence.keep_signals.is_empty() && !evidence.exclude_signals.is_empty() {
                    local_remove_count += 1;
                    (
                        "localRule",
                        "low",
                        evidence.kind.as_str(),
                        evidence.exclude_signals.join("；"),
                    )
                } else {
                    ai_review_count += 1;
                    let risk_level = if evidence.keep_signals.is_empty() {
                        "medium"
                    } else {
                        "high"
                    };
                    let hint = if evidence.keep_signals.is_empty()
                        && evidence.exclude_signals.is_empty()
                    {
                        "本地规则无法确定该文件用途".to_string()
                    } else {
                        format!(
                            "保留证据：{}；排除证据：{}",
                            evidence.keep_signals.join("、"),
                            evidence.exclude_signals.join("、")
                        )
                    };
                    ("acuAi", risk_level, "ambiguous", hint)
                };
            candidates.push(ModCleanupCandidate {
                candidate_id: cleanup_candidate_id(
                    &context.manifest.id,
                    &file.library_relative_path,
                ),
                mod_id: context.manifest.id.clone(),
                mod_name: manifest_display_name(&context.manifest),
                library_relative_path: file.library_relative_path.clone(),
                deploy_relative_path: effective_deploy_path.clone(),
                extension: cleanup_file_extension(&file.deploy_relative_path),
                size_bytes,
                local_kind: local_kind.to_string(),
                local_hint,
                currently_deployed: deployed_path_keys
                    .contains(&conflict_path_key(&effective_deploy_path)),
                review_source: review_source.to_string(),
                risk_level: risk_level.to_string(),
                keep_signals: evidence.keep_signals,
                exclude_signals: evidence.exclude_signals,
            });
        }
    }

    candidates.sort_by(|left, right| {
        left.mod_name
            .to_lowercase()
            .cmp(&right.mod_name.to_lowercase())
            .then_with(|| {
                left.library_relative_path
                    .to_lowercase()
                    .cmp(&right.library_relative_path.to_lowercase())
            })
    });
    let message = if candidates.is_empty() {
        format!("已盘点 {scanned_file_count} 个文件，本地规则未发现需要清理或审核的项目。")
    } else {
        format!(
            "已盘点 {scanned_file_count} 个文件：本地保留 {local_keep_count} 个，本地建议排除 {local_remove_count} 个，需要 AcuAI 审核 {ai_review_count} 个。"
        )
    };
    Ok(ModCleanupScan {
        installed_mod_count: contexts.len(),
        scanned_file_count,
        local_keep_count,
        local_remove_count,
        ai_review_count,
        rule_version: rules.schema_version,
        candidate_count: candidates.len(),
        candidates,
        warnings,
        message,
    })
}

struct CleanupRuleEvidence {
    keep_signals: Vec<String>,
    exclude_signals: Vec<String>,
    kind: String,
}

fn mod_cleanup_rules() -> Result<&'static ModCleanupRules, String> {
    static RULES: OnceLock<Result<ModCleanupRules, String>> = OnceLock::new();
    match RULES.get_or_init(|| {
        serde_json::from_str(MOD_CLEANUP_RULES_JSON)
            .map_err(|error| format!("无法解析内置 MOD 清理规则：{error}"))
    }) {
        Ok(rules) if rules.schema_version > 0 => Ok(rules),
        Ok(_) => Err("内置 MOD 清理规则版本无效。".to_string()),
        Err(error) => Err(error.clone()),
    }
}

fn recognized_cleanup_path_keys(manifest: &InstalledModManifest) -> HashSet<String> {
    manifest
        .model_replacements
        .iter()
        .flat_map(|replacement| {
            replacement.matched_files.iter().chain(
                replacement
                    .associations
                    .iter()
                    .flat_map(|association| association.matched_files.iter()),
            )
        })
        .map(|path| conflict_path_key(path))
        .collect()
}

fn cleanup_rule_evidence(
    context: &InstalledManifestContext,
    file: &InstalledModFile,
    effective_deploy_path: &str,
    recognized_path_keys: &HashSet<String>,
    rules: &ModCleanupRules,
    warnings: &mut Vec<String>,
) -> CleanupRuleEvidence {
    let normalized_path = file.deploy_relative_path.replace('\\', "/");
    let lower_path = normalized_path.to_lowercase();
    let file_name = lower_path.rsplit('/').next().unwrap_or(&lower_path);
    let extension = cleanup_file_extension(&lower_path);
    let components = lower_path.split('/').collect::<Vec<_>>();
    let in_native_pc = lower_path.starts_with("nativepc/");
    let in_plugins =
        lower_path.starts_with("nativepc/plugins/") || lower_path.starts_with("plugins/");
    let mut keep_signals = Vec::new();
    let mut exclude_signals = Vec::new();
    let mut kind = "ambiguous".to_string();

    if (recognized_path_keys.contains(&conflict_path_key(&file.deploy_relative_path))
        || recognized_path_keys.contains(&conflict_path_key(effective_deploy_path)))
        && (rules.runtime_extensions.contains(&extension) || in_plugins)
    {
        keep_signals.push("已被游戏内容识别器命中".to_string());
    }
    if in_native_pc && rules.runtime_extensions.contains(&extension) {
        keep_signals.push("位于 nativePC 的已知 MHW 运行资源".to_string());
    }
    if in_plugins {
        keep_signals.push(if rules.plugin_runtime_extensions.contains(&extension) {
            "位于插件目录的已知运行依赖".to_string()
        } else {
            "位于高风险插件运行目录".to_string()
        });
    }
    if !in_native_pc && rules.game_root_runtime_extensions.contains(&extension) {
        keep_signals.push("游戏根目录加载器资源".to_string());
    }

    if rules.exact_junk_names.contains(file_name)
        || components
            .iter()
            .any(|component| rules.junk_path_components.contains(*component))
    {
        kind = "systemJunk".to_string();
        exclude_signals.push("操作系统、压缩工具或开发环境生成文件".to_string());
    }
    if rules
        .backup_suffixes
        .iter()
        .any(|suffix| file_name.ends_with(suffix))
    {
        kind = "backup".to_string();
        exclude_signals.push("文件名使用明确的备份后缀".to_string());
    }
    let normalized_tool_name = file_name.trim_start_matches(['.', '_', '-']);
    let file_stem = normalized_tool_name
        .split('.')
        .next()
        .unwrap_or(normalized_tool_name);
    if extension == "exe"
        && rules
            .known_authoring_tool_prefixes
            .iter()
            .any(|prefix| file_stem.starts_with(prefix))
    {
        kind = "authoringTool".to_string();
        exclude_signals.push("已知 MOD 制作工具，不是游戏运行文件".to_string());
    }
    let has_document_keyword = rules
        .documentation_keywords
        .iter()
        .any(|keyword| file_name.contains(keyword));
    let native_pc_root_file = lower_path
        .strip_prefix("nativepc/")
        .is_some_and(|relative| !relative.contains('/'));
    if rules.documentation_extensions.contains(&extension)
        && (has_document_keyword
            || native_pc_root_file
            || matches!(extension.as_str(), "url" | "lnk"))
    {
        kind = "document".to_string();
        exclude_signals.push("明确的说明、教程或链接文件".to_string());
    }
    if rules.preview_extensions.contains(&extension)
        && rules
            .preview_keywords
            .iter()
            .any(|keyword| file_name.contains(keyword))
    {
        kind = "preview".to_string();
        exclude_signals.push("文件名具有明确的预览或截图特征".to_string());
    }
    if in_native_pc && rules.archive_extensions.contains(&extension) {
        kind = "archive".to_string();
        exclude_signals.push("nativePC 内的嵌套压缩包不会被游戏直接读取".to_string());
    }
    match duplicate_copy_original(context, file) {
        Ok(Some(original_path)) => {
            kind = "duplicateCopy".to_string();
            exclude_signals.push(format!("与规范名称文件内容相同：{original_path}"));
        }
        Ok(None) => {}
        Err(error) => warnings.push(error),
    }

    CleanupRuleEvidence {
        keep_signals,
        exclude_signals,
        kind,
    }
}

fn cleanup_file_extension(path: &str) -> String {
    let file_name = path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_lowercase();
    file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_string())
        .unwrap_or_default()
}

fn decode_cleanup_text(bytes: &[u8]) -> Result<String, String> {
    if let Some(content) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(content.to_vec())
            .map_err(|_| "文本不是有效的 UTF-8 内容。".to_string());
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little_endian = bytes.starts_with(&[0xff, 0xfe]);
        let content = &bytes[2..];
        if content.len() % 2 != 0 {
            return Err("UTF-16 文本长度无效。".to_string());
        }
        let units = content
            .chunks_exact(2)
            .map(|chunk| {
                if little_endian {
                    u16::from_le_bytes([chunk[0], chunk[1]])
                } else {
                    u16::from_be_bytes([chunk[0], chunk[1]])
                }
            })
            .collect::<Vec<_>>();
        return String::from_utf16(&units).map_err(|_| "文本不是有效的 UTF-16 内容。".to_string());
    }
    if bytes.contains(&0) {
        return Err("文件包含二进制内容，拒绝发送给 AcuAI。".to_string());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "文本不是有效的 UTF-8 内容。".to_string())
}

fn redact_cleanup_text(content: String) -> String {
    content
        .lines()
        .map(|line| {
            let lower = line.to_lowercase();
            if [
                "api_key",
                "apikey",
                "access_token",
                "password",
                "secret",
                ":\\users\\",
                ":/users/",
            ]
            .iter()
            .any(|token| lower.contains(token))
            {
                "[已隐藏可能的凭据或本地路径]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn duplicate_copy_original(
    context: &InstalledManifestContext,
    file: &InstalledModFile,
) -> Result<Option<String>, String> {
    let Some(original_deploy_path) = original_path_for_named_copy(&file.deploy_relative_path)
    else {
        return Ok(None);
    };
    let Some(original_file) = context.manifest.files.iter().find(|candidate| {
        conflict_path_key(&candidate.deploy_relative_path)
            == conflict_path_key(&original_deploy_path)
    }) else {
        return Ok(None);
    };
    let copy_source = source_path_for_installed_file(context, file)?;
    let original_source = source_path_for_installed_file(context, original_file)?;
    regular_files_are_equal(&copy_source, &original_source)
        .map(|same| same.then_some(original_file.deploy_relative_path.clone()))
}

fn original_path_for_named_copy(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let (parent, file_name) = normalized
        .rsplit_once('/')
        .map_or(("", normalized.as_str()), |(parent, file_name)| {
            (parent, file_name)
        });
    let (stem, extension) = file_name.rsplit_once('.')?;
    let lower_stem = stem.to_lowercase();
    let suffix = [" - copy", " copy", " - 副本", " 副本"]
        .into_iter()
        .find(|suffix| lower_stem.ends_with(suffix))?;
    let original_stem = stem[..stem.len() - suffix.len()].trim_end();
    if original_stem.is_empty() {
        return None;
    }
    let original_name = format!("{original_stem}.{extension}");
    Some(if parent.is_empty() {
        original_name
    } else {
        format!("{parent}/{original_name}")
    })
}

fn cleanup_candidate_id(mod_id: &str, library_relative_path: &str) -> String {
    // 稳定 FNV-1a 足以作为本地候选键；执行时仍会同时复核 MOD ID 和相对路径。
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in format!("{mod_id}\0{}", conflict_path_key(library_relative_path)).bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("cleanup-{hash:016x}")
}

fn mod_cleanup_exclusion_list_from(
    contexts: &[InstalledManifestContext],
) -> ModCleanupExclusionList {
    let mut latest = None::<(u64, String)>;
    let mut groups = contexts
        .iter()
        .filter_map(|context| {
            if context.manifest.deployment_exclusions.is_empty() {
                return None;
            }
            for exclusion in &context.manifest.deployment_exclusions {
                if latest
                    .as_ref()
                    .is_none_or(|(timestamp, _)| exclusion.excluded_at_unix_seconds >= *timestamp)
                {
                    latest = Some((
                        exclusion.excluded_at_unix_seconds,
                        exclusion.batch_id.clone(),
                    ));
                }
            }
            Some(ModCleanupExclusionGroup {
                mod_id: context.manifest.id.clone(),
                mod_name: manifest_display_name(&context.manifest),
                exclusions: context.manifest.deployment_exclusions.clone(),
            })
        })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        left.mod_name
            .to_lowercase()
            .cmp(&right.mod_name.to_lowercase())
    });
    let exclusion_count = groups.iter().map(|group| group.exclusions.len()).sum();
    ModCleanupExclusionList {
        exclusion_count,
        latest_batch_id: latest.map(|(_, batch_id)| batch_id),
        groups,
        message: format!("当前共有 {exclusion_count} 个部署排除项。"),
    }
}

fn apply_mod_cleanup_exclusions_from_with_progress(
    paths: &LibraryPaths,
    game_root: &Path,
    batch_id: String,
    selections: Vec<ModCleanupSelection>,
    progress: &OperationReporter,
) -> Result<ModCleanupApplyResult, String> {
    let batch_id = batch_id.trim().to_string();
    if batch_id.is_empty() || batch_id.len() > 128 {
        return Err("清理批次 ID 无效。".to_string());
    }
    if selections.is_empty() || selections.len() > 2_000 {
        return Err("一次清理计划必须包含 1 到 2000 个文件。".to_string());
    }

    let contexts = load_all_installed_manifests(&paths.installed_path)?;
    let selected_mod_ids = selections
        .iter()
        .map(|selection| selection.mod_id.as_str())
        .collect::<HashSet<_>>();
    let selected_contexts = contexts
        .iter()
        .filter(|context| selected_mod_ids.contains(context.manifest.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let scan = scan_mod_cleanup_candidates_from(&selected_contexts, &OperationReporter::default())?;
    let candidates = scan
        .candidates
        .into_iter()
        .map(|candidate| (candidate.candidate_id.clone(), candidate))
        .collect::<HashMap<_, _>>();
    let mut seen_ids = HashSet::new();
    let mut selections_by_mod =
        BTreeMap::<String, Vec<(ModCleanupSelection, ModCleanupCandidate)>>::new();
    for mut selection in selections {
        if !seen_ids.insert(selection.candidate_id.clone()) {
            continue;
        }
        let candidate = candidates
            .get(&selection.candidate_id)
            .cloned()
            .ok_or_else(|| format!("清理候选已经变化：{}", selection.candidate_id))?;
        if candidate.mod_id != selection.mod_id
            || conflict_path_key(&candidate.library_relative_path)
                != conflict_path_key(&selection.library_relative_path)
        {
            return Err("清理候选的 MOD 或相对路径不匹配，请重新扫描。".to_string());
        }
        selection.reason = selection.reason.trim().chars().take(300).collect();
        selections_by_mod
            .entry(selection.mod_id.clone())
            .or_default()
            .push((selection, candidate));
    }

    let excluded_at = unix_seconds_now()?;
    let mut changed_mod_ids = Vec::new();
    let mut reconcile_paths = Vec::new();
    let mut removed_deployed_file_count = 0;
    let mut warnings = Vec::new();
    let total_mods = selections_by_mod.len();
    progress.report("正在保存部署排除项", 0, Some(total_mods), None);

    for (index, (mod_id, mod_selections)) in selections_by_mod.into_iter().enumerate() {
        let mut context = load_installed_manifest(&paths.installed_path, &mod_id)?;
        let original_context = context.clone();
        let effective_by_library_path = effective_installed_files_for_context(&context)?
            .into_iter()
            .map(|file| {
                (
                    conflict_path_key(&file.installed_file.library_relative_path),
                    file.deploy_relative_path,
                )
            })
            .collect::<HashMap<_, _>>();
        let selected_deploy_paths = mod_selections
            .iter()
            .filter_map(|(_, candidate)| {
                effective_by_library_path
                    .get(&conflict_path_key(&candidate.library_relative_path))
                    .cloned()
            })
            .collect::<Vec<_>>();
        let selected_path_keys = selected_deploy_paths
            .iter()
            .map(|path| conflict_path_key(path))
            .collect::<HashSet<_>>();
        let selected_deployed_files = context
            .manifest
            .deployed_files
            .iter()
            .filter(|file| {
                selected_path_keys.contains(&conflict_path_key(&file.deploy_relative_path))
            })
            .cloned()
            .collect::<Vec<_>>();

        for (selection, candidate) in mod_selections {
            context
                .manifest
                .deployment_exclusions
                .push(ModDeploymentExclusion {
                    candidate_id: selection.candidate_id,
                    library_relative_path: candidate.library_relative_path,
                    deploy_relative_path: candidate.deploy_relative_path,
                    reason: selection.reason,
                    batch_id: batch_id.clone(),
                    excluded_at_unix_seconds: excluded_at,
                });
        }

        let removed_paths = remove_deployed_files_with_progress(
            &paths.installed_path,
            game_root,
            &selected_deployed_files,
            Some(&original_context),
            &mut warnings,
            progress,
            "正在移除已排除的部署文件",
        )?;
        removed_deployed_file_count += removed_paths.len();
        reconcile_paths.extend(
            selected_deployed_files
                .iter()
                .map(|file| file.deploy_relative_path.clone()),
        );
        context.manifest.deployed_files.retain(|file| {
            !selected_path_keys.contains(&conflict_path_key(&file.deploy_relative_path))
        });
        save_manifest(&context.manifest_path, &context.manifest)?;
        changed_mod_ids.push(mod_id);
        progress.report(
            "正在保存部署排除项",
            index + 1,
            Some(total_mods),
            Some(manifest_display_name(&context.manifest)),
        );
    }

    update_workspace_snapshot_after_mod_changes(paths, &changed_mod_ids, &[])?;
    let restored_conflict_file_count = reconcile_paths
        .iter()
        .map(|path| conflict_path_key(path))
        .collect::<HashSet<_>>()
        .len();
    restore_enabled_versions_for_paths_with_progress(
        &paths.installed_path,
        game_root,
        &reconcile_paths,
        &mut warnings,
        progress,
    )?;
    update_workspace_snapshot_after_mod_changes(paths, &changed_mod_ids, &[])?;
    let exclusion_count = seen_ids.len();
    Ok(ModCleanupApplyResult {
        batch_id,
        affected_mod_count: changed_mod_ids.len(),
        exclusion_count,
        removed_deployed_file_count,
        restored_conflict_file_count,
        warnings,
        message: format!("已记录 {exclusion_count} 个部署排除项；本地 MOD 库原始文件均已保留。"),
    })
}

fn restore_mod_cleanup_exclusions_from_with_progress(
    paths: &LibraryPaths,
    game_root: &Path,
    candidate_ids: Vec<String>,
    progress: &OperationReporter,
) -> Result<ModCleanupRestoreResult, String> {
    let requested_ids = candidate_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<HashSet<_>>();
    if requested_ids.is_empty() || requested_ids.len() > 2_000 {
        return Err("一次恢复计划必须包含 1 到 2000 个排除项。".to_string());
    }
    let contexts = load_all_installed_manifests(&paths.installed_path)?;
    let available_ids = contexts
        .iter()
        .flat_map(|context| {
            context
                .manifest
                .deployment_exclusions
                .iter()
                .map(|exclusion| exclusion.candidate_id.clone())
        })
        .collect::<HashSet<_>>();
    if !requested_ids.is_subset(&available_ids) {
        return Err("部署排除记录已经变化，请重新生成恢复计划。".to_string());
    }

    let mut changed_mod_ids = Vec::new();
    let mut deploy_paths = Vec::new();
    let mut restored_exclusion_count = 0;
    let affected_contexts = contexts
        .iter()
        .filter(|context| {
            context
                .manifest
                .deployment_exclusions
                .iter()
                .any(|exclusion| requested_ids.contains(&exclusion.candidate_id))
        })
        .count();
    progress.report("正在恢复部署排除项", 0, Some(affected_contexts), None);

    for context in contexts {
        let selected_exclusions = context
            .manifest
            .deployment_exclusions
            .iter()
            .filter(|exclusion| requested_ids.contains(&exclusion.candidate_id))
            .cloned()
            .collect::<Vec<_>>();
        if selected_exclusions.is_empty() {
            continue;
        }
        let mut context = context;
        let selected_library_keys = selected_exclusions
            .iter()
            .map(|exclusion| conflict_path_key(&exclusion.library_relative_path))
            .collect::<HashSet<_>>();
        context
            .manifest
            .deployment_exclusions
            .retain(|exclusion| !requested_ids.contains(&exclusion.candidate_id));
        if context.manifest.enabled {
            deploy_paths.extend(
                effective_installed_files_for_context(&context)?
                    .into_iter()
                    .filter(|file| {
                        selected_library_keys.contains(&conflict_path_key(
                            &file.installed_file.library_relative_path,
                        ))
                    })
                    .map(|file| file.deploy_relative_path),
            );
        }
        restored_exclusion_count += selected_exclusions.len();
        save_manifest(&context.manifest_path, &context.manifest)?;
        changed_mod_ids.push(context.manifest.id.clone());
        progress.report(
            "正在恢复部署排除项",
            changed_mod_ids.len(),
            Some(affected_contexts),
            Some(manifest_display_name(&context.manifest)),
        );
    }

    update_workspace_snapshot_after_mod_changes(paths, &changed_mod_ids, &[])?;
    let mut warnings = Vec::new();
    let deployed_file_count = deploy_paths
        .iter()
        .map(|path| conflict_path_key(path))
        .collect::<HashSet<_>>()
        .len();
    restore_enabled_versions_for_paths_with_progress(
        &paths.installed_path,
        game_root,
        &deploy_paths,
        &mut warnings,
        progress,
    )?;
    update_workspace_snapshot_after_mod_changes(paths, &changed_mod_ids, &[])?;
    Ok(ModCleanupRestoreResult {
        affected_mod_count: changed_mod_ids.len(),
        restored_exclusion_count,
        deployed_file_count,
        warnings,
        message: format!("已恢复 {restored_exclusion_count} 个部署排除项。"),
    })
}

/// 计算部署路径时可带入已安装的 DAT 内容；DAT 只影响本次有效部署，绝不改写库内文件。
fn effective_remap_files_for_manifest_with_armor_dat(
    manifest: &InstalledModManifest,
    replacements: &[ModelReplacement],
    armor_dat_bytes: Option<&[u8]>,
) -> Result<Vec<EffectiveRemapFile>, String> {
    let paths = manifest
        .files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let mut effective = match armor_dat_bytes {
        Some(bytes) => build_effective_remap_files_with_armor_dat(
            &paths,
            replacements,
            &manifest.model_remaps,
            Some(bytes),
        )?,
        None => build_effective_remap_files(&paths, replacements, &manifest.model_remaps)?,
    };
    let effect_paths = build_effective_effect_remap_paths(&paths, &manifest.effect_remaps)?;
    for (file, effect_path) in effective.iter_mut().zip(effect_paths) {
        // 特效规则只在版本化兼容索引命中本地槽位时覆盖部署路径。
        if !paths[file.file_index].eq_ignore_ascii_case(&effect_path) {
            file.deploy_relative_path = effect_path;
        }
    }
    Ok(effective)
}

fn armor_dat_bytes_for_context(
    context: &InstalledManifestContext,
    manifest: &InstalledModManifest,
) -> Result<Option<Vec<u8>>, String> {
    // 未选择防具改绑时不读取二进制 DAT，避免无关 MOD 的异常文件阻断其它操作。
    if !manifest
        .model_remaps
        .iter()
        .any(|selection| selection.group_key.starts_with("armor:"))
    {
        return Ok(None);
    }

    let armor_dat_files = manifest
        .files
        .iter()
        .filter(|file| {
            file.deploy_relative_path
                .replace('\\', "/")
                .eq_ignore_ascii_case("nativePC/common/equip/armor.am_dat")
        })
        .collect::<Vec<_>>();
    let Some(armor_dat_file) = armor_dat_files.first() else {
        return Ok(None);
    };
    if armor_dat_files.len() > 1 {
        return Err("MOD 清单中存在多个 armor.am_dat，无法安全进行 DAT 型防具改绑。".to_string());
    }

    let source_path = source_path_for_installed_file(context, armor_dat_file)?;
    let size = fs::metadata(&source_path)
        .map_err(|error| format!("无法读取 armor.am_dat 文件信息：{error}"))?
        .len();
    if size > MAX_ARMOR_DAT_SIZE_BYTES {
        return Err(format!(
            "armor.am_dat 过大（{size} 字节），超过安全解析上限 {MAX_ARMOR_DAT_SIZE_BYTES} 字节。"
        ));
    }
    fs::read(&source_path)
        .map(Some)
        .map_err(|error| format!("无法读取 armor.am_dat：{error}"))
}

fn effective_remap_files_for_context_with_manifest(
    context: &InstalledManifestContext,
    manifest: &InstalledModManifest,
    replacements: &[ModelReplacement],
) -> Result<Vec<EffectiveRemapFile>, String> {
    let armor_dat_bytes = armor_dat_bytes_for_context(context, manifest)?;
    effective_remap_files_for_manifest_with_armor_dat(
        manifest,
        replacements,
        armor_dat_bytes.as_deref(),
    )
}

fn effective_installed_file_from_remap(
    manifest: &InstalledModManifest,
    excluded_library_paths: &HashSet<String>,
    effective: EffectiveRemapFile,
) -> Option<Result<EffectiveInstalledModFile, String>> {
    // 自动排除仅存在于有效部署视图；原始 DAT 仍完整保存在 MOD 库中。
    if effective.automatic_exclusion_reason.is_some() {
        return None;
    }
    let installed_file = manifest.files.get(effective.file_index).cloned();
    let Some(installed_file) = installed_file else {
        return Some(Err("有效部署文件索引超出范围。".to_string()));
    };
    if excluded_library_paths.contains(&conflict_path_key(&installed_file.library_relative_path)) {
        return None;
    }
    Some(Ok(EffectiveInstalledModFile {
        installed_file,
        deploy_relative_path: effective.deploy_relative_path,
        texture_path_rewrites: effective.texture_path_rewrites,
        evam_slinger_rewrite: effective.evam_slinger_rewrite,
    }))
}

fn effective_installed_files_for_context_with_manifest(
    context: &InstalledManifestContext,
    manifest: &InstalledModManifest,
    replacements: &[ModelReplacement],
) -> Result<Vec<EffectiveInstalledModFile>, String> {
    let excluded_library_paths = manifest
        .deployment_exclusions
        .iter()
        .map(|exclusion| conflict_path_key(&exclusion.library_relative_path))
        .collect::<HashSet<_>>();
    effective_remap_files_for_context_with_manifest(context, manifest, replacements)?
        .into_iter()
        .filter_map(|effective| {
            effective_installed_file_from_remap(manifest, &excluded_library_paths, effective)
        })
        .collect()
}

fn effective_installed_files_for_context(
    context: &InstalledManifestContext,
) -> Result<Vec<EffectiveInstalledModFile>, String> {
    let replacements = model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    effective_installed_files_for_context_with_manifest(context, &context.manifest, &replacements)
}

fn preview_mrl3_rewrite_count(
    source_path: &Path,
    file: &EffectiveInstalledModFile,
) -> Result<usize, String> {
    if !path_has_extension(source_path, "mrl3") || file.texture_path_rewrites.is_empty() {
        return Ok(0);
    }
    let bytes = fs::read(source_path).map_err(|error| {
        format!(
            "无法读取用于改绑预览的 MRL3 文件 {}：{error}",
            source_path.display()
        )
    })?;
    rewrite_mrl3_texture_paths(&bytes, &file.texture_path_rewrites)
        .map(|(_, count)| count)
        .map_err(|error| format!("无法预览 MRL3 文件 {}：{error}", source_path.display()))
}

fn preview_evam_rewrite_count(
    source_path: &Path,
    file: &EffectiveInstalledModFile,
) -> Result<usize, String> {
    let Some(rewrite) = file.evam_slinger_rewrite.as_ref() else {
        return Ok(0);
    };
    if !path_has_extension(source_path, "evam") {
        return Err(format!(
            "飞翔爪绑定改写目标不是 EVAM 文件：{}",
            source_path.display()
        ));
    }
    let bytes = fs::read(source_path).map_err(|error| {
        format!(
            "无法读取用于改绑预览的 EVAM 文件 {}：{error}",
            source_path.display()
        )
    })?;
    rewrite_evam_slinger_id(&bytes, rewrite)
        .map(|_| 1)
        .map_err(|error| format!("无法预览 EVAM 文件 {}：{error}", source_path.display()))
}

fn deploy_effective_file(
    context: &InstalledManifestContext,
    file: &EffectiveInstalledModFile,
    target_path: &Path,
) -> Result<(), String> {
    let source_path = source_path_for_installed_file(context, &file.installed_file)?;
    if let Some(rewrite) = file.evam_slinger_rewrite.as_ref() {
        let source_bytes = fs::read(&source_path)
            .map_err(|error| format!("无法读取 EVAM 文件 {}：{error}", source_path.display()))?;
        let output = rewrite_evam_slinger_id(&source_bytes, rewrite)
            .map_err(|error| format!("无法改绑 EVAM 文件 {}：{error}", source_path.display()))?;
        fs::write(target_path, output).map_err(|error| {
            format!(
                "无法把改绑后的 EVAM 文件 {} 部署到 {}：{error}",
                source_path.display(),
                target_path.display()
            )
        })?;
    } else if path_has_extension(&source_path, "mrl3") && !file.texture_path_rewrites.is_empty() {
        let source_bytes = fs::read(&source_path)
            .map_err(|error| format!("无法读取 MRL3 文件 {}：{error}", source_path.display()))?;
        let (output, _) = rewrite_mrl3_texture_paths(&source_bytes, &file.texture_path_rewrites)
            .map_err(|error| format!("无法改绑 MRL3 文件 {}：{error}", source_path.display()))?;
        fs::write(target_path, output).map_err(|error| {
            format!(
                "无法把改绑后的 MRL3 文件 {} 部署到 {}：{error}",
                source_path.display(),
                target_path.display()
            )
        })?;
    } else {
        fs::copy(&source_path, target_path).map_err(|error| {
            format!(
                "无法把 {} 部署到 {}：{error}",
                source_path.display(),
                target_path.display()
            )
        })?;
    }
    Ok(())
}

fn effective_file_matches_target(
    context: &InstalledManifestContext,
    file: &EffectiveInstalledModFile,
    target_path: &Path,
) -> Result<bool, String> {
    let source_path = source_path_for_installed_file(context, &file.installed_file)?;
    if let Some(rewrite) = file.evam_slinger_rewrite.as_ref() {
        let source_bytes = fs::read(&source_path)
            .map_err(|error| format!("无法读取 EVAM 文件 {}：{error}", source_path.display()))?;
        let expected = rewrite_evam_slinger_id(&source_bytes, rewrite).map_err(|error| {
            format!(
                "无法生成 EVAM 接管校验内容 {}：{error}",
                source_path.display()
            )
        })?;
        return bytes_match_file(&expected, target_path);
    }
    if path_has_extension(&source_path, "mrl3") && !file.texture_path_rewrites.is_empty() {
        let source_bytes = fs::read(&source_path)
            .map_err(|error| format!("无法读取 MRL3 文件 {}：{error}", source_path.display()))?;
        let (expected, _) = rewrite_mrl3_texture_paths(&source_bytes, &file.texture_path_rewrites)
            .map_err(|error| {
                format!(
                    "无法生成 MRL3 接管校验内容 {}：{error}",
                    source_path.display()
                )
            })?;
        return bytes_match_file(&expected, target_path);
    }
    regular_files_are_equal(&source_path, target_path)
}

fn bytes_match_file(expected: &[u8], target_path: &Path) -> Result<bool, String> {
    let target_size = fs::metadata(target_path)
        .map_err(|error| format!("无法读取游戏文件 {}：{error}", target_path.display()))?
        .len();
    if target_size != expected.len() as u64 {
        return Ok(false);
    }
    let target = fs::read(target_path)
        .map_err(|error| format!("无法读取游戏文件 {}：{error}", target_path.display()))?;
    Ok(target == expected)
}

fn regular_files_are_equal(source_path: &Path, target_path: &Path) -> Result<bool, String> {
    let source_size = fs::metadata(source_path)
        .map_err(|error| format!("无法读取本地 MOD 文件 {}：{error}", source_path.display()))?
        .len();
    let target_size = fs::metadata(target_path)
        .map_err(|error| format!("无法读取游戏文件 {}：{error}", target_path.display()))?
        .len();
    if source_size != target_size {
        return Ok(false);
    }

    let mut source =
        BufReader::new(fs::File::open(source_path).map_err(|error| {
            format!("无法打开本地 MOD 文件 {}：{error}", source_path.display())
        })?);
    let mut target = BufReader::new(
        fs::File::open(target_path)
            .map_err(|error| format!("无法打开游戏文件 {}：{error}", target_path.display()))?,
    );
    let mut source_buffer = vec![0; 1024 * 1024];
    let mut target_buffer = vec![0; 1024 * 1024];

    loop {
        let source_read = source
            .read(&mut source_buffer)
            .map_err(|error| format!("无法读取本地 MOD 文件 {}：{error}", source_path.display()))?;
        let target_read = target
            .read(&mut target_buffer)
            .map_err(|error| format!("无法读取游戏文件 {}：{error}", target_path.display()))?;
        if source_read != target_read
            || source_buffer[..source_read] != target_buffer[..target_read]
        {
            return Ok(false);
        }
        if source_read == 0 {
            return Ok(true);
        }
    }
}

fn path_has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn preview_enable_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
) -> Result<ModDeploymentPlan, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    build_deployment_plan(installed_root, game_root, &context)
}

fn batch_update_mods_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    action: BatchModAction,
    mod_ids: Vec<String>,
    progress: &OperationReporter,
) -> Result<BatchModOperationResult, String> {
    let mut seen_ids = HashSet::new();
    let mod_ids = mod_ids
        .into_iter()
        .map(|mod_id| mod_id.trim().to_string())
        .filter(|mod_id| !mod_id.is_empty() && seen_ids.insert(mod_id.clone()))
        .collect::<Vec<_>>();
    if mod_ids.is_empty() {
        return Err("请至少选择一个 MOD。".to_string());
    }

    let requested_count = mod_ids.len();
    let action_label = batch_action_label(action);
    let progress_phase = format!("正在批量{action_label}");
    let mut items = Vec::with_capacity(requested_count);
    let mut succeeded_count = 0;
    let mut skipped_count = 0;
    let mut failed_count = 0;
    let mut affected_file_count = 0;
    let mut warnings = Vec::new();

    for (index, mod_id) in mod_ids.into_iter().enumerate() {
        let context = match load_installed_manifest(installed_root, &mod_id) {
            Ok(context) => context,
            Err(error) => {
                failed_count += 1;
                items.push(BatchModOperationItem {
                    mod_id: mod_id.clone(),
                    name: mod_id.clone(),
                    status: "failed".to_string(),
                    affected_file_count: 0,
                    warnings: Vec::new(),
                    message: error,
                });
                progress.report(
                    progress_phase.clone(),
                    index + 1,
                    Some(requested_count),
                    Some(mod_id),
                );
                continue;
            }
        };
        let name = manifest_display_name(&context.manifest);
        let should_skip = match action {
            BatchModAction::Enable => context.manifest.enabled,
            BatchModAction::Disable => {
                !context.manifest.enabled && context.manifest.deployed_files.is_empty()
            }
            BatchModAction::Uninstall => false,
        };
        if should_skip {
            skipped_count += 1;
            items.push(BatchModOperationItem {
                mod_id,
                name: name.clone(),
                status: "skipped".to_string(),
                affected_file_count: 0,
                warnings: Vec::new(),
                message: format!("{name} 已经处于目标状态。"),
            });
            progress.report(
                progress_phase.clone(),
                index + 1,
                Some(requested_count),
                Some(name),
            );
            continue;
        }

        // 批量入口复用单项核心函数，确保冲突顺序、观察所得文件保护和版本恢复
        // 与用户逐个点击时完全一致。
        let operation_result = match action {
            BatchModAction::Enable => {
                enable_mod_from_with_progress(installed_root, game_root, &mod_id, true, progress)
                    .map(|result| (result.affected_file_count, result.warnings, result.message))
            }
            BatchModAction::Disable => {
                disable_mod_from_with_progress(installed_root, game_root, &mod_id, progress)
                    .map(|result| (result.affected_file_count, result.warnings, result.message))
            }
            BatchModAction::Uninstall => {
                uninstall_mod_with_paths_and_progress(installed_root, game_root, &mod_id, progress)
                    .map(|result| {
                        (
                            result.removed_deployed_file_count + result.removed_library_file_count,
                            result.warnings,
                            result.message,
                        )
                    })
            }
        };

        match operation_result {
            Ok((item_affected_file_count, item_warnings, message)) => {
                succeeded_count += 1;
                affected_file_count += item_affected_file_count;
                warnings.extend(
                    item_warnings
                        .iter()
                        .map(|warning| format!("{name}：{warning}")),
                );
                items.push(BatchModOperationItem {
                    mod_id,
                    name: name.clone(),
                    status: "succeeded".to_string(),
                    affected_file_count: item_affected_file_count,
                    warnings: item_warnings,
                    message,
                });
            }
            Err(error) => {
                failed_count += 1;
                items.push(BatchModOperationItem {
                    mod_id,
                    name: name.clone(),
                    status: "failed".to_string(),
                    affected_file_count: 0,
                    warnings: Vec::new(),
                    message: error,
                });
            }
        }
        progress.report(
            progress_phase.clone(),
            index + 1,
            Some(requested_count),
            Some(name),
        );
    }

    let message = format!(
        "批量{action_label}完成：成功 {succeeded_count} 个，跳过 {skipped_count} 个，失败 {failed_count} 个。"
    );
    Ok(BatchModOperationResult {
        action,
        requested_count,
        succeeded_count,
        skipped_count,
        failed_count,
        affected_file_count,
        items,
        warnings,
        message,
    })
}

fn batch_action_label(action: BatchModAction) -> &'static str {
    match action {
        BatchModAction::Enable => "启用",
        BatchModAction::Disable => "禁用",
        BatchModAction::Uninstall => "卸载",
    }
}

fn preview_disable_mod_from(installed_root: &Path, mod_id: &str) -> Result<ModDisablePlan, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let name = manifest_display_name(&context.manifest);
    let files = context.manifest.deployed_files.clone();
    let warnings = files
        .iter()
        .filter(|file| !Path::new(&file.deployed_path).is_file())
        .map(|file| {
            format!(
                "Recorded deployed file is already missing: {}",
                file.deployed_path
            )
        })
        .collect::<Vec<_>>();
    let message = if files.is_empty() {
        "MOD is already disabled; no deployed files will be removed.".to_string()
    } else {
        format!(
            "Disabling this MOD will remove {} recorded file(s) from the game directory; the local library copy will be kept.",
            files.len()
        )
    };

    Ok(ModDisablePlan {
        mod_id: context.manifest.id,
        name,
        enabled: context.manifest.enabled,
        file_count: files.len(),
        files,
        warnings,
        message,
    })
}

#[cfg(test)]
fn enable_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    confirm_overwrite: bool,
) -> Result<ModDeploymentResult, String> {
    enable_mod_from_with_progress(
        installed_root,
        game_root,
        mod_id,
        confirm_overwrite,
        &OperationReporter::default(),
    )
}

fn enable_mod_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    confirm_overwrite: bool,
    progress: &OperationReporter,
) -> Result<ModDeploymentResult, String> {
    progress.report("正在检查部署文件", 0, None, None);
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    let plan = build_deployment_plan(installed_root, game_root, &context)?;

    if plan.requires_overwrite_confirmation && !confirm_overwrite {
        return Err(
            "Enable plan requires overwrite confirmation because target files already exist."
                .to_string(),
        );
    }

    let deployed_at = unix_seconds_now()?;
    let mut deployed_files = Vec::new();

    let effective_files = effective_installed_files_for_context(&context)?;
    let file_total = effective_files.len();
    progress.report("正在部署 MOD 文件", 0, Some(file_total), None);
    for (index, file) in effective_files.iter().enumerate() {
        let target_relative_path = relative_string_to_path(&file.deploy_relative_path)?;
        let target_path = game_root.join(target_relative_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Could not create deployment directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        deploy_effective_file(&context, file, &target_path)?;

        deployed_files.push(DeployedModFile {
            deploy_relative_path: file.deploy_relative_path.clone(),
            deployed_path: path_to_string(&target_path),
            deployed_at_unix_seconds: deployed_at,
            deployment_origin: DeploymentOrigin::Copied,
            is_adopted: false,
        });
        progress.report(
            "正在部署 MOD 文件",
            index + 1,
            Some(file_total),
            Some(file.deploy_relative_path.clone()),
        );
    }

    context.manifest.enabled = true;
    context.manifest.partially_overridden = false;
    context.manifest.deployed_files = deployed_files.clone();
    progress.report("正在保存部署记录", 0, Some(1), None);
    save_manifest(&context.manifest_path, &context.manifest)?;
    let mut warnings = plan.warnings;
    if let Err(error) = update_workspace_mod_index_only(installed_root, &context.manifest.id) {
        invalidate_workspace_snapshot(installed_root);
        warnings.push(format!("冲突索引更新失败，将使用兼容扫描：{error}"));
    }
    progress.report("正在记录冲突优先级", 0, None, None);
    record_enabled_mod_conflict_order(installed_root, &context.manifest.id)?;
    reapply_conflict_groups_for_mod_with_progress(
        installed_root,
        game_root,
        &context.manifest.id,
        &mut warnings,
        progress,
    )?;
    progress.report("正在保存部署记录", 1, Some(1), None);

    let name = manifest_display_name(&context.manifest);
    Ok(ModDeploymentResult {
        mod_id: context.manifest.id,
        name,
        enabled: true,
        affected_file_count: deployed_files.len(),
        files: deployed_files,
        warnings,
        message: "MOD was enabled and copied to the MHW game directory.".to_string(),
    })
}

#[cfg(test)]
fn disable_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
) -> Result<ModDeploymentResult, String> {
    disable_mod_from_with_progress(
        installed_root,
        game_root,
        mod_id,
        &OperationReporter::default(),
    )
}

fn disable_mod_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    progress: &OperationReporter,
) -> Result<ModDeploymentResult, String> {
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    let deployed_files = context.manifest.deployed_files.clone();
    let mut warnings = Vec::new();
    let disabled_file_paths = remove_deployed_files_with_progress(
        installed_root,
        game_root,
        &deployed_files,
        Some(&context),
        &mut warnings,
        progress,
        "正在清理游戏文件",
    )?;
    let removed_count = disabled_file_paths.len();

    context.manifest.enabled = false;
    context.manifest.partially_overridden = false;
    context.manifest.deployed_files = Vec::new();
    progress.report("正在保存部署记录", 0, Some(1), None);
    save_manifest(&context.manifest_path, &context.manifest)?;
    if let Err(error) = update_workspace_mod_index_only(installed_root, &context.manifest.id) {
        invalidate_workspace_snapshot(installed_root);
        warnings.push(format!("冲突索引更新失败，将使用兼容扫描：{error}"));
    }

    restore_enabled_versions_for_paths_with_progress(
        installed_root,
        game_root,
        &disabled_file_paths,
        &mut warnings,
        progress,
    )?;
    progress.report("正在保存部署记录", 1, Some(1), None);

    let message = if deployed_files.is_empty() {
        "MOD was already disabled; no deployment records were found.".to_string()
    } else {
        "MOD was disabled and its recorded deployed files were removed.".to_string()
    };

    let name = manifest_display_name(&context.manifest);
    Ok(ModDeploymentResult {
        mod_id: context.manifest.id,
        name,
        enabled: false,
        affected_file_count: removed_count,
        files: deployed_files,
        warnings,
        message,
    })
}

fn preview_uninstall_mod_from(
    installed_root: &Path,
    mod_id: &str,
) -> Result<ModUninstallPlan, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let name = manifest_display_name(&context.manifest);
    let mut warnings = Vec::new();

    if context.manifest.enabled {
        warnings.push(
            "This MOD is enabled. Uninstalling it will remove its recorded deployed files first."
                .to_string(),
        );
    }

    Ok(ModUninstallPlan {
        mod_id: context.manifest.id,
        name,
        enabled: context.manifest.enabled,
        deployed_file_count: context.manifest.deployed_files.len(),
        library_file_count: context.manifest.files.len(),
        deployed_files: context.manifest.deployed_files,
        library_files: context.manifest.files,
        warnings,
        message: "Uninstall will remove this MOD from the local Acumod library.".to_string(),
    })
}

#[cfg(test)]
fn uninstall_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
) -> Result<ModUninstallResult, String> {
    uninstall_mod_from_with_progress(
        installed_root,
        game_root,
        mod_id,
        &OperationReporter::default(),
    )
}

fn uninstall_mod_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    progress: &OperationReporter,
) -> Result<ModUninstallResult, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let removed_library_file_count = context.manifest.files.len();
    let mut warnings = Vec::new();
    let removed_deployed_file_count = if context.manifest.enabled {
        let disable_result =
            disable_mod_from_with_progress(installed_root, game_root, mod_id, progress)?;
        warnings.extend(disable_result.warnings);
        disable_result.affected_file_count
    } else {
        remove_deployed_files_with_progress(
            installed_root,
            game_root,
            &context.manifest.deployed_files,
            Some(&context),
            &mut warnings,
            progress,
            "正在清理游戏文件",
        )?
        .len()
    };
    let name = manifest_display_name(&context.manifest);
    let mod_id = context.manifest.id;

    progress.report("正在删除本地 MOD", 0, Some(1), Some(name.clone()));
    fs::remove_dir_all(&context.mod_path).map_err(|error| {
        format!(
            "Could not remove installed MOD directory {}: {error}",
            context.mod_path.display()
        )
    })?;
    progress.report("正在删除本地 MOD", 1, Some(1), Some(name.clone()));

    Ok(ModUninstallResult {
        mod_id,
        name,
        removed_deployed_file_count,
        removed_library_file_count,
        warnings,
        message: "MOD was uninstalled from the local Acumod library.".to_string(),
    })
}

fn preview_restore_all_mods_from(installed_root: &Path) -> Result<RestoreAllPlan, String> {
    let contexts = load_all_installed_manifests(installed_root)?;
    let mods = restore_plan_items(&contexts);
    let deployed_file_count = mods.iter().map(|item| item.deployed_file_count).sum();
    let message = if mods.is_empty() {
        "No enabled or deployed MODs were found.".to_string()
    } else {
        format!(
            "Restore will disable {} MOD(s) and remove {} recorded deployed file(s).",
            mods.len(),
            deployed_file_count
        )
    };

    Ok(RestoreAllPlan {
        affected_mod_count: mods.len(),
        deployed_file_count,
        mods,
        warnings: Vec::new(),
        message,
    })
}

#[cfg(test)]
fn restore_all_mods_from(
    installed_root: &Path,
    game_root: &Path,
) -> Result<RestoreAllResult, String> {
    restore_all_mods_from_with_progress(installed_root, game_root, &OperationReporter::default())
}

fn restore_all_mods_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    progress: &OperationReporter,
) -> Result<RestoreAllResult, String> {
    progress.report("正在读取已部署 MOD", 0, None, None);
    let contexts = load_all_installed_manifests(installed_root)?;
    let plan_mods = restore_plan_items(&contexts);
    let mut warnings = Vec::new();
    let mut removed_deployed_file_count = 0;
    let affected_mod_count = plan_mods.len();
    let mut completed_mod_count = 0;

    for mut context in contexts {
        if !context.manifest.enabled && context.manifest.deployed_files.is_empty() {
            continue;
        }

        let deployed_files = context.manifest.deployed_files.clone();
        removed_deployed_file_count += remove_deployed_files_with_progress(
            installed_root,
            game_root,
            &deployed_files,
            Some(&context),
            &mut warnings,
            progress,
            "正在清理游戏文件",
        )?
        .len();
        context.manifest.enabled = false;
        context.manifest.partially_overridden = false;
        context.manifest.deployed_files = Vec::new();
        save_manifest(&context.manifest_path, &context.manifest)?;
        completed_mod_count += 1;
        progress.report(
            "正在还原 MOD",
            completed_mod_count,
            Some(affected_mod_count),
            Some(manifest_display_name(&context.manifest)),
        );
    }

    let message = if plan_mods.is_empty() {
        "No enabled or deployed MODs needed restoring.".to_string()
    } else {
        "All recorded deployed MOD files were removed and affected MODs were disabled.".to_string()
    };

    Ok(RestoreAllResult {
        affected_mod_count: plan_mods.len(),
        removed_deployed_file_count,
        mods: plan_mods,
        warnings,
        message,
    })
}

fn restore_plan_items(contexts: &[InstalledManifestContext]) -> Vec<RestoreModPlanItem> {
    contexts
        .iter()
        .filter(|context| context.manifest.enabled || !context.manifest.deployed_files.is_empty())
        .map(|context| RestoreModPlanItem {
            mod_id: context.manifest.id.clone(),
            name: manifest_display_name(&context.manifest),
            enabled: context.manifest.enabled,
            deployed_file_count: context.manifest.deployed_files.len(),
        })
        .collect()
}

fn get_mod_conflict_report_from(installed_root: &Path) -> Result<ModConflictReport, String> {
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let store = read_conflict_order_store(installed_root)?;
        return Ok(build_mod_conflict_report_from_workspace_index(
            &stored.mod_index,
            &store,
        ));
    }
    let contexts = load_all_installed_manifests(installed_root)?;
    let store = read_conflict_order_store(installed_root)?;
    build_mod_conflict_report(&contexts, &store)
}

fn get_mod_conflict_report_with_changed_mod(
    installed_root: &Path,
    changed_mod_id: &str,
) -> Result<ModConflictReport, String> {
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    if let Some(mut stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let context = load_installed_manifest(installed_root, changed_mod_id)?;
        upsert_workspace_mod_index_entry(&mut stored.mod_index, &context)?;
        let store = read_conflict_order_store(installed_root)?;
        return Ok(build_mod_conflict_report_from_workspace_index(
            &stored.mod_index,
            &store,
        ));
    }
    let contexts = load_all_installed_manifests(installed_root)?;
    let store = read_conflict_order_store(installed_root)?;
    build_mod_conflict_report(&contexts, &store)
}

fn move_conflict_participant_from(
    installed_root: &Path,
    group_id: &str,
    mod_id: &str,
    direction: &str,
    mut order: Vec<String>,
) -> Result<ModConflictMoveResult, String> {
    validate_mod_id(mod_id)?;
    if order.len() < 2 {
        return Err("冲突组至少需要两个 MOD。".to_string());
    }
    let mut unique_ids = HashSet::new();
    for participant_id in &order {
        validate_mod_id(participant_id)?;
        if !unique_ids.insert(participant_id.clone()) {
            return Err("冲突组顺序包含重复 MOD。".to_string());
        }
    }
    let mut sorted_ids = order.clone();
    sorted_ids.sort();
    if conflict_group_id(&sorted_ids) != group_id {
        return Err("冲突组已经变化，请刷新后重试。".to_string());
    }

    let mut store = read_conflict_order_store(installed_root)?;
    let index = order
        .iter()
        .position(|participant_id| participant_id == mod_id)
        .ok_or_else(|| format!("当前 MOD 不属于该冲突组：{mod_id}"))?;
    let target_index = match direction {
        "up" if index > 0 => Some(index - 1),
        "down" if index + 1 < order.len() => Some(index + 1),
        "up" | "down" => None,
        other => return Err(format!("未知的冲突顺序移动方向：{other}")),
    };

    let Some(target_index) = target_index else {
        return Ok(ModConflictMoveResult {
            group_id: group_id.to_string(),
            mod_id: mod_id.to_string(),
            direction: direction.to_string(),
            moved: false,
            participant_order: order,
            message: "当前 MOD 已位于冲突顺序边界。".to_string(),
        });
    };

    order.swap(index, target_index);
    store.orders.insert(group_id.to_string(), order.clone());
    save_conflict_order_store(installed_root, &store)?;
    update_workspace_snapshot_conflict_order(installed_root, group_id, &order)?;

    Ok(ModConflictMoveResult {
        group_id: group_id.to_string(),
        mod_id: mod_id.to_string(),
        direction: direction.to_string(),
        moved: true,
        participant_order: order,
        message: "冲突优先级已更新，应用该冲突组后会同步游戏目录文件。".to_string(),
    })
}

fn set_conflict_participant_order_from(
    installed_root: &Path,
    group_id: &str,
    participant_order: Vec<String>,
) -> Result<(), String> {
    if participant_order.len() < 2 {
        return Err("冲突组至少需要两个 MOD。".to_string());
    }

    let mut unique_ids = HashSet::new();
    for participant_id in &participant_order {
        validate_mod_id(participant_id)?;
        if !unique_ids.insert(participant_id.clone()) {
            return Err("冲突组顺序包含重复 MOD。".to_string());
        }
    }

    let report = get_mod_conflict_report_from(installed_root)?;
    let group = find_conflict_group(&report, group_id)?;
    let current_ids = group
        .participants
        .iter()
        .map(|participant| participant.mod_id.clone())
        .collect::<HashSet<_>>();
    if current_ids != unique_ids {
        return Err("冲突组成员已经变化，请重新生成操作计划。".to_string());
    }

    let mut store = read_conflict_order_store(installed_root)?;
    store
        .orders
        .insert(group_id.to_string(), participant_order.clone());
    save_conflict_order_store(installed_root, &store)?;
    update_workspace_snapshot_conflict_order(installed_root, group_id, &participant_order)
}

fn update_workspace_snapshot_conflict_order(
    installed_root: &Path,
    group_id: &str,
    participant_order: &[String],
) -> Result<(), String> {
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let Some(mut stored) = read_stored_workspace_snapshot(&snapshot_path)? else {
        return Ok(());
    };
    let Some(group) = stored
        .snapshot
        .conflict_report
        .groups
        .iter_mut()
        .find(|group| group.group_id == group_id)
    else {
        return Ok(());
    };
    let participants_by_id = group
        .participants
        .iter()
        .cloned()
        .map(|participant| (participant.mod_id.clone(), participant))
        .collect::<HashMap<_, _>>();
    if !participant_order
        .iter()
        .all(|mod_id| participants_by_id.contains_key(mod_id))
    {
        return Ok(());
    }
    group.participants = participant_order
        .iter()
        .enumerate()
        .filter_map(|(index, mod_id)| {
            participants_by_id
                .get(mod_id)
                .cloned()
                .map(|mut participant| {
                    participant.order = index + 1;
                    participant
                })
        })
        .collect();
    update_snapshot_partial_override_flags(&mut stored);
    save_stored_workspace_snapshot(&snapshot_path, &stored)
}

fn preview_apply_conflict_order_from(
    installed_root: &Path,
    game_root: &Path,
    group_id: &str,
) -> Result<ApplyConflictOrderPlan, String> {
    let report = get_mod_conflict_report_from(installed_root)?;
    let group = find_conflict_group(&report, group_id)?;
    let contexts = load_conflict_group_contexts(installed_root, group)?;
    let conflict_paths = conflict_paths_for_group(&contexts, group)?;
    let deployed_file_index = deployed_file_index(installed_root)?;
    let mut warnings = Vec::new();
    let mut requires_overwrite_confirmation = false;
    let mut applicable_file_count = 0;

    for conflict_path in &conflict_paths {
        if winner_for_conflict_path(group, conflict_path).is_none() {
            continue;
        }

        applicable_file_count += 1;
        let target_path = game_root.join(relative_string_to_path(
            &conflict_path.deploy_relative_path,
        )?);

        if target_path.exists() && !deployed_file_index.contains_key(&deployment_key(&target_path))
        {
            requires_overwrite_confirmation = true;
            warnings.push(format!(
                "目标文件已存在，但未记录为 Acumod 管理的文件：{}",
                target_path.display()
            ));
        }
    }

    let message = if applicable_file_count == 0 {
        "当前没有已启用 MOD 的冲突文件可应用。".to_string()
    } else {
        format!(
            "应用此优先级将更新 {applicable_file_count} / {} 个冲突文件。",
            conflict_paths.len()
        )
    };

    Ok(ApplyConflictOrderPlan {
        group_id: group.group_id.clone(),
        conflict_file_count: conflict_paths.len(),
        applicable_file_count,
        enabled_participant_count: group.enabled_participant_count,
        requires_overwrite_confirmation,
        warnings,
        message,
    })
}

#[cfg(test)]
fn apply_conflict_order_from(
    installed_root: &Path,
    game_root: &Path,
    group_id: &str,
    confirm_overwrite: bool,
) -> Result<ApplyConflictOrderResult, String> {
    apply_conflict_order_from_with_progress(
        installed_root,
        game_root,
        group_id,
        confirm_overwrite,
        &OperationReporter::default(),
    )
}

fn apply_conflict_order_from_with_progress(
    installed_root: &Path,
    game_root: &Path,
    group_id: &str,
    confirm_overwrite: bool,
    progress: &OperationReporter,
) -> Result<ApplyConflictOrderResult, String> {
    progress.report("正在分析冲突文件", 0, None, None);
    let plan = preview_apply_conflict_order_from(installed_root, game_root, group_id)?;

    if plan.requires_overwrite_confirmation && !confirm_overwrite {
        return Err("Applying this conflict requires overwrite confirmation.".to_string());
    }

    if plan.applicable_file_count == 0 {
        return Err("No enabled MOD can provide a file for this conflict group.".to_string());
    }

    let report = get_mod_conflict_report_from(installed_root)?;
    let group = find_conflict_group(&report, group_id)?;
    let mut contexts = load_conflict_group_contexts(installed_root, group)?;
    let conflict_paths = conflict_paths_for_group(&contexts, group)?;
    let deployed_index = deployed_file_index(installed_root)?;
    let loaded_ids = contexts
        .iter()
        .map(|context| context.manifest.id.clone())
        .collect::<HashSet<_>>();
    let additional_owner_ids = conflict_paths
        .iter()
        .filter_map(|conflict_path| {
            let target_path =
                game_root.join(relative_string_to_path(&conflict_path.deploy_relative_path).ok()?);
            deployed_index.get(&deployment_key(&target_path)).cloned()
        })
        .filter(|mod_id| !loaded_ids.contains(mod_id))
        .collect::<HashSet<_>>();
    for mod_id in additional_owner_ids {
        contexts.push(load_installed_manifest(installed_root, &mod_id)?);
    }
    let deployed_at = unix_seconds_now()?;
    let mut applied_file_count = 0;
    progress.report(
        "正在应用冲突优先级",
        0,
        Some(plan.applicable_file_count),
        None,
    );

    for conflict_path in &conflict_paths {
        let Some(winner_mod_id) = winner_for_conflict_path(group, conflict_path) else {
            continue;
        };
        let winner_index = contexts
            .iter()
            .position(|context| context.manifest.id == winner_mod_id)
            .ok_or_else(|| format!("Conflict winner was not found: {winner_mod_id}"))?;
        let source_file = effective_installed_files_for_context(&contexts[winner_index])?
            .into_iter()
            .find(|file| {
                conflict_path_key(&file.deploy_relative_path)
                    == conflict_path_key(&conflict_path.deploy_relative_path)
            })
            .ok_or_else(|| "Conflict winner does not contain the selected file.".to_string())?;
        let target_path = game_root.join(relative_string_to_path(
            &conflict_path.deploy_relative_path,
        )?);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Could not create deployment directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        deploy_effective_file(&contexts[winner_index], &source_file, &target_path)?;

        for context in &mut contexts {
            context.manifest.deployed_files.retain(|file| {
                conflict_path_key(&file.deploy_relative_path)
                    != conflict_path_key(&conflict_path.deploy_relative_path)
            });

            if context.manifest.id == winner_mod_id {
                context.manifest.deployed_files.push(DeployedModFile {
                    deploy_relative_path: conflict_path.deploy_relative_path.clone(),
                    deployed_path: path_to_string(&target_path),
                    deployed_at_unix_seconds: deployed_at,
                    deployment_origin: DeploymentOrigin::Copied,
                    is_adopted: false,
                });
            }
        }

        applied_file_count += 1;
        progress.report(
            "正在应用冲突优先级",
            applied_file_count,
            Some(plan.applicable_file_count),
            Some(conflict_path.deploy_relative_path.clone()),
        );
    }

    progress.report("正在保存冲突部署记录", 0, Some(contexts.len()), None);
    for (index, context) in contexts.iter().enumerate() {
        save_manifest(&context.manifest_path, &context.manifest)?;
        progress.report(
            "正在保存冲突部署记录",
            index + 1,
            Some(contexts.len()),
            Some(manifest_display_name(&context.manifest)),
        );
    }

    Ok(ApplyConflictOrderResult {
        group_id: plan.group_id,
        applied_file_count,
        skipped_file_count: plan.conflict_file_count - applied_file_count,
        warnings: plan.warnings,
        message: format!("已按当前 MOD 优先级更新 {applied_file_count} 个冲突文件。"),
    })
}

fn load_conflict_group_contexts(
    installed_root: &Path,
    group: &ModConflictGroup,
) -> Result<Vec<InstalledManifestContext>, String> {
    group
        .participants
        .iter()
        .map(|participant| load_installed_manifest(installed_root, &participant.mod_id))
        .collect()
}

fn build_mod_conflict_report(
    contexts: &[InstalledManifestContext],
    store: &ConflictOrderStore,
) -> Result<ModConflictReport, String> {
    let conflict_paths = collect_conflict_path_groups(contexts)?;
    let mut adjacency: HashMap<String, HashSet<String>> = HashMap::new();

    for conflict_path in &conflict_paths {
        for mod_id in &conflict_path.participant_ids {
            let neighbors = adjacency.entry(mod_id.clone()).or_default();
            neighbors.extend(
                conflict_path
                    .participant_ids
                    .iter()
                    .filter(|participant_id| *participant_id != mod_id)
                    .cloned(),
            );
        }
    }

    let mut visited = HashSet::new();
    let mut groups = Vec::new();

    for mod_id in adjacency.keys() {
        if !visited.insert(mod_id.clone()) {
            continue;
        }

        let mut participant_ids = Vec::new();
        let mut pending = vec![mod_id.clone()];

        while let Some(current_id) = pending.pop() {
            participant_ids.push(current_id.clone());

            if let Some(neighbors) = adjacency.get(&current_id) {
                for neighbor in neighbors {
                    if visited.insert(neighbor.clone()) {
                        pending.push(neighbor.clone());
                    }
                }
            }
        }

        participant_ids.sort();
        let group_id = conflict_group_id(&participant_ids);
        let participant_id_set = participant_ids.iter().cloned().collect::<HashSet<_>>();
        let mut participants = contexts
            .iter()
            .filter(|context| participant_id_set.contains(&context.manifest.id))
            .map(|context| ModConflictParticipant {
                mod_id: context.manifest.id.clone(),
                name: manifest_display_name(&context.manifest),
                enabled: context.manifest.enabled,
                order: 0,
            })
            .collect::<Vec<_>>();
        let stored_order = store
            .orders
            .get(&group_id)
            .or_else(|| find_best_stored_order(store, &participant_ids));
        sort_participants_by_conflict_order(&mut participants, stored_order);
        let enabled_participant_count = participants
            .iter()
            .filter(|participant| participant.enabled)
            .count();
        let conflict_file_count = conflict_paths
            .iter()
            .filter(|path| {
                path.participant_ids
                    .iter()
                    .any(|participant_id| participant_id_set.contains(participant_id))
            })
            .count();
        let conflict_files = conflict_paths
            .iter()
            .filter(|path| {
                path.participant_ids
                    .iter()
                    .any(|participant_id| participant_id_set.contains(participant_id))
            })
            .map(|path| path.deploy_relative_path.clone())
            .collect::<Vec<_>>();

        groups.push(ModConflictGroup {
            group_id,
            participant_count: participants.len(),
            conflict_file_count,
            conflict_files,
            enabled_participant_count,
            participants,
            shared_model_targets: shared_model_targets_for_group(contexts, &participant_id_set)?,
        });
    }

    groups.sort_by(|left, right| {
        left.participants
            .first()
            .map(|participant| participant.name.to_lowercase())
            .cmp(
                &right
                    .participants
                    .first()
                    .map(|participant| participant.name.to_lowercase()),
            )
    });

    let conflict_count = groups.len();
    let conflict_file_count = conflict_paths.len();
    let message = if conflict_count == 0 {
        "No conflicting MOD groups were found.".to_string()
    } else {
        format!("{conflict_count} independent conflicting MOD group(s) were found.")
    };

    Ok(ModConflictReport {
        conflict_count,
        conflict_file_count,
        groups,
        warnings: Vec::new(),
        message,
    })
}

fn build_mod_conflict_report_from_workspace_index(
    mod_index: &[WorkspaceModIndexEntry],
    store: &ConflictOrderStore,
) -> ModConflictReport {
    let conflict_paths = collect_conflict_path_groups_from_workspace_index(mod_index);
    let mut adjacency: HashMap<String, HashSet<String>> = HashMap::new();

    for conflict_path in &conflict_paths {
        for mod_id in &conflict_path.participant_ids {
            adjacency.entry(mod_id.clone()).or_default().extend(
                conflict_path
                    .participant_ids
                    .iter()
                    .filter(|participant_id| *participant_id != mod_id)
                    .cloned(),
            );
        }
    }

    let mut visited = HashSet::new();
    let mut groups = Vec::new();
    for mod_id in adjacency.keys() {
        if !visited.insert(mod_id.clone()) {
            continue;
        }

        let mut participant_ids = Vec::new();
        let mut pending = vec![mod_id.clone()];
        while let Some(current_id) = pending.pop() {
            participant_ids.push(current_id.clone());
            if let Some(neighbors) = adjacency.get(&current_id) {
                for neighbor in neighbors {
                    if visited.insert(neighbor.clone()) {
                        pending.push(neighbor.clone());
                    }
                }
            }
        }

        participant_ids.sort();
        let group_id = conflict_group_id(&participant_ids);
        let participant_id_set = participant_ids.iter().cloned().collect::<HashSet<_>>();
        let mut participants = mod_index
            .iter()
            .filter(|entry| participant_id_set.contains(&entry.mod_id))
            .map(|entry| ModConflictParticipant {
                mod_id: entry.mod_id.clone(),
                name: entry.name.clone(),
                enabled: entry.enabled,
                order: 0,
            })
            .collect::<Vec<_>>();
        let stored_order = store
            .orders
            .get(&group_id)
            .or_else(|| find_best_stored_order(store, &participant_ids));
        sort_participants_by_conflict_order(&mut participants, stored_order);
        let conflict_files = conflict_paths
            .iter()
            .filter(|path| {
                path.participant_ids
                    .iter()
                    .any(|participant_id| participant_id_set.contains(participant_id))
            })
            .map(|path| path.deploy_relative_path.clone())
            .collect::<Vec<_>>();

        groups.push(ModConflictGroup {
            group_id,
            participant_count: participants.len(),
            conflict_file_count: conflict_files.len(),
            conflict_files,
            enabled_participant_count: participants.len(),
            participants,
            shared_model_targets: shared_model_targets_from_workspace_index(
                mod_index,
                &participant_id_set,
            ),
        });
    }

    groups.sort_by(|left, right| {
        left.participants
            .first()
            .map(|participant| participant.name.to_lowercase())
            .cmp(
                &right
                    .participants
                    .first()
                    .map(|participant| participant.name.to_lowercase()),
            )
    });
    let conflict_count = groups.len();
    let conflict_file_count = conflict_paths.len();
    let message = if conflict_count == 0 {
        "未发现已启用 MOD 之间的文件冲突。".to_string()
    } else {
        format!("发现 {conflict_count} 个相互独立的 MOD 冲突组。")
    };

    ModConflictReport {
        conflict_count,
        conflict_file_count,
        groups,
        warnings: Vec::new(),
        message,
    }
}

fn collect_conflict_path_groups_from_workspace_index(
    mod_index: &[WorkspaceModIndexEntry],
) -> Vec<ConflictPathGroup> {
    let mut participants_by_path: HashMap<String, (String, HashSet<String>)> = HashMap::new();
    for entry in mod_index.iter().filter(|entry| entry.enabled) {
        for deploy_relative_path in &entry.effective_files {
            participants_by_path
                .entry(conflict_path_key(deploy_relative_path))
                .or_insert_with(|| (deploy_relative_path.clone(), HashSet::new()))
                .1
                .insert(entry.mod_id.clone());
        }
    }

    let mut groups = participants_by_path
        .into_values()
        .filter_map(|(deploy_relative_path, participant_ids)| {
            if participant_ids.len() < 2 {
                return None;
            }
            let mut participant_ids = participant_ids.into_iter().collect::<Vec<_>>();
            participant_ids.sort();
            Some(ConflictPathGroup {
                deploy_relative_path,
                participant_ids,
            })
        })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        conflict_path_key(&left.deploy_relative_path)
            .cmp(&conflict_path_key(&right.deploy_relative_path))
    });
    groups
}

fn shared_model_targets_from_workspace_index(
    mod_index: &[WorkspaceModIndexEntry],
    participant_ids: &HashSet<String>,
) -> Vec<SharedModelTarget> {
    struct SharedTargetAccumulator {
        model_kind: String,
        model_id: String,
        sub_kinds: HashSet<String>,
        display_names: HashSet<String>,
        participant_ids: HashSet<String>,
    }

    let mut targets = BTreeMap::<(String, String), SharedTargetAccumulator>::new();
    for entry in mod_index
        .iter()
        .filter(|entry| participant_ids.contains(&entry.mod_id))
    {
        for replacement in &entry.model_replacements {
            let key = (replacement.model_kind.clone(), replacement.model_id.clone());
            let target = targets
                .entry(key)
                .or_insert_with(|| SharedTargetAccumulator {
                    model_kind: replacement.model_kind.clone(),
                    model_id: replacement.model_id.clone(),
                    sub_kinds: HashSet::new(),
                    display_names: HashSet::new(),
                    participant_ids: HashSet::new(),
                });
            target.sub_kinds.insert(replacement.sub_kind.clone());
            target
                .display_names
                .extend(replacement.display_names.iter().cloned());
            target.participant_ids.insert(entry.mod_id.clone());
        }
    }

    let mut shared_targets = targets
        .into_values()
        .filter(|target| target.participant_ids.len() >= 2)
        .map(|target| {
            let mut sub_kinds = target.sub_kinds.into_iter().collect::<Vec<_>>();
            let mut display_names = target.display_names.into_iter().collect::<Vec<_>>();
            sub_kinds.sort();
            display_names.sort();
            SharedModelTarget {
                model_kind: target.model_kind,
                sub_kind: sub_kinds.join("、"),
                model_id: target.model_id,
                display_names,
            }
        })
        .collect::<Vec<_>>();
    shared_targets.sort_by(|left, right| {
        left.model_kind
            .cmp(&right.model_kind)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    shared_targets
}

fn shared_model_targets_for_group(
    contexts: &[InstalledManifestContext],
    participant_ids: &HashSet<String>,
) -> Result<Vec<SharedModelTarget>, String> {
    struct SharedTargetAccumulator {
        model_kind: String,
        model_id: String,
        sub_kinds: HashSet<String>,
        display_names: HashSet<String>,
        participant_ids: HashSet<String>,
    }

    let mut targets = BTreeMap::<(String, String), SharedTargetAccumulator>::new();

    for context in contexts
        .iter()
        .filter(|context| participant_ids.contains(&context.manifest.id))
    {
        let original_replacements =
            model_replacements_for_manifest(&context.manifest, &context.content_path)?;
        for replacement in effective_model_replacements_for_context(
            context,
            &context.manifest,
            &original_replacements,
        )? {
            let key = (replacement.model_kind.clone(), replacement.model_id.clone());
            let target = targets
                .entry(key)
                .or_insert_with(|| SharedTargetAccumulator {
                    model_kind: replacement.model_kind.clone(),
                    model_id: replacement.model_id.clone(),
                    sub_kinds: HashSet::new(),
                    display_names: HashSet::new(),
                    participant_ids: HashSet::new(),
                });
            target.sub_kinds.insert(replacement.sub_kind);
            target.display_names.extend(replacement.display_names);
            target.participant_ids.insert(context.manifest.id.clone());
        }
    }

    let mut shared_targets = targets
        .into_values()
        .filter(|target| target.participant_ids.len() >= 2)
        .map(|target| {
            let mut sub_kinds = target.sub_kinds.into_iter().collect::<Vec<_>>();
            let mut display_names = target.display_names.into_iter().collect::<Vec<_>>();
            sub_kinds.sort();
            display_names.sort();
            SharedModelTarget {
                model_kind: target.model_kind,
                sub_kind: sub_kinds.join("、"),
                model_id: target.model_id,
                display_names,
            }
        })
        .collect::<Vec<_>>();

    shared_targets.sort_by(|left, right| {
        left.model_kind
            .cmp(&right.model_kind)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    Ok(shared_targets)
}

fn find_conflict_group<'a>(
    report: &'a ModConflictReport,
    group_id: &str,
) -> Result<&'a ModConflictGroup, String> {
    report
        .groups
        .iter()
        .find(|group| group.group_id == group_id)
        .ok_or_else(|| format!("No conflict group was found for: {group_id}"))
}

fn collect_conflict_path_groups(
    contexts: &[InstalledManifestContext],
) -> Result<Vec<ConflictPathGroup>, String> {
    let mut participants_by_path: HashMap<String, (String, HashSet<String>)> = HashMap::new();

    for context in contexts {
        if !context.manifest.enabled {
            continue;
        }

        for file in effective_installed_files_for_context(context)? {
            participants_by_path
                .entry(conflict_path_key(&file.deploy_relative_path))
                .or_insert_with(|| (file.deploy_relative_path.clone(), HashSet::new()))
                .1
                .insert(context.manifest.id.clone());
        }
    }

    let mut groups = participants_by_path
        .into_values()
        .filter_map(|(deploy_relative_path, participant_ids)| {
            if participant_ids.len() < 2 {
                return None;
            }

            let mut participant_ids = participant_ids.into_iter().collect::<Vec<_>>();
            participant_ids.sort();
            Some(ConflictPathGroup {
                deploy_relative_path,
                participant_ids,
            })
        })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        conflict_path_key(&left.deploy_relative_path)
            .cmp(&conflict_path_key(&right.deploy_relative_path))
    });
    Ok(groups)
}

fn conflict_group_id(participant_ids: &[String]) -> String {
    format!("mods:{}", participant_ids.join("|"))
}

fn find_best_stored_order<'a>(
    store: &'a ConflictOrderStore,
    participant_ids: &[String],
) -> Option<&'a Vec<String>> {
    store
        .orders
        .values()
        .filter(|stored_order| {
            participant_ids
                .iter()
                .any(|participant_id| stored_order.contains(participant_id))
        })
        .max_by(|left, right| {
            let left_matches = participant_ids
                .iter()
                .filter(|participant_id| left.contains(participant_id))
                .count();
            let right_matches = participant_ids
                .iter()
                .filter(|participant_id| right.contains(participant_id))
                .count();
            left_matches
                .cmp(&right_matches)
                .then_with(|| right.len().cmp(&left.len()))
        })
}

fn conflict_paths_for_group(
    contexts: &[InstalledManifestContext],
    group: &ModConflictGroup,
) -> Result<Vec<ConflictPathGroup>, String> {
    let participant_ids = group
        .participants
        .iter()
        .map(|participant| participant.mod_id.as_str())
        .collect::<HashSet<_>>();

    Ok(collect_conflict_path_groups(contexts)?
        .into_iter()
        .filter(|path| {
            path.participant_ids
                .iter()
                .any(|participant_id| participant_ids.contains(participant_id.as_str()))
        })
        .collect())
}

fn winner_for_conflict_path<'a>(
    group: &'a ModConflictGroup,
    conflict_path: &ConflictPathGroup,
) -> Option<&'a str> {
    group
        .participants
        .iter()
        .find(|participant| {
            participant.enabled && conflict_path.participant_ids.contains(&participant.mod_id)
        })
        .map(|participant| participant.mod_id.as_str())
}

fn reapply_conflict_groups_for_mod_with_progress(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    warnings: &mut Vec<String>,
    progress: &OperationReporter,
) -> Result<(), String> {
    progress.report("正在同步关联冲突", 0, None, None);
    let report = get_mod_conflict_report_with_changed_mod(installed_root, mod_id)?;

    for group in report.groups {
        if group.enabled_participant_count == 0
            || !group
                .participants
                .iter()
                .any(|participant| participant.mod_id == mod_id)
        {
            continue;
        }

        if let Err(error) = apply_conflict_order_from_with_progress(
            installed_root,
            game_root,
            &group.group_id,
            false,
            progress,
        ) {
            warnings.push(format!(
                "Could not reapply conflict group {}: {error}",
                group.group_id
            ));
        }
    }

    Ok(())
}

fn record_enabled_mod_conflict_order(
    installed_root: &Path,
    enabled_mod_id: &str,
) -> Result<(), String> {
    let mut store = read_conflict_order_store(installed_root)?;
    let report = get_mod_conflict_report_with_changed_mod(installed_root, enabled_mod_id)?;
    let mut changed = false;

    for group in report.groups {
        if !group
            .participants
            .iter()
            .any(|participant| participant.mod_id == enabled_mod_id)
        {
            continue;
        }

        let mut order = group
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();
        order.retain(|mod_id| mod_id != enabled_mod_id);
        order.insert(0, enabled_mod_id.to_string());
        store.orders.insert(group.group_id, order);
        changed = true;
    }

    if changed {
        save_conflict_order_store(installed_root, &store)?;
    }

    Ok(())
}

fn restore_enabled_versions_for_paths_with_progress(
    installed_root: &Path,
    game_root: &Path,
    deploy_relative_paths: &[String],
    warnings: &mut Vec<String>,
    progress: &OperationReporter,
) -> Result<(), String> {
    progress.report(
        "正在恢复仍启用的 MOD",
        0,
        Some(deploy_relative_paths.len()),
        None,
    );
    let requested_path_keys = deploy_relative_paths
        .iter()
        .map(|path| conflict_path_key(path))
        .collect::<HashSet<_>>();
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let mut contexts = if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let candidate_ids = stored
            .mod_index
            .iter()
            .filter(|entry| {
                entry.enabled
                    && entry
                        .effective_files
                        .iter()
                        .any(|path| requested_path_keys.contains(&conflict_path_key(path)))
            })
            .map(|entry| entry.mod_id.clone())
            .collect::<Vec<_>>();
        candidate_ids
            .iter()
            .map(|mod_id| load_installed_manifest(installed_root, mod_id))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        load_all_installed_manifests(installed_root)?
    };
    let store = read_conflict_order_store(installed_root)?;
    let deployed_at = unix_seconds_now()?;
    let mut seen_paths = HashSet::new();
    let mut changed = false;
    let effective_files_by_mod = contexts
        .iter()
        .map(|context| {
            Ok((
                context.manifest.id.clone(),
                effective_installed_files_for_context(context)?,
            ))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;

    let mut completed_path_count = 0;
    for deploy_relative_path in deploy_relative_paths {
        let path_key = conflict_path_key(deploy_relative_path);

        if !seen_paths.insert(path_key.clone()) {
            completed_path_count += 1;
            progress.report(
                "正在恢复仍启用的 MOD",
                completed_path_count,
                Some(deploy_relative_paths.len()),
                Some(deploy_relative_path.clone()),
            );
            continue;
        }

        let mut participants = contexts
            .iter()
            .filter(|context| {
                context.manifest.enabled
                    && effective_files_by_mod
                        .get(&context.manifest.id)
                        .into_iter()
                        .flatten()
                        .any(|file| conflict_path_key(&file.deploy_relative_path) == path_key)
            })
            .map(|context| ModConflictParticipant {
                mod_id: context.manifest.id.clone(),
                name: manifest_display_name(&context.manifest),
                enabled: true,
                order: 0,
            })
            .collect::<Vec<_>>();

        if participants.is_empty() {
            completed_path_count += 1;
            progress.report(
                "正在恢复仍启用的 MOD",
                completed_path_count,
                Some(deploy_relative_paths.len()),
                Some(deploy_relative_path.clone()),
            );
            continue;
        }

        let mut participant_ids = participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();
        participant_ids.sort();
        let stored_order = find_best_stored_order(&store, &participant_ids);
        sort_participants_by_conflict_order(&mut participants, stored_order);
        let winner_mod_id = participants.first().unwrap().mod_id.clone();
        let Some(winner_index) = contexts
            .iter()
            .position(|context| context.manifest.id == winner_mod_id)
        else {
            completed_path_count += 1;
            progress.report(
                "正在恢复仍启用的 MOD",
                completed_path_count,
                Some(deploy_relative_paths.len()),
                Some(deploy_relative_path.clone()),
            );
            continue;
        };
        let Some(source_file) = effective_files_by_mod
            .get(&winner_mod_id)
            .into_iter()
            .flatten()
            .find(|file| conflict_path_key(&file.deploy_relative_path) == path_key)
            .cloned()
        else {
            completed_path_count += 1;
            progress.report(
                "正在恢复仍启用的 MOD",
                completed_path_count,
                Some(deploy_relative_paths.len()),
                Some(deploy_relative_path.clone()),
            );
            continue;
        };
        let target_path = game_root.join(relative_string_to_path(deploy_relative_path)?);
        let copy_result = (|| {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "Could not create deployment directory {}: {error}",
                        parent.display()
                    )
                })?;
            }

            deploy_effective_file(&contexts[winner_index], &source_file, &target_path)?;
            Ok::<(), String>(())
        })();

        if let Err(error) = copy_result {
            warnings.push(error);
            completed_path_count += 1;
            progress.report(
                "正在恢复仍启用的 MOD",
                completed_path_count,
                Some(deploy_relative_paths.len()),
                Some(deploy_relative_path.clone()),
            );
            continue;
        }

        for context in &mut contexts {
            context
                .manifest
                .deployed_files
                .retain(|file| conflict_path_key(&file.deploy_relative_path) != path_key);

            if context.manifest.id == winner_mod_id {
                context.manifest.deployed_files.push(DeployedModFile {
                    deploy_relative_path: deploy_relative_path.clone(),
                    deployed_path: path_to_string(&target_path),
                    deployed_at_unix_seconds: deployed_at,
                    deployment_origin: DeploymentOrigin::Copied,
                    is_adopted: false,
                });
            }
        }

        changed = true;
        completed_path_count += 1;
        progress.report(
            "正在恢复仍启用的 MOD",
            completed_path_count,
            Some(deploy_relative_paths.len()),
            Some(deploy_relative_path.clone()),
        );
    }

    if changed {
        for context in &contexts {
            save_manifest(&context.manifest_path, &context.manifest)?;
        }
    }

    Ok(())
}

fn remove_deployed_files_with_progress(
    installed_root: &Path,
    game_root: &Path,
    deployed_files: &[DeployedModFile],
    context: Option<&InstalledManifestContext>,
    warnings: &mut Vec<String>,
    progress: &OperationReporter,
    phase: &str,
) -> Result<Vec<String>, String> {
    let mut removed_paths = Vec::new();
    let file_total = deployed_files.len();
    progress.report(phase, 0, Some(file_total), None);

    for (index, deployed_file) in deployed_files.iter().enumerate() {
        let target_relative_path = relative_string_to_path(&deployed_file.deploy_relative_path)?;
        let target_path = game_root.join(target_relative_path);

        if !target_path.exists() {
            warnings.push(format!(
                "Deployment target was already missing: {}",
                target_path.display()
            ));
            progress.report(
                phase,
                index + 1,
                Some(file_total),
                Some(deployed_file.deploy_relative_path.clone()),
            );
            continue;
        }

        if target_path.is_dir() {
            return Err(format!(
                "Refusing to remove a directory during deployment cleanup: {}",
                target_path.display()
            ));
        }

        if is_observed_deployment_file(deployed_file) {
            let matches_expected = context
                .map(|context| {
                    effective_deployed_file_matches_target(context, deployed_file, &target_path)
                })
                .transpose()?;
            if matches_expected != Some(true) {
                warnings.push(format!(
                    "已保留接管文件：{} 当前内容不再与 Acumod 本地库一致。",
                    target_path.display()
                ));
                progress.report(
                    phase,
                    index + 1,
                    Some(file_total),
                    Some(deployed_file.deploy_relative_path.clone()),
                );
                continue;
            }
            if let Some(context) = context {
                if has_other_enabled_equivalent_provider(
                    installed_root,
                    &context.manifest.id,
                    &deployed_file.deploy_relative_path,
                    &target_path,
                )? {
                    warnings.push(format!(
                        "已保留观察文件：仍有其它已启用 MOD 提供相同内容：{}",
                        target_path.display()
                    ));
                    progress.report(
                        phase,
                        index + 1,
                        Some(file_total),
                        Some(deployed_file.deploy_relative_path.clone()),
                    );
                    continue;
                }
            }
        }

        fs::remove_file(&target_path).map_err(|error| {
            format!(
                "Could not remove deployed file {}: {error}",
                target_path.display()
            )
        })?;
        removed_paths.push(deployed_file.deploy_relative_path.clone());
        cleanup_empty_parent_directories(&target_path, game_root, warnings);
        progress.report(
            phase,
            index + 1,
            Some(file_total),
            Some(deployed_file.deploy_relative_path.clone()),
        );
    }

    Ok(removed_paths)
}

fn is_observed_deployment_file(file: &DeployedModFile) -> bool {
    file.deployment_origin == DeploymentOrigin::Observed || file.is_adopted
}

fn has_other_enabled_equivalent_provider(
    installed_root: &Path,
    current_mod_id: &str,
    deploy_relative_path: &str,
    target_path: &Path,
) -> Result<bool, String> {
    let path_key = conflict_path_key(deploy_relative_path);
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let contexts = if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let candidate_ids = stored
            .mod_index
            .iter()
            .filter(|entry| {
                entry.enabled
                    && entry.mod_id != current_mod_id
                    && entry
                        .effective_files
                        .iter()
                        .any(|path| conflict_path_key(path) == path_key)
            })
            .map(|entry| entry.mod_id.clone())
            .collect::<Vec<_>>();
        candidate_ids
            .iter()
            .map(|mod_id| load_installed_manifest(installed_root, mod_id))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        load_all_installed_manifests(installed_root)?
    };

    for context in contexts {
        if !context.manifest.enabled || context.manifest.id == current_mod_id {
            continue;
        }
        let has_equivalent_file = effective_installed_files_for_context(&context)?
            .into_iter()
            .filter(|file| conflict_path_key(&file.deploy_relative_path) == path_key)
            .map(|file| effective_file_matches_target(&context, &file, target_path))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|matches| matches);
        if has_equivalent_file {
            return Ok(true);
        }
    }
    Ok(false)
}

fn effective_deployed_file_matches_target(
    context: &InstalledManifestContext,
    deployed_file: &DeployedModFile,
    target_path: &Path,
) -> Result<bool, String> {
    let effective_file = effective_installed_files_for_context(context)?
        .into_iter()
        .find(|file| {
            conflict_path_key(&file.deploy_relative_path)
                == conflict_path_key(&deployed_file.deploy_relative_path)
        })
        .ok_or_else(|| {
            format!(
                "接管记录在本地 MOD 中找不到对应文件：{}",
                deployed_file.deploy_relative_path
            )
        })?;
    effective_file_matches_target(context, &effective_file, target_path)
}

fn cleanup_empty_parent_directories(
    file_path: &Path,
    game_root: &Path,
    warnings: &mut Vec<String>,
) {
    let Some(mut directory) = file_path.parent().map(Path::to_path_buf) else {
        return;
    };

    // Only climb through directories that contained a file Acumod just removed.
    // `remove_dir` is intentionally non-recursive: it succeeds only when the
    // directory is empty, so manually installed files stop the cleanup safely.
    while directory.starts_with(game_root) && directory != game_root {
        match fs::remove_dir(&directory) {
            Ok(()) => {
                let Some(parent) = directory.parent() else {
                    break;
                };
                directory = parent.to_path_buf();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let Some(parent) = directory.parent() else {
                    break;
                };
                directory = parent.to_path_buf();
            }
            Err(error) if error.kind() == ErrorKind::DirectoryNotEmpty => break,
            Err(error) => {
                warnings.push(format!(
                    "Could not remove empty deployment directory {}: {error}",
                    directory.display()
                ));
                break;
            }
        }
    }
}

fn build_deployment_plan(
    installed_root: &Path,
    game_root: &Path,
    context: &InstalledManifestContext,
) -> Result<ModDeploymentPlan, String> {
    let deployed_index = deployed_file_index(installed_root)?;
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut requires_overwrite_confirmation = false;

    let effective_files = effective_installed_files_for_context(context)?;
    let conflicts =
        enabled_mod_conflicts_for_files(installed_root, &context.manifest.id, &effective_files)?;
    for file in &effective_files {
        let source_path = source_path_for_installed_file(context, &file.installed_file)?;
        preview_mrl3_rewrite_count(&source_path, file)?;
        preview_evam_rewrite_count(&source_path, file)?;
        let target_relative_path = relative_string_to_path(&file.deploy_relative_path)?;
        let target_path = game_root.join(target_relative_path);
        let target_key = deployment_key(&target_path);
        let target_managed_mod_id = deployed_index.get(&target_key).cloned();
        let target_managed_by_current_mod = target_managed_mod_id
            .as_deref()
            .map(|id| id == context.manifest.id)
            .unwrap_or(false);
        let target_managed_by_other_mod = target_managed_mod_id
            .as_deref()
            .map(|id| id != context.manifest.id)
            .unwrap_or(false);
        let target_exists = target_path.exists();

        if target_exists && !target_managed_by_current_mod {
            requires_overwrite_confirmation = true;
        }

        if target_managed_by_other_mod {
            warnings.push(format!(
                "Target is recorded as deployed by another MOD: {}",
                target_path.display()
            ));
        } else if target_exists && target_managed_mod_id.is_none() {
            warnings.push(format!(
                "Target already exists but is not recorded as Acumod-managed: {}",
                target_path.display()
            ));
        }

        files.push(ModDeploymentPlanFile {
            deploy_relative_path: file.deploy_relative_path.clone(),
            source_path: path_to_string(&source_path),
            target_path: path_to_string(&target_path),
            target_exists,
            target_managed_by_current_mod,
            target_managed_by_other_mod,
            target_managed_mod_id,
        });
    }

    let status = if requires_overwrite_confirmation {
        "needsOverwriteConfirmation"
    } else {
        "ready"
    };
    let message = if requires_overwrite_confirmation {
        "Some target files already exist. Confirm before enabling this MOD."
    } else if context.manifest.enabled {
        "MOD is already enabled; enabling again will refresh its deployed files."
    } else {
        "MOD can be enabled without overwriting untracked files."
    };

    Ok(ModDeploymentPlan {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        status: status.to_string(),
        message: message.to_string(),
        file_count: effective_files.len(),
        files,
        conflicts,
        warnings,
        requires_overwrite_confirmation,
    })
}

fn enabled_mod_conflicts_for_files(
    installed_root: &Path,
    current_mod_id: &str,
    effective_files: &[EffectiveInstalledModFile],
) -> Result<Vec<ModDeploymentConflict>, String> {
    let current_paths = effective_files
        .iter()
        .map(|file| {
            (
                conflict_path_key(&file.deploy_relative_path),
                file.deploy_relative_path.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let mut conflicts = Vec::new();

    if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        for entry in stored
            .mod_index
            .iter()
            .filter(|entry| entry.enabled && entry.mod_id != current_mod_id)
        {
            let mut conflict_files = entry
                .effective_files
                .iter()
                .filter_map(|path| current_paths.get(&conflict_path_key(path)).cloned())
                .collect::<Vec<_>>();
            conflict_files.sort_by_key(|path| conflict_path_key(path));
            conflict_files
                .dedup_by(|left, right| conflict_path_key(left) == conflict_path_key(right));
            if !conflict_files.is_empty() {
                conflicts.push(ModDeploymentConflict {
                    mod_id: entry.mod_id.clone(),
                    name: entry.name.clone(),
                    conflict_files,
                });
            }
        }
    } else {
        for other in load_all_installed_manifests(installed_root)? {
            if !other.manifest.enabled || other.manifest.id == current_mod_id {
                continue;
            }
            let mut conflict_files = effective_installed_files_for_context(&other)?
                .iter()
                .filter_map(|file| {
                    current_paths
                        .get(&conflict_path_key(&file.deploy_relative_path))
                        .cloned()
                })
                .collect::<Vec<_>>();
            conflict_files.sort_by_key(|path| conflict_path_key(path));
            conflict_files
                .dedup_by(|left, right| conflict_path_key(left) == conflict_path_key(right));
            if !conflict_files.is_empty() {
                conflicts.push(ModDeploymentConflict {
                    mod_id: other.manifest.id.clone(),
                    name: manifest_display_name(&other.manifest),
                    conflict_files,
                });
            }
        }
    }

    conflicts.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.mod_id.cmp(&right.mod_id))
    });
    Ok(conflicts)
}

fn load_installed_manifest(
    installed_root: &Path,
    mod_id: &str,
) -> Result<InstalledManifestContext, String> {
    validate_mod_id(mod_id)?;
    let mod_path = installed_root.join(mod_id);

    if !mod_path.is_dir() {
        return Err(format!("Installed MOD was not found: {mod_id}"));
    }

    let installed_root = installed_root.canonicalize().map_err(|error| {
        format!(
            "Could not resolve installed MOD root {}: {error}",
            installed_root.display()
        )
    })?;
    let mod_path = mod_path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve installed MOD path {}: {error}",
            mod_path.display()
        )
    })?;

    if !mod_path.starts_with(&installed_root) {
        return Err(format!(
            "Installed MOD path escaped the managed library: {}",
            mod_path.display()
        ));
    }

    let content_path = mod_path.join("content");
    let manifest_path = mod_path.join("manifest.json");
    let manifest = read_manifest(&manifest_path)?;

    Ok(InstalledManifestContext {
        mod_path,
        content_path,
        manifest_path,
        manifest,
    })
}

fn load_all_installed_manifests(
    installed_root: &Path,
) -> Result<Vec<InstalledManifestContext>, String> {
    load_all_installed_manifests_with_progress(installed_root, &OperationReporter::default())
}

fn load_all_installed_manifests_with_progress(
    installed_root: &Path,
    progress: &OperationReporter,
) -> Result<Vec<InstalledManifestContext>, String> {
    load_all_installed_manifests_with_progress_phase(installed_root, progress, "正在读取 MOD 清单")
}

fn load_all_installed_manifests_with_progress_phase(
    installed_root: &Path,
    progress: &OperationReporter,
    phase: &str,
) -> Result<Vec<InstalledManifestContext>, String> {
    let mut mod_ids = Vec::new();

    if !installed_root.exists() {
        return Ok(Vec::new());
    }

    for entry in fs::read_dir(installed_root).map_err(|error| {
        format!(
            "Could not read installed MOD directory {}: {error}",
            installed_root.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Could not read entry under {}: {error}",
                installed_root.display()
            )
        })?;
        let mod_path = entry.path();

        if !mod_path.is_dir()
            || mod_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
        {
            continue;
        }

        let Some(mod_id) = mod_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !mod_path.join("manifest.json").is_file() {
            continue;
        }

        mod_ids.push(mod_id.to_string());
    }

    let total = mod_ids.len();
    let mut contexts = Vec::with_capacity(total);
    for (index, mod_id) in mod_ids.into_iter().enumerate() {
        let context = load_installed_manifest(installed_root, &mod_id)?;
        progress.report(
            phase,
            index + 1,
            Some(total),
            Some(manifest_display_name(&context.manifest)),
        );
        contexts.push(context);
    }

    sort_contexts_by_installation(&mut contexts);

    Ok(contexts)
}

fn sort_contexts_by_installation(contexts: &mut [InstalledManifestContext]) {
    contexts.sort_by(|left, right| {
        left.manifest
            .installed_at_unix_seconds
            .cmp(&right.manifest.installed_at_unix_seconds)
            .then_with(|| {
                left.manifest
                    .name
                    .to_lowercase()
                    .cmp(&right.manifest.name.to_lowercase())
            })
    });
}

fn sort_participants_by_conflict_order(
    participants: &mut [ModConflictParticipant],
    stored_order: Option<&Vec<String>>,
) {
    let mut order_index = HashMap::new();

    if let Some(stored_order) = stored_order {
        for (index, mod_id) in stored_order.iter().enumerate() {
            order_index.insert(mod_id, index);
        }
    }

    participants.sort_by(|left, right| {
        order_index
            .get(&left.mod_id)
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(
                &order_index
                    .get(&right.mod_id)
                    .copied()
                    .unwrap_or(usize::MAX),
            )
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    for (index, participant) in participants.iter_mut().enumerate() {
        participant.order = index + 1;
    }
}

fn mod_branch_group_store_path(installed_root: &Path) -> PathBuf {
    let is_standard_installed_root = installed_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("installed"));
    if is_standard_installed_root {
        return installed_root
            .parent()
            .map(|mods_path| mods_path.join("branch-groups.json"))
            .unwrap_or_else(|| installed_root.join(".branch-groups.json"));
    }
    installed_root.join(".branch-groups.json")
}

fn read_mod_branch_group_store(installed_root: &Path) -> Result<ModBranchGroupStore, String> {
    let path = mod_branch_group_store_path(installed_root);
    if !path.is_file() {
        return Ok(ModBranchGroupStore {
            schema_version: MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION,
            groups: Vec::new(),
        });
    }
    let json = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取 MOD 分支组文件 {}：{error}", path.display()))?;
    serde_json::from_str(&json)
        .map_err(|error| format!("无法解析 MOD 分支组文件 {}：{error}", path.display()))
}

fn save_mod_branch_group_store(
    installed_root: &Path,
    store: &ModBranchGroupStore,
) -> Result<(), String> {
    let path = mod_branch_group_store_path(installed_root);
    let mut store = store.clone();
    store.schema_version = MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION;
    let json = serde_json::to_string_pretty(&store)
        .map_err(|error| format!("无法序列化 MOD 分支组：{error}"))?;
    write_text_atomically(&path, &json, "MOD 分支组文件")
}

fn normalize_mod_branch_group_store(
    store: &mut ModBranchGroupStore,
    installed_ids: &HashSet<String>,
) -> bool {
    let previous = store.groups.clone();
    let schema_changed = store.schema_version != MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION;
    let mut assigned_ids = HashSet::new();
    for group in &mut store.groups {
        group
            .mod_ids
            .retain(|mod_id| installed_ids.contains(mod_id) && assigned_ids.insert(mod_id.clone()));
    }
    // 单个 MOD 不构成分支组；卸载或移出分支后自动拆散残余单例。
    store.groups.retain(|group| group.mod_ids.len() >= 2);
    store.schema_version = MOD_BRANCH_GROUP_STORE_SCHEMA_VERSION;
    schema_changed || store.groups != previous
}

fn load_normalized_mod_branch_groups(
    installed_root: &Path,
    installed_ids: &HashSet<String>,
) -> Result<Vec<ModBranchGroup>, String> {
    let mut store = read_mod_branch_group_store(installed_root)?;
    if normalize_mod_branch_group_store(&mut store, installed_ids) {
        save_mod_branch_group_store(installed_root, &store)?;
    }
    Ok(store.groups)
}

fn mod_library_order_store_path(installed_root: &Path) -> PathBuf {
    let is_standard_installed_root = installed_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("installed"))
        .unwrap_or(false);

    if is_standard_installed_root {
        return installed_root
            .parent()
            .map(|mods_path| mods_path.join("mod-library-order.json"))
            .unwrap_or_else(|| installed_root.join(".mod-library-order.json"));
    }

    installed_root.join(".mod-library-order.json")
}

fn read_mod_library_order_store(installed_root: &Path) -> Result<ModLibraryOrderStore, String> {
    let store_path = mod_library_order_store_path(installed_root);
    if !store_path.exists() {
        return Ok(ModLibraryOrderStore {
            schema_version: default_mod_library_order_store_schema_version(),
            manual_mod_ids: Vec::new(),
            import_mod_ids: Vec::new(),
        });
    }

    let store_json = fs::read_to_string(&store_path).map_err(|error| {
        format!(
            "Could not read MOD library order store {}: {error}",
            store_path.display()
        )
    })?;
    serde_json::from_str::<ModLibraryOrderStore>(&store_json).map_err(|error| {
        format!(
            "Could not parse MOD library order store {}: {error}",
            store_path.display()
        )
    })
}

fn save_mod_library_order_store(
    installed_root: &Path,
    store: &ModLibraryOrderStore,
) -> Result<(), String> {
    let store_path = mod_library_order_store_path(installed_root);
    if let Some(parent) = store_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create MOD library order directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let store_json = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Could not serialize MOD library order store: {error}"))?;
    fs::write(&store_path, store_json).map_err(|error| {
        format!(
            "Could not save MOD library order store {}: {error}",
            store_path.display()
        )
    })
}

fn installation_mod_library_order(mods: &[InstalledModSummary]) -> Vec<String> {
    let mut ordered = mods.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.installed_at_unix_seconds
            .cmp(&right.installed_at_unix_seconds)
            .then_with(|| left.id.cmp(&right.id))
    });
    ordered
        .into_iter()
        .map(|installed_mod| installed_mod.id.clone())
        .collect()
}

fn normalized_order_ids(stored_ids: &[String], installation_order: &[String]) -> Vec<String> {
    let installed_ids = installation_order
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(installation_order.len());
    for mod_id in stored_ids {
        if installed_ids.contains(mod_id.as_str()) && seen.insert(mod_id.as_str()) {
            normalized.push(mod_id.clone());
        }
    }
    for mod_id in installation_order {
        if seen.insert(mod_id.as_str()) {
            normalized.push(mod_id.clone());
        }
    }
    normalized
}

fn normalized_mod_library_orders(
    store: &ModLibraryOrderStore,
    mods: &[InstalledModSummary],
) -> (Vec<String>, Vec<String>) {
    let installation_order = installation_mod_library_order(mods);
    normalized_mod_library_orders_from_installation(store, &installation_order)
}

fn normalized_mod_library_orders_from_installation(
    store: &ModLibraryOrderStore,
    installation_order: &[String],
) -> (Vec<String>, Vec<String>) {
    let import_order = normalized_order_ids(&store.import_mod_ids, installation_order);
    let manual_order = normalized_order_ids(&store.manual_mod_ids, &import_order);
    (manual_order, import_order)
}

fn apply_mod_library_order(
    installed_root: &Path,
    mods: &mut Vec<InstalledModSummary>,
) -> Result<(), String> {
    let mut store = read_mod_library_order_store(installed_root)?;
    let (manual_order, import_order) = normalized_mod_library_orders(&store, mods);
    if store.schema_version != MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION
        || store.manual_mod_ids != manual_order
        || store.import_mod_ids != import_order
    {
        store.schema_version = MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION;
        store.manual_mod_ids = manual_order.clone();
        store.import_mod_ids = import_order;
        save_mod_library_order_store(installed_root, &store)?;
    }
    let order_index = manual_order
        .iter()
        .enumerate()
        .map(|(index, mod_id)| (mod_id.as_str(), index))
        .collect::<HashMap<_, _>>();

    mods.sort_by(|left, right| {
        order_index
            .get(left.id.as_str())
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(
                &order_index
                    .get(right.id.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
            )
    });
    Ok(())
}

fn move_mod_library_item_from(
    installed_root: &Path,
    mod_id: &str,
    target_mod_id: &str,
    place_after: bool,
) -> Result<(), String> {
    move_mod_library_items_from(
        installed_root,
        &[mod_id.to_string()],
        &[target_mod_id.to_string()],
        place_after,
    )
}

fn move_mod_library_items_from(
    installed_root: &Path,
    mod_ids: &[String],
    target_mod_ids: &[String],
    place_after: bool,
) -> Result<(), String> {
    if mod_ids.is_empty() || target_mod_ids.is_empty() {
        return Err("排序来源和目标不能为空。".to_string());
    }
    let source_ids = mod_ids.iter().cloned().collect::<HashSet<_>>();
    let target_ids = target_mod_ids.iter().cloned().collect::<HashSet<_>>();
    if source_ids.len() != mod_ids.len() || target_ids.len() != target_mod_ids.len() {
        return Err("排序来源或目标包含重复 MOD。".to_string());
    }
    if source_ids.iter().any(|mod_id| target_ids.contains(mod_id)) {
        return Ok(());
    }
    for mod_id in source_ids.iter().chain(target_ids.iter()) {
        validate_mod_id(mod_id)?;
        load_installed_manifest(installed_root, mod_id)?;
    }

    let mut store = read_mod_library_order_store(installed_root)?;
    let has_all_items = source_ids.iter().chain(target_ids.iter()).all(|mod_id| {
        store
            .manual_mod_ids
            .iter()
            .any(|stored_id| stored_id == mod_id)
    });
    let mut order = if has_all_items
        && store.schema_version == MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION
        && !store.import_mod_ids.is_empty()
    {
        // The list view persists a complete order. Reuse it so one drag does not
        // rescan every installed MOD and its model-recognition data.
        store.manual_mod_ids.clone()
    } else {
        let installed_mods = list_installed_mods_from(installed_root)?.mods;
        normalized_mod_library_orders(&store, &installed_mods).0
    };
    let moved_mod_ids = order
        .iter()
        .filter(|mod_id| source_ids.contains(*mod_id))
        .cloned()
        .collect::<Vec<_>>();
    if moved_mod_ids.len() != source_ids.len() {
        return Err("未能在 MOD 库顺序中找到全部排序来源。".to_string());
    }
    order.retain(|mod_id| !source_ids.contains(mod_id));
    let target_positions = order
        .iter()
        .enumerate()
        .filter_map(|(index, mod_id)| target_ids.contains(mod_id).then_some(index))
        .collect::<Vec<_>>();
    if target_positions.len() != target_ids.len() {
        return Err("未能在 MOD 库顺序中找到全部排序目标。".to_string());
    }
    let insert_index = if place_after {
        target_positions.iter().max().copied().unwrap_or(0) + 1
    } else {
        target_positions.iter().min().copied().unwrap_or(0)
    };
    order.splice(insert_index..insert_index, moved_mod_ids);
    store.schema_version = MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION;
    store.manual_mod_ids = order;
    save_mod_library_order_store(installed_root, &store)
}

fn replace_mod_library_order_from(
    installed_root: &Path,
    mod_ids: Vec<String>,
    installation_order: &[String],
) -> Result<(), String> {
    let submitted_ids = mod_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let installed_ids = installation_order
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if submitted_ids.len() != mod_ids.len() {
        return Err("保存的排序中包含重复 MOD。".to_string());
    }
    if submitted_ids != installed_ids {
        return Err("保存排序时必须包含本地库中的全部 MOD，且不能包含未知 MOD。".to_string());
    }

    let mut store = read_mod_library_order_store(installed_root)?;
    let (_, import_order) =
        normalized_mod_library_orders_from_installation(&store, installation_order);
    store.schema_version = MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION;
    store.manual_mod_ids = mod_ids;
    store.import_mod_ids = import_order;
    save_mod_library_order_store(installed_root, &store)
}

fn restore_mod_library_import_order_from(
    installed_root: &Path,
    installation_order: &[String],
) -> Result<(), String> {
    let mut store = read_mod_library_order_store(installed_root)?;
    let (_, import_order) =
        normalized_mod_library_orders_from_installation(&store, installation_order);
    store.schema_version = MOD_LIBRARY_ORDER_STORE_SCHEMA_VERSION;
    store.manual_mod_ids = import_order.clone();
    store.import_mod_ids = import_order;
    save_mod_library_order_store(installed_root, &store)
}

fn remove_mod_from_library_order(installed_root: &Path, mod_id: &str) -> Result<(), String> {
    let mut store = read_mod_library_order_store(installed_root)?;
    let original_manual_count = store.manual_mod_ids.len();
    let original_import_count = store.import_mod_ids.len();
    store
        .manual_mod_ids
        .retain(|current_id| current_id != mod_id);
    store
        .import_mod_ids
        .retain(|current_id| current_id != mod_id);
    if store.manual_mod_ids.len() != original_manual_count
        || store.import_mod_ids.len() != original_import_count
    {
        save_mod_library_order_store(installed_root, &store)?;
    }
    Ok(())
}

fn conflict_order_store_path(installed_root: &Path) -> PathBuf {
    installed_root.join("conflict-orders.json")
}

fn conflict_path_key(deploy_relative_path: &str) -> String {
    deploy_relative_path.replace('\\', "/").to_lowercase()
}

fn read_conflict_order_store(installed_root: &Path) -> Result<ConflictOrderStore, String> {
    let store_path = conflict_order_store_path(installed_root);

    if !store_path.exists() {
        return Ok(ConflictOrderStore {
            schema_version: default_conflict_order_schema_version(),
            orders: HashMap::new(),
        });
    }

    let store_json = fs::read_to_string(&store_path).map_err(|error| {
        format!(
            "Could not read conflict order store {}: {error}",
            store_path.display()
        )
    })?;

    let mut store = serde_json::from_str::<ConflictOrderStore>(&store_json).map_err(|error| {
        format!(
            "Could not parse conflict order store {}: {error}",
            store_path.display()
        )
    })?;

    // Schema 1 used the last entry as the winner. Reverse it once so existing
    // deployments retain the same winner under the top-first priority rule.
    if store.schema_version < default_conflict_order_schema_version() {
        for order in store.orders.values_mut() {
            order.reverse();
        }
        store.schema_version = default_conflict_order_schema_version();
        save_conflict_order_store(installed_root, &store)?;
    }

    Ok(store)
}

fn save_conflict_order_store(
    installed_root: &Path,
    store: &ConflictOrderStore,
) -> Result<(), String> {
    let store_path = conflict_order_store_path(installed_root);
    let store_json = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Could not serialize conflict order store: {error}"))?;

    write_text_atomically(&store_path, &store_json, "冲突顺序文件")
}

fn read_manifest(manifest_path: &Path) -> Result<InstalledModManifest, String> {
    let manifest_json = fs::read_to_string(manifest_path).map_err(|error| {
        format!(
            "Could not read MOD manifest {}: {error}",
            manifest_path.display()
        )
    })?;

    let mut manifest =
        serde_json::from_str::<InstalledModManifest>(&manifest_json).map_err(|error| {
            format!(
                "Could not parse MOD manifest {}: {error}",
                manifest_path.display()
            )
        })?;
    for deployed_file in &mut manifest.deployed_files {
        if deployed_file.is_adopted {
            // 上一版原型的 isAdopted 与新字段 observed 语义相同，读取时立即按更严格的
            // 删除保护处理，下一次保存会完成字段迁移。
            deployed_file.deployment_origin = DeploymentOrigin::Observed;
        }
    }
    Ok(manifest)
}

fn save_manifest(manifest_path: &Path, manifest: &InstalledModManifest) -> Result<(), String> {
    let mut manifest_to_save = manifest.clone();
    manifest_to_save.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    for deployed_file in &mut manifest_to_save.deployed_files {
        if deployed_file.is_adopted {
            deployed_file.deployment_origin = DeploymentOrigin::Observed;
            deployed_file.is_adopted = false;
        }
    }
    let manifest_json = serde_json::to_string_pretty(&manifest_to_save)
        .map_err(|error| format!("Could not serialize MOD manifest: {error}"))?;
    write_text_atomically(manifest_path, &manifest_json, "MOD 清单")
}

fn write_text_atomically(path: &Path, contents: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定{label}的父目录：{}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("无法确定{label}文件名：{}", path.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("无法生成{label}临时文件名：{error}"))?
        .as_nanos();
    let temporary_path = parent.join(format!(".{file_name}.{stamp}.tmp"));

    fs::write(&temporary_path, contents).map_err(|error| {
        format!(
            "无法写入{label}临时文件 {}：{error}",
            temporary_path.display()
        )
    })?;
    fs::rename(&temporary_path, path)
        .map_err(|error| format!("无法原子替换{label} {}：{error}", path.display()))
}

fn source_path_for_installed_file(
    context: &InstalledManifestContext,
    file: &InstalledModFile,
) -> Result<PathBuf, String> {
    let library_relative_path = relative_string_to_path(&file.library_relative_path)?;
    let source_path = context.mod_path.join(library_relative_path);

    if !source_path.is_file() {
        return Err(format!(
            "Installed MOD file is missing from library: {}",
            source_path.display()
        ));
    }

    let content_root = context.content_path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve MOD content directory {}: {error}",
            context.content_path.display()
        )
    })?;
    let source_path = source_path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve MOD library file {}: {error}",
            source_path.display()
        )
    })?;

    if !source_path.starts_with(&content_root) {
        return Err(format!(
            "Installed MOD file escaped its content directory: {}",
            source_path.display()
        ));
    }

    Ok(source_path)
}

fn deployed_file_index(installed_root: &Path) -> Result<HashMap<String, String>, String> {
    let mut deployed_files = HashMap::new();

    if !installed_root.exists() {
        return Ok(deployed_files);
    }

    for entry in fs::read_dir(installed_root).map_err(|error| {
        format!(
            "Could not read installed MOD directory {}: {error}",
            installed_root.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Could not read entry under {}: {error}",
                installed_root.display()
            )
        })?;
        let mod_path = entry.path();

        if !mod_path.is_dir()
            || mod_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with('.'))
                .unwrap_or(false)
        {
            continue;
        }

        let manifest_path = mod_path.join("manifest.json");

        if !manifest_path.is_file() {
            continue;
        }

        let manifest = read_manifest(&manifest_path)?;

        for deployed_file in manifest.deployed_files {
            deployed_files.insert(
                deployment_key(Path::new(&deployed_file.deployed_path)),
                manifest.id.clone(),
            );
        }
    }

    Ok(deployed_files)
}

fn resolve_game_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_config = config::load(app)?;
    let raw_path = app_config
        .game_directory
        .ok_or_else(|| "Set a valid MHW game directory before deploying MODs.".to_string())?;
    let game_root = PathBuf::from(raw_path);

    if !game_root.is_dir() {
        return Err(format!(
            "Configured MHW game directory does not exist: {}",
            game_root.display()
        ));
    }

    if !game_root.join("MonsterHunterWorld.exe").is_file() {
        return Err(format!(
            "Configured MHW game directory is missing MonsterHunterWorld.exe: {}",
            game_root.display()
        ));
    }

    game_root
        .canonicalize()
        .map_err(|error| format!("Could not resolve MHW game directory: {error}"))
}

fn validate_mod_id(mod_id: &str) -> Result<(), String> {
    if mod_id.is_empty()
        || mod_id == "."
        || mod_id == ".."
        || mod_id.contains('/')
        || mod_id.contains('\\')
        || mod_id.contains(':')
    {
        return Err(format!("Unsafe MOD id: {mod_id}"));
    }

    Ok(())
}

fn validate_mod_display_name(display_name: &str) -> Result<String, String> {
    let display_name = display_name.trim();
    if display_name.chars().count() > 120 {
        return Err("MOD display name must contain at most 120 characters.".to_string());
    }
    Ok(display_name.to_string())
}

/// 导入名称会成为 manifest 的原始名称，空值不能像显示名一样回退为空字符串。
fn validate_import_mod_name(name: &str) -> Result<String, String> {
    let name = validate_mod_display_name(name)?;
    if name.is_empty() {
        return Err("MOD 导入名称不能为空。".to_string());
    }
    Ok(name)
}

fn validate_mod_note(note: &str) -> Result<String, String> {
    let note = note.trim();
    if note.chars().count() > 800 {
        return Err("MOD note must contain at most 800 characters.".to_string());
    }
    Ok(note.to_string())
}

fn validate_mod_branch_group_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分支组名称不能为空。".to_string());
    }
    if name.chars().count() > MOD_BRANCH_GROUP_NAME_LIMIT {
        return Err(format!(
            "分支组名称不能超过 {MOD_BRANCH_GROUP_NAME_LIMIT} 个字符。"
        ));
    }
    Ok(name.to_string())
}

fn validate_mod_category_id(category_id: &str) -> Result<(), String> {
    if category_id.is_empty()
        || category_id == "."
        || category_id == ".."
        || category_id.contains('/')
        || category_id.contains('\\')
        || category_id.contains(':')
    {
        return Err(format!("分类 ID 不安全：{category_id}"));
    }

    Ok(())
}

fn validate_mod_category_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分类名称不能为空。".to_string());
    }
    if name.chars().count() > MOD_CATEGORY_NAME_LIMIT {
        return Err(format!(
            "分类名称不能超过 {MOD_CATEGORY_NAME_LIMIT} 个字符。"
        ));
    }

    Ok(name.to_string())
}

fn deployment_key(path: &Path) -> String {
    path_to_string(path).to_lowercase()
}

fn validate_archive_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("Choose a MOD archive before importing.".to_string());
    }

    if !path.exists() {
        return Err(format!("Archive does not exist: {}", path.display()));
    }

    if !path.is_file() {
        return Err(format!("Archive path is not a file: {}", path.display()));
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .ok_or_else(|| format!("Archive has no extension: {}", path.display()))?;

    if !matches!(extension.as_str(), "zip" | "7z" | "rar") {
        return Err(format!(
            "Unsupported archive extension .{extension}. Supported: .zip, .7z, .rar."
        ));
    }

    path.canonicalize()
        .map_err(|error| format!("Could not resolve archive path {}: {error}", path.display()))
}

pub(crate) fn extract_archive_with_bundled_7zip(
    app: &tauri::AppHandle,
    archive_path: &Path,
    destination: &Path,
    progress: &OperationReporter,
) -> Result<(), String> {
    let seven_zip = bundled_7zip_executable(app).ok_or_else(|| {
        "Bundled 7-Zip unpacker is missing. Expected resources/unpackers/7zip/7z.exe and 7z.dll in the Acumod application resources.".to_string()
    })?;
    progress.report("正在解包压缩包", 0, Some(100), None);
    let mut child = Command::new(&seven_zip)
        .arg("x")
        .arg("-y")
        .arg("-bsp1")
        .arg("-bb1")
        .arg(format!("-o{}", destination.display()))
        .arg(archive_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "Could not run bundled unpacker {}: {error}",
                seven_zip.display()
            )
        })?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取内置解包器输出。".to_string())?;
    let stderr = child.stderr.take();
    let stderr_reader = thread::spawn(move || {
        let mut output = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut output);
        }
        output
    });
    let mut stdout_buffer = [0_u8; 1024];
    let mut stdout_text = String::new();

    loop {
        let read_count = stdout
            .read(&mut stdout_buffer)
            .map_err(|error| format!("无法读取内置解包器进度：{error}"))?;
        if read_count == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&stdout_buffer[..read_count]);
        stdout_text.push_str(&chunk);
        if let Some(percent) = latest_7zip_progress_percent(&stdout_text) {
            progress.report("正在解包压缩包", percent, Some(100), None);
        }
        if stdout_text.len() > 8_192 {
            let retained = stdout_text.split_off(stdout_text.len() - 1_024);
            stdout_text = retained;
        }
    }

    let status = child
        .wait()
        .map_err(|error| format!("无法等待内置解包器结束：{error}"))?;
    let stderr = stderr_reader.join().unwrap_or_default();

    if status.success() {
        progress.report("正在解包压缩包", 100, Some(100), None);
        return Ok(());
    }

    Err(format!(
        "Bundled 7-Zip could not extract archive {}.\nstdout: {}\nstderr: {}",
        archive_path.display(),
        stdout_text.trim(),
        stderr.trim()
    ))
}

fn latest_7zip_progress_percent(output: &str) -> Option<usize> {
    let percent_index = output.rfind('%')?;
    let digits = output[..percent_index]
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }

    digits
        .chars()
        .rev()
        .collect::<String>()
        .parse::<usize>()
        .ok()
        .filter(|percent| *percent <= 100)
}

fn bundled_7zip_executable(app: &tauri::AppHandle) -> Option<PathBuf> {
    bundled_resource_candidates(app)
        .into_iter()
        .map(|base| base.join("unpackers").join("7zip").join("7z.exe"))
        .find(|path| path.is_file())
}

fn bundled_resource_candidates(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir);
    }

    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("resources"));
        candidates.push(current_dir.join("src-tauri").join("resources"));
    }

    if let Ok(executable_path) = env::current_exe() {
        if let Some(executable_dir) = executable_path.parent() {
            candidates.push(executable_dir.join("resources"));
        }
    }

    candidates
}

fn deploy_root_from_preview(
    preview: &ModImportPreview,
    source_root: &Path,
) -> Result<DeployRoot, String> {
    match preview.deploy_root.as_str() {
        "gameRoot" => Ok(DeployRoot::GameRoot),
        "nativePC" if preview.detection_method == "selectedNativePcChildDirectory" => {
            let child_name = common_native_pc_child_name(source_root).ok_or_else(|| {
                format!(
                    "Could not resolve selected nativePC child directory from {}.",
                    source_root.display()
                )
            })?;

            Ok(DeployRoot::NativePcChild(child_name))
        }
        "nativePC" => Ok(DeployRoot::NativePc),
        other => Err(format!("Unknown deploy root from import preview: {other}")),
    }
}

fn relative_string_to_path(path: &str) -> Result<PathBuf, String> {
    let mut output = PathBuf::new();

    for part in path.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("Unsafe relative path: {path}"));
        }

        output.push(part);
    }

    if output.as_os_str().is_empty() {
        return Err("Empty relative path.".to_string());
    }

    Ok(output)
}

fn safe_relative_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Unsafe relative path: {}", path.display()));
            }
        }
    }

    if parts.is_empty() {
        return Err("Empty relative path.".to_string());
    }

    Ok(parts.join("/"))
}

fn file_name_equals(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

fn depth_from(root: &Path, path: &Path) -> usize {
    path.strip_prefix(root)
        .map(|relative| relative.components().count())
        .unwrap_or(usize::MAX)
}

fn deploy_root_label(deploy_root: &DeployRoot) -> &'static str {
    match deploy_root {
        DeployRoot::NativePc | DeployRoot::NativePcChild(_) => "nativePC",
        DeployRoot::GameRoot => "gameRoot",
    }
}

fn normalize_user_path(path: &str) -> PathBuf {
    PathBuf::from(path.trim().trim_matches('"'))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("{label} is not a directory: {}", path.display()));
    }

    path.canonicalize()
        .map_err(|error| format!("Could not resolve {label} {}: {error}", path.display()))
}

fn derive_mod_name(source_path: &Path) -> String {
    let name = if source_path.is_file() {
        source_path.file_stem()
    } else {
        source_path.file_name()
    };

    name.and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Imported MOD")
        .to_string()
}

fn unique_mod_id(name: &str) -> Result<String, String> {
    let slug = slugify(name);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("System clock is before UNIX_EPOCH: {error}"))?
        .as_nanos();

    Ok(format!("{slug}-{stamp}"))
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let slug = slug.trim_matches('-');

    if slug.is_empty() {
        "mod".to_string()
    } else {
        slug.to_string()
    }
}

fn unix_seconds_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("System clock is before UNIX_EPOCH: {error}"))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct LibraryPaths {
    software_data_path: PathBuf,
    mods_path: PathBuf,
    installed_path: PathBuf,
    categories_path: PathBuf,
    workspace_snapshot_path: PathBuf,
    staging_path: PathBuf,
    import_staging_path: PathBuf,
}

fn library_paths(_app: &tauri::AppHandle) -> Result<LibraryPaths, String> {
    let software_data_path = software_data_path()?;
    let mods_path = software_data_path.join("mods");
    let installed_path = mods_path.join("installed");
    let categories_path = mods_path.join("categories.json");
    let workspace_snapshot_path = mods_path.join("workspace-snapshot.json");
    let staging_path = mods_path.join("staging");
    let import_staging_path = staging_path.join("imports");

    Ok(LibraryPaths {
        software_data_path,
        mods_path,
        installed_path,
        categories_path,
        workspace_snapshot_path,
        staging_path,
        import_staging_path,
    })
}

fn ensure_library_directories(paths: &LibraryPaths) -> Result<(), String> {
    for path in [
        &paths.mods_path,
        &paths.installed_path,
        &paths.staging_path,
        &paths.import_staging_path,
    ] {
        fs::create_dir_all(path)
            .map_err(|error| format!("Could not create directory {}: {error}", path.display()))?;
    }

    Ok(())
}

fn load_or_initialize_mod_category_store(paths: &LibraryPaths) -> Result<ModCategoryStore, String> {
    load_or_initialize_mod_category_store_from(&paths.installed_path, &paths.categories_path)
}

fn load_or_initialize_mod_category_store_for_installed_root(
    installed_root: &Path,
) -> Result<ModCategoryStore, String> {
    let Some(categories_path) = mod_category_store_path(installed_root) else {
        return Ok(ModCategoryStore::default());
    };

    load_or_initialize_mod_category_store_from(installed_root, &categories_path)
}

fn mod_category_store_path(installed_root: &Path) -> Option<PathBuf> {
    let is_standard_installed_root = installed_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("installed"))
        .unwrap_or(false);

    if is_standard_installed_root {
        installed_root
            .parent()
            .map(|mods_path| mods_path.join("categories.json"))
    } else {
        Some(installed_root.join(".categories.json"))
    }
}

fn load_or_initialize_mod_category_store_from(
    installed_root: &Path,
    categories_path: &Path,
) -> Result<ModCategoryStore, String> {
    let store_exists = categories_path.is_file();
    let mut store = load_mod_category_store(categories_path)?;
    let mut changed = !store_exists;
    let requires_hierarchy_migration = store.schema_version < MOD_CATEGORY_STORE_SCHEMA_VERSION;

    if store.schema_version != MOD_CATEGORY_STORE_SCHEMA_VERSION {
        store.schema_version = MOD_CATEGORY_STORE_SCHEMA_VERSION;
        changed = true;
    }

    let has_current_workspace_snapshot = workspace_snapshot_path_for_installed_root(installed_root)
        .ok()
        .and_then(|path| read_stored_workspace_snapshot(&path).ok().flatten())
        .is_some();
    // 有效快照已经证明全部 manifest 完成当前版本迁移；日常分类读取不应再次扫描整库。
    if requires_hierarchy_migration || !has_current_workspace_snapshot {
        changed |= migrate_installed_mod_categories(
            installed_root,
            &mut store,
            requires_hierarchy_migration,
        )?;
    }

    if changed {
        save_mod_category_store(categories_path, &store)?;
    }

    Ok(store)
}

fn load_mod_category_store(categories_path: &Path) -> Result<ModCategoryStore, String> {
    if !categories_path.exists() {
        return Ok(ModCategoryStore::default());
    }

    if !categories_path.is_file() {
        return Err(format!(
            "分类数据路径不是文件：{}",
            categories_path.display()
        ));
    }

    let category_json = fs::read_to_string(categories_path)
        .map_err(|error| format!("无法读取分类数据 {}：{error}", categories_path.display()))?;
    serde_json::from_str::<ModCategoryStore>(&category_json)
        .map_err(|error| format!("无法解析分类数据 {}：{error}", categories_path.display()))
}

fn save_mod_category_store(categories_path: &Path, store: &ModCategoryStore) -> Result<(), String> {
    if let Some(parent) = categories_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建分类数据目录 {}：{error}", parent.display()))?;
    }
    let category_json = serde_json::to_string_pretty(store)
        .map_err(|error| format!("无法序列化分类数据：{error}"))?;
    fs::write(categories_path, category_json)
        .map_err(|error| format!("无法写入分类数据 {}：{error}", categories_path.display()))
}

fn migrate_installed_mod_categories(
    installed_root: &Path,
    store: &mut ModCategoryStore,
    requires_hierarchy_migration: bool,
) -> Result<bool, String> {
    let mut store_changed = false;

    for mut context in load_all_installed_manifests(installed_root)? {
        if !requires_hierarchy_migration
            && context.manifest.schema_version >= CURRENT_MOD_MANIFEST_SCHEMA_VERSION
            && context.manifest.category_override.is_none()
        {
            continue;
        }

        if context.manifest.schema_version < CURRENT_MODEL_RECOGNITION_SCHEMA_VERSION {
            refresh_manifest_model_replacements(&mut context)?;
        }

        let (recognized_category_ids, categories_changed) =
            ensure_recognition_categories(store, &context.manifest.model_replacements)?;
        store_changed |= categories_changed;

        let mut category_ids = context.manifest.category_ids.clone();
        category_ids.extend(recognized_category_ids);
        if let Some(category_override) = context.manifest.category_override.as_deref() {
            if store
                .categories
                .iter()
                .any(|category| category.id == category_override)
            {
                category_ids.push(category_override.to_string());
            }
        }
        category_ids.retain(|category_id| {
            store
                .categories
                .iter()
                .any(|category| category.id == *category_id)
        });
        category_ids = resolve_category_ids(store, &category_ids)?;

        context.manifest.category_ids = category_ids;
        context.manifest.category_override = None;
        context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
        save_manifest(&context.manifest_path, &context.manifest)?;
    }

    Ok(store_changed)
}

#[derive(Clone)]
struct RecognitionCategorySpec {
    recognition_key: String,
    name: String,
    parent_recognition_key: Option<String>,
    assign_to_mod: bool,
}

fn ensure_recognition_categories(
    store: &mut ModCategoryStore,
    model_replacements: &[ModelReplacement],
) -> Result<(Vec<String>, bool), String> {
    let mut specs = BTreeMap::<String, RecognitionCategorySpec>::new();

    for replacement in model_replacements {
        let model_kind = replacement.model_kind.trim();
        if model_kind_label(model_kind) == "未识别" {
            continue;
        }

        if model_kind == "weapon" {
            let parent =
                specs
                    .entry("weapon".to_string())
                    .or_insert_with(|| RecognitionCategorySpec {
                        recognition_key: "weapon".to_string(),
                        name: "武器".to_string(),
                        parent_recognition_key: None,
                        assign_to_mod: false,
                    });
            let weapon_type = replacement.sub_kind.trim();
            if weapon_type.is_empty() {
                parent.assign_to_mod = true;
                continue;
            }

            let recognition_key = format!("weapon:{weapon_type}");
            specs
                .entry(recognition_key.clone())
                .or_insert_with(|| RecognitionCategorySpec {
                    recognition_key,
                    name: weapon_type.to_string(),
                    parent_recognition_key: Some("weapon".to_string()),
                    assign_to_mod: true,
                });
            continue;
        }

        if model_kind == "weaponVoice" {
            let parent =
                specs
                    .entry("weaponVoice".to_string())
                    .or_insert_with(|| RecognitionCategorySpec {
                        recognition_key: "weaponVoice".to_string(),
                        name: "武器语音".to_string(),
                        parent_recognition_key: None,
                        assign_to_mod: false,
                    });
            let weapon_type = replacement.sub_kind.trim();
            if weapon_type.is_empty() {
                parent.assign_to_mod = true;
                continue;
            }

            let recognition_key = format!("weaponVoice:{weapon_type}");
            specs
                .entry(recognition_key.clone())
                .or_insert_with(|| RecognitionCategorySpec {
                    recognition_key,
                    name: weapon_type.to_string(),
                    parent_recognition_key: Some("weaponVoice".to_string()),
                    assign_to_mod: true,
                });
            continue;
        }

        specs
            .entry(model_kind.to_string())
            .or_insert_with(|| RecognitionCategorySpec {
                recognition_key: model_kind.to_string(),
                name: model_kind_label(model_kind).to_string(),
                parent_recognition_key: None,
                assign_to_mod: true,
            });
    }

    let mut category_ids = Vec::new();
    let mut changed = false;
    let mut category_ids_by_recognition_key = HashMap::new();

    for spec in specs.into_values() {
        if store
            .suppressed_recognition_keys
            .contains(&spec.recognition_key)
        {
            continue;
        }

        let parent_id = spec
            .parent_recognition_key
            .as_deref()
            .and_then(|key| category_ids_by_recognition_key.get(key))
            .cloned();
        let category_index = store
            .categories
            .iter()
            .position(|category| category.recognition_keys.contains(&spec.recognition_key))
            .or_else(|| {
                store.categories.iter().position(|category| {
                    category.name == spec.name && category.parent_id == parent_id
                })
            });

        let category_id = if let Some(category_index) = category_index {
            let category = &mut store.categories[category_index];
            if !category.recognition_keys.contains(&spec.recognition_key) {
                category.recognition_keys.push(spec.recognition_key.clone());
                category.recognition_keys.sort();
                changed = true;
            }
            if category.parent_id != parent_id {
                category.parent_id = parent_id.clone();
                changed = true;
            }
            category.id.clone()
        } else {
            let base_id = format!("category-recognition-{}", slugify(&spec.recognition_key));
            let category_id = unique_mod_category_id_from_base(&store.categories, &base_id);
            store.categories.push(StoredModCategory {
                id: category_id.clone(),
                name: spec.name,
                parent_id,
                created_at_unix_seconds: unix_seconds_now()?,
                recognition_keys: vec![spec.recognition_key.clone()],
            });
            changed = true;
            category_id
        };

        category_ids_by_recognition_key.insert(spec.recognition_key, category_id.clone());
        if spec.assign_to_mod {
            category_ids.push(category_id);
        }
    }

    Ok((resolve_category_ids(store, &category_ids)?, changed))
}

fn sorted_mod_categories(categories: &[StoredModCategory]) -> Vec<ModCategory> {
    let mut categories_by_parent = HashMap::<Option<&str>, Vec<&StoredModCategory>>::new();
    let category_ids = categories
        .iter()
        .map(|category| category.id.as_str())
        .collect::<HashSet<_>>();

    for category in categories {
        let parent_id = category
            .parent_id
            .as_deref()
            .filter(|parent_id| category_ids.contains(parent_id));
        categories_by_parent
            .entry(parent_id)
            .or_default()
            .push(category);
    }

    for entries in categories_by_parent.values_mut() {
        entries.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    let mut output = Vec::new();
    for parent in categories_by_parent.get(&None).into_iter().flatten() {
        output.push(ModCategory::from(*parent));
        if let Some(children) = categories_by_parent.get(&Some(parent.id.as_str())) {
            output.extend(children.iter().map(|child| ModCategory::from(*child)));
        }
    }
    output
}

fn resolve_mod_categories(store: &ModCategoryStore, category_ids: &[String]) -> Vec<ModCategory> {
    let category_ids = category_ids.iter().collect::<HashSet<_>>();
    let selected = store
        .categories
        .iter()
        .filter(|category| category_ids.contains(&category.id))
        .cloned()
        .collect::<Vec<_>>();
    sorted_mod_categories(&selected)
}

fn resolve_category_ids(
    store: &ModCategoryStore,
    raw_category_ids: &[String],
) -> Result<Vec<String>, String> {
    let mut category_ids = Vec::new();

    for raw_category_id in raw_category_ids {
        let category_id = raw_category_id.trim();
        validate_mod_category_id(category_id)?;
        if !store
            .categories
            .iter()
            .any(|category| category.id == category_id)
        {
            return Err(format!("未找到分类：{category_id}"));
        }
        if !category_ids.iter().any(|existing| existing == category_id) {
            category_ids.push(category_id.to_string());
        }
    }

    let selected_parent_ids = category_ids
        .iter()
        .filter_map(|category_id| {
            store
                .categories
                .iter()
                .find(|category| category.id == *category_id)
                .and_then(|category| category.parent_id.clone())
        })
        .collect::<HashSet<_>>();
    category_ids.retain(|category_id| !selected_parent_ids.contains(category_id));

    Ok(category_ids)
}

fn ensure_mod_category_name_is_available(
    categories: &[StoredModCategory],
    name: &str,
    parent_id: Option<&str>,
    excluded_category_id: Option<&str>,
) -> Result<(), String> {
    let normalized_name = name.to_lowercase();
    if categories.iter().any(|category| {
        Some(category.id.as_str()) != excluded_category_id
            && category.parent_id.as_deref() == parent_id
            && category.name.trim().to_lowercase() == normalized_name
    }) {
        return Err(format!("分类已存在：{name}"));
    }

    Ok(())
}

fn resolve_category_parent_id(
    store: &ModCategoryStore,
    raw_parent_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(parent_id) = raw_parent_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    validate_mod_category_id(parent_id)?;
    let parent = store
        .categories
        .iter()
        .find(|category| category.id == parent_id)
        .ok_or_else(|| format!("未找到父分类：{parent_id}"))?;
    if parent.parent_id.is_some() {
        return Err("子分类只能创建在顶级分类下。".to_string());
    }
    Ok(Some(parent.id.clone()))
}

fn unique_mod_category_id(categories: &[StoredModCategory], name: &str) -> Result<String, String> {
    let base_id = format!("category-{}-{}", unix_seconds_now()?, slugify(name));
    Ok(unique_mod_category_id_from_base(categories, &base_id))
}

fn unique_mod_category_id_from_base(categories: &[StoredModCategory], base_id: &str) -> String {
    let mut category_id = base_id.to_string();
    let mut suffix = 2;

    while categories.iter().any(|category| category.id == category_id) {
        category_id = format!("{base_id}-{suffix}");
        suffix += 1;
    }

    category_id
}

fn remove_category_from_manifests(
    installed_root: &Path,
    category_id: &str,
) -> Result<usize, String> {
    let mut affected_mod_count = 0;
    let snapshot_path = workspace_snapshot_path_for_installed_root(installed_root)?;
    let contexts = if let Some(stored) = read_stored_workspace_snapshot(&snapshot_path)? {
        let affected_ids = stored
            .snapshot
            .installed_mods
            .mods
            .iter()
            .filter(|installed| installed.category_ids.iter().any(|id| id == category_id))
            .map(|installed| installed.id.clone())
            .collect::<Vec<_>>();
        affected_ids
            .iter()
            .map(|mod_id| load_installed_manifest(installed_root, mod_id))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        load_all_installed_manifests(installed_root)?
    };

    for mut context in contexts {
        let original_count = context.manifest.category_ids.len();
        context
            .manifest
            .category_ids
            .retain(|existing_id| existing_id != category_id);
        let legacy_override_removed =
            context.manifest.category_override.as_deref() == Some(category_id);

        if original_count == context.manifest.category_ids.len() && !legacy_override_removed {
            continue;
        }

        context.manifest.category_override = None;
        context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
        save_manifest(&context.manifest_path, &context.manifest)?;
        affected_mod_count += 1;
    }

    Ok(affected_mod_count)
}

fn initialize_import_staging(paths: &LibraryPaths) -> Result<(), String> {
    static INITIALIZATION: OnceLock<Result<(), String>> = OnceLock::new();

    match INITIALIZATION.get_or_init(|| clear_import_staging(&paths.import_staging_path)) {
        Ok(()) => Ok(()),
        Err(error) => Err(error.clone()),
    }
}

fn clear_import_staging(import_staging_path: &Path) -> Result<(), String> {
    fs::create_dir_all(import_staging_path).map_err(|error| {
        format!(
            "Could not create archive staging root {}: {error}",
            import_staging_path.display()
        )
    })?;

    for entry in fs::read_dir(import_staging_path).map_err(|error| {
        format!(
            "Could not read archive staging root {}: {error}",
            import_staging_path.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Could not read an entry under archive staging root {}: {error}",
                import_staging_path.display()
            )
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "Could not inspect staging entry {}: {error}",
                path.display()
            )
        })?;

        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(&path).map_err(|error| {
                format!(
                    "Could not remove archive staging directory {}: {error}",
                    path.display()
                )
            })?;
        } else {
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "Could not remove archive staging file {}: {error}",
                    path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn software_data_path() -> Result<PathBuf, String> {
    let executable_path = env::current_exe()
        .map_err(|error| format!("Could not resolve executable path: {error}"))?;
    let executable_dir = executable_path.parent().ok_or_else(|| {
        format!(
            "Could not resolve executable directory from {}.",
            executable_path.display()
        )
    })?;

    Ok(executable_dir.join("AcumodData"))
}

fn remove_mod_from_conflict_orders(installed_root: &Path, mod_id: &str) -> Result<(), String> {
    let mut store = read_conflict_order_store(installed_root)?;
    let mut changed = false;

    for order in store.orders.values_mut() {
        let original_count = order.len();
        order.retain(|id| id != mod_id);
        changed |= original_count != order.len();
    }

    let original_group_count = store.orders.len();
    store.orders.retain(|_, order| !order.is_empty());
    changed |= original_group_count != store.orders.len();

    if changed {
        save_conflict_order_store(installed_root, &store)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        env, fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        apply_conflict_order_from, apply_mod_remap_from, armor_set_label,
        build_mod_conflict_report_from_workspace_index, build_workspace_mod_index,
        clear_import_staging, collect_nested_archives, copy_import_source_directory,
        create_mod_branch_group_from, disable_mod_from, effective_installed_files_for_context,
        enable_mod_from, find_installed_mod_by_content_with_options, get_mod_conflict_report_from,
        install_mod_from_candidate_into, install_mod_from_folder_into,
        install_mod_from_folder_into_with_options_and_progress, installed_mod_content_path,
        installed_mod_list_from_contexts, list_installed_mods_from, load_all_installed_manifests,
        load_installed_manifest, load_normalized_mod_branch_groups,
        load_or_initialize_mod_category_store_for_installed_root,
        mod_cleanup_candidate_folder_from, move_conflict_participant_from,
        move_mod_library_item_from, move_mod_library_items_from, preview_disable_mod_from,
        preview_enable_mod_from, preview_mod_import, preview_mod_remap_from,
        preview_restore_all_mods_from, preview_uninstall_mod_from, read_conflict_order_store,
        read_mod_library_order_store, remove_category_from_manifests,
        remove_mod_from_conflict_orders, replace_mod_library_order_from, restore_all_mods_from,
        restore_mod_library_import_order_from, save_manifest, save_mod_category_store,
        scan_mod_cleanup_candidates_from, source_path_for_installed_file, uninstall_mod_from,
        validate_archive_path, ModCategoryStore, ModDeploymentExclusion, ModLibraryOrderStore,
        StoredModCategory, MOD_CATEGORY_STORE_SCHEMA_VERSION,
    };
    use crate::operations::OperationReporter;

    #[test]
    fn summarizes_complete_armor_target_as_set_name() {
        let display_names = vec![
            "【冰狼】服装·头部".to_string(),
            "【冰狼】服装·身体".to_string(),
            "【冰狼】服装·腕部".to_string(),
            "【冰狼】服装·腰部".to_string(),
            "【冰狼】服装·脚部".to_string(),
        ];

        assert_eq!(
            armor_set_label(&display_names, "pl105_0000"),
            "【冰狼】服装"
        );
    }

    #[test]
    fn persists_branch_groups_and_removes_single_member_remainders() {
        let root = temp_root("branch_groups");
        let installed_root = root.join("installed");
        let first_source = root.join("first");
        let second_source = root.join("second");
        write_file(&first_source.join("nativePC").join("wp").join("first.mod3"));
        write_file(
            &second_source
                .join("nativePC")
                .join("wp")
                .join("second.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();

        let (group, _) = create_mod_branch_group_from(
            &installed_root,
            "外观版本".to_string(),
            vec![first.mod_id.clone(), second.mod_id.clone()],
        )
        .unwrap();
        assert_eq!(group.mod_ids.len(), 2);

        fs::remove_dir_all(installed_root.join(&second.mod_id)).unwrap();
        let installed_ids = HashSet::from([first.mod_id]);
        let groups = load_normalized_mod_branch_groups(&installed_root, &installed_ids).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn finds_supported_nested_archives_without_rescanning_nested_staging() {
        let root = temp_root("nested_archives");
        write_file(&root.join("versions").join("red.zip"));
        write_file(&root.join("versions").join("blue.7z"));
        write_file(&root.join("versions").join("readme.txt"));
        write_file(&root.join(".acumod-nested").join("ignored.rar"));

        let archives = collect_nested_archives(&root).unwrap();
        let names = archives
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["blue.7z", "red.zip"]);
    }

    #[test]
    fn copies_folder_with_nested_archive_into_isolated_staging() {
        let root = temp_root("folder_nested_staging");
        let source = root.join("source");
        let staging = root.join("staging");
        write_file(&source.join("nativePC").join("wp").join("base.mod3"));
        write_file(&source.join("versions").join("alternate.zip"));

        copy_import_source_directory(&source, &staging, &OperationReporter::default()).unwrap();

        assert!(source.join("versions").join("alternate.zip").is_file());
        assert!(staging
            .join("nativePC")
            .join("wp")
            .join("base.mod3")
            .is_file());
        assert!(staging.join("versions").join("alternate.zip").is_file());
    }

    #[test]
    fn previews_native_pc_directory() {
        let root = temp_root("native_pc");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));

        let preview = preview_mod_import(root_to_string(&root), false).unwrap();

        assert_eq!(preview.status, "ready");
        assert_eq!(preview.detection_method, "nativePcDirectory");
        assert_eq!(
            preview.files[0].deploy_relative_path,
            "nativePC/weapon/sword.mod3"
        );

        cleanup(root);
    }

    #[test]
    fn prefixes_known_native_pc_child_directory() {
        let root = temp_root("known_child");
        write_file(&root.join("weapon").join("sword.mod3"));

        let preview = preview_mod_import(root_to_string(&root), false).unwrap();

        assert_eq!(preview.status, "ready");
        assert_eq!(preview.detection_method, "nativePcChildDirectory");
        assert_eq!(
            preview.files[0].deploy_relative_path,
            "nativePC/weapon/sword.mod3"
        );

        cleanup(root);
    }

    #[test]
    fn prefixes_selected_native_pc_child_directory() {
        let root = temp_root("selected_child");
        let plugins = root.join("nativePC").join("plugins");
        write_file(&plugins.join("loader.dll"));

        let preview = preview_mod_import(root_to_string(&plugins), false).unwrap();

        assert_eq!(preview.status, "ready");
        assert_eq!(preview.detection_method, "selectedNativePcChildDirectory");
        assert_eq!(
            preview.files[0].deploy_relative_path,
            "nativePC/plugins/loader.dll"
        );

        cleanup(root);
    }

    #[test]
    fn asks_before_game_root_fallback() {
        let root = temp_root("game_root_prompt");
        write_file(&root.join("dinput8.dll"));

        let preview = preview_mod_import(root_to_string(&root), false).unwrap();

        assert_eq!(preview.status, "needsGameRootConfirmation");
        assert!(preview.requires_game_root_confirmation);
        assert_eq!(preview.files[0].deploy_relative_path, "dinput8.dll");

        cleanup(root);
    }

    #[test]
    fn uses_game_root_after_confirmation() {
        let root = temp_root("game_root_confirmed");
        write_file(&root.join("dinput8.dll"));

        let preview = preview_mod_import(root_to_string(&root), true).unwrap();

        assert_eq!(preview.status, "ready");
        assert_eq!(preview.detection_method, "userConfirmedGameRoot");
        assert_eq!(preview.files[0].deploy_relative_path, "dinput8.dll");

        cleanup(root);
    }

    #[test]
    fn content_matching_accepts_game_root_files_when_the_caller_allows_it() {
        let root = temp_root("game_root_content_matching");
        write_file(&root.join("lua_framework").join("scripts.lua"));
        write_file(&root.join("hid.dll"));

        let blocked = find_installed_mod_by_content_with_options(
            &[],
            &root,
            false,
            &OperationReporter::default(),
        );
        assert!(blocked.is_err());

        let matched = find_installed_mod_by_content_with_options(
            &[],
            &root,
            true,
            &OperationReporter::default(),
        )
        .unwrap();
        assert!(matched.is_none());

        cleanup(root);
    }

    #[test]
    fn reports_ambiguous_same_depth_candidates() {
        let root = temp_root("ambiguous");
        write_file(
            &root
                .join("OptionA")
                .join("nativePC")
                .join("weapon")
                .join("a.mod3"),
        );
        write_file(
            &root
                .join("OptionB")
                .join("nativePC")
                .join("weapon")
                .join("b.mod3"),
        );

        let preview = preview_mod_import(root_to_string(&root), false).unwrap();

        assert_eq!(preview.status, "ambiguous");
        assert_eq!(preview.candidates.len(), 2);

        cleanup(root);
    }

    #[test]
    fn installs_only_the_selected_ambiguous_candidate() {
        let root = temp_root("candidate_source");
        let installed_root = temp_root("candidate_target");
        let option_a = root.join("OptionA").join("nativePC");
        let option_b = root.join("OptionB").join("nativePC");
        write_file(&option_a.join("weapon").join("a.mod3"));
        write_file(&option_b.join("weapon").join("b.mod3"));

        let result = install_mod_from_candidate_into(
            root_to_string(&root),
            root_to_string(&option_b),
            None,
            &installed_root,
        )
        .unwrap();
        let content_path = PathBuf::from(result.content_path);

        assert!(content_path
            .join("nativePC")
            .join("weapon")
            .join("b.mod3")
            .is_file());
        assert!(!content_path
            .join("nativePC")
            .join("weapon")
            .join("a.mod3")
            .exists());

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn installs_folder_mod_into_library() {
        let root = temp_root("install_source");
        let installed_root = temp_root("install_target");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();

        assert_eq!(result.file_count, 1);
        assert!(PathBuf::from(&result.manifest_path).is_file());
        assert!(PathBuf::from(&result.content_path)
            .join("nativePC")
            .join("weapon")
            .join("sword.mod3")
            .is_file());

        let manifest = fs::read_to_string(&result.manifest_path).unwrap();
        assert!(manifest.contains("\"enabled\": false"));
        assert!(manifest.contains("nativePC/weapon/sword.mod3"));

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn finds_same_content_from_preloaded_manifest_index() {
        let source = temp_root("content_index_source");
        let duplicate = temp_root("content_index_duplicate");
        let installed_root = temp_root("content_index_target");
        write_file(&source.join("nativePC/weapon/sword.mod3"));
        write_file(&duplicate.join("nativePC/weapon/sword.mod3"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source), false, &installed_root).unwrap();
        let contexts = load_all_installed_manifests(&installed_root).unwrap();
        let matched = find_installed_mod_by_content_with_options(
            &contexts,
            &duplicate,
            false,
            &OperationReporter::default(),
        )
        .unwrap()
        .expect("相同内容应从预加载索引中找到");

        assert_eq!(matched.mod_id, installed.mod_id);
        assert!(matched.already_installed);

        cleanup(source);
        cleanup(duplicate);
        cleanup(installed_root);
    }

    #[test]
    fn resolves_only_the_installed_content_folder_for_opening() {
        let root = temp_root("open_folder_source");
        let installed_root = temp_root("open_folder_target");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let content_path = installed_mod_content_path(&installed_root, &result.mod_id).unwrap();

        assert_eq!(
            content_path.canonicalize().unwrap(),
            PathBuf::from(&result.content_path).canonicalize().unwrap()
        );
        assert!(content_path.join("nativePC/weapon/sword.mod3").is_file());

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn cleanup_exclusion_keeps_library_source_and_removes_file_from_effective_deployment() {
        let source = temp_root("cleanup_exclusion_source");
        let installed_root = temp_root("cleanup_exclusion_target");
        write_file(&source.join("nativePC/wp/model.mod3"));
        write_file(&source.join("nativePC/wp/preview.png"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source), false, &installed_root).unwrap();
        let mut context = load_installed_manifest(&installed_root, &installed.mod_id).unwrap();
        let scan =
            scan_mod_cleanup_candidates_from(&[context.clone()], &OperationReporter::default())
                .unwrap();
        assert_eq!(scan.scanned_file_count, 2);
        assert_eq!(scan.local_keep_count, 1);
        assert_eq!(scan.local_remove_count, 1);
        assert_eq!(scan.ai_review_count, 0);
        assert_eq!(scan.candidate_count, 1);
        let candidate = scan.candidates.into_iter().next().unwrap();
        assert_eq!(candidate.review_source, "localRule");
        let installed_file = context
            .manifest
            .files
            .iter()
            .find(|file| file.library_relative_path == candidate.library_relative_path)
            .unwrap();
        let library_source = source_path_for_installed_file(&context, installed_file).unwrap();
        assert!(library_source.is_file());
        assert_eq!(
            mod_cleanup_candidate_folder_from(
                &installed_root,
                &installed.mod_id,
                &candidate.candidate_id,
            )
            .unwrap()
            .canonicalize()
            .unwrap(),
            library_source.parent().unwrap().canonicalize().unwrap()
        );
        assert_eq!(
            effective_installed_files_for_context(&context)
                .unwrap()
                .len(),
            2
        );

        context
            .manifest
            .deployment_exclusions
            .push(ModDeploymentExclusion {
                candidate_id: candidate.candidate_id,
                library_relative_path: candidate.library_relative_path,
                deploy_relative_path: candidate.deploy_relative_path,
                reason: "预览图".to_string(),
                batch_id: "test-batch".to_string(),
                excluded_at_unix_seconds: 1,
            });
        save_manifest(&context.manifest_path, &context.manifest).unwrap();

        let context = load_installed_manifest(&installed_root, &installed.mod_id).unwrap();
        let effective_files = effective_installed_files_for_context(&context).unwrap();
        assert_eq!(effective_files.len(), 1);
        assert!(effective_files[0]
            .deploy_relative_path
            .ends_with("model.mod3"));
        assert!(library_source.is_file());
        assert_eq!(
            scan_mod_cleanup_candidates_from(&[context], &OperationReporter::default())
                .unwrap()
                .candidate_count,
            0
        );

        cleanup(source);
        cleanup(installed_root);
    }

    #[test]
    fn cleanup_rules_only_send_ambiguous_or_conflicting_evidence_to_acuai() {
        let source = temp_root("cleanup_rule_source");
        let installed_root = temp_root("cleanup_rule_target");
        write_file(&source.join("nativePC/wp/model.mod3"));
        write_file(&source.join("nativePC/wp/source.dds"));
        write_file(&source.join("nativePC/wp/model.mrl3.bak"));
        write_file(&source.join("nativePC/plugins/readme.txt"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source), false, &installed_root).unwrap();
        let context = load_installed_manifest(&installed_root, &installed.mod_id).unwrap();
        let scan =
            scan_mod_cleanup_candidates_from(&[context], &OperationReporter::default()).unwrap();

        assert_eq!(scan.scanned_file_count, 4);
        assert_eq!(scan.local_keep_count, 1);
        assert_eq!(scan.local_remove_count, 1);
        assert_eq!(scan.ai_review_count, 2);
        assert_eq!(scan.candidate_count, 3);
        assert!(scan.candidates.iter().any(|candidate| {
            candidate.library_relative_path.ends_with("model.mrl3.bak")
                && candidate.review_source == "localRule"
        }));
        assert!(scan.candidates.iter().any(|candidate| {
            candidate
                .library_relative_path
                .ends_with("plugins/readme.txt")
                && candidate.review_source == "acuAi"
                && candidate.risk_level == "high"
        }));

        cleanup(source);
        cleanup(installed_root);
    }

    #[test]
    fn clears_only_archive_import_staging_contents() {
        let root = temp_root("clear_import_staging");
        let import_staging = root.join("mods").join("staging").join("imports");
        write_file(&import_staging.join("old-import").join("nativePC/mod.bin"));
        write_file(&import_staging.join("stale-marker.txt"));

        clear_import_staging(&import_staging).unwrap();

        assert!(import_staging.is_dir());
        assert_eq!(fs::read_dir(&import_staging).unwrap().count(), 0);

        cleanup(root);
    }

    #[test]
    fn stores_model_replacements_in_new_manifest() {
        let root = temp_root("recognized_install_source");
        let installed_root = temp_root("recognized_install_target");
        write_file(
            &root
                .join("nativePC")
                .join("wp")
                .join("swo")
                .join("bs_swo001")
                .join("mod")
                .join("bs_swo001.mod3"),
        );

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let manifest_json = fs::read_to_string(&result.manifest_path).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_json).unwrap();

        assert_eq!(
            manifest["schemaVersion"],
            serde_json::Value::from(super::CURRENT_MOD_MANIFEST_SCHEMA_VERSION)
        );
        assert_eq!(result.model_replacements[0].sub_kind, "太刀");
        assert_eq!(manifest["modelReplacements"][0]["modelKind"], "weapon");

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn recognizes_models_when_listing_legacy_manifest() {
        let root = temp_root("legacy_recognition_source");
        let installed_root = temp_root("legacy_recognition_target");
        write_file(
            &root
                .join("nativePC")
                .join("pl")
                .join("f_equip")
                .join("pl001_0000")
                .join("helm")
                .join("mod")
                .join("f_pl001_0000_helm.mod3"),
        );

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["schemaVersion"] = serde_json::Value::from(1);
        manifest
            .as_object_mut()
            .unwrap()
            .remove("modelReplacements");
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let list = list_installed_mods_from(&installed_root).unwrap();

        assert_eq!(list.mods[0].model_replacements[0].sub_kind, "头盔");
        assert!(list.mods[0].model_replacements[0]
            .display_names
            .iter()
            .any(|name| name == "皮制头饰"));

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn refreshes_model_recognition_when_listing_schema_seven_manifest() {
        let root = temp_root("schema_seven_hair_source");
        let installed_root = temp_root("schema_seven_hair_target");
        write_file(
            &root
                .join("nativePC")
                .join("pl")
                .join("hair")
                .join("hair100")
                .join("mod")
                .join("hair100.mod3"),
        );

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["schemaVersion"] = serde_json::Value::from(7);
        manifest["modelReplacements"] = serde_json::json!([]);
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let list = list_installed_mods_from(&installed_root).unwrap();
        let hair = &list.mods[0].model_replacements[0];

        assert_eq!(hair.model_id, "hair100");
        assert_eq!(hair.game_ids, ["1-1"]);
        assert_eq!(hair.display_names, ["发型 1-1"]);

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn refreshes_partial_armor_recognition_from_schema_ten_manifest() {
        let root = temp_root("schema_ten_partial_armor_source");
        let installed_root = temp_root("schema_ten_partial_armor_target");
        for part in ["helm", "body", "arm", "wst"] {
            write_file(
                &root
                    .join("nativePC/pl/f_equip/pl105_0000")
                    .join(part)
                    .join("mod/model.mod3"),
            );
        }

        let result =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["schemaVersion"] = serde_json::Value::from(10);
        manifest["modelReplacements"] = serde_json::json!([]);
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let list = list_installed_mods_from(&installed_root).unwrap();
        let armor = list.mods[0]
            .model_replacements
            .iter()
            .filter(|replacement| replacement.model_kind == "armor")
            .collect::<Vec<_>>();

        assert_eq!(armor.len(), 4);
        assert!(armor
            .iter()
            .all(|replacement| replacement.model_part != "set"));

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn same_name_mod_returns_existing_installation() {
        let root = temp_root("duplicate_source");
        let installed_root = temp_root("duplicate_target");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));

        let first =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let duplicate =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let list = list_installed_mods_from(&installed_root).unwrap();

        assert!(!first.already_installed);
        assert!(duplicate.already_installed);
        assert_eq!(duplicate.mod_id, first.mod_id);
        assert_eq!(list.mods.len(), 1);

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn same_name_mod_can_be_imported_with_a_user_supplied_new_name() {
        let root = temp_root("duplicate_renamed_source");
        let installed_root = temp_root("duplicate_renamed_target");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));

        let first =
            install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();
        let renamed = install_mod_from_folder_into_with_options_and_progress(
            root_to_string(&root),
            false,
            &installed_root,
            Some("同名 MOD（副本）".to_string()),
            None,
            &OperationReporter::default(),
        )
        .unwrap();
        let list = list_installed_mods_from(&installed_root).unwrap();

        assert!(!renamed.already_installed);
        assert_ne!(renamed.mod_id, first.mod_id);
        assert_eq!(renamed.name, "同名 MOD（副本）");
        assert_eq!(list.mods.len(), 2);

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn lists_installed_mods_from_manifests() {
        let root = temp_root("list_source");
        let installed_root = temp_root("list_target");
        write_file(&root.join("nativePC").join("weapon").join("sword.mod3"));
        install_mod_from_folder_into(root_to_string(&root), false, &installed_root).unwrap();

        let list = list_installed_mods_from(&installed_root).unwrap();

        assert_eq!(list.mods.len(), 1);
        assert_eq!(
            list.mods[0].name,
            root.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(list.mods[0].file_count, 1);
        assert_eq!(list.mods[0].files.len(), 1);
        assert_eq!(
            list.mods[0].files[0].deploy_relative_path,
            "nativePC/weapon/sword.mod3"
        );
        assert!(!list.mods[0].enabled);

        cleanup(root);
        cleanup(installed_root);
    }

    #[test]
    fn rejects_unsupported_archive_extension() {
        let root = temp_root("archive_extension");
        let archive_path = root.join("not-a-mod.txt");
        write_file(&archive_path);

        let error = validate_archive_path(&archive_path).unwrap_err();

        assert!(error.contains("Unsupported archive extension"));

        cleanup(root);
    }

    #[test]
    fn enables_mod_by_copying_files_and_recording_deployment() {
        let source_root = temp_root("enable_source");
        let installed_root = temp_root("enable_installed");
        let game_root = temp_root("enable_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();

        let enable_result =
            enable_mod_from(&installed_root, &game_root, &install_result.mod_id, false).unwrap();

        assert!(enable_result.enabled);
        assert_eq!(enable_result.affected_file_count, 1);
        assert!(game_root
            .join("nativePC")
            .join("weapon")
            .join("sword.mod3")
            .is_file());
        let manifest = fs::read_to_string(&install_result.manifest_path).unwrap();
        assert!(manifest.contains("\"enabled\": true"));
        assert!(manifest.contains("\"deployedFiles\""));

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn disable_mod_removes_only_recorded_deployed_files() {
        let source_root = temp_root("disable_source");
        let installed_root = temp_root("disable_installed");
        let game_root = temp_root("disable_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(&game_root.join("nativePC").join("manual").join("keep.txt"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &install_result.mod_id, false).unwrap();

        let disable_result =
            disable_mod_from(&installed_root, &game_root, &install_result.mod_id).unwrap();

        assert!(!disable_result.enabled);
        assert_eq!(disable_result.affected_file_count, 1);
        assert!(!game_root
            .join("nativePC")
            .join("weapon")
            .join("sword.mod3")
            .exists());
        assert!(!game_root.join("nativePC").join("weapon").exists());
        assert!(game_root
            .join("nativePC")
            .join("manual")
            .join("keep.txt")
            .is_file());

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn disable_preview_lists_only_recorded_deployed_files() {
        let source_root = temp_root("disable_preview_source");
        let installed_root = temp_root("disable_preview_installed");
        let game_root = temp_root("disable_preview_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &install_result.mod_id, false).unwrap();

        let plan = preview_disable_mod_from(&installed_root, &install_result.mod_id).unwrap();

        assert!(plan.enabled);
        assert_eq!(plan.file_count, 1);
        assert_eq!(
            plan.files[0].deploy_relative_path,
            "nativePC/weapon/sword.mod3"
        );
        assert!(plan.warnings.is_empty());

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn enable_preview_requires_confirmation_for_unmanaged_existing_target() {
        let source_root = temp_root("overwrite_source");
        let installed_root = temp_root("overwrite_installed");
        let game_root = temp_root("overwrite_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(&game_root.join("nativePC").join("weapon").join("sword.mod3"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();

        let plan =
            preview_enable_mod_from(&installed_root, &game_root, &install_result.mod_id).unwrap();
        let error =
            match enable_mod_from(&installed_root, &game_root, &install_result.mod_id, false) {
                Ok(_) => panic!("enable should require overwrite confirmation"),
                Err(error) => error,
            };

        assert!(plan.requires_overwrite_confirmation);
        assert_eq!(plan.status, "needsOverwriteConfirmation");
        assert!(error.contains("requires overwrite confirmation"));

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn enable_preview_groups_conflict_files_by_enabled_mod() {
        let first_source = temp_root("enable_conflict_first_source");
        let second_source = temp_root("enable_conflict_second_source");
        let installed_root = temp_root("enable_conflict_installed");
        let game_root = temp_root("enable_conflict_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        for source in [&first_source, &second_source] {
            write_file(&source.join("nativePC").join("weapon").join("shared.mod3"));
        }
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();

        let plan = preview_enable_mod_from(&installed_root, &game_root, &second.mod_id).unwrap();

        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].mod_id, first.mod_id);
        assert_eq!(
            plan.conflicts[0].conflict_files,
            vec!["nativePC/weapon/shared.mod3"]
        );

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn uninstalls_disabled_mod_from_local_library() {
        let source_root = temp_root("uninstall_disabled_source");
        let installed_root = temp_root("uninstall_disabled_installed");
        let game_root = temp_root("uninstall_disabled_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let mod_path = PathBuf::from(&install_result.mod_path);

        let plan = preview_uninstall_mod_from(&installed_root, &install_result.mod_id).unwrap();
        let result =
            uninstall_mod_from(&installed_root, &game_root, &install_result.mod_id).unwrap();

        assert_eq!(plan.library_file_count, 1);
        assert_eq!(plan.deployed_file_count, 0);
        assert_eq!(result.removed_library_file_count, 1);
        assert_eq!(result.removed_deployed_file_count, 0);
        assert!(!mod_path.exists());

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn uninstalls_enabled_mod_and_removes_recorded_deployment() {
        let source_root = temp_root("uninstall_enabled_source");
        let installed_root = temp_root("uninstall_enabled_installed");
        let game_root = temp_root("uninstall_enabled_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let install_result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &install_result.mod_id, false).unwrap();

        let result =
            uninstall_mod_from(&installed_root, &game_root, &install_result.mod_id).unwrap();

        assert_eq!(result.removed_library_file_count, 1);
        assert_eq!(result.removed_deployed_file_count, 1);
        assert!(!PathBuf::from(&install_result.mod_path).exists());
        assert!(!game_root
            .join("nativePC")
            .join("weapon")
            .join("sword.mod3")
            .exists());

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn previews_restore_all_enabled_mods() {
        let first_source = temp_root("restore_preview_first_source");
        let second_source = temp_root("restore_preview_second_source");
        let installed_root = temp_root("restore_preview_installed");
        let game_root = temp_root("restore_preview_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("first.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("second.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, false).unwrap();

        let plan = preview_restore_all_mods_from(&installed_root).unwrap();

        assert_eq!(plan.affected_mod_count, 2);
        assert_eq!(plan.deployed_file_count, 2);

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn restore_all_disables_mods_and_removes_recorded_deployments() {
        let first_source = temp_root("restore_execute_first_source");
        let second_source = temp_root("restore_execute_second_source");
        let installed_root = temp_root("restore_execute_installed");
        let game_root = temp_root("restore_execute_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("first.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("second.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, false).unwrap();

        let result = restore_all_mods_from(&installed_root, &game_root).unwrap();
        let list = list_installed_mods_from(&installed_root).unwrap();

        assert_eq!(result.affected_mod_count, 2);
        assert_eq!(result.removed_deployed_file_count, 2);
        assert!(list.mods.iter().all(|mod_summary| !mod_summary.enabled));
        assert!(!game_root
            .join("nativePC")
            .join("weapon")
            .join("first.mod3")
            .exists());
        assert!(!game_root
            .join("nativePC")
            .join("weapon")
            .join("second.mod3")
            .exists());

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn detects_conflicting_deploy_paths() {
        let first_source = temp_root("conflict_first_source");
        let second_source = temp_root("conflict_second_source");
        let installed_root = temp_root("conflict_installed");
        let game_root = temp_root("conflict_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();

        let disabled_report = get_mod_conflict_report_from(&installed_root).unwrap();
        assert_eq!(disabled_report.conflict_count, 0);

        enable_mod_from(&installed_root, &game_root, &second.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, true).unwrap();

        let report = get_mod_conflict_report_from(&installed_root).unwrap();

        assert_eq!(report.conflict_count, 1);
        assert_eq!(report.conflict_file_count, 1);
        assert_eq!(report.groups[0].conflict_file_count, 1);
        assert_eq!(report.groups[0].participant_count, 2);

        let contexts = load_all_installed_manifests(&installed_root).unwrap();
        let index = build_workspace_mod_index(&contexts).unwrap();
        let store = read_conflict_order_store(&installed_root).unwrap();
        let cached_report = build_mod_conflict_report_from_workspace_index(&index, &store);
        assert_eq!(cached_report.conflict_count, report.conflict_count);
        assert_eq!(
            cached_report.conflict_file_count,
            report.conflict_file_count
        );
        assert_eq!(cached_report.groups[0].participants.len(), 2);

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn groups_indirectly_conflicting_mods_together() {
        let first_source = temp_root("connected_first_source");
        let bridge_source = temp_root("connected_bridge_source");
        let third_source = temp_root("connected_third_source");
        let installed_root = temp_root("connected_installed");
        let game_root = temp_root("connected_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("first-shared.mod3"),
        );
        write_file(
            &bridge_source
                .join("nativePC")
                .join("weapon")
                .join("first-shared.mod3"),
        );
        write_file(
            &bridge_source
                .join("nativePC")
                .join("weapon")
                .join("second-shared.mod3"),
        );
        write_file(
            &third_source
                .join("nativePC")
                .join("weapon")
                .join("second-shared.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let bridge =
            install_mod_from_folder_into(root_to_string(&bridge_source), false, &installed_root)
                .unwrap();
        let third =
            install_mod_from_folder_into(root_to_string(&third_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &bridge.mod_id, true).unwrap();
        enable_mod_from(&installed_root, &game_root, &third.mod_id, true).unwrap();

        let report = get_mod_conflict_report_from(&installed_root).unwrap();

        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].participant_count, 3);
        assert_eq!(report.groups[0].conflict_file_count, 2);

        cleanup(first_source);
        cleanup(bridge_source);
        cleanup(third_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn moving_one_conflict_order_changes_only_that_winner() {
        let first_source = temp_root("priority_first_source");
        let second_source = temp_root("priority_second_source");
        let installed_root = temp_root("priority_installed");
        let game_root = temp_root("priority_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, true).unwrap();

        let initial_report = get_mod_conflict_report_from(&installed_root).unwrap();
        let group_id = initial_report.groups[0].group_id.clone();
        let participant_order = initial_report.groups[0]
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect();
        move_conflict_participant_from(
            &installed_root,
            &group_id,
            &first.mod_id,
            "down",
            participant_order,
        )
        .unwrap();
        let moved_report = get_mod_conflict_report_from(&installed_root).unwrap();

        assert_eq!(
            initial_report.groups[0]
                .participants
                .first()
                .unwrap()
                .mod_id,
            first.mod_id
        );
        assert_eq!(
            moved_report.groups[0].participants.first().unwrap().mod_id,
            second.mod_id
        );

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn moving_one_conflict_does_not_change_another_conflict_order() {
        let first_source = temp_root("isolated_first_source");
        let second_source = temp_root("isolated_second_source");
        let third_source = temp_root("isolated_third_source");
        let fourth_source = temp_root("isolated_fourth_source");
        let installed_root = temp_root("isolated_installed");
        let game_root = temp_root("isolated_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));

        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        write_file(
            &third_source
                .join("nativePC")
                .join("weapon")
                .join("other.mod3"),
        );
        write_file(
            &fourth_source
                .join("nativePC")
                .join("weapon")
                .join("other.mod3"),
        );

        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        let third =
            install_mod_from_folder_into(root_to_string(&third_source), false, &installed_root)
                .unwrap();
        let fourth =
            install_mod_from_folder_into(root_to_string(&fourth_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, true).unwrap();
        enable_mod_from(&installed_root, &game_root, &third.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &fourth.mod_id, true).unwrap();

        let before = get_mod_conflict_report_from(&installed_root).unwrap();
        assert_eq!(before.groups.len(), 2);
        let edited_group = before
            .groups
            .iter()
            .find(|group| {
                group
                    .participants
                    .iter()
                    .any(|participant| participant.mod_id == first.mod_id)
            })
            .unwrap();
        let other_group = before
            .groups
            .iter()
            .find(|group| group.group_id != edited_group.group_id)
            .unwrap();
        let edited_group_id = edited_group.group_id.clone();
        let other_group_id = other_group.group_id.clone();
        let other_before = other_group
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();

        let participant_order = edited_group
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect();
        move_conflict_participant_from(
            &installed_root,
            &edited_group_id,
            &first.mod_id,
            "down",
            participant_order,
        )
        .unwrap();

        let after = get_mod_conflict_report_from(&installed_root).unwrap();
        let other_after = after
            .groups
            .iter()
            .find(|group| group.group_id == other_group_id)
            .unwrap()
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();

        assert_eq!(other_before, other_after);

        cleanup(first_source);
        cleanup(second_source);
        cleanup(third_source);
        cleanup(fourth_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn applying_one_conflict_group_updates_all_shared_files() {
        let first_source = temp_root("apply_order_first_source");
        let second_source = temp_root("apply_order_second_source");
        let installed_root = temp_root("apply_order_installed");
        let game_root = temp_root("apply_order_game");
        let target_path = game_root.join("nativePC").join("weapon").join("same.mod3");
        let second_target_path = game_root.join("nativePC").join("weapon").join("same.mrl3");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file_with_contents(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
            "first",
        );
        write_file_with_contents(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
            "second",
        );
        write_file_with_contents(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mrl3"),
            "first material",
        );
        write_file_with_contents(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mrl3"),
            "second material",
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, true).unwrap();
        assert_eq!(fs::read_to_string(&target_path).unwrap(), "second");
        assert_eq!(
            fs::read_to_string(&second_target_path).unwrap(),
            "second material"
        );

        let report = get_mod_conflict_report_from(&installed_root).unwrap();
        let group_id = report.groups[0].group_id.clone();
        assert_eq!(report.groups[0].conflict_file_count, 2);
        let participant_order = report.groups[0]
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect();
        move_conflict_participant_from(
            &installed_root,
            &group_id,
            &second.mod_id,
            "down",
            participant_order,
        )
        .unwrap();
        let result =
            apply_conflict_order_from(&installed_root, &game_root, &group_id, false).unwrap();

        assert_eq!(result.applied_file_count, 2);
        assert_eq!(fs::read_to_string(&target_path).unwrap(), "first");
        assert_eq!(
            fs::read_to_string(&second_target_path).unwrap(),
            "first material"
        );

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn disabling_conflict_winner_restores_the_next_enabled_participant() {
        let first_source = temp_root("handoff_first_source");
        let second_source = temp_root("handoff_second_source");
        let installed_root = temp_root("handoff_installed");
        let game_root = temp_root("handoff_game");
        let target_path = game_root.join("nativePC").join("weapon").join("same.mod3");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file_with_contents(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
            "first",
        );
        write_file_with_contents(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
            "second",
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, true).unwrap();

        let report = get_mod_conflict_report_from(&installed_root).unwrap();
        let group_id = report.groups[0].group_id.clone();
        let winner = report.groups[0]
            .participants
            .iter()
            .find(|participant| participant.enabled)
            .unwrap()
            .mod_id
            .clone();
        apply_conflict_order_from(&installed_root, &game_root, &group_id, false).unwrap();
        disable_mod_from(&installed_root, &game_root, &winner).unwrap();

        let expected = if winner == first.mod_id {
            "second"
        } else {
            "first"
        };
        assert_eq!(fs::read_to_string(&target_path).unwrap(), expected);

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn lists_custom_display_name_and_note_without_changing_original_name() {
        let source_root = temp_root("metadata_source");
        let installed_root = temp_root("metadata_installed");
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["displayName"] = serde_json::Value::from("测试显示名称");
        manifest["note"] = serde_json::Value::from("用于回归测试的备注");
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let list = list_installed_mods_from(&installed_root).unwrap();

        assert_eq!(list.mods[0].name, "测试显示名称");
        assert_eq!(list.mods[0].original_name, result.name);
        assert_eq!(list.mods[0].note, "用于回归测试的备注");
        assert_eq!(
            list.mods[0].source_path,
            manifest["sourcePath"].as_str().unwrap()
        );

        cleanup(source_root);
        cleanup(installed_root);
    }

    #[test]
    fn persists_manual_mod_library_order_separately_from_installation_order() {
        let first_source = temp_root("manual_order_first_source");
        let second_source = temp_root("manual_order_second_source");
        let installed_root = temp_root("manual_order_installed");
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("first.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("second.mod3"),
        );

        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();

        // A normal library read creates the persisted order used by later drag sorting.
        list_installed_mods_from(&installed_root).unwrap();
        move_mod_library_item_from(&installed_root, &first.mod_id, &second.mod_id, false).unwrap();
        let ordered = list_installed_mods_from(&installed_root).unwrap();
        let ordered_ids = ordered
            .mods
            .iter()
            .map(|installed_mod| installed_mod.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ordered_ids,
            vec![first.mod_id.as_str(), second.mod_id.as_str()]
        );

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
    }

    #[test]
    fn partial_snapshot_summary_does_not_rewrite_complete_manual_order() {
        let first_source = temp_root("partial_order_first_source");
        let second_source = temp_root("partial_order_second_source");
        let installed_root = temp_root("partial_order_installed");
        write_file(&first_source.join("nativePC/weapon/first.mod3"));
        write_file(&second_source.join("nativePC/weapon/second.mod3"));
        install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
            .unwrap();
        install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
            .unwrap();

        list_installed_mods_from(&installed_root).unwrap();
        let initial_store = read_mod_library_order_store(&installed_root).unwrap();
        let mut manual_order = initial_store.import_mod_ids.clone();
        manual_order.reverse();
        replace_mod_library_order_from(
            &installed_root,
            manual_order.clone(),
            &initial_store.import_mod_ids,
        )
        .unwrap();

        let contexts = load_all_installed_manifests(&installed_root).unwrap();
        installed_mod_list_from_contexts(
            &installed_root,
            std::slice::from_ref(&contexts[0]),
            &ModCategoryStore::default(),
            &OperationReporter::default(),
            false,
        )
        .unwrap();

        let stored = read_mod_library_order_store(&installed_root).unwrap();
        assert_eq!(stored.manual_mod_ids, manual_order);
        assert_eq!(stored.import_mod_ids, initial_store.import_mod_ids);

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
    }

    #[test]
    fn migrates_legacy_library_order_without_changing_manual_sequence() {
        let store = serde_json::from_str::<ModLibraryOrderStore>(
            r#"{"schemaVersion":1,"modIds":["mod-b","mod-a"]}"#,
        )
        .unwrap();
        let installation_order = vec!["mod-a".to_string(), "mod-b".to_string()];

        let (manual_order, import_order) =
            super::normalized_mod_library_orders_from_installation(&store, &installation_order);

        assert_eq!(manual_order, vec!["mod-b", "mod-a"]);
        assert_eq!(import_order, vec!["mod-a", "mod-b"]);
    }

    #[test]
    fn replaces_and_restores_complete_mod_library_order() {
        let installed_root = temp_root("replace_library_order");
        let installation_order = vec![
            "mod-a".to_string(),
            "mod-b".to_string(),
            "mod-c".to_string(),
        ];

        replace_mod_library_order_from(
            &installed_root,
            vec![
                "mod-c".to_string(),
                "mod-a".to_string(),
                "mod-b".to_string(),
            ],
            &installation_order,
        )
        .unwrap();
        let replaced = read_mod_library_order_store(&installed_root).unwrap();
        assert_eq!(replaced.manual_mod_ids, vec!["mod-c", "mod-a", "mod-b"]);
        assert_eq!(replaced.import_mod_ids, installation_order.clone());

        restore_mod_library_import_order_from(&installed_root, &installation_order).unwrap();
        let restored = read_mod_library_order_store(&installed_root).unwrap();
        assert_eq!(restored.manual_mod_ids, installation_order);

        cleanup(installed_root);
    }

    #[test]
    fn rejects_incomplete_mod_library_order() {
        let installed_root = temp_root("incomplete_library_order");
        let installation_order = vec!["mod-a".to_string(), "mod-b".to_string()];

        let error = replace_mod_library_order_from(
            &installed_root,
            vec!["mod-a".to_string()],
            &installation_order,
        )
        .unwrap_err();

        assert!(error.contains("全部 MOD"));
        cleanup(installed_root);
    }

    #[test]
    fn moves_branch_group_members_as_one_library_order_block() {
        let sources_root = temp_root("branch_group_order_sources");
        let installed_root = temp_root("branch_group_order_installed");
        for index in 0..4 {
            let source_root = sources_root.join(format!("source_{index}"));
            write_file(
                &source_root
                    .join("nativePC")
                    .join("weapon")
                    .join(format!("weapon_{index}.mod3")),
            );
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        }

        let initial_ids = list_installed_mods_from(&installed_root)
            .unwrap()
            .mods
            .into_iter()
            .map(|installed_mod| installed_mod.id)
            .collect::<Vec<_>>();
        let source_ids = vec![initial_ids[1].clone(), initial_ids[2].clone()];
        let target_ids = vec![initial_ids[3].clone()];

        move_mod_library_items_from(&installed_root, &source_ids, &target_ids, true).unwrap();

        let ordered_ids = list_installed_mods_from(&installed_root)
            .unwrap()
            .mods
            .into_iter()
            .map(|installed_mod| installed_mod.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ordered_ids,
            vec![
                initial_ids[0].clone(),
                initial_ids[3].clone(),
                initial_ids[1].clone(),
                initial_ids[2].clone(),
            ]
        );

        cleanup(sources_root);
        cleanup(installed_root);
    }

    #[test]
    fn assigns_recognized_weapon_to_its_weapon_subcategory() {
        let source_root = temp_root("weapon_subcategory_source");
        let installed_root = temp_root("weapon_subcategory_installed");
        write_file(
            &source_root
                .join("nativePC")
                .join("wp")
                .join("swo")
                .join("bs_swo001")
                .join("mod")
                .join("bs_swo001.mod3"),
        );

        install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root).unwrap();
        let mut installed_mod_list = list_installed_mods_from(&installed_root).unwrap();
        let listed_mod = installed_mod_list.mods.pop().unwrap();

        assert_eq!(listed_mod.categories.len(), 1);
        assert_eq!(listed_mod.categories[0].name, "太刀");
        let parent_id = listed_mod.categories[0].parent_id.as_deref().unwrap();
        let category_store =
            load_or_initialize_mod_category_store_for_installed_root(&installed_root).unwrap();
        assert!(category_store
            .categories
            .iter()
            .any(|category| category.id == parent_id && category.name == "武器"));

        cleanup(source_root);
        cleanup(installed_root);
    }

    #[test]
    fn migrates_legacy_category_override_into_unified_categories() {
        let source_root = temp_root("legacy_category_source");
        let mods_root = temp_root("legacy_category_mods");
        let installed_root = mods_root.join("installed");
        fs::create_dir_all(&installed_root).unwrap();
        write_file(
            &source_root
                .join("nativePC")
                .join("pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3"),
        );
        let result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();

        let category = StoredModCategory {
            id: "category-visual".to_string(),
            name: "外观收藏".to_string(),
            parent_id: None,
            created_at_unix_seconds: 1,
            recognition_keys: Vec::new(),
        };
        save_mod_category_store(
            &mods_root.join("categories.json"),
            &ModCategoryStore {
                schema_version: 1,
                categories: vec![category.clone()],
                suppressed_recognition_keys: Vec::new(),
            },
        )
        .unwrap();

        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["schemaVersion"] = serde_json::Value::from(13);
        manifest.as_object_mut().unwrap().remove("categoryIds");
        manifest["categoryOverride"] = serde_json::Value::from(category.id.clone());
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let categorized = list_installed_mods_from(&installed_root).unwrap();
        assert_eq!(categorized.mods[0].category_ids.len(), 2);
        assert!(categorized.mods[0]
            .categories
            .iter()
            .any(|current| current.name == "防具"));
        assert!(categorized.mods[0]
            .categories
            .iter()
            .any(|current| current.name == category.name));

        assert_eq!(
            remove_category_from_manifests(&installed_root, "category-visual").unwrap(),
            1
        );
        let remaining = list_installed_mods_from(&installed_root).unwrap();
        assert_eq!(remaining.mods[0].categories.len(), 1);
        assert_eq!(remaining.mods[0].categories[0].name, "防具");

        let store: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(mods_root.join("categories.json")).unwrap())
                .unwrap();
        assert_eq!(
            store["schemaVersion"],
            serde_json::Value::from(MOD_CATEGORY_STORE_SCHEMA_VERSION)
        );

        cleanup(source_root);
        cleanup(mods_root);
    }

    #[test]
    fn reports_shared_model_targets_for_conflicting_mods() {
        let first_source = temp_root("shared_model_first_source");
        let second_source = temp_root("shared_model_second_source");
        let installed_root = temp_root("shared_model_installed");
        let game_root = temp_root("shared_model_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("wp")
                .join("swo")
                .join("bs_swo001")
                .join("mod")
                .join("shared.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("wp")
                .join("swo")
                .join("bs_swo001")
                .join("mod")
                .join("shared.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, true).unwrap();

        let report = get_mod_conflict_report_from(&installed_root).unwrap();
        let shared_target = report.groups[0]
            .shared_model_targets
            .iter()
            .find(|target| target.model_kind == "weapon")
            .unwrap();

        assert_eq!(shared_target.model_id, "wp/swo/bs_swo001");
        assert!(shared_target
            .display_names
            .iter()
            .any(|name| name == "铁刀1"));

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn removes_uninstalled_mod_from_conflict_order() {
        let first_source = temp_root("conflict_order_cleanup_first_source");
        let second_source = temp_root("conflict_order_cleanup_second_source");
        let installed_root = temp_root("conflict_order_cleanup_installed");
        let game_root = temp_root("conflict_order_cleanup_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file(
            &first_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        write_file(
            &second_source
                .join("nativePC")
                .join("weapon")
                .join("same.mod3"),
        );
        let first =
            install_mod_from_folder_into(root_to_string(&first_source), false, &installed_root)
                .unwrap();
        let second =
            install_mod_from_folder_into(root_to_string(&second_source), false, &installed_root)
                .unwrap();
        enable_mod_from(&installed_root, &game_root, &first.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &second.mod_id, true).unwrap();

        let report = get_mod_conflict_report_from(&installed_root).unwrap();
        let group_id = report.groups[0].group_id.clone();
        let participant_order = report.groups[0]
            .participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect();
        move_conflict_participant_from(
            &installed_root,
            &group_id,
            &first.mod_id,
            "down",
            participant_order,
        )
        .unwrap();
        assert!(read_conflict_order_store(&installed_root)
            .unwrap()
            .orders
            .get(&group_id)
            .unwrap()
            .contains(&first.mod_id));

        remove_mod_from_conflict_orders(&installed_root, &first.mod_id).unwrap();

        let store = read_conflict_order_store(&installed_root).unwrap();
        assert!(store
            .orders
            .values()
            .all(|order| !order.contains(&first.mod_id)));

        cleanup(first_source);
        cleanup(second_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn saves_armor_remap_without_modifying_the_local_library_copy() {
        let source_root = temp_root("armor_remap_source");
        let installed_root = temp_root("armor_remap_installed");
        let game_root = temp_root("armor_remap_game");
        let source_file =
            source_root.join("nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3");
        write_file_with_contents(&source_file, "original armor data");
        write_file(&game_root.join("MonsterHunterWorld.exe"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let plan = preview_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl001_0000".to_string()),
        )
        .unwrap();
        assert_eq!(plan.changed_file_count, 1);
        assert_eq!(
            plan.files[0].effective_deploy_relative_path,
            "nativePC/pl/f_equip/pl001_0000/body/mod/f_body001_0000.mod3"
        );

        apply_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl001_0000".to_string()),
        )
        .unwrap();
        let local_source = PathBuf::from(&installed.content_path)
            .join("nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3");
        assert_eq!(
            fs::read_to_string(&local_source).unwrap(),
            "original armor data"
        );

        enable_mod_from(&installed_root, &game_root, &installed.mod_id, false).unwrap();
        let effective_target =
            game_root.join("nativePC/pl/f_equip/pl001_0000/body/mod/f_body001_0000.mod3");
        assert_eq!(
            fs::read_to_string(&effective_target).unwrap(),
            "original armor data"
        );
        assert!(!game_root
            .join("nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3")
            .exists());

        let listed = list_installed_mods_from(&installed_root).unwrap();
        assert_eq!(listed.mods[0].model_remap_count, 1);
        assert!(listed.mods[0]
            .model_replacements
            .iter()
            .any(|replacement| replacement.model_id == "pl001_0000"));
        assert!(listed.mods[0]
            .original_model_replacements
            .iter()
            .any(|replacement| replacement.model_id == "pl105_0000"));

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn dat_armor_remap_deploys_standardized_core_file_without_global_dat() {
        let source_root = temp_root("dat_armor_remap_source");
        let installed_root = temp_root("dat_armor_remap_installed");
        let game_root = temp_root("dat_armor_remap_game");
        let source_model =
            source_root.join("nativePC/pl/f_equip/pl105_0000/body/mod/f_body106_0000.mod3");
        let source_dat = source_root.join("nativePC/common/equip/armor.am_dat");
        write_file_with_contents(&source_model, "dat armor model");
        write_bytes(&source_dat, &armor_dat_bytes(&[(250, 300, 1, 106)]));
        write_file(&game_root.join("MonsterHunterWorld.exe"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let plan = preview_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl067_0000".to_string()),
        )
        .unwrap();
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("自动排除") && warning.contains("armor.am_dat")));
        assert!(plan.files.iter().any(|file| {
            file.effective_deploy_relative_path
                == "nativePC/pl/f_equip/pl067_0000/body/mod/f_body067_0000.mod3"
        }));

        apply_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl067_0000".to_string()),
        )
        .unwrap();
        let restore_plan =
            preview_mod_remap_from(&installed_root, &installed.mod_id, "armor:pl105_0000", None)
                .unwrap();
        assert!(restore_plan
            .warnings
            .iter()
            .any(|warning| warning.contains("重新部署") && warning.contains("armor.am_dat")));
        enable_mod_from(&installed_root, &game_root, &installed.mod_id, false).unwrap();

        assert_eq!(
            fs::read_to_string(
                game_root.join("nativePC/pl/f_equip/pl067_0000/body/mod/f_body067_0000.mod3"),
            )
            .unwrap(),
            "dat armor model"
        );
        assert!(!game_root
            .join("nativePC/common/equip/armor.am_dat")
            .exists());
        assert_eq!(
            fs::read(
                PathBuf::from(&installed.content_path).join("nativePC/common/equip/armor.am_dat"),
            )
            .unwrap(),
            armor_dat_bytes(&[(250, 300, 1, 106)])
        );

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn warns_when_armor_remap_moves_epv_effect_triggers() {
        let source_root = temp_root("armor_epv_remap_warning_source");
        let installed_root = temp_root("armor_epv_remap_warning_installed");
        let source_file =
            source_root.join("nativePC/pl/f_equip/pl105_0000/body/epv/f_body105.epv3");
        write_file(&source_file);

        let installed =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let remapped_plan = preview_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl001_0000".to_string()),
        )
        .unwrap();
        assert!(remapped_plan
            .warnings
            .iter()
            .any(|warning| warning.contains("装备特效")));

        apply_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl001_0000".to_string()),
        )
        .unwrap();
        let restore_plan =
            preview_mod_remap_from(&installed_root, &installed.mod_id, "armor:pl105_0000", None)
                .unwrap();
        assert!(restore_plan
            .warnings
            .iter()
            .any(|warning| warning.contains("装备特效")));

        cleanup(source_root);
        cleanup(installed_root);
    }

    #[test]
    fn armor_remap_deploys_originally_paired_slinger_to_target_binding() {
        let source_root = temp_root("armor_paired_slinger_source");
        let installed_root = temp_root("armor_paired_slinger_installed");
        let game_root = temp_root("armor_paired_slinger_game");
        let armor_relative = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3";
        let slinger_relative = "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3";
        write_file_with_contents(&source_root.join(armor_relative), "paired armor");
        write_file_with_contents(&source_root.join(slinger_relative), "paired slinger");
        write_file(&game_root.join("MonsterHunterWorld.exe"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let plan = preview_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl106_0000".to_string()),
        )
        .unwrap();
        assert_eq!(plan.changed_file_count, 2);
        assert_eq!(plan.evam_rewrite_count, 0);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("slg000_0000 -> slg106_0000")));
        assert!(plan.files.iter().any(|file| {
            file.effective_deploy_relative_path
                == "nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3"
        }));

        apply_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "armor:pl105_0000",
            Some("armor:pl106_0000".to_string()),
        )
        .unwrap();
        enable_mod_from(&installed_root, &game_root, &installed.mod_id, false).unwrap();

        assert_eq!(
            fs::read_to_string(PathBuf::from(&installed.content_path).join(slinger_relative))
                .unwrap(),
            "paired slinger"
        );
        assert_eq!(
            fs::read_to_string(game_root.join("nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3"))
                .unwrap(),
            "paired slinger"
        );
        assert!(game_root
            .join("nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3")
            .is_file());
        assert!(!game_root.join(slinger_relative).exists());

        let listed = list_installed_mods_from(&installed_root).unwrap();
        assert!(listed.mods[0]
            .model_replacements
            .iter()
            .any(|replacement| replacement.model_id == "slg106_0000"));

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn slinger_remap_rewrites_deployed_evam_but_keeps_library_original() {
        let source_root = temp_root("slinger_evam_source");
        let installed_root = temp_root("slinger_evam_installed");
        let game_root = temp_root("slinger_evam_game");
        write_file(&source_root.join("nativePC/wp/slg/slg128_0000/mod/slg128_0000.mod3"));
        let evam_relative = "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm105_0000.evam";
        write_bytes(&source_root.join(evam_relative), &evam_bytes(128));
        write_file(&game_root.join("MonsterHunterWorld.exe"));

        let installed =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();
        let slinger = installed
            .model_replacements
            .iter()
            .find(|replacement| replacement.model_kind == "slinger")
            .unwrap();
        assert_eq!(slinger.associations[0].display_names[0], "【冰狼】服装");

        let mut legacy_manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&installed.manifest_path).unwrap()).unwrap();
        legacy_manifest["schemaVersion"] = serde_json::Value::from(12);
        legacy_manifest["modelReplacements"] = serde_json::json!([]);
        fs::write(
            &installed.manifest_path,
            serde_json::to_string_pretty(&legacy_manifest).unwrap(),
        )
        .unwrap();
        let listed = list_installed_mods_from(&installed_root).unwrap();
        assert_eq!(
            listed.mods[0]
                .original_model_replacements
                .iter()
                .find(|replacement| replacement.model_kind == "slinger")
                .unwrap()
                .associations[0]
                .model_id,
            "pl105_0000"
        );

        let plan = preview_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "slinger:slg128_0000",
            Some("slinger:slg106_0000".to_string()),
        )
        .unwrap();
        assert_eq!(plan.changed_file_count, 2);
        assert_eq!(plan.evam_rewrite_count, 1);

        apply_mod_remap_from(
            &installed_root,
            &installed.mod_id,
            "slinger:slg128_0000",
            Some("slinger:slg106_0000".to_string()),
        )
        .unwrap();
        enable_mod_from(&installed_root, &game_root, &installed.mod_id, false).unwrap();

        let local_evam =
            fs::read(PathBuf::from(&installed.content_path).join(evam_relative)).unwrap();
        let deployed_evam = fs::read(game_root.join(evam_relative)).unwrap();
        assert_eq!(
            u32::from_le_bytes(local_evam[16..20].try_into().unwrap()),
            128
        );
        assert_eq!(
            u32::from_le_bytes(deployed_evam[16..20].try_into().unwrap()),
            106
        );
        assert!(game_root
            .join("nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3")
            .is_file());

        cleanup(source_root);
        cleanup(installed_root);
        cleanup(game_root);
    }

    #[test]
    fn conflict_and_disable_restore_use_effective_remap_paths() {
        let remapped_source = temp_root("remap_conflict_first_source");
        let original_source = temp_root("remap_conflict_second_source");
        let installed_root = temp_root("remap_conflict_installed");
        let game_root = temp_root("remap_conflict_game");
        write_file(&game_root.join("MonsterHunterWorld.exe"));
        write_file_with_contents(
            &remapped_source.join("nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3"),
            "remapped winner candidate",
        );
        write_file_with_contents(
            &original_source.join("nativePC/pl/f_equip/pl001_0000/body/mod/f_body001_0000.mod3"),
            "original target winner",
        );
        let remapped =
            install_mod_from_folder_into(root_to_string(&remapped_source), false, &installed_root)
                .unwrap();
        let original =
            install_mod_from_folder_into(root_to_string(&original_source), false, &installed_root)
                .unwrap();
        apply_mod_remap_from(
            &installed_root,
            &remapped.mod_id,
            "armor:pl105_0000",
            Some("armor:pl001_0000".to_string()),
        )
        .unwrap();

        enable_mod_from(&installed_root, &game_root, &remapped.mod_id, false).unwrap();
        enable_mod_from(&installed_root, &game_root, &original.mod_id, true).unwrap();
        let report = get_mod_conflict_report_from(&installed_root).unwrap();
        assert_eq!(report.groups.len(), 1);
        assert!(report.groups[0]
            .shared_model_targets
            .iter()
            .any(|target| target.model_id == "pl001_0000"));

        disable_mod_from(&installed_root, &game_root, &original.mod_id).unwrap();
        let target = game_root.join("nativePC/pl/f_equip/pl001_0000/body/mod/f_body001_0000.mod3");
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "remapped winner candidate"
        );

        cleanup(remapped_source);
        cleanup(original_source);
        cleanup(installed_root);
        cleanup(game_root);
    }

    fn temp_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "acumod_mod_import_test_{}_{}_{}",
            process::id(),
            name,
            stamp
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "test").unwrap();
    }

    fn write_file_with_contents(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn write_bytes(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn evam_bytes(slinger_id: u32) -> Vec<u8> {
        let mut bytes = vec![0; 26];
        bytes[..4].copy_from_slice(&[0x01, 0x10, 0x09, 0x18]);
        bytes[4..8].copy_from_slice(b"EVAM");
        bytes[8..12].copy_from_slice(&3_u32.to_le_bytes());
        bytes[12..16].fill(0xff);
        bytes[16..20].copy_from_slice(&slinger_id.to_le_bytes());
        bytes[24..26].copy_from_slice(&[0x01, 0x01]);
        bytes
    }

    fn armor_dat_bytes(entries: &[(u16, u16, u8, u16)]) -> Vec<u8> {
        const HEADER_SIZE: usize = 10;
        const ENTRY_SIZE: usize = 60;
        let mut bytes = vec![0; HEADER_SIZE + entries.len() * ENTRY_SIZE];
        bytes[4..6].copy_from_slice(&0x005F_u16.to_le_bytes());
        bytes[6..10].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (index, (set_id, set_group, equip_slot, model_id)) in entries.iter().enumerate() {
            let offset = HEADER_SIZE + index * ENTRY_SIZE;
            bytes[offset + 7..offset + 9].copy_from_slice(&set_id.to_le_bytes());
            bytes[offset + 10] = *equip_slot;
            bytes[offset + 13..offset + 15].copy_from_slice(&model_id.to_le_bytes());
            bytes[offset + 53..offset + 55].copy_from_slice(&set_group.to_le_bytes());
        }
        bytes
    }

    fn root_to_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn cleanup(root: PathBuf) {
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("acumod_mod_import_test_"))
            .unwrap_or(false)
        {
            let _ = fs::remove_dir_all(root);
        }
    }
}
