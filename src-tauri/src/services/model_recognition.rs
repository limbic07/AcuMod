use std::{
    collections::{BTreeMap, HashSet},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

const MODEL_INDEX_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../references/mhwi-data/curated/model-index.json"
));

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReplacement {
    pub model_kind: String,
    pub sub_kind: String,
    pub model_part: String,
    pub model_id: String,
    pub game_ids: Vec<String>,
    pub variant_ids: Vec<String>,
    pub display_names: Vec<String>,
    #[serde(default)]
    pub affected_parts: Vec<String>,
    pub matched_files: Vec<String>,
    pub recognition_source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelIndex {
    weapon_models: Vec<WeaponModelEntry>,
    armor_models: Vec<ArmorModelEntry>,
    hair_models: Vec<HairModelEntry>,
    asset_models: Vec<AssetModelEntry>,
    voice_models: Vec<VoiceModelEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeaponModelEntry {
    model_path: String,
    model_part: String,
    weapon_type: String,
    weapon_ids: Vec<String>,
    display_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArmorModelEntry {
    model_path: String,
    armor_part: String,
    armor_ids: Vec<String>,
    layered_armor_ids: Vec<String>,
    display_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HairModelEntry {
    model_path: String,
    model_id: String,
    game_ids: Vec<String>,
    display_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetModelEntry {
    model_kind: String,
    sub_kind: String,
    model_part: String,
    model_path: String,
    model_id: String,
    game_ids: Vec<String>,
    variant_ids: Vec<String>,
    display_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceModelEntry {
    file_name: String,
    model_id: String,
    gender: String,
    voice_number: String,
    display_names: Vec<String>,
}

pub fn recognize_model_replacements(
    deploy_relative_paths: &[String],
) -> Result<Vec<ModelReplacement>, String> {
    let index = model_index()?;
    let normalized_files = deploy_relative_paths
        .iter()
        .map(|path| (normalize_path(path), path.clone()))
        .collect::<Vec<_>>();
    let mut replacements = Vec::new();

    for entry in &index.weapon_models {
        let matched_files =
            matching_files_in_nativepc_root(&normalized_files, &entry.model_path, |_| true);

        if matched_files.is_empty() {
            continue;
        }

        replacements.push(ModelReplacement {
            model_kind: "weapon".to_string(),
            sub_kind: entry.weapon_type.clone(),
            model_part: entry.model_part.clone(),
            model_id: entry.model_path.clone(),
            game_ids: entry.weapon_ids.clone(),
            variant_ids: Vec::new(),
            display_names: entry.display_names.clone(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }

    let mut recognized_armor_models = HashSet::new();

    for entry in &index.armor_models {
        let matched_files = matching_armor_files(&normalized_files, &entry.model_path, |path| {
            detect_armor_part(path) == Some(entry.armor_part.as_str())
        });

        if matched_files.is_empty() {
            continue;
        }

        recognized_armor_models.insert(entry.model_path.as_str());
        replacements.push(ModelReplacement {
            model_kind: "armor".to_string(),
            sub_kind: entry.armor_part.clone(),
            model_part: "model".to_string(),
            model_id: entry.model_path.clone(),
            game_ids: entry.armor_ids.clone(),
            variant_ids: entry.layered_armor_ids.clone(),
            display_names: entry.display_names.clone(),
            affected_parts: vec![entry.armor_part.clone()],
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }

    add_unknown_part_armor_matches(
        &mut replacements,
        &normalized_files,
        &index.armor_models,
        &recognized_armor_models,
    );
    add_hair_matches(&mut replacements, &normalized_files, &index.hair_models);
    add_asset_matches(&mut replacements, &normalized_files, &index.asset_models);
    add_path_pattern_asset_matches(&mut replacements, &normalized_files, &index.asset_models);
    add_unknown_slinger_matches(&mut replacements, &normalized_files, &index.asset_models);
    add_voice_matches(&mut replacements, &normalized_files, &index.voice_models);
    merge_armor_set_matches(&mut replacements);

    replacements.sort_by(|left, right| {
        model_kind_order(&left.model_kind)
            .cmp(&model_kind_order(&right.model_kind))
            .then_with(|| left.sub_kind.cmp(&right.sub_kind))
            .then_with(|| left.model_id.cmp(&right.model_id))
            .then_with(|| left.model_part.cmp(&right.model_part))
    });

    Ok(replacements)
}

fn model_index() -> Result<&'static ModelIndex, String> {
    static MODEL_INDEX: OnceLock<Result<ModelIndex, String>> = OnceLock::new();

    match MODEL_INDEX.get_or_init(|| {
        serde_json::from_str(MODEL_INDEX_JSON)
            .map_err(|error| format!("Could not parse bundled MHWI model index: {error}"))
    }) {
        Ok(index) => Ok(index),
        Err(error) => Err(error.clone()),
    }
}

fn matching_files_in_nativepc_root<F>(
    normalized_files: &[(String, String)],
    model_path: &str,
    include: F,
) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    normalized_files
        .iter()
        .filter(|(normalized, _)| {
            let Some(directory_path) = nativepc_relative_directory(normalized) else {
                return false;
            };

            directory_starts_with_path(directory_path, model_path) && include(directory_path)
        })
        .map(|(_, original)| original.clone())
        .collect()
}

fn matching_armor_files<F>(
    normalized_files: &[(String, String)],
    model_id: &str,
    include: F,
) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    normalized_files
        .iter()
        .filter(|(normalized, _)| {
            let Some(directory_path) = nativepc_relative_directory(normalized) else {
                return false;
            };

            let is_equipment_model =
                directory_starts_with_path(directory_path, &format!("pl/f_equip/{model_id}"))
                    || directory_starts_with_path(
                        directory_path,
                        &format!("pl/m_equip/{model_id}"),
                    );

            is_equipment_model && include(directory_path)
        })
        .map(|(_, original)| original.clone())
        .collect()
}

fn add_unknown_part_armor_matches<'a>(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    armor_models: &'a [ArmorModelEntry],
    recognized_armor_models: &HashSet<&'a str>,
) {
    let mut models_by_path = BTreeMap::<&str, Vec<&ArmorModelEntry>>::new();

    for entry in armor_models {
        models_by_path
            .entry(entry.model_path.as_str())
            .or_default()
            .push(entry);
    }

    for (model_path, entries) in models_by_path {
        if recognized_armor_models.contains(model_path) {
            continue;
        }

        let matched_files = matching_armor_files(normalized_files, model_path, |path| {
            detect_armor_part(path).is_none()
        });

        if matched_files.is_empty() {
            continue;
        }

        let mut game_ids = entries
            .iter()
            .flat_map(|entry| entry.armor_ids.iter().cloned())
            .collect::<Vec<_>>();
        let mut variant_ids = entries
            .iter()
            .flat_map(|entry| entry.layered_armor_ids.iter().cloned())
            .collect::<Vec<_>>();
        let mut display_names = entries
            .iter()
            .flat_map(|entry| entry.display_names.iter().cloned())
            .collect::<Vec<_>>();
        sort_and_deduplicate(&mut game_ids);
        sort_and_deduplicate(&mut variant_ids);
        sort_and_deduplicate(&mut display_names);

        replacements.push(ModelReplacement {
            model_kind: "armor".to_string(),
            sub_kind: "防具（部位未识别）".to_string(),
            model_part: "model".to_string(),
            model_id: model_path.to_string(),
            game_ids,
            variant_ids,
            display_names,
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }
}

fn add_hair_matches(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    hair_models: &[HairModelEntry],
) {
    let mut recognized_ids = HashSet::new();

    for entry in hair_models {
        let matched_files =
            matching_files_in_nativepc_root(normalized_files, &entry.model_path, |_| true);

        if matched_files.is_empty() {
            continue;
        }

        recognized_ids.insert(entry.model_id.as_str());
        replacements.push(ModelReplacement {
            model_kind: "hair".to_string(),
            sub_kind: "发型".to_string(),
            model_part: "model".to_string(),
            model_id: entry.model_id.clone(),
            game_ids: entry.game_ids.clone(),
            variant_ids: Vec::new(),
            display_names: entry.display_names.clone(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }

    let mut matches = BTreeMap::<String, Vec<String>>::new();

    for (normalized, original) in normalized_files {
        let Some(hair_id) = extract_hair_id(normalized) else {
            continue;
        };

        if recognized_ids.contains(hair_id.as_str()) {
            continue;
        }

        matches.entry(hair_id).or_default().push(original.clone());
    }

    for (hair_id, mut matched_files) in matches {
        sort_and_deduplicate(&mut matched_files);
        replacements.push(ModelReplacement {
            model_kind: "hair".to_string(),
            sub_kind: "发型".to_string(),
            model_part: "model".to_string(),
            model_id: hair_id,
            game_ids: Vec::new(),
            variant_ids: Vec::new(),
            display_names: Vec::new(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "pathPattern".to_string(),
        });
    }
}

fn add_asset_matches(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    asset_models: &[AssetModelEntry],
) {
    for entry in asset_models {
        let matched_files = matching_asset_files(normalized_files, entry);

        if matched_files.is_empty() {
            continue;
        }

        replacements.push(ModelReplacement {
            model_kind: entry.model_kind.clone(),
            sub_kind: entry.sub_kind.clone(),
            model_part: entry.model_part.clone(),
            model_id: entry.model_id.clone(),
            game_ids: entry.game_ids.clone(),
            variant_ids: entry.variant_ids.clone(),
            display_names: entry.display_names.clone(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }
}

// These resource roots are stable enough to classify, but the bundled data has no
// authoritative Chinese name for every ID. Keep the underlying ID visible instead
// of inventing a game name from an incomplete community table.
fn add_path_pattern_asset_matches(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    asset_models: &[AssetModelEntry],
) {
    let known_models = asset_models
        .iter()
        .map(|entry| (entry.model_kind.as_str(), entry.model_id.as_str()))
        .collect::<HashSet<_>>();
    let mut matches = BTreeMap::<(String, String, String), Vec<String>>::new();

    for (normalized, original) in normalized_files {
        let Some((model_kind, sub_kind, model_id)) = detect_path_pattern_asset(normalized) else {
            continue;
        };

        if known_models.contains(&(model_kind.as_str(), model_id.as_str())) {
            continue;
        }

        matches
            .entry((model_kind, sub_kind, model_id))
            .or_default()
            .push(original.clone());
    }

    for ((model_kind, sub_kind, model_id), mut matched_files) in matches {
        sort_and_deduplicate(&mut matched_files);
        replacements.push(ModelReplacement {
            model_kind,
            sub_kind: sub_kind.clone(),
            model_part: "model".to_string(),
            game_ids: vec![model_id.clone()],
            variant_ids: Vec::new(),
            display_names: vec![format!("{sub_kind} {model_id}")],
            model_id,
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "pathPattern".to_string(),
        });
    }
}

fn detect_path_pattern_asset(path: &str) -> Option<(String, String, String)> {
    let directory = nativepc_relative_directory(path)?;

    if directory_has_component(directory, "vfx") {
        return None;
    }

    let components = directory.split('/').collect::<Vec<_>>();

    for window in components.windows(2) {
        let [root, model_id] = window else {
            continue;
        };

        if matches!(*root, "m_face" | "f_face") && is_face_model_id(model_id) {
            let sub_kind = if *root == "m_face" {
                "男性脸型"
            } else {
                "女性脸型"
            };
            return Some((
                "face".to_string(),
                sub_kind.to_string(),
                format!("{root}/{model_id}"),
            ));
        }

        if *root == "em" && is_monster_model_id(model_id) {
            return Some((
                "monster".to_string(),
                "怪物".to_string(),
                (*model_id).to_string(),
            ));
        }

        if *root == "pg" && is_poogie_model_id(model_id) {
            return Some((
                "poogie".to_string(),
                "噗吱猪服装".to_string(),
                (*model_id).to_string(),
            ));
        }

        if *root == "ft" && is_furniture_model_id(model_id) {
            return Some((
                "furniture".to_string(),
                "家具".to_string(),
                (*model_id).to_string(),
            ));
        }

        if *root == "acc" && is_prefixed_numeric_id(model_id, "acc", 3) {
            return Some((
                "playerAccessory".to_string(),
                "玩家附件".to_string(),
                (*model_id).to_string(),
            ));
        }
    }

    for component in components {
        if is_prefixed_numeric_id(component, "ot_acc", 3) {
            return Some((
                "palicoAccessory".to_string(),
                "随从附件".to_string(),
                component.to_string(),
            ));
        }
    }

    None
}

fn is_face_model_id(value: &str) -> bool {
    is_prefixed_numeric_id(value, "face", 3)
}

fn is_poogie_model_id(value: &str) -> bool {
    is_prefixed_numeric_id(value, "pg", 3)
}

fn is_prefixed_numeric_id(value: &str, prefix: &str, digit_count: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == digit_count && suffix.chars().all(|value| value.is_ascii_digit())
    })
}

fn is_monster_model_id(value: &str) -> bool {
    let numeric_suffix = value
        .strip_prefix("ems")
        .or_else(|| value.strip_prefix("em"));
    let Some(numeric_suffix) = numeric_suffix else {
        return false;
    };
    let mut segments = numeric_suffix.split('_');
    let Some(base_id) = segments.next() else {
        return false;
    };

    if base_id.len() != 3 || !base_id.chars().all(|value| value.is_ascii_digit()) {
        return false;
    }

    segments.all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|value| value.is_ascii_digit() || value.is_ascii_lowercase())
    })
}

fn is_furniture_model_id(value: &str) -> bool {
    value.starts_with("ft")
        && value.len() > 2
        && value
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_')
}

fn matching_asset_files(
    normalized_files: &[(String, String)],
    entry: &AssetModelEntry,
) -> Vec<String> {
    if entry.model_path.contains('/') {
        return matching_files_in_nativepc_root(normalized_files, &entry.model_path, |_| true);
    }

    normalized_files
        .iter()
        .filter(|(normalized, _)| {
            let Some(directory_path) = nativepc_relative_directory(normalized) else {
                return false;
            };

            !directory_starts_with_path(directory_path, "vfx")
                && directory_has_component(directory_path, &entry.model_path)
        })
        .map(|(_, original)| original.clone())
        .collect()
}

fn add_unknown_slinger_matches(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    asset_models: &[AssetModelEntry],
) {
    let known_ids = asset_models
        .iter()
        .filter(|entry| entry.model_kind == "slinger")
        .map(|entry| entry.model_id.as_str())
        .collect::<HashSet<_>>();
    let mut matches = BTreeMap::<String, Vec<String>>::new();

    for (normalized, original) in normalized_files {
        let Some(slinger_id) = extract_slinger_id(normalized) else {
            continue;
        };

        if known_ids.contains(slinger_id.as_str()) {
            continue;
        }

        matches
            .entry(slinger_id)
            .or_default()
            .push(original.clone());
    }

    for (slinger_id, mut matched_files) in matches {
        sort_and_deduplicate(&mut matched_files);
        replacements.push(ModelReplacement {
            model_kind: "slinger".to_string(),
            sub_kind: "投射器".to_string(),
            model_part: "model".to_string(),
            model_id: slinger_id,
            game_ids: Vec::new(),
            variant_ids: Vec::new(),
            display_names: Vec::new(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "pathPattern".to_string(),
        });
    }
}

fn add_voice_matches(
    replacements: &mut Vec<ModelReplacement>,
    normalized_files: &[(String, String)],
    voice_models: &[VoiceModelEntry],
) {
    for entry in voice_models {
        let matched_files = normalized_files
            .iter()
            .filter(|(normalized, _)| {
                nativepc_relative_directory(normalized).is_some_and(|directory| {
                    directory_starts_with_path(directory, "sound/wwise/windows")
                }) && file_name(normalized) == entry.file_name
            })
            .map(|(_, original)| original.clone())
            .collect::<Vec<_>>();

        if matched_files.is_empty() {
            continue;
        }

        let gender = match entry.gender.as_str() {
            "female" => "女性",
            "male" => "男性",
            _ => "未知性别",
        };

        replacements.push(ModelReplacement {
            model_kind: "voice".to_string(),
            sub_kind: format!("人物语音 · {gender}"),
            model_part: "soundBank".to_string(),
            model_id: entry.model_id.clone(),
            game_ids: vec![entry.voice_number.clone()],
            variant_ids: Vec::new(),
            display_names: entry.display_names.clone(),
            affected_parts: Vec::new(),
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }
}

fn merge_armor_set_matches(replacements: &mut Vec<ModelReplacement>) {
    let mut armor_sets = BTreeMap::<String, Vec<ModelReplacement>>::new();
    let mut remaining_replacements = Vec::new();

    for replacement in std::mem::take(replacements) {
        if replacement.model_kind == "armor"
            && replacement.recognition_source == "idTable"
            && is_armor_part(&replacement.sub_kind)
        {
            armor_sets
                .entry(replacement.model_id.clone())
                .or_default()
                .push(replacement);
        } else {
            remaining_replacements.push(replacement);
        }
    }

    for (_, mut set_parts) in armor_sets {
        set_parts.sort_by_key(|replacement| armor_part_order(&replacement.sub_kind));

        if set_parts.len() == 1 {
            remaining_replacements.push(set_parts.pop().expect("armor set has one part"));
            continue;
        }

        let mut combined = set_parts.remove(0);
        let mut game_ids = combined.game_ids.clone();
        let mut variant_ids = combined.variant_ids.clone();
        let mut display_names = combined.display_names.clone();
        let mut matched_files = combined.matched_files.clone();
        let mut affected_parts = vec![combined.sub_kind.clone()];

        for part in set_parts {
            game_ids.extend(part.game_ids);
            variant_ids.extend(part.variant_ids);
            display_names.extend(part.display_names);
            matched_files.extend(part.matched_files);
            affected_parts.push(part.sub_kind);
        }

        sort_and_deduplicate(&mut game_ids);
        sort_and_deduplicate(&mut variant_ids);
        sort_and_deduplicate(&mut display_names);
        sort_and_deduplicate(&mut matched_files);
        affected_parts.sort_by_key(|part| armor_part_order(part));
        affected_parts.dedup();

        combined.sub_kind = "防具套装".to_string();
        combined.model_part = "set".to_string();
        combined.game_ids = game_ids;
        combined.variant_ids = variant_ids;
        combined.display_names = display_names;
        combined.affected_parts = affected_parts;
        combined.matched_files = matched_files;
        remaining_replacements.push(combined);
    }

    *replacements = remaining_replacements;
}

fn is_armor_part(part: &str) -> bool {
    matches!(part, "头盔" | "铠甲" | "护手" | "腰甲" | "护腿")
}

fn armor_part_order(part: &str) -> u8 {
    match part {
        "头盔" => 0,
        "铠甲" => 1,
        "护手" => 2,
        "腰甲" => 3,
        "护腿" => 4,
        _ => 5,
    }
}

fn detect_armor_part(path: &str) -> Option<&'static str> {
    const ARMOR_PART_MARKERS: &[(&str, &[&str])] = &[
        ("头盔", &["helm", "head"]),
        ("铠甲", &["body", "chest"]),
        ("护手", &["arm"]),
        ("腰甲", &["wst", "waist"]),
        ("护腿", &["leg"]),
    ];

    ARMOR_PART_MARKERS
        .iter()
        .find(|(_, markers)| {
            markers
                .iter()
                .any(|marker| directory_has_component(path, marker))
        })
        .map(|(armor_part, _)| *armor_part)
}

fn extract_hair_id(path: &str) -> Option<String> {
    let directory_components = nativepc_relative_directory(path)?
        .split('/')
        .collect::<Vec<_>>();

    directory_components.get(2).and_then(|hair_id| {
        let numeric_id = hair_id.strip_prefix("hair")?;

        (directory_components[0] == "pl"
            && directory_components[1] == "hair"
            && numeric_id.len() == 3
            && numeric_id
                .chars()
                .all(|character| character.is_ascii_digit()))
        .then(|| (*hair_id).to_string())
    })
}

fn extract_slinger_id(path: &str) -> Option<String> {
    let directory_components = nativepc_relative_directory(path)?
        .split('/')
        .collect::<Vec<_>>();
    let slinger_id = *directory_components.get(2)?;
    let numeric_id = slinger_id.strip_prefix("slg")?;
    let is_legacy_id = numeric_id.len() == 3
        && numeric_id
            .chars()
            .all(|character| character.is_ascii_digit());
    let is_full_id = numeric_id.split_once('_').is_some_and(|(model, variant)| {
        model.len() == 3
            && variant.len() == 4
            && model.chars().all(|character| character.is_ascii_digit())
            && variant.chars().all(|character| character.is_ascii_digit())
    });

    (directory_components[0] == "wp"
        && directory_components[1] == "slg"
        && (is_legacy_id || is_full_id))
        .then(|| slinger_id.to_string())
}

fn parent_directory(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(directory, _)| directory)
        .unwrap_or("")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn nativepc_relative_directory(path: &str) -> Option<&str> {
    parent_directory(path).strip_prefix("nativepc/")
}

fn directory_starts_with_path(directory_path: &str, expected_path: &str) -> bool {
    let directory_components = directory_path.split('/').collect::<Vec<_>>();
    let expected_components = expected_path.split('/').collect::<Vec<_>>();

    !expected_components.is_empty()
        && directory_components.len() >= expected_components.len()
        && directory_components[..expected_components.len()] == expected_components
}

fn directory_has_component(directory_path: &str, expected_component: &str) -> bool {
    directory_path
        .split('/')
        .any(|component| component == expected_component)
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn sort_and_deduplicate(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn model_kind_order(model_kind: &str) -> u8 {
    match model_kind {
        "weapon" => 0,
        "armor" => 1,
        "hair" => 2,
        "palicoWeapon" => 3,
        "palicoArmor" => 4,
        "kinsect" => 5,
        "pendant" => 6,
        "npc" => 7,
        "slinger" => 8,
        "voice" => 9,
        "face" => 10,
        "monster" => 11,
        "poogie" => 12,
        "furniture" => 13,
        "playerAccessory" => 14,
        "palicoAccessory" => 15,
        _ => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::recognize_model_replacements;

    #[test]
    fn recognizes_weapon_model_from_bundled_id_table() {
        let paths = vec!["nativePC/wp/swo/bs_swo001/mod/bs_swo001.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let weapon = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "weapon")
            .unwrap();

        assert_eq!(weapon.sub_kind, "太刀");
        assert_eq!(weapon.model_id, "wp/swo/bs_swo001");
        assert!(weapon.display_names.iter().any(|name| name == "铁刀1"));
    }

    #[test]
    fn recognizes_armor_model_and_part_from_bundled_id_table() {
        let paths =
            vec!["nativePC/pl/f_equip/pl001_0000/helm/mod/f_pl001_0000_helm.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let armor = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "armor")
            .unwrap();

        assert_eq!(armor.sub_kind, "头盔");
        assert_eq!(armor.model_id, "pl001_0000");
        assert!(armor.display_names.iter().any(|name| name == "皮制头饰"));
    }

    #[test]
    fn recognizes_multiple_targets_in_one_mod() {
        let paths = vec![
            "nativePC/wp/swo/bs_swo001/mod/bs_swo001.mod3".to_string(),
            "nativePC/pl/f_equip/pl001_0000/body/mod/f_pl001_0000_body.mod3".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements
            .iter()
            .any(|replacement| replacement.model_kind == "weapon"));
        assert!(replacements
            .iter()
            .any(|replacement| replacement.sub_kind == "铠甲"));
    }

    #[test]
    fn recognizes_base_hairstyle_slot_from_bundled_id_table() {
        let paths = vec!["nativePC/pl/hair/hair100/mod/hair100.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let hair = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "hair")
            .unwrap();

        assert_eq!(hair.model_id, "hair100");
        assert_eq!(hair.game_ids, ["1-1"]);
        assert_eq!(hair.display_names, ["发型 1-1"]);
        assert_eq!(hair.recognition_source, "idTable");
    }

    #[test]
    fn recognizes_named_hairstyle_from_bundled_id_table() {
        let paths = vec!["nativePC/pl/hair/hair120/mod/hair120.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let hair = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "hair")
            .unwrap();

        assert_eq!(hair.model_id, "hair120");
        assert_eq!(hair.game_ids, ["11-2"]);
        assert_eq!(hair.display_names, ["发型 11-2", "优美"]);
        assert_eq!(hair.recognition_source, "idTable");
    }

    #[test]
    fn recognizes_face_from_a_stable_resource_path() {
        let paths = vec!["nativePC/pl/f_face/face003/mod/f_face003.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let face = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "face")
            .unwrap();

        assert_eq!(face.sub_kind, "女性脸型");
        assert_eq!(face.model_id, "f_face/face003");
        assert_eq!(face.recognition_source, "pathPattern");
    }

    #[test]
    fn recognizes_monster_and_poogie_from_the_bundled_index() {
        let paths = vec![
            "nativePC/em/em100/mod/em100.mod3".to_string(),
            "nativePC/pg/pg000/mod/pg000.mod3".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let monster = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "monster")
            .unwrap();
        let poogie = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "poogie")
            .unwrap();

        assert!(monster.display_names.iter().any(|name| name == "蛮颚龙"));
        assert!(poogie.display_names.iter().any(|name| name == "回忆条纹"));
    }

    #[test]
    fn recognizes_furniture_and_accessories_without_guessing_names() {
        let paths = vec![
            "nativePC/ft/ft001_000/mod/ft001_000.mod3".to_string(),
            "nativePC/acc/acc000/mod/acc000.mod3".to_string(),
            "nativePC/otomo/ot_acc005/mod/ot_acc005.mod3".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements
            .iter()
            .any(|replacement| replacement.model_kind == "furniture"));
        assert!(replacements
            .iter()
            .any(|replacement| replacement.model_kind == "playerAccessory"));
        assert!(replacements
            .iter()
            .any(|replacement| replacement.model_kind == "palicoAccessory"));
    }

    #[test]
    fn recognizes_collaboration_hairstyle_without_inventing_a_game_id() {
        let paths = vec!["nativePC/pl/hair/hair404/mod/hair404.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let hair = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "hair")
            .unwrap();

        assert_eq!(hair.model_id, "hair404");
        assert!(hair.game_ids.is_empty());
        assert_eq!(hair.display_names, ["希里"]);
    }

    #[test]
    fn keeps_path_pattern_fallback_for_unknown_hairstyle_ids() {
        let paths = vec!["nativePC/pl/hair/hair999/mod/hair999.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let hair = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "hair")
            .unwrap();

        assert_eq!(hair.model_id, "hair999");
        assert!(hair.game_ids.is_empty());
        assert!(hair.display_names.is_empty());
        assert_eq!(hair.recognition_source, "pathPattern");
    }

    #[test]
    fn ignores_hairstyle_texture_names_outside_a_model_id_directory() {
        let paths = vec![
            "nativePC/pl/hair/hair404/mod/hair404.mod3".to_string(),
            "nativePC/pl/hair/hair_BM.tex".to_string(),
            "nativePC/pl/hair/hair_NM.tex".to_string(),
            "nativePC/pl/hair/hair_RMT.tex".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let hair_replacements = replacements
            .iter()
            .filter(|replacement| replacement.model_kind == "hair")
            .collect::<Vec<_>>();

        assert_eq!(hair_replacements.len(), 1);
        assert_eq!(hair_replacements[0].model_id, "hair404");
    }

    #[test]
    fn ignores_armor_ids_that_only_appear_in_file_names() {
        let paths =
            vec!["nativePC/pl/f_equip/unrelated/helm/mod/f_pl001_0000_helm.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements.is_empty());
    }

    #[test]
    fn recognizes_armor_sets_and_slinger_without_treating_vfx_assets_as_targets() {
        let paths = vec![
            "nativePC/pl/f_equip/pl105_0000/helm/mod/f_helm105_0000.mod3".to_string(),
            "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string(),
            "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm105_0000.mod3".to_string(),
            "nativePC/pl/f_equip/pl105_0000/wst/mod/f_wst105_0000.mod3".to_string(),
            "nativePC/pl/f_equip/pl105_0000/leg/mod/f_leg105_0000.mod3".to_string(),
            "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3".to_string(),
            "nativePC/vfx/mod/wp/caxe/caxe030/md_caxe030_016.mod3".to_string(),
            "nativePC/vfx/mod/pl/pl126_0000/md_pl126_000.mod3".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let armor_set = replacements
            .iter()
            .find(|replacement| {
                replacement.model_kind == "armor" && replacement.model_id == "pl105_0000"
            })
            .unwrap();
        let slinger = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "slinger")
            .unwrap();

        assert_eq!(armor_set.sub_kind, "防具套装");
        assert_eq!(
            armor_set.affected_parts,
            ["头盔", "铠甲", "护手", "腰甲", "护腿"]
        );
        assert!(armor_set
            .display_names
            .iter()
            .any(|name| name.contains("冰狼")));
        assert_eq!(slinger.model_id, "slg000_0000");
        assert_eq!(slinger.display_names, ["通用投射器/飞翔爪"]);
        assert!(!replacements
            .iter()
            .any(|replacement| replacement.model_kind == "weapon"));
        assert!(!replacements
            .iter()
            .any(|replacement| replacement.model_id == "pl126_0000"));
    }

    #[test]
    fn recognizes_extended_model_categories_from_directory_ids() {
        let paths = vec![
            "nativePC/otomo/wp/ot_we001/mod/ot_we001.mod3".to_string(),
            "nativePC/otomo/equip/ot001/helm/mod/ot001_helm.mod3".to_string(),
            "nativePC/wp/mus/mus001/mod/mus001.mod3".to_string(),
            "nativePC/pl/charm/charm002/mod/charm002.mod3".to_string(),
            "nativePC/npc/npc001/mod/npc001.mod3".to_string(),
            "nativePC/wp/slg/slg001/mod/slg001.mod3".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "palicoWeapon"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "橡子猫铲")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "palicoArmor"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "皮制猫头饰")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "kinsect"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "克里多隆虫1")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "pendant"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "公会奖章·铜之支援队")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "npc"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "总司令")
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "slinger"
                && replacement
                    .display_names
                    .iter()
                    .any(|name| name == "投射器")
        }));
    }

    #[test]
    fn recognizes_unknown_slinger_id_from_verified_directory_shape() {
        let paths = vec!["nativePC/wp/slg/slg999_0000/mod/slg999_0000.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let slinger = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "slinger")
            .unwrap();

        assert_eq!(slinger.model_id, "slg999_0000");
        assert_eq!(slinger.recognition_source, "pathPattern");
    }

    #[test]
    fn recognizes_character_creation_voice_number_from_exact_sound_bank_name() {
        let paths = vec![
            "nativePC/sound/wwise/Windows/pl_act_vo_f_07_m.nbnk".to_string(),
            "nativePC/sound/wwise/Windows/pl_act_vo_m_07_m.nbnk".to_string(),
        ];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "voice"
                && replacement.sub_kind == "人物语音 · 女性"
                && replacement.game_ids == ["16"]
                && replacement.display_names == ["女性语音 16 号"]
        }));
        assert!(replacements.iter().any(|replacement| {
            replacement.model_kind == "voice"
                && replacement.sub_kind == "人物语音 · 男性"
                && replacement.game_ids == ["16"]
                && replacement.display_names == ["男性语音 16 号"]
        }));
    }

    #[test]
    fn ignores_voice_file_name_outside_the_wwise_windows_directory() {
        let paths = vec!["nativePC/plugins/pl_act_vo_f_07_m.nbnk".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements.is_empty());
    }

    #[test]
    fn ignores_unrelated_mod_files() {
        let paths = vec!["nativePC/plugins/example.dll".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();

        assert!(replacements.is_empty());
    }
}
