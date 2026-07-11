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
        let matched_files = matching_files(&normalized_files, &entry.model_path, |_| true);

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
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }

    let mut recognized_armor_models = HashSet::new();

    for entry in &index.armor_models {
        let matched_files = matching_files(&normalized_files, &entry.model_path, |path| {
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
    add_unknown_slinger_matches(&mut replacements, &normalized_files, &index.asset_models);
    add_voice_matches(&mut replacements, &normalized_files, &index.voice_models);

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

fn matching_files<F>(
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
            let directory_path = parent_directory(normalized);
            directory_contains_path(directory_path, model_path) && include(directory_path)
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

        let matched_files = matching_files(normalized_files, model_path, |path| {
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
        let matched_files = matching_files(normalized_files, &entry.model_path, |_| true);

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
        let matched_files = matching_files(normalized_files, &entry.model_path, |_| true);

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
            matched_files,
            recognition_source: "idTable".to_string(),
        });
    }
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
                directory_contains_path(parent_directory(normalized), "sound/wwise/windows")
                    && file_name(normalized) == entry.file_name
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
            matched_files,
            recognition_source: "idTable".to_string(),
        });
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
    let directory_components = parent_directory(path).split('/').collect::<Vec<_>>();

    directory_components.windows(3).find_map(|components| {
        let hair_id = components[2];
        let numeric_id = hair_id.strip_prefix("hair")?;

        (components[0] == "pl"
            && components[1] == "hair"
            && numeric_id.len() == 3
            && numeric_id
                .chars()
                .all(|character| character.is_ascii_digit()))
        .then(|| hair_id.to_string())
    })
}

fn extract_slinger_id(path: &str) -> Option<String> {
    let directory_components = parent_directory(path).split('/').collect::<Vec<_>>();

    directory_components.windows(3).find_map(|components| {
        let slinger_id = components[2];
        let numeric_id = slinger_id.strip_prefix("slg")?;

        (components[0] == "wp"
            && components[1] == "slg"
            && numeric_id.len() == 3
            && numeric_id
                .chars()
                .all(|character| character.is_ascii_digit()))
        .then(|| slinger_id.to_string())
    })
}

fn parent_directory(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(directory, _)| directory)
        .unwrap_or("")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn directory_contains_path(directory_path: &str, expected_path: &str) -> bool {
    let directory_components = directory_path.split('/').collect::<Vec<_>>();
    let expected_components = expected_path.split('/').collect::<Vec<_>>();

    !expected_components.is_empty()
        && directory_components
            .windows(expected_components.len())
            .any(|components| components == expected_components)
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
        _ => 10,
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
        let paths = vec!["nativePC/wp/slg/slg999/mod/slg999.mod3".to_string()];

        let replacements = recognize_model_replacements(&paths).unwrap();
        let slinger = replacements
            .iter()
            .find(|replacement| replacement.model_kind == "slinger")
            .unwrap();

        assert_eq!(slinger.model_id, "slg999");
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
