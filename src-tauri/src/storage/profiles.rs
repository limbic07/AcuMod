use std::{collections::HashMap, fs, path::Path};

use serde::{Deserialize, Serialize};

pub const CURRENT_PROFILE_STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStore {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub active_profile_id: String,
    #[serde(default)]
    pub profiles: Vec<ProfileRecord>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRecord {
    pub id: String,
    pub name: String,
    pub created_at_unix_seconds: u64,
    #[serde(default)]
    pub enabled_mod_ids: Vec<String>,
    #[serde(default)]
    pub conflict_orders: HashMap<String, Vec<String>>,
}

fn default_schema_version() -> u32 {
    CURRENT_PROFILE_STORE_SCHEMA_VERSION
}

pub fn load(path: &Path) -> Result<Option<ProfileStore>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Could not read profile store {}: {error}", path.display()))?;
    let store = serde_json::from_str(&contents)
        .map_err(|error| format!("Could not parse profile store {}: {error}", path.display()))?;

    Ok(Some(store))
}

pub fn save(path: &Path, store: &ProfileStore) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Could not resolve profile store parent for {}.",
            path.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Could not create profile store directory {}: {error}",
            parent.display()
        )
    })?;

    let contents = serde_json::to_string_pretty(store)
        .map_err(|error| format!("Could not serialize profile store: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("Could not write profile store {}: {error}", path.display()))
}
