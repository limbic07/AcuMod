use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::storage::config;

use super::model_recognition::{
    recognize_model_replacements, recognize_model_replacements_with_evam, EvamRecognitionFile,
    ModelReplacement,
};
use super::model_remap::{
    build_effective_remap_files, build_model_remap_groups, rewrite_evam_slinger_id,
    rewrite_mrl3_texture_paths, EffectiveRemapFile, EvamSlingerIdRewrite, ModelRemapGroup,
    ModelRemapSelection,
};

const PREVIEW_FILE_LIMIT: usize = 200;
const CURRENT_MOD_MANIFEST_SCHEMA_VERSION: u32 = 13;
const CURRENT_MODEL_RECOGNITION_SCHEMA_VERSION: u32 = 13;
const USER_MOD_CATEGORY_STORE_SCHEMA_VERSION: u32 = 1;
const USER_MOD_CATEGORY_NAME_LIMIT: usize = 40;
const COMMON_NATIVE_PC_CHILDREN: &[&str] = &[
    "weapon", "wp", "pl", "armor", "common", "npc", "em", "quest", "stage", "sound", "vfx",
    "effect", "ui", "otomo", "charm", "mus", "plugins",
];

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
    pub relative_path: String,
    pub detection_method: String,
    pub deploy_root: String,
    pub file_count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModFile {
    pub source_relative_path: String,
    pub deploy_relative_path: String,
    pub library_relative_path: String,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModSummary {
    pub id: String,
    pub name: String,
    pub original_name: String,
    pub note: String,
    pub categories: Vec<String>,
    pub category_override: Option<String>,
    pub user_category: Option<UserModCategory>,
    pub mod_path: String,
    pub content_path: String,
    pub manifest_path: String,
    pub file_count: usize,
    pub files: Vec<InstalledModFile>,
    pub enabled: bool,
    pub deploy_root: String,
    pub detection_method: String,
    pub installed_at_unix_seconds: u64,
    pub model_replacements: Vec<ModelReplacement>,
    pub original_model_replacements: Vec<ModelReplacement>,
    pub model_remap_count: usize,
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
pub struct InstalledModList {
    pub mods: Vec<InstalledModSummary>,
    pub warnings: Vec<String>,
    pub message: String,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDeploymentPlan {
    pub mod_id: String,
    pub name: String,
    pub status: String,
    pub message: String,
    pub file_count: usize,
    pub files: Vec<ModDeploymentPlanFile>,
    pub warnings: Vec<String>,
    pub requires_overwrite_confirmation: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployedModFile {
    pub deploy_relative_path: String,
    pub deployed_path: String,
    pub deployed_at_unix_seconds: u64,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictParticipant {
    pub mod_id: String,
    pub name: String,
    pub enabled: bool,
    pub order: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedModelTarget {
    pub model_kind: String,
    pub sub_kind: String,
    pub model_id: String,
    pub display_names: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConflictGroup {
    pub group_id: String,
    pub participant_count: usize,
    pub conflict_file_count: usize,
    pub enabled_participant_count: usize,
    pub participants: Vec<ModConflictParticipant>,
    pub shared_model_targets: Vec<SharedModelTarget>,
}

#[derive(Serialize)]
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
    pub category_override: Option<String>,
    pub user_category: Option<UserModCategory>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserModCategory {
    pub id: String,
    pub name: String,
    pub created_at_unix_seconds: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserModCategoryList {
    pub categories: Vec<UserModCategory>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserModCategoryDeleteResult {
    pub category_id: String,
    pub cleared_mod_count: usize,
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
    pub category_override: Option<String>,
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
    category_override: Option<String>,
    source_path: String,
    content_root_path: String,
    detection_method: String,
    deploy_root: String,
    installed_at_unix_seconds: u64,
    enabled: bool,
    file_count: usize,
    files: Vec<InstalledModFile>,
    #[serde(default)]
    model_replacements: Vec<ModelReplacement>,
    #[serde(default)]
    model_remaps: Vec<ModelRemapSelection>,
    #[serde(default)]
    deployed_files: Vec<DeployedModFile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserModCategoryStore {
    #[serde(default = "default_user_mod_category_store_schema_version")]
    schema_version: u32,
    #[serde(default)]
    categories: Vec<UserModCategory>,
}

impl Default for UserModCategoryStore {
    fn default() -> Self {
        Self {
            schema_version: USER_MOD_CATEGORY_STORE_SCHEMA_VERSION,
            categories: Vec::new(),
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

struct ConflictPathGroup {
    deploy_relative_path: String,
    participant_ids: Vec<String>,
}

fn default_conflict_order_schema_version() -> u32 {
    1
}

fn default_user_mod_category_store_schema_version() -> u32 {
    USER_MOD_CATEGORY_STORE_SCHEMA_VERSION
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

pub fn preview_mod_import(
    raw_path: String,
    allow_game_root: bool,
) -> Result<ModImportPreview, String> {
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
    let scan = scan_directories(&source_path)?;
    let candidates = detect_candidates(&source_path, &scan.directories);

    if let Some(preview) = preview_from_candidates(&source_path, candidates, scan.warnings.clone())?
    {
        return Ok(preview);
    }

    preview_game_root_fallback(&source_path, allow_game_root, scan.warnings)
}

pub fn install_mod_from_folder(
    app: &tauri::AppHandle,
    raw_path: String,
    allow_game_root: bool,
) -> Result<ModInstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    install_mod_from_folder_into(raw_path, allow_game_root, &paths.installed_path)
}

pub fn install_mod_from_archive(
    app: &tauri::AppHandle,
    raw_path: String,
    allow_game_root: bool,
) -> Result<ModArchiveImportOutcome, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    initialize_import_staging(&paths)?;
    clear_import_staging(&paths.import_staging_path)?;
    let archive_path = normalize_user_path(&raw_path);
    let archive_path = validate_archive_path(&archive_path)?;
    let archive_name = derive_mod_name(&archive_path);

    if let Some(existing) = find_installed_mod_by_name(&paths.installed_path, &archive_name)? {
        return Ok(ModArchiveImportOutcome {
            status: "alreadyInstalled".to_string(),
            source_path: String::new(),
            original_archive_path: path_to_string(&archive_path),
            preview: None,
            install_result: Some(existing),
            message: "A MOD with the same name is already installed.".to_string(),
        });
    }

    let staging_path = paths
        .import_staging_path
        .join(unique_mod_id(&archive_name)?);

    fs::create_dir_all(&staging_path).map_err(|error| {
        format!(
            "Could not create archive staging directory {}: {error}",
            staging_path.display()
        )
    })?;

    if let Err(error) = extract_archive_with_bundled_7zip(app, &archive_path, &staging_path) {
        let _ = fs::remove_dir_all(&staging_path);
        return Err(error);
    }
    let preview = preview_mod_import(path_to_string(&staging_path), allow_game_root)?;

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

    let result = install_mod_from_folder_into_with_options(
        path_to_string(&staging_path),
        allow_game_root,
        &paths.installed_path,
        Some(archive_name),
        Some(path_to_string(&archive_path)),
    );

    match result {
        Ok(install_result) => {
            let cleanup_message = match fs::remove_dir_all(&staging_path) {
                Ok(()) => "Archive MOD import completed.".to_string(),
                Err(error) => format!(
                    "Archive MOD import completed, but staging cleanup failed at {}: {error}",
                    staging_path.display()
                ),
            };

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

pub fn install_mod_from_candidate(
    app: &tauri::AppHandle,
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
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
    let result = install_mod_from_candidate_into(
        source_path,
        candidate_root_path,
        original_archive_path,
        &paths.installed_path,
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

            Ok(install_result)
        }
        Err(error) => Err(error),
    }
}

fn install_mod_from_candidate_into(
    source_path: String,
    candidate_root_path: String,
    original_archive_path: Option<String>,
    installed_root: &Path,
) -> Result<ModInstallResult, String> {
    let source = canonical_directory(&normalize_user_path(&source_path), "candidate source")?;
    let candidate = canonical_directory(
        &normalize_user_path(&candidate_root_path),
        "candidate content root",
    )?;
    let preview = preview_mod_import(path_to_string(&source), false)?;
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
    install_mod_from_folder_into_with_options(
        path_to_string(&candidate),
        false,
        installed_root,
        Some(preferred_name),
        Some(path_to_string(&original_source)),
    )
}

pub fn list_installed_mods(app: &tauri::AppHandle) -> Result<InstalledModList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    load_or_initialize_user_mod_category_store(&paths)?;
    list_installed_mods_from(&paths.installed_path)
}

pub fn update_mod_metadata(
    app: &tauri::AppHandle,
    mod_id: String,
    patch: ModMetadataPatch,
) -> Result<ModMetadataUpdateResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_mod_id(&mod_id)?;
    if patch.display_name.is_none() && patch.note.is_none() && patch.category_override.is_none() {
        return Err("MOD metadata update must include at least one field.".to_string());
    }

    let category_store = load_or_initialize_user_mod_category_store(&paths)?;
    let mut context = load_installed_manifest(&paths.installed_path, &mod_id)?;

    if let Some(display_name) = patch.display_name {
        let display_name = validate_mod_display_name(&display_name)?;
        context.manifest.display_name = (!display_name.is_empty()).then_some(display_name);
    }

    if let Some(note) = patch.note {
        context.manifest.note = validate_mod_note(&note)?;
    }

    if let Some(category_override) = patch.category_override {
        context.manifest.category_override =
            resolve_category_override(&category_store, &category_override)?;
    }

    refresh_manifest_model_replacements(&mut context)?;
    context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
    save_manifest(&context.manifest_path, &context.manifest)?;

    let user_category = resolve_user_category(&category_store, &context.manifest.category_override);

    Ok(ModMetadataUpdateResult {
        mod_id: context.manifest.id.clone(),
        name: manifest_display_name(&context.manifest),
        original_name: context.manifest.name.clone(),
        note: context.manifest.note.clone(),
        category_override: context.manifest.category_override.clone(),
        user_category,
        message: "MOD metadata was updated without changing deployment files.".to_string(),
    })
}

pub fn list_user_mod_categories(app: &tauri::AppHandle) -> Result<UserModCategoryList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let store = load_or_initialize_user_mod_category_store(&paths)?;
    let categories = sorted_user_mod_categories(&store.categories);

    Ok(UserModCategoryList {
        message: format!("{} user MOD category(s) are available.", categories.len()),
        categories,
    })
}

pub fn create_user_mod_category(
    app: &tauri::AppHandle,
    name: String,
) -> Result<UserModCategory, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let mut store = load_or_initialize_user_mod_category_store(&paths)?;
    let name = validate_user_mod_category_name(&name)?;

    ensure_user_mod_category_name_is_available(&store.categories, &name, None)?;
    let category = UserModCategory {
        id: unique_user_mod_category_id(&store.categories, &name)?,
        name,
        created_at_unix_seconds: unix_seconds_now()?,
    };
    store.categories.push(category.clone());
    save_user_mod_category_store(&paths.categories_path, &store)?;

    Ok(category)
}

pub fn rename_user_mod_category(
    app: &tauri::AppHandle,
    category_id: String,
    name: String,
) -> Result<UserModCategory, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_user_mod_category_id(&category_id)?;
    let mut store = load_or_initialize_user_mod_category_store(&paths)?;
    let name = validate_user_mod_category_name(&name)?;

    ensure_user_mod_category_name_is_available(&store.categories, &name, Some(&category_id))?;
    let category = store
        .categories
        .iter_mut()
        .find(|category| category.id == category_id)
        .ok_or_else(|| format!("User MOD category was not found: {category_id}"))?;
    category.name = name;
    let category = category.clone();
    save_user_mod_category_store(&paths.categories_path, &store)?;

    Ok(category)
}

pub fn delete_user_mod_category(
    app: &tauri::AppHandle,
    category_id: String,
) -> Result<UserModCategoryDeleteResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    validate_user_mod_category_id(&category_id)?;
    let mut store = load_or_initialize_user_mod_category_store(&paths)?;
    let category_index = store
        .categories
        .iter()
        .position(|category| category.id == category_id)
        .ok_or_else(|| format!("User MOD category was not found: {category_id}"))?;

    let cleared_mod_count =
        clear_category_override_from_manifests(&paths.installed_path, &category_id)?;
    store.categories.remove(category_index);
    save_user_mod_category_store(&paths.categories_path, &store)?;

    Ok(UserModCategoryDeleteResult {
        category_id,
        cleared_mod_count,
        message:
            "User MOD category was deleted and affected MODs returned to automatic classification."
                .to_string(),
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

pub fn apply_mod_remap(
    app: &tauri::AppHandle,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<ModRemapApplyResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    apply_mod_remap_from(&paths.installed_path, &mod_id, &group_key, target_id)
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

pub fn enable_mod(
    app: &tauri::AppHandle,
    mod_id: String,
    confirm_overwrite: bool,
) -> Result<ModDeploymentResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    enable_mod_from(
        &paths.installed_path,
        &game_root,
        &mod_id,
        confirm_overwrite,
    )
}

pub fn disable_mod(app: &tauri::AppHandle, mod_id: String) -> Result<ModDeploymentResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    disable_mod_from(&paths.installed_path, &game_root, &mod_id)
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

pub fn uninstall_mod(app: &tauri::AppHandle, mod_id: String) -> Result<ModUninstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    let mut result = uninstall_mod_from(&paths.installed_path, &game_root, &mod_id)?;
    if let Err(error) = remove_mod_from_conflict_orders(&paths.installed_path, &mod_id) {
        result.warnings.push(format!(
            "MOD was uninstalled, but conflict order entries could not be cleaned: {error}"
        ));
    }
    Ok(result)
}

pub fn preview_restore_all_mods(app: &tauri::AppHandle) -> Result<RestoreAllPlan, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    preview_restore_all_mods_from(&paths.installed_path)
}

pub fn restore_all_mods(app: &tauri::AppHandle) -> Result<RestoreAllResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    restore_all_mods_from(&paths.installed_path, &game_root)
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
) -> Result<ModConflictMoveResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    move_conflict_participant_from(&paths.installed_path, &group_id, &mod_id, &direction)
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

pub fn apply_conflict_order(
    app: &tauri::AppHandle,
    group_id: String,
    confirm_overwrite: bool,
) -> Result<ApplyConflictOrderResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    let game_root = resolve_game_root(app)?;
    apply_conflict_order_from(
        &paths.installed_path,
        &game_root,
        &group_id,
        confirm_overwrite,
    )
}

fn install_mod_from_folder_into(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
) -> Result<ModInstallResult, String> {
    install_mod_from_folder_into_with_options(raw_path, allow_game_root, installed_root, None, None)
}

fn install_mod_from_folder_into_with_options(
    raw_path: String,
    allow_game_root: bool,
    installed_root: &Path,
    preferred_name: Option<String>,
    original_source_path: Option<String>,
) -> Result<ModInstallResult, String> {
    let preview = preview_mod_import(raw_path, allow_game_root)?;

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
    let files = build_file_previews(&source_root, &deploy_root)?;

    if files.is_empty() {
        return Err("MOD import has no files to copy.".to_string());
    }

    let source_path = PathBuf::from(&preview.source_path);
    let mod_name = preferred_name.unwrap_or_else(|| derive_mod_name(&source_path));

    if let Some(existing) = find_installed_mod_by_name(installed_root, &mod_name)? {
        return Ok(existing);
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

        for file in &files {
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
        }

        let model_replacements =
            recognize_model_replacements_for_library_files(&installed_files, &content_path)?;
        let manifest_path = temp_mod_path.join("manifest.json");
        let manifest = InstalledModManifest {
            schema_version: CURRENT_MOD_MANIFEST_SCHEMA_VERSION,
            id: mod_id.clone(),
            name: mod_name.clone(),
            display_name: None,
            note: String::new(),
            category_override: None,
            source_path: original_source_path.unwrap_or_else(|| preview.source_path.clone()),
            content_root_path,
            detection_method: preview.detection_method.clone(),
            deploy_root: preview.deploy_root.clone(),
            installed_at_unix_seconds: unix_seconds_now()?,
            enabled: false,
            file_count: installed_files.len(),
            files: installed_files.clone(),
            model_replacements: model_replacements.clone(),
            model_remaps: Vec::new(),
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
    let model_replacements =
        model_replacements_for_manifest(&context.manifest, &context.content_path)?;

    let display_name = manifest_display_name(&context.manifest);

    Ok(Some(ModInstallResult {
        mod_id: context.manifest.id,
        name: display_name,
        already_installed: true,
        mod_path: path_to_string(&context.mod_path),
        content_path: path_to_string(&context.content_path),
        manifest_path: path_to_string(&context.manifest_path),
        file_count: context.manifest.file_count,
        files: context.manifest.files,
        model_replacements,
        message: "A MOD with the same name is already installed. The existing MOD was kept."
            .to_string(),
    }))
}

fn preview_from_candidates(
    source_path: &Path,
    candidates: Vec<Candidate>,
    warnings: Vec<String>,
) -> Result<Option<ModImportPreview>, String> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut shallowest = candidates;
    shallowest.sort_by_key(|candidate| candidate.depth);
    let selected_depth = shallowest[0].depth;
    shallowest.retain(|candidate| candidate.depth == selected_depth);

    if shallowest.len() > 1 {
        let candidate_dtos = build_candidate_dtos(source_path, &shallowest)?;

        return Ok(Some(ModImportPreview {
            source_path: path_to_string(source_path),
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
    let files = build_file_previews(&candidate.root_path, &candidate.deploy_root)?;
    let file_count = files.len();
    let mut preview_files = files;

    if preview_files.len() > PREVIEW_FILE_LIMIT {
        preview_files.truncate(PREVIEW_FILE_LIMIT);
    }

    Ok(Some(ModImportPreview {
        source_path: path_to_string(source_path),
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
) -> Result<ModImportPreview, String> {
    let files = build_file_previews(source_path, &DeployRoot::GameRoot)?;
    let file_count = files.len();

    if file_count == 0 {
        return Ok(ModImportPreview {
            source_path: path_to_string(source_path),
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

fn scan_directories(root: &Path) -> Result<ScanResult, String> {
    let mut directories = vec![root.to_path_buf()];
    let mut warnings = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
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
                warnings.push(format!("Skipped symbolic link: {}", path.display()));
                continue;
            }

            if metadata.is_dir() {
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
) -> Result<Vec<ModImportCandidate>, String> {
    let mut dtos = Vec::new();

    for candidate in candidates {
        dtos.push(ModImportCandidate {
            root_path: path_to_string(&candidate.root_path),
            relative_path: candidate
                .root_path
                .strip_prefix(source_path)
                .map(path_to_string)
                .unwrap_or_else(|_| path_to_string(&candidate.root_path)),
            detection_method: candidate.detection_method.to_string(),
            deploy_root: deploy_root_label(&candidate.deploy_root).to_string(),
            file_count: build_file_previews(&candidate.root_path, &candidate.deploy_root)?.len(),
        });
    }

    dtos.sort_by_key(|candidate| Reverse(candidate.file_count));

    Ok(dtos)
}

fn build_file_previews(
    root: &Path,
    deploy_root: &DeployRoot,
) -> Result<Vec<ModImportFilePreview>, String> {
    let mut files = Vec::new();
    collect_file_previews(root, root, deploy_root, &mut files)?;
    files.sort_by_key(|file| file.deploy_relative_path.to_lowercase());
    Ok(files)
}

fn collect_file_previews(
    root: &Path,
    directory: &Path,
    deploy_root: &DeployRoot,
    files: &mut Vec<ModImportFilePreview>,
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
            collect_file_previews(root, &path, deploy_root, files)?;
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
    let user_category_store = load_user_mod_category_store_for_installed_root(installed_root)?;

    if !installed_root.exists() {
        return Ok(InstalledModList {
            mods,
            warnings,
            message: "No installed MOD directory exists yet.".to_string(),
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
            warnings.push(format!(
                "Skipped installed MOD without manifest: {}",
                mod_path.display()
            ));
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
        let original_model_replacements =
            match model_replacements_for_manifest(&manifest, &content_path) {
                Ok(model_replacements) => model_replacements,
                Err(error) => {
                    warnings.push(format!(
                        "Could not recognize model replacements for {}: {error}",
                        manifest.name
                    ));
                    Vec::new()
                }
            };
        let model_replacements = match effective_model_replacements_for_manifest(
            &manifest,
            &original_model_replacements,
        ) {
            Ok(model_replacements) => model_replacements,
            Err(error) => {
                warnings.push(format!(
                    "Could not apply saved model remaps for {}: {error}",
                    manifest.name
                ));
                original_model_replacements.clone()
            }
        };

        let categories = model_categories(&model_replacements);
        let user_category =
            resolve_user_category(&user_category_store, &manifest.category_override);
        if manifest.category_override.is_some() && user_category.is_none() {
            warnings.push(format!(
                "MOD {} references a missing user category and is using automatic classification.",
                manifest.name
            ));
        }

        mods.push(InstalledModSummary {
            id: manifest.id.clone(),
            name: manifest_display_name(&manifest),
            original_name: manifest.name.clone(),
            note: manifest.note.clone(),
            categories,
            category_override: user_category.as_ref().map(|category| category.id.clone()),
            user_category,
            mod_path: path_to_string(&mod_path),
            content_path: path_to_string(&mod_path.join("content")),
            manifest_path: path_to_string(&manifest_path),
            file_count: manifest.file_count,
            files: manifest.files,
            enabled: manifest.enabled,
            deploy_root: manifest.deploy_root,
            detection_method: manifest.detection_method,
            installed_at_unix_seconds: manifest.installed_at_unix_seconds,
            model_replacements,
            original_model_replacements,
            model_remap_count: manifest.model_remaps.len(),
        });
    }

    mods.sort_by(|left, right| {
        right
            .installed_at_unix_seconds
            .cmp(&left.installed_at_unix_seconds)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let message = if mods.is_empty() {
        "No MODs have been imported into the local library yet.".to_string()
    } else {
        format!("{} MOD(s) are installed in the local library.", mods.len())
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

fn effective_model_replacements_for_manifest(
    manifest: &InstalledModManifest,
    original_replacements: &[ModelReplacement],
) -> Result<Vec<ModelReplacement>, String> {
    let effective_files = effective_remap_files_for_manifest(manifest, original_replacements)?;
    let paths = effective_files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let mut effective_replacements = recognize_model_replacements(&paths)?;
    copy_effective_slinger_associations(
        original_replacements,
        &manifest.model_remaps,
        &mut effective_replacements,
    );
    Ok(effective_replacements)
}

fn copy_effective_slinger_associations(
    original_replacements: &[ModelReplacement],
    selections: &[ModelRemapSelection],
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
        let effective_model_id = selections
            .iter()
            .find(|selection| selection.group_key == group_key)
            .and_then(|selection| selection.target_id.strip_prefix("slinger:"))
            .unwrap_or(&original.model_id);
        let associations = original
            .associations
            .iter()
            .map(|association| {
                let group_key = format!("armor:{}", association.model_id);
                let effective_armor_id = selections
                    .iter()
                    .find(|selection| selection.group_key == group_key)
                    .and_then(|selection| selection.target_id.strip_prefix("armor:"))
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

fn model_categories(model_replacements: &[ModelReplacement]) -> Vec<String> {
    let mut categories = model_replacements
        .iter()
        .map(|replacement| model_kind_label(&replacement.model_kind).to_string())
        .collect::<Vec<_>>();
    categories.sort();
    categories.dedup();
    categories
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
        "face" => "脸型",
        "monster" => "怪物",
        "poogie" => "噗吱猪服装",
        "furniture" => "家具",
        "playerAccessory" => "玩家附件",
        "palicoAccessory" => "随从附件",
        _ => "未识别",
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
    let effective_files = effective_installed_files_for_manifest(&preview_manifest, &replacements)?;
    let mut files = Vec::new();
    let mut total_mrl3_rewrite_count = 0;
    let mut total_evam_rewrite_count = 0;

    for effective_file in &effective_files {
        let source_path = source_path_for_installed_file(&context, &effective_file.installed_file)?;
        let mrl3_rewrite_count = preview_mrl3_rewrite_count(&source_path, effective_file)?;
        let evam_rewrite_count = preview_evam_rewrite_count(&source_path, effective_file)?;
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

fn apply_mod_remap_from(
    installed_root: &Path,
    mod_id: &str,
    group_key: &str,
    target_id: Option<String>,
) -> Result<ModRemapApplyResult, String> {
    let plan = preview_mod_remap_from(installed_root, mod_id, group_key, target_id.clone())?;
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

fn effective_remap_files_for_manifest(
    manifest: &InstalledModManifest,
    replacements: &[ModelReplacement],
) -> Result<Vec<EffectiveRemapFile>, String> {
    let paths = manifest
        .files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    build_effective_remap_files(&paths, replacements, &manifest.model_remaps)
}

fn effective_installed_files_for_manifest(
    manifest: &InstalledModManifest,
    replacements: &[ModelReplacement],
) -> Result<Vec<EffectiveInstalledModFile>, String> {
    effective_remap_files_for_manifest(manifest, replacements)?
        .into_iter()
        .map(|effective| {
            let installed_file = manifest
                .files
                .get(effective.file_index)
                .cloned()
                .ok_or_else(|| "有效部署文件索引超出范围。".to_string())?;
            Ok(EffectiveInstalledModFile {
                installed_file,
                deploy_relative_path: effective.deploy_relative_path,
                texture_path_rewrites: effective.texture_path_rewrites,
                evam_slinger_rewrite: effective.evam_slinger_rewrite,
            })
        })
        .collect()
}

fn effective_installed_files_for_context(
    context: &InstalledManifestContext,
) -> Result<Vec<EffectiveInstalledModFile>, String> {
    let replacements = model_replacements_for_manifest(&context.manifest, &context.content_path)?;
    effective_installed_files_for_manifest(&context.manifest, &replacements)
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

fn enable_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    confirm_overwrite: bool,
) -> Result<ModDeploymentResult, String> {
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
    for file in &effective_files {
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
        });
    }

    context.manifest.enabled = true;
    context.manifest.deployed_files = deployed_files.clone();
    save_manifest(&context.manifest_path, &context.manifest)?;
    let mut warnings = plan.warnings;
    record_enabled_mod_conflict_order(installed_root, &context.manifest.id)?;
    reapply_conflict_groups_for_mod(
        installed_root,
        game_root,
        &context.manifest.id,
        &mut warnings,
    )?;

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

fn disable_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
) -> Result<ModDeploymentResult, String> {
    let mut context = load_installed_manifest(installed_root, mod_id)?;
    let deployed_files = context.manifest.deployed_files.clone();
    let disabled_file_paths = deployed_files
        .iter()
        .map(|file| file.deploy_relative_path.clone())
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let removed_count = remove_deployed_files(game_root, &deployed_files, &mut warnings)?;

    context.manifest.enabled = false;
    context.manifest.deployed_files = Vec::new();
    save_manifest(&context.manifest_path, &context.manifest)?;

    restore_enabled_versions_for_paths(
        installed_root,
        game_root,
        &disabled_file_paths,
        &mut warnings,
    )?;

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

fn uninstall_mod_from(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
) -> Result<ModUninstallResult, String> {
    let context = load_installed_manifest(installed_root, mod_id)?;
    let removed_library_file_count = context.manifest.files.len();
    let mut warnings = Vec::new();
    let removed_deployed_file_count = if context.manifest.enabled {
        let disable_result = disable_mod_from(installed_root, game_root, mod_id)?;
        warnings.extend(disable_result.warnings);
        disable_result.affected_file_count
    } else {
        remove_deployed_files(game_root, &context.manifest.deployed_files, &mut warnings)?
    };
    let name = manifest_display_name(&context.manifest);
    let mod_id = context.manifest.id;

    fs::remove_dir_all(&context.mod_path).map_err(|error| {
        format!(
            "Could not remove installed MOD directory {}: {error}",
            context.mod_path.display()
        )
    })?;

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

fn restore_all_mods_from(
    installed_root: &Path,
    game_root: &Path,
) -> Result<RestoreAllResult, String> {
    let contexts = load_all_installed_manifests(installed_root)?;
    let plan_mods = restore_plan_items(&contexts);
    let mut warnings = Vec::new();
    let mut removed_deployed_file_count = 0;

    for mut context in contexts {
        if !context.manifest.enabled && context.manifest.deployed_files.is_empty() {
            continue;
        }

        let deployed_files = context.manifest.deployed_files.clone();
        removed_deployed_file_count +=
            remove_deployed_files(game_root, &deployed_files, &mut warnings)?;
        context.manifest.enabled = false;
        context.manifest.deployed_files = Vec::new();
        save_manifest(&context.manifest_path, &context.manifest)?;
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
    let contexts = load_all_installed_manifests(installed_root)?;
    let store = read_conflict_order_store(installed_root)?;
    build_mod_conflict_report(&contexts, &store)
}

fn move_conflict_participant_from(
    installed_root: &Path,
    group_id: &str,
    mod_id: &str,
    direction: &str,
) -> Result<ModConflictMoveResult, String> {
    validate_mod_id(mod_id)?;
    let contexts = load_all_installed_manifests(installed_root)?;
    let mut store = read_conflict_order_store(installed_root)?;
    let report = build_mod_conflict_report(&contexts, &store)?;
    let group = find_conflict_group(&report, group_id)?;
    let mut order = group
        .participants
        .iter()
        .map(|participant| participant.mod_id.clone())
        .collect::<Vec<_>>();
    let index = order
        .iter()
        .position(|participant_id| participant_id == mod_id)
        .ok_or_else(|| format!("MOD is not part of this conflict: {mod_id}"))?;
    let target_index = match direction {
        "up" if index > 0 => Some(index - 1),
        "down" if index + 1 < order.len() => Some(index + 1),
        "up" | "down" => None,
        other => return Err(format!("Unknown conflict move direction: {other}")),
    };

    let Some(target_index) = target_index else {
        return Ok(ModConflictMoveResult {
            group_id: group.group_id.clone(),
            mod_id: mod_id.to_string(),
            direction: direction.to_string(),
            moved: false,
            message: "MOD is already at the requested edge of this conflict order.".to_string(),
        });
    };

    order.swap(index, target_index);
    store.orders.insert(group.group_id.clone(), order);
    save_conflict_order_store(installed_root, &store)?;

    Ok(ModConflictMoveResult {
        group_id: group.group_id.clone(),
        mod_id: mod_id.to_string(),
        direction: direction.to_string(),
        moved: true,
        message: "Conflict order was updated. Apply this group to update its game files."
            .to_string(),
    })
}

fn preview_apply_conflict_order_from(
    installed_root: &Path,
    game_root: &Path,
    group_id: &str,
) -> Result<ApplyConflictOrderPlan, String> {
    let contexts = load_all_installed_manifests(installed_root)?;
    let store = read_conflict_order_store(installed_root)?;
    let report = build_mod_conflict_report(&contexts, &store)?;
    let group = find_conflict_group(&report, group_id)?;
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
                "Target exists but is not recorded as Acumod-managed: {}",
                target_path.display()
            ));
        }
    }

    let message = if applicable_file_count == 0 {
        "No conflict files have an enabled MOD version to apply.".to_string()
    } else {
        format!(
            "Applying this order will update {applicable_file_count} of {} conflicting file(s).",
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

fn apply_conflict_order_from(
    installed_root: &Path,
    game_root: &Path,
    group_id: &str,
    confirm_overwrite: bool,
) -> Result<ApplyConflictOrderResult, String> {
    let plan = preview_apply_conflict_order_from(installed_root, game_root, group_id)?;

    if plan.requires_overwrite_confirmation && !confirm_overwrite {
        return Err("Applying this conflict requires overwrite confirmation.".to_string());
    }

    if plan.applicable_file_count == 0 {
        return Err("No enabled MOD can provide a file for this conflict group.".to_string());
    }

    let mut contexts = load_all_installed_manifests(installed_root)?;
    let store = read_conflict_order_store(installed_root)?;
    let report = build_mod_conflict_report(&contexts, &store)?;
    let group = find_conflict_group(&report, group_id)?;
    let conflict_paths = conflict_paths_for_group(&contexts, group)?;
    let deployed_at = unix_seconds_now()?;
    let mut applied_file_count = 0;

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
                });
            }
        }

        applied_file_count += 1;
    }

    for context in &contexts {
        save_manifest(&context.manifest_path, &context.manifest)?;
    }

    Ok(ApplyConflictOrderResult {
        group_id: plan.group_id,
        applied_file_count,
        skipped_file_count: plan.conflict_file_count - applied_file_count,
        warnings: plan.warnings,
        message: format!("Applied the MOD order to {applied_file_count} conflicting file(s)."),
    })
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

        groups.push(ModConflictGroup {
            group_id,
            participant_count: participants.len(),
            conflict_file_count,
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
        for replacement in
            effective_model_replacements_for_manifest(&context.manifest, &original_replacements)?
        {
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
        .rev()
        .find(|participant| {
            participant.enabled && conflict_path.participant_ids.contains(&participant.mod_id)
        })
        .map(|participant| participant.mod_id.as_str())
}

fn reapply_conflict_groups_for_mod(
    installed_root: &Path,
    game_root: &Path,
    mod_id: &str,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let report = get_mod_conflict_report_from(installed_root)?;

    for group in report.groups {
        if group.enabled_participant_count == 0
            || !group
                .participants
                .iter()
                .any(|participant| participant.mod_id == mod_id)
        {
            continue;
        }

        if let Err(error) =
            apply_conflict_order_from(installed_root, game_root, &group.group_id, false)
        {
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
    let contexts = load_all_installed_manifests(installed_root)?;
    let mut store = read_conflict_order_store(installed_root)?;
    let report = build_mod_conflict_report(&contexts, &store)?;
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
        order.push(enabled_mod_id.to_string());
        store.orders.insert(group.group_id, order);
        changed = true;
    }

    if changed {
        save_conflict_order_store(installed_root, &store)?;
    }

    Ok(())
}

fn restore_enabled_versions_for_paths(
    installed_root: &Path,
    game_root: &Path,
    deploy_relative_paths: &[String],
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let mut contexts = load_all_installed_manifests(installed_root)?;
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

    for deploy_relative_path in deploy_relative_paths {
        let path_key = conflict_path_key(deploy_relative_path);

        if !seen_paths.insert(path_key.clone()) {
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
            continue;
        }

        let mut participant_ids = participants
            .iter()
            .map(|participant| participant.mod_id.clone())
            .collect::<Vec<_>>();
        participant_ids.sort();
        let stored_order = find_best_stored_order(&store, &participant_ids);
        sort_participants_by_conflict_order(&mut participants, stored_order);
        let winner_mod_id = participants.last().unwrap().mod_id.clone();
        let Some(winner_index) = contexts
            .iter()
            .position(|context| context.manifest.id == winner_mod_id)
        else {
            continue;
        };
        let Some(source_file) = effective_files_by_mod
            .get(&winner_mod_id)
            .into_iter()
            .flatten()
            .find(|file| conflict_path_key(&file.deploy_relative_path) == path_key)
            .cloned()
        else {
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
                });
            }
        }

        changed = true;
    }

    if changed {
        for context in &contexts {
            save_manifest(&context.manifest_path, &context.manifest)?;
        }
    }

    Ok(())
}

fn remove_deployed_files(
    game_root: &Path,
    deployed_files: &[DeployedModFile],
    warnings: &mut Vec<String>,
) -> Result<usize, String> {
    let mut removed_count = 0;

    for deployed_file in deployed_files {
        let target_relative_path = relative_string_to_path(&deployed_file.deploy_relative_path)?;
        let target_path = game_root.join(target_relative_path);

        if !target_path.exists() {
            warnings.push(format!(
                "Deployment target was already missing: {}",
                target_path.display()
            ));
            continue;
        }

        if target_path.is_dir() {
            return Err(format!(
                "Refusing to remove a directory during deployment cleanup: {}",
                target_path.display()
            ));
        }

        fs::remove_file(&target_path).map_err(|error| {
            format!(
                "Could not remove deployed file {}: {error}",
                target_path.display()
            )
        })?;
        removed_count += 1;
        cleanup_empty_parent_directories(&target_path, game_root, warnings);
    }

    Ok(removed_count)
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
        warnings,
        requires_overwrite_confirmation,
    })
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
    let mut contexts = Vec::new();

    if !installed_root.exists() {
        return Ok(contexts);
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

        contexts.push(load_installed_manifest(installed_root, mod_id)?);
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

    serde_json::from_str(&store_json).map_err(|error| {
        format!(
            "Could not parse conflict order store {}: {error}",
            store_path.display()
        )
    })
}

fn save_conflict_order_store(
    installed_root: &Path,
    store: &ConflictOrderStore,
) -> Result<(), String> {
    let store_path = conflict_order_store_path(installed_root);
    let store_json = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Could not serialize conflict order store: {error}"))?;

    fs::write(&store_path, store_json).map_err(|error| {
        format!(
            "Could not save conflict order store {}: {error}",
            store_path.display()
        )
    })
}

fn read_manifest(manifest_path: &Path) -> Result<InstalledModManifest, String> {
    let manifest_json = fs::read_to_string(manifest_path).map_err(|error| {
        format!(
            "Could not read MOD manifest {}: {error}",
            manifest_path.display()
        )
    })?;

    serde_json::from_str::<InstalledModManifest>(&manifest_json).map_err(|error| {
        format!(
            "Could not parse MOD manifest {}: {error}",
            manifest_path.display()
        )
    })
}

fn save_manifest(manifest_path: &Path, manifest: &InstalledModManifest) -> Result<(), String> {
    let manifest_json = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("Could not serialize MOD manifest: {error}"))?;

    fs::write(manifest_path, manifest_json).map_err(|error| {
        format!(
            "Could not write MOD manifest {}: {error}",
            manifest_path.display()
        )
    })
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

fn validate_mod_note(note: &str) -> Result<String, String> {
    let note = note.trim();
    if note.chars().count() > 800 {
        return Err("MOD note must contain at most 800 characters.".to_string());
    }
    Ok(note.to_string())
}

fn validate_user_mod_category_id(category_id: &str) -> Result<(), String> {
    if category_id.is_empty()
        || category_id == "."
        || category_id == ".."
        || category_id.contains('/')
        || category_id.contains('\\')
        || category_id.contains(':')
    {
        return Err(format!("Unsafe user MOD category id: {category_id}"));
    }

    Ok(())
}

fn validate_user_mod_category_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("User MOD category name cannot be empty.".to_string());
    }
    if name == "自动" {
        return Err("User MOD category name is reserved: 自动".to_string());
    }
    if name.chars().count() > USER_MOD_CATEGORY_NAME_LIMIT {
        return Err(format!(
            "User MOD category name must contain at most {USER_MOD_CATEGORY_NAME_LIMIT} characters."
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

fn extract_archive_with_bundled_7zip(
    app: &tauri::AppHandle,
    archive_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let seven_zip = bundled_7zip_executable(app).ok_or_else(|| {
        "Bundled 7-Zip unpacker is missing. Expected resources/unpackers/7zip/7z.exe and 7z.dll in the Acumod application resources.".to_string()
    })?;
    let output = Command::new(&seven_zip)
        .arg("x")
        .arg("-y")
        .arg(format!("-o{}", destination.display()))
        .arg(archive_path)
        .output()
        .map_err(|error| {
            format!(
                "Could not run bundled unpacker {}: {error}",
                seven_zip.display()
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    Err(format!(
        "Bundled 7-Zip could not extract archive {}.\nstdout: {}\nstderr: {}",
        archive_path.display(),
        stdout.trim(),
        stderr.trim()
    ))
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
    staging_path: PathBuf,
    import_staging_path: PathBuf,
}

fn library_paths(_app: &tauri::AppHandle) -> Result<LibraryPaths, String> {
    let software_data_path = software_data_path()?;
    let mods_path = software_data_path.join("mods");
    let installed_path = mods_path.join("installed");
    let categories_path = mods_path.join("categories.json");
    let staging_path = mods_path.join("staging");
    let import_staging_path = staging_path.join("imports");

    Ok(LibraryPaths {
        software_data_path,
        mods_path,
        installed_path,
        categories_path,
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

fn load_or_initialize_user_mod_category_store(
    paths: &LibraryPaths,
) -> Result<UserModCategoryStore, String> {
    let store_exists = paths.categories_path.is_file();
    let mut store = load_user_mod_category_store(&paths.categories_path)?;
    let mut changed = !store_exists;

    if store.schema_version != USER_MOD_CATEGORY_STORE_SCHEMA_VERSION {
        store.schema_version = USER_MOD_CATEGORY_STORE_SCHEMA_VERSION;
        changed = true;
    }

    if changed {
        save_user_mod_category_store(&paths.categories_path, &store)?;
    }

    Ok(store)
}

fn load_user_mod_category_store(categories_path: &Path) -> Result<UserModCategoryStore, String> {
    if !categories_path.exists() {
        return Ok(UserModCategoryStore::default());
    }

    if !categories_path.is_file() {
        return Err(format!(
            "User MOD category store is not a file: {}",
            categories_path.display()
        ));
    }

    let category_json = fs::read_to_string(categories_path).map_err(|error| {
        format!(
            "Could not read user MOD category store {}: {error}",
            categories_path.display()
        )
    })?;
    serde_json::from_str::<UserModCategoryStore>(&category_json).map_err(|error| {
        format!(
            "Could not parse user MOD category store {}: {error}",
            categories_path.display()
        )
    })
}

fn load_user_mod_category_store_for_installed_root(
    installed_root: &Path,
) -> Result<UserModCategoryStore, String> {
    let Some(mods_path) = installed_root.parent() else {
        return Ok(UserModCategoryStore::default());
    };

    load_user_mod_category_store(&mods_path.join("categories.json"))
}

fn save_user_mod_category_store(
    categories_path: &Path,
    store: &UserModCategoryStore,
) -> Result<(), String> {
    let category_json = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Could not serialize user MOD category store: {error}"))?;
    fs::write(categories_path, category_json).map_err(|error| {
        format!(
            "Could not write user MOD category store {}: {error}",
            categories_path.display()
        )
    })
}

fn sorted_user_mod_categories(categories: &[UserModCategory]) -> Vec<UserModCategory> {
    let mut categories = categories.to_vec();
    categories.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    categories
}

fn resolve_user_category(
    store: &UserModCategoryStore,
    category_override: &Option<String>,
) -> Option<UserModCategory> {
    category_override.as_deref().and_then(|category_id| {
        store
            .categories
            .iter()
            .find(|category| category.id == category_id)
            .cloned()
    })
}

fn resolve_category_override(
    store: &UserModCategoryStore,
    raw_category_id: &str,
) -> Result<Option<String>, String> {
    let category_id = raw_category_id.trim();
    if category_id.is_empty() {
        return Ok(None);
    }

    validate_user_mod_category_id(category_id)?;
    if !store
        .categories
        .iter()
        .any(|category| category.id == category_id)
    {
        return Err(format!("User MOD category was not found: {category_id}"));
    }

    Ok(Some(category_id.to_string()))
}

fn ensure_user_mod_category_name_is_available(
    categories: &[UserModCategory],
    name: &str,
    excluded_category_id: Option<&str>,
) -> Result<(), String> {
    let normalized_name = name.to_lowercase();
    if categories.iter().any(|category| {
        Some(category.id.as_str()) != excluded_category_id
            && category.name.trim().to_lowercase() == normalized_name
    }) {
        return Err(format!("User MOD category already exists: {name}"));
    }

    Ok(())
}

fn unique_user_mod_category_id(
    categories: &[UserModCategory],
    name: &str,
) -> Result<String, String> {
    let base_id = format!("category-{}-{}", unix_seconds_now()?, slugify(name));
    let mut category_id = base_id.clone();
    let mut suffix = 2;

    while categories.iter().any(|category| category.id == category_id) {
        category_id = format!("{base_id}-{suffix}");
        suffix += 1;
    }

    Ok(category_id)
}

fn clear_category_override_from_manifests(
    installed_root: &Path,
    category_id: &str,
) -> Result<usize, String> {
    let mut cleared_mod_count = 0;

    for mut context in load_all_installed_manifests(installed_root)? {
        if context.manifest.category_override.as_deref() != Some(category_id) {
            continue;
        }

        context.manifest.category_override = None;
        refresh_manifest_model_replacements(&mut context)?;
        context.manifest.schema_version = CURRENT_MOD_MANIFEST_SCHEMA_VERSION;
        save_manifest(&context.manifest_path, &context.manifest)?;
        cleared_mod_count += 1;
    }

    Ok(cleared_mod_count)
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
        env, fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        apply_conflict_order_from, apply_mod_remap_from, armor_set_label,
        clear_category_override_from_manifests, clear_import_staging, disable_mod_from,
        enable_mod_from, get_mod_conflict_report_from, install_mod_from_candidate_into,
        install_mod_from_folder_into, installed_mod_content_path, list_installed_mods_from,
        move_conflict_participant_from, preview_disable_mod_from, preview_enable_mod_from,
        preview_mod_import, preview_mod_remap_from, preview_restore_all_mods_from,
        preview_uninstall_mod_from, read_conflict_order_store, remove_mod_from_conflict_orders,
        restore_all_mods_from, save_user_mod_category_store, uninstall_mod_from,
        validate_archive_path, UserModCategory, UserModCategoryStore,
        USER_MOD_CATEGORY_STORE_SCHEMA_VERSION,
    };

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
        move_conflict_participant_from(&installed_root, &group_id, &second.mod_id, "down").unwrap();
        let moved_report = get_mod_conflict_report_from(&installed_root).unwrap();

        assert_eq!(
            initial_report.groups[0].participants.last().unwrap().mod_id,
            first.mod_id
        );
        assert_eq!(
            moved_report.groups[0].participants.last().unwrap().mod_id,
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

        move_conflict_participant_from(&installed_root, &edited_group_id, &first.mod_id, "down")
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
        move_conflict_participant_from(&installed_root, &group_id, &first.mod_id, "down").unwrap();
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
            .rev()
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

        cleanup(source_root);
        cleanup(installed_root);
    }

    #[test]
    fn user_category_override_is_resolved_and_can_return_to_automatic_classification() {
        let source_root = temp_root("user_category_source");
        let mods_root = temp_root("user_category_mods");
        let installed_root = mods_root.join("installed");
        fs::create_dir_all(&installed_root).unwrap();
        write_file(
            &source_root
                .join("nativePC")
                .join("weapon")
                .join("sword.mod3"),
        );
        let result =
            install_mod_from_folder_into(root_to_string(&source_root), false, &installed_root)
                .unwrap();

        let category = UserModCategory {
            id: "category-visual".to_string(),
            name: "外观收藏".to_string(),
            created_at_unix_seconds: 1,
        };
        save_user_mod_category_store(
            &mods_root.join("categories.json"),
            &UserModCategoryStore {
                schema_version: USER_MOD_CATEGORY_STORE_SCHEMA_VERSION,
                categories: vec![category.clone()],
            },
        )
        .unwrap();

        let mut manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result.manifest_path).unwrap()).unwrap();
        manifest["categoryOverride"] = serde_json::Value::from(category.id.clone());
        fs::write(
            &result.manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let categorized = list_installed_mods_from(&installed_root).unwrap();
        assert_eq!(
            categorized.mods[0].category_override.as_deref(),
            Some("category-visual")
        );
        assert_eq!(categorized.mods[0].user_category.as_ref(), Some(&category));

        assert_eq!(
            clear_category_override_from_manifests(&installed_root, "category-visual").unwrap(),
            1
        );
        let automatic = list_installed_mods_from(&installed_root).unwrap();
        assert!(automatic.mods[0].category_override.is_none());
        assert!(automatic.mods[0].user_category.is_none());

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

        let group_id = get_mod_conflict_report_from(&installed_root)
            .unwrap()
            .groups[0]
            .group_id
            .clone();
        move_conflict_participant_from(&installed_root, &group_id, &first.mod_id, "down").unwrap();
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
