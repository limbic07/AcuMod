use std::{
    cmp::Reverse,
    collections::HashSet,
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::Manager;

const PREVIEW_FILE_LIMIT: usize = 200;
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
    pub mod_path: String,
    pub content_path: String,
    pub manifest_path: String,
    pub file_count: usize,
    pub files: Vec<InstalledModFile>,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModSummary {
    pub id: String,
    pub name: String,
    pub mod_path: String,
    pub content_path: String,
    pub manifest_path: String,
    pub file_count: usize,
    pub enabled: bool,
    pub deploy_root: String,
    pub detection_method: String,
    pub installed_at_unix_seconds: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModList {
    pub mods: Vec<InstalledModSummary>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledModManifest {
    schema_version: u32,
    id: String,
    name: String,
    source_path: String,
    content_root_path: String,
    detection_method: String,
    deploy_root: String,
    installed_at_unix_seconds: u64,
    enabled: bool,
    file_count: usize,
    files: Vec<InstalledModFile>,
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

pub fn get_mod_library_status(app: &tauri::AppHandle) -> Result<ModLibraryStatus, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;

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
) -> Result<ModInstallResult, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
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

    let result = (|| {
        extract_archive_with_bundled_7zip(app, &archive_path, &staging_path)?;
        install_mod_from_folder_into_with_options(
            path_to_string(&staging_path),
            allow_game_root,
            &paths.installed_path,
            Some(archive_name),
            Some(path_to_string(&archive_path)),
        )
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging_path);
    }

    result
}

pub fn list_installed_mods(app: &tauri::AppHandle) -> Result<InstalledModList, String> {
    let paths = library_paths(app)?;
    ensure_library_directories(&paths)?;
    list_installed_mods_from(&paths.installed_path)
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

        let manifest_path = temp_mod_path.join("manifest.json");
        let manifest = InstalledModManifest {
            schema_version: 1,
            id: mod_id.clone(),
            name: mod_name.clone(),
            source_path: original_source_path.unwrap_or_else(|| preview.source_path.clone()),
            content_root_path,
            detection_method: preview.detection_method.clone(),
            deploy_root: preview.deploy_root.clone(),
            installed_at_unix_seconds: unix_seconds_now()?,
            enabled: false,
            file_count: installed_files.len(),
            files: installed_files.clone(),
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
            mod_path: path_to_string(&final_mod_path),
            content_path: path_to_string(&final_mod_path.join("content")),
            manifest_path: path_to_string(&final_mod_path.join("manifest.json")),
            file_count: installed_files.len(),
            files: installed_files,
            message: "MOD was imported into the local Acumod library.".to_string(),
        })
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&temp_mod_path);
    }

    result
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
        let candidate_dtos = build_candidate_dtos(&shallowest)?;

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

fn build_candidate_dtos(candidates: &[Candidate]) -> Result<Vec<ModImportCandidate>, String> {
    let mut dtos = Vec::new();

    for candidate in candidates {
        dtos.push(ModImportCandidate {
            root_path: path_to_string(&candidate.root_path),
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

        mods.push(InstalledModSummary {
            id: manifest.id,
            name: manifest.name,
            mod_path: path_to_string(&mod_path),
            content_path: path_to_string(&mod_path.join("content")),
            manifest_path: path_to_string(&manifest_path),
            file_count: manifest.file_count,
            enabled: manifest.enabled,
            deploy_root: manifest.deploy_root,
            detection_method: manifest.detection_method,
            installed_at_unix_seconds: manifest.installed_at_unix_seconds,
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
    staging_path: PathBuf,
    import_staging_path: PathBuf,
}

fn library_paths(_app: &tauri::AppHandle) -> Result<LibraryPaths, String> {
    let software_data_path = software_data_path()?;
    let mods_path = software_data_path.join("mods");
    let installed_path = mods_path.join("installed");
    let staging_path = mods_path.join("staging");
    let import_staging_path = staging_path.join("imports");

    Ok(LibraryPaths {
        software_data_path,
        mods_path,
        installed_path,
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

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        install_mod_from_folder_into, list_installed_mods_from, preview_mod_import,
        validate_archive_path,
    };

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
