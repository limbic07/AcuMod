use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub game_directory: Option<String>,
}

pub fn path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve app data directory: {error}"))?
        .join("config.json"))
}

pub fn load(app: &tauri::AppHandle) -> Result<AppConfig, String> {
    let config_path = path(app)?;

    if !config_path.exists() {
        return Ok(AppConfig::default());
    }

    let contents = fs::read_to_string(&config_path).map_err(|error| {
        format!(
            "Could not read config file {}: {error}",
            config_path.display()
        )
    })?;

    serde_json::from_str(&contents).map_err(|error| {
        format!(
            "Could not parse config file {}: {error}",
            config_path.display()
        )
    })
}

pub fn save(app: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let config_path = path(app)?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| "Could not resolve config directory.".to_string())?;

    fs::create_dir_all(config_dir).map_err(|error| {
        format!(
            "Could not create config directory {}: {error}",
            config_dir.display()
        )
    })?;

    let contents = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Could not serialize config: {error}"))?;

    fs::write(&config_path, contents).map_err(|error| {
        format!(
            "Could not write config file {}: {error}",
            config_path.display()
        )
    })
}
