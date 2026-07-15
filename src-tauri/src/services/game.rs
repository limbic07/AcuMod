use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::storage::config::{self, GameTextLanguage};

const MHW_EXECUTABLE: &str = "MonsterHunterWorld.exe";
const MHW_STEAM_FOLDER: &str = "Monster Hunter World";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDirectoryStatus {
    pub path: Option<String>,
    pub is_configured: bool,
    pub is_valid: bool,
    pub message: String,
    pub executable_path: Option<String>,
    pub native_pc_path: Option<String>,
    pub config_path: String,
    pub source: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTextSettings {
    pub language: GameTextLanguage,
}

pub fn get_game_text_settings(app: &tauri::AppHandle) -> Result<GameTextSettings, String> {
    Ok(GameTextSettings {
        language: config::load(app)?.game_text_language,
    })
}

pub fn save_game_text_language(
    app: &tauri::AppHandle,
    language: GameTextLanguage,
) -> Result<GameTextSettings, String> {
    let mut app_config = config::load(app)?;
    app_config.game_text_language = language;
    config::save(app, &app_config)?;
    Ok(GameTextSettings { language })
}

pub fn get_game_directory_status(app: &tauri::AppHandle) -> Result<GameDirectoryStatus, String> {
    let app_config = config::load(app)?;
    let config_path = path_to_string(&config::path(app)?);

    match app_config.game_directory {
        Some(path) => {
            let path = PathBuf::from(path);
            Ok(build_status(&path, true, "savedConfig", config_path, None))
        }
        None => Ok(GameDirectoryStatus {
            path: None,
            is_configured: false,
            is_valid: false,
            message: "No MHW game directory has been saved yet.".to_string(),
            executable_path: None,
            native_pc_path: None,
            config_path,
            source: "savedConfig",
        }),
    }
}

pub fn detect_game_directory(app: &tauri::AppHandle) -> Result<GameDirectoryStatus, String> {
    let config_path = path_to_string(&config::path(app)?);

    for candidate in steam_mhw_candidates() {
        let status = build_status(
            &candidate,
            false,
            "autoDetection",
            config_path.clone(),
            Some("Auto-detected a valid MHW game directory."),
        );

        if status.is_valid {
            save_valid_path(app, &candidate)?;
            return Ok(GameDirectoryStatus {
                is_configured: true,
                ..status
            });
        }
    }

    Ok(GameDirectoryStatus {
        path: None,
        is_configured: false,
        is_valid: false,
        message: "Could not auto-detect MHW. Set the directory manually.".to_string(),
        executable_path: None,
        native_pc_path: None,
        config_path,
        source: "autoDetection",
    })
}

pub fn save_game_directory(
    app: &tauri::AppHandle,
    raw_path: String,
) -> Result<GameDirectoryStatus, String> {
    let path = normalize_user_path(&raw_path);
    let config_path = path_to_string(&config::path(app)?);
    let status = build_status(&path, false, "manualInput", config_path, None);

    if status.is_valid {
        save_valid_path(app, &path)?;
        Ok(GameDirectoryStatus {
            is_configured: true,
            message: "Saved MHW game directory.".to_string(),
            ..status
        })
    } else {
        Ok(status)
    }
}

fn save_valid_path(app: &tauri::AppHandle, path: &Path) -> Result<(), String> {
    let mut app_config = config::load(app)?;
    app_config.game_directory = Some(path_to_string(path));
    config::save(app, &app_config)
}

fn build_status(
    path: &Path,
    is_configured: bool,
    source: &'static str,
    config_path: String,
    valid_message: Option<&str>,
) -> GameDirectoryStatus {
    let executable_path = path.join(MHW_EXECUTABLE);
    let native_pc_path = path.join("nativePC");
    let is_valid = path.is_dir() && executable_path.is_file();

    let message = if is_valid {
        valid_message
            .unwrap_or("Valid MHW game directory.")
            .to_string()
    } else if !path.exists() {
        "Directory does not exist.".to_string()
    } else if !path.is_dir() {
        "Path is not a directory.".to_string()
    } else {
        format!("{MHW_EXECUTABLE} was not found in this directory.")
    };

    GameDirectoryStatus {
        path: Some(path_to_string(path)),
        is_configured,
        is_valid,
        message,
        executable_path: Some(path_to_string(&executable_path)),
        native_pc_path: Some(path_to_string(&native_pc_path)),
        config_path,
        source,
    }
}

fn steam_mhw_candidates() -> Vec<PathBuf> {
    let mut steam_roots = common_steam_roots();

    for root in steam_roots.clone() {
        add_steam_libraries_from_vdf(&root, &mut steam_roots);
    }

    dedupe_paths(
        steam_roots
            .into_iter()
            .map(|root| root.join("steamapps").join("common").join(MHW_STEAM_FOLDER))
            .collect(),
    )
}

fn common_steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(program_files_x86).join("Steam"));
    }

    if let Some(program_files) = env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("Steam"));
    }

    roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    roots.push(PathBuf::from(r"C:\Program Files\Steam"));

    dedupe_paths(roots)
}

fn add_steam_libraries_from_vdf(steam_root: &Path, steam_roots: &mut Vec<PathBuf>) {
    let library_file = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(contents) = fs::read_to_string(library_file) else {
        return;
    };

    for line in contents.lines() {
        if let Some(path) = parse_vdf_path_value(line) {
            steam_roots.push(PathBuf::from(path));
        }
    }
}

fn parse_vdf_path_value(line: &str) -> Option<String> {
    let values = quoted_values(line);

    if values.len() >= 2 && values[0] == "path" {
        Some(values[1].replace("\\\\", "\\"))
    } else {
        None
    }
}

fn quoted_values(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }

        if character == '\\' {
            escaped = true;
            continue;
        }

        if character == '"' {
            if in_quotes {
                values.push(current.clone());
                current.clear();
            }

            in_quotes = !in_quotes;
            continue;
        }

        if in_quotes {
            current.push(character);
        }
    }

    values
}

fn normalize_user_path(path: &str) -> PathBuf {
    PathBuf::from(path.trim().trim_matches('"'))
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for path in paths {
        let key = path_to_string(&path).to_lowercase();

        if seen.insert(key) {
            deduped.push(path);
        }
    }

    deduped
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
