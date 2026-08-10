use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use super::model_recognition::{read_evam_slinger_id, ModelReplacement};

const MODEL_INDEX_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../references/mhwi-data/curated/model-index.json"
));
const ARMOR_MENU_ORDER_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../references/mhwi-data/curated/armor-menu-order.json"
));
const MRL3_TEXTURE_ENTRY_SIZE: usize = 272;
const MRL3_TEXTURE_PATH_OFFSET: usize = 16;
const MRL3_TEXTURE_PATH_CAPACITY: usize = 256;
const ARMOR_PARTS: [&str; 5] = ["head", "body", "arm", "wst", "leg"];
const ARMOR_DAT_DEPLOY_PATH: &str = "nativePC/common/equip/armor.am_dat";
const ARMOR_DAT_MAGIC: u16 = 0x005F;
const ARMOR_DAT_HEADER_SIZE: usize = 10;
const ARMOR_DAT_ENTRY_SIZE: usize = 60;

/// 角色外观套装的已核实资源链。
///
/// 只有 `resource_gender` 明确存在时才改变资源根和文件名前缀；没有该字段的
/// 角色套装仍是专用外观，但必须保留来源 MOD 的男女资源，不能凭角色形象猜测目录。
const SPECIAL_CHARACTER_ARMOR_TARGETS: [SpecialCharacterArmorTarget; 9] = [
    SpecialCharacterArmorTarget {
        model_id: "pl069_0000",
        character_name: "隆",
        resource_gender: None,
        slinger_model_id: None,
        face_path: Some("nativePC/pl/m_face/face003"),
        hair_path: None,
        appearance_note: "该套装没有可核实的独立投射器绑定，保留来源 MOD 的原有绑定。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl070_0000",
        character_name: "樱",
        resource_gender: None,
        slinger_model_id: None,
        face_path: Some("nativePC/pl/f_face/face002"),
        hair_path: None,
        appearance_note: "该套装没有可核实的独立投射器绑定，保留来源 MOD 的原有绑定。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl118_0000",
        character_name: "杰洛特",
        resource_gender: Some("male"),
        slinger_model_id: Some("slg118_0000"),
        face_path: Some("nativePC/pl/m_face/face004"),
        hair_path: Some("nativePC/pl/hair/hair403"),
        appearance_note: "人物语音不随防具改绑自动部署。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl119_0000",
        character_name: "希里",
        resource_gender: Some("female"),
        slinger_model_id: Some("slg119_0000"),
        face_path: Some("nativePC/pl/f_face/face003"),
        hair_path: Some("nativePC/pl/hair/hair404"),
        appearance_note: "人物语音不随防具改绑自动部署。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl120_0000",
        character_name: "巴耶克",
        resource_gender: Some("male"),
        slinger_model_id: Some("slg120_0000"),
        face_path: Some("nativePC/pl/m_face/face005"),
        hair_path: None,
        appearance_note: "该套装在男女猎人身上均使用男性资源链；脸部资源不会自动搬运。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl130_0000",
        character_name: "里昂",
        resource_gender: Some("male"),
        slinger_model_id: Some("slg130_0000"),
        face_path: Some("nativePC/pl/m_face/face006"),
        hair_path: Some("nativePC/pl/hair/hair406"),
        appearance_note: "人物语音不随防具改绑自动部署。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl131_0000",
        character_name: "克莱尔",
        resource_gender: Some("female"),
        // 克莱尔的原版 EVAM 绑定复用里昂的 slg130，而不是按防具编号猜测 slg131。
        slinger_model_id: Some("slg130_0000"),
        face_path: Some("nativePC/pl/f_face/face004"),
        hair_path: Some("nativePC/pl/hair/hair407"),
        appearance_note: "人物语音不随防具改绑自动部署。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl132_0010",
        character_name: "阿尔忒弥斯",
        resource_gender: Some("female"),
        slinger_model_id: Some("slg132_0000"),
        face_path: Some("nativePC/pl/f_face/face005"),
        hair_path: None,
        appearance_note: "没有可核实的独立发型路径；不会自动移动脸部或语音资源。",
    },
    SpecialCharacterArmorTarget {
        model_id: "pl133_0000",
        character_name: "埃洛伊",
        resource_gender: None,
        slinger_model_id: Some("slg133_0000"),
        face_path: None,
        hair_path: None,
        appearance_note: "完整套装由游戏自身处理埃洛伊外观；保留来源 MOD 的男女防具资源根。",
    },
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRemapSelection {
    pub group_key: String,
    pub target_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRemapTarget {
    pub target_id: String,
    pub model_id: String,
    pub model_paths: Vec<String>,
    pub game_ids: Vec<String>,
    pub display_names: Vec<String>,
    pub affected_parts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRemapGroup {
    pub group_key: String,
    pub model_kind: String,
    pub sub_kind: String,
    pub source_model_ids: Vec<String>,
    pub source_game_ids: Vec<String>,
    pub source_display_names: Vec<String>,
    pub source_affected_parts: Vec<String>,
    pub original_target_id: Option<String>,
    pub selected_target_id: Option<String>,
    pub allows_manual_target: bool,
    pub targets: Vec<ModelRemapTarget>,
}

#[derive(Clone, Debug)]
pub struct EffectiveRemapFile {
    pub file_index: usize,
    pub deploy_relative_path: String,
    pub texture_path_rewrites: BTreeMap<String, String>,
    pub evam_slinger_rewrite: Option<EvamSlingerIdRewrite>,
    /// DAT 型防具已被标准化后，不再部署其全局防具表，避免改写来源槽位。
    pub automatic_exclusion_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvamSlingerIdRewrite {
    pub source_id: u32,
    pub target_id: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemapIndex {
    weapon_remap_targets: Vec<WeaponRemapTargetEntry>,
    armor_remap_targets: Vec<ArmorRemapTargetEntry>,
    armor_slinger_bindings: Vec<ArmorSlingerBindingEntry>,
    hair_models: Vec<HairRemapTargetEntry>,
    palico_armor_remap_targets: Vec<SimpleRemapTargetEntry>,
    slinger_remap_targets: Vec<SimpleRemapTargetEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeaponRemapTargetEntry {
    target_id: String,
    weapon_type: String,
    main_model_path: String,
    accessory_model_path: Option<String>,
    model_paths: Vec<String>,
    game_ids: Vec<String>,
    display_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArmorRemapTargetEntry {
    target_id: String,
    model_id: String,
    game_ids: Vec<String>,
    display_names: Vec<String>,
    affected_parts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArmorMenuOrderIndex {
    target_orders: HashMap<String, ArmorMenuOrderEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArmorMenuOrderEntry {
    global_order: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArmorSlingerBindingEntry {
    armor_model_id: String,
    gender: String,
    slinger_model_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HairRemapTargetEntry {
    model_path: String,
    model_id: String,
    game_ids: Vec<String>,
    display_names: Vec<String>,
    category: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimpleRemapTargetEntry {
    target_id: String,
    model_id: String,
    game_ids: Vec<String>,
    display_names: Vec<String>,
    #[serde(default)]
    affected_parts: Vec<String>,
}

#[derive(Clone)]
struct PathRemapRule {
    source_root: String,
    target_root: String,
    source_token: String,
    target_token: String,
    source_numeric_token: Option<String>,
    target_numeric_token: Option<String>,
    file_name_rules: Vec<FileNameRemapRule>,
}

#[derive(Clone)]
enum FileNameRemapRule {
    ModelToken {
        rename_outside_root: bool,
    },
    ArmorEpvBaseId {
        source_set_id: String,
        target_set_id: String,
    },
    /// 杰洛特、希里等角色套装的实际资源根固定为单一性别；根目录变化时同步修正文件前缀。
    ArmorFileGenderPrefix {
        source_gender: String,
        target_gender: String,
    },
}

struct PairedSlingerInference {
    targets: HashMap<String, String>,
    warnings: Vec<String>,
    blocking_errors: Vec<String>,
}

#[derive(Clone)]
struct ArmorDatNormalizationRule {
    source_model_id: String,
    target_model_id: String,
    /// 特殊角色目标的文件名前缀必须使用其实际资源性别，而不是来源 MOD 的性别。
    target_file_gender: Option<&'static str>,
    /// 部位到原 MOD 实际使用的三位主模型编号。记录 DAT 跨槽位编号，或被 DAT 别名指向的资源目录编号。
    part_model_ids: BTreeMap<String, String>,
}

struct SpecialCharacterArmorTarget {
    model_id: &'static str,
    character_name: &'static str,
    resource_gender: Option<&'static str>,
    slinger_model_id: Option<&'static str>,
    face_path: Option<&'static str>,
    hair_path: Option<&'static str>,
    appearance_note: &'static str,
}

#[derive(Clone, Copy)]
struct ArmorDatEntry {
    set_id: u16,
    equip_slot: u8,
    mdl_main_id: u16,
    set_group: u16,
}

fn special_character_armor_target(model_id: &str) -> Option<&'static SpecialCharacterArmorTarget> {
    SPECIAL_CHARACTER_ARMOR_TARGETS
        .iter()
        .find(|target| target.model_id.eq_ignore_ascii_case(model_id))
}

fn armor_equip_directory(gender: &str) -> &'static str {
    match gender {
        "female" => "f_equip",
        "male" => "m_equip",
        _ => unreachable!("防具资源性别只能是 female 或 male"),
    }
}

fn armor_file_gender(gender: &str) -> &'static str {
    match gender {
        "female" => "f",
        "male" => "m",
        _ => unreachable!("防具资源性别只能是 female 或 male"),
    }
}

/// 返回特殊角色套装的预览提示；只说明实际资源边界，不自动改写未知脸、头发或语音文件。
pub fn special_character_armor_target_warning(target_id: &str) -> Option<String> {
    let model_id = target_id.strip_prefix("armor:").unwrap_or(target_id);
    let target = special_character_armor_target(model_id)?;
    let resources = [target.face_path, target.hair_path]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let deployment = match target.resource_gender {
        Some(gender) => format!(
            "防具会固定部署到 {} 并同步文件名前缀",
            armor_equip_directory(gender)
        ),
        None => "防具保留来源 MOD 的男女资源根和文件名前缀".to_string(),
    };
    let slinger = target
        .slinger_model_id
        .map(|model_id| format!("配套投射器按原版绑定使用 {model_id}"))
        .unwrap_or_else(|| "没有可核实的专用投射器绑定，保留来源 MOD 的原有绑定".to_string());
    let companion_resources = if resources.is_empty() {
        "未发现可自动处理的独立脸部或发型路径".to_string()
    } else {
        format!("配套脸部/发型资源为 {}", resources.join("、"))
    };
    Some(format!(
        "{}为专用角色套装：{}；{}。{}；{}。",
        target.character_name, deployment, slinger, companion_resources, target.appearance_note
    ))
}

pub fn build_model_remap_groups(
    replacements: &[ModelReplacement],
    selections: &[ModelRemapSelection],
) -> Result<(Vec<ModelRemapGroup>, Vec<String>), String> {
    let index = remap_index()?;
    let selection_by_group = selection_map(selections)?;
    let mut groups = Vec::new();
    let mut warnings = Vec::new();

    add_weapon_groups(
        &mut groups,
        &mut warnings,
        replacements,
        &selection_by_group,
        index,
    );
    add_armor_groups(&mut groups, replacements, &selection_by_group, index)?;
    add_palico_armor_groups(&mut groups, replacements, &selection_by_group, index);
    add_hair_groups(&mut groups, replacements, &selection_by_group, index);
    add_slinger_groups(
        &mut groups,
        &mut warnings,
        replacements,
        &selection_by_group,
        index,
    )?;

    let known_group_keys = groups
        .iter()
        .map(|group| group.group_key.as_str())
        .collect::<HashSet<_>>();
    for selection in selections {
        if !known_group_keys.contains(selection.group_key.as_str()) {
            warnings.push(format!(
                "已保存的改绑分组已不在当前 MOD 中：{}",
                selection.group_key
            ));
        }
    }

    groups.sort_by(|left, right| {
        model_kind_order(&left.model_kind)
            .cmp(&model_kind_order(&right.model_kind))
            .then_with(|| left.sub_kind.cmp(&right.sub_kind))
            .then_with(|| left.group_key.cmp(&right.group_key))
    });
    Ok((groups, warnings))
}

pub fn build_effective_remap_files(
    deploy_relative_paths: &[String],
    replacements: &[ModelReplacement],
    selections: &[ModelRemapSelection],
) -> Result<Vec<EffectiveRemapFile>, String> {
    build_effective_remap_files_with_armor_dat(
        deploy_relative_paths,
        replacements,
        selections,
        None,
    )
}

/// 根据已验证的 `armor.am_dat` 生成 DAT 型防具的标准化部署路径。
///
/// 普通防具只需要从来源槽位移动到目标槽位。DAT 型 MOD 既可能让来源槽位
/// 加载另一组三位模型编号，也可能把多个来源槽位别名到 MOD 自己的目录编号；
/// 此处仅在表记录和实际核心文件同时吻合时，把该编号标准化为目标编号，并排除
/// 全局 DAT，避免原来源槽位继续被影响。
pub fn build_effective_remap_files_with_armor_dat(
    deploy_relative_paths: &[String],
    replacements: &[ModelReplacement],
    selections: &[ModelRemapSelection],
    armor_dat_bytes: Option<&[u8]>,
) -> Result<Vec<EffectiveRemapFile>, String> {
    let (groups, warnings) = build_model_remap_groups(replacements, selections)?;
    if !warnings.is_empty()
        && selections.iter().any(|selection| {
            !groups
                .iter()
                .any(|group| group.group_key == selection.group_key)
        })
    {
        return Err(warnings.join(" "));
    }
    let index = remap_index()?;
    let selection_by_group = selection_map(selections)?;
    let paired_slinger_inference =
        infer_paired_slinger_targets(replacements, &selection_by_group, index);
    if !paired_slinger_inference.blocking_errors.is_empty() {
        return Err(paired_slinger_inference.blocking_errors.join(" "));
    }

    let armor_dat_rules = build_armor_dat_normalization_rules(
        armor_dat_bytes,
        deploy_relative_paths,
        replacements,
        &groups,
    )?;

    let mut rules = Vec::new();
    for group in &groups {
        let Some(selected_target_id) = group.selected_target_id.as_deref() else {
            continue;
        };
        let target = resolve_group_target(group, selected_target_id)?;
        rules.extend(path_rules_for_group(group, &target)?);
    }

    let mut effective_paths = Vec::with_capacity(deploy_relative_paths.len());
    let mut seen_paths = HashMap::<String, usize>::new();

    for (file_index, original_path) in deploy_relative_paths.iter().enumerate() {
        let mut effective_path = normalize_deploy_path(original_path);
        for rule in &rules {
            effective_path = apply_path_rule(&effective_path, rule);
        }
        for rule in &armor_dat_rules {
            effective_path =
                apply_armor_dat_normalization_rule(original_path, &effective_path, rule);
        }

        let key = effective_path.to_lowercase();
        if let Some(previous_index) = seen_paths.insert(key, file_index) {
            return Err(format!(
                "模型改绑会让两个文件部署到同一目标：{} 和 {} -> {}",
                deploy_relative_paths[previous_index], original_path, effective_path
            ));
        }

        effective_paths.push(effective_path);
    }

    let texture_path_rewrites =
        build_texture_path_rewrites(deploy_relative_paths, &effective_paths)?;
    let evam_slinger_rewrites = build_evam_slinger_rewrites(&groups, replacements)?;
    Ok(effective_paths
        .into_iter()
        .enumerate()
        .map(|(file_index, deploy_relative_path)| EffectiveRemapFile {
            file_index,
            deploy_relative_path,
            texture_path_rewrites: texture_path_rewrites.clone(),
            evam_slinger_rewrite: evam_slinger_rewrites
                .get(&normalize_deploy_path(&deploy_relative_paths[file_index]).to_lowercase())
                .cloned(),
            automatic_exclusion_reason: (!armor_dat_rules.is_empty()
                && normalize_deploy_path(&deploy_relative_paths[file_index])
                    .eq_ignore_ascii_case(ARMOR_DAT_DEPLOY_PATH))
            .then(|| {
                "已标准化 DAT 型防具资源；不部署 armor.am_dat，避免影响来源防具槽位。".to_string()
            }),
        })
        .collect())
}

fn build_armor_dat_normalization_rules(
    armor_dat_bytes: Option<&[u8]>,
    deploy_relative_paths: &[String],
    replacements: &[ModelReplacement],
    groups: &[ModelRemapGroup],
) -> Result<Vec<ArmorDatNormalizationRule>, String> {
    let selected_armor_groups = groups
        .iter()
        .filter(|group| {
            group.model_kind == "armor"
                && group
                    .selected_target_id
                    .as_deref()
                    .is_some_and(|target_id| group.original_target_id.as_deref() != Some(target_id))
        })
        .collect::<Vec<_>>();
    if selected_armor_groups.is_empty() {
        return Ok(Vec::new());
    }

    let Some(armor_dat_bytes) = armor_dat_bytes else {
        return Ok(Vec::new());
    };
    let entries = parse_armor_dat_entries(armor_dat_bytes)?;
    let mut rules = Vec::new();

    for group in selected_armor_groups {
        let source_model_id = single_source_model_id(group)?;
        let source_set_id = armor_set_base_id(source_model_id).ok_or_else(|| {
            format!("DAT 型防具来源模型 ID 不符合 plNNN_NNNN 格式：{source_model_id}")
        })?;
        let target = resolve_group_target(
            group,
            group
                .selected_target_id
                .as_deref()
                .expect("selected armor target"),
        )?;
        let source_game_ids = parse_armor_dat_ids(&group.source_game_ids, "游戏")?;
        let source_variant_ids = source_armor_variant_ids(replacements, source_model_id)?;
        if source_game_ids.is_empty() || source_variant_ids.is_empty() {
            return Err(format!(
                "无法从防具索引确定 {source_model_id} 的 DAT 槽位，已拒绝自动标准化。"
            ));
        }

        let mut mapped_models = BTreeMap::<String, String>::new();
        for part in ARMOR_PARTS {
            let matching_entries = entries
                .iter()
                .filter(|entry| {
                    armor_dat_part_name(entry.equip_slot) == Some(part)
                        && source_game_ids.contains(&entry.set_group)
                        && source_variant_ids.contains(&entry.set_id)
                })
                .collect::<Vec<_>>();
            if matching_entries.is_empty() {
                continue;
            }
            let model_ids = matching_entries
                .iter()
                .map(|entry| entry.mdl_main_id)
                .collect::<HashSet<_>>();
            if model_ids.len() != 1 {
                return Err(format!(
                    "DAT 型防具 {source_model_id} 的 {part} 部位存在多个主模型编号，无法安全自动标准化。"
                ));
            }
            let model_id = format!("{:03}", model_ids.into_iter().next().unwrap());
            if model_id != source_set_id
                && has_armor_core_file(deploy_relative_paths, source_model_id, part, &model_id)
            {
                mapped_models.insert(part.to_string(), model_id);
            }

            // 一些 MOD 的资源目录本身就是 DAT 的目标模型号。例如绮梦花嫁把
            // 冰狼和浴场都映射到 pl106，目录和 DAT 主模型号因此同为 106。
            // 这里必须识别“其它装备槽位 -> 当前资源目录”的别名，否则改绑后
            // 会继续部署全局 DAT，令原槽位仍加载同一套 MOD。
            let has_foreign_alias = entries.iter().any(|entry| {
                armor_dat_part_name(entry.equip_slot) == Some(part)
                    && format!("{:03}", entry.mdl_main_id) == source_set_id
                    && !(source_game_ids.contains(&entry.set_group)
                        && source_variant_ids.contains(&entry.set_id))
            });
            if has_foreign_alias
                && has_armor_core_file(deploy_relative_paths, source_model_id, part, &source_set_id)
            {
                mapped_models.insert(part.to_string(), source_set_id.clone());
            }
        }

        // 只有 DAT 跨槽位资源或外部槽位别名确实命中本地核心资源时，才删除全局表。
        if !mapped_models.is_empty() {
            let target_file_gender = special_character_armor_target(&target.model_id)
                .and_then(|target| target.resource_gender)
                .map(armor_file_gender);
            rules.push(ArmorDatNormalizationRule {
                source_model_id: source_model_id.to_string(),
                target_model_id: target.model_id,
                target_file_gender,
                part_model_ids: mapped_models,
            });
        }
    }

    Ok(rules)
}

fn parse_armor_dat_entries(bytes: &[u8]) -> Result<Vec<ArmorDatEntry>, String> {
    if bytes.len() < ARMOR_DAT_HEADER_SIZE {
        return Err("armor.am_dat 文件过短，无法验证 DAT 型防具改绑。".to_string());
    }
    let magic = read_u16(bytes, 4)?;
    if magic != ARMOR_DAT_MAGIC {
        return Err(format!(
            "armor.am_dat 文件头无效：期望 {ARMOR_DAT_MAGIC:04X}，实际 {magic:04X}。"
        ));
    }
    let entry_count = read_u32(bytes, 6)? as usize;
    let entries_size = entry_count
        .checked_mul(ARMOR_DAT_ENTRY_SIZE)
        .ok_or_else(|| "armor.am_dat 记录数量溢出。".to_string())?;
    let expected_size = ARMOR_DAT_HEADER_SIZE
        .checked_add(entries_size)
        .ok_or_else(|| "armor.am_dat 文件大小溢出。".to_string())?;
    if bytes.len() != expected_size {
        return Err(format!(
            "armor.am_dat 文件大小不匹配：期望 {expected_size} 字节，实际 {} 字节。",
            bytes.len()
        ));
    }

    let mut entries = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let offset = ARMOR_DAT_HEADER_SIZE + index * ARMOR_DAT_ENTRY_SIZE;
        entries.push(ArmorDatEntry {
            set_id: read_u16(bytes, offset + 7)?,
            equip_slot: bytes[offset + 10],
            mdl_main_id: read_u16(bytes, offset + 13)?,
            set_group: read_u16(bytes, offset + 53)?,
        });
    }
    Ok(entries)
}

fn source_armor_variant_ids(
    replacements: &[ModelReplacement],
    source_model_id: &str,
) -> Result<HashSet<u16>, String> {
    let raw_ids = replacements
        .iter()
        .filter(|replacement| {
            replacement.model_kind == "armor"
                && replacement.model_id.eq_ignore_ascii_case(source_model_id)
        })
        .flat_map(|replacement| replacement.variant_ids.iter().cloned())
        .collect::<Vec<_>>();
    parse_armor_dat_ids(&raw_ids, "外观")
}

fn parse_armor_dat_ids(values: &[String], label: &str) -> Result<HashSet<u16>, String> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| format!("防具索引中的{label} ID 不是有效的无符号整数：{value}"))
        })
        .collect()
}

fn armor_dat_part_name(equip_slot: u8) -> Option<&'static str> {
    match equip_slot {
        0 => Some("head"),
        1 => Some("body"),
        2 => Some("arm"),
        3 => Some("wst"),
        4 => Some("leg"),
        _ => None,
    }
}

fn has_armor_core_file(
    deploy_relative_paths: &[String],
    source_model_id: &str,
    part: &str,
    model_id: &str,
) -> bool {
    deploy_relative_paths.iter().any(|path| {
        armor_dat_file_context(path, source_model_id).is_some_and(|(gender, actual_part)| {
            let Some(file_name) = file_name_from_deploy_path(path) else {
                return false;
            };
            actual_part == part
                && is_armor_core_resource_file(&file_name)
                && armor_core_file_name_matches(&file_name, gender, part, model_id)
        })
    })
}

fn is_armor_core_resource_file(file_name: &str) -> bool {
    file_name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "mod3" | "mrl3" | "tex" | "mdf2" | "evam" | "epv3"
        )
    })
}

fn apply_armor_dat_normalization_rule(
    original_path: &str,
    effective_path: &str,
    rule: &ArmorDatNormalizationRule,
) -> String {
    let Some((gender, part)) = armor_dat_file_context(original_path, &rule.source_model_id) else {
        return effective_path.to_string();
    };
    let Some(source_model_id) = rule.part_model_ids.get(part) else {
        return effective_path.to_string();
    };
    let Some(target_set_id) = armor_set_base_id(&rule.target_model_id) else {
        return effective_path.to_string();
    };
    // 通用路径规则可能已把 f_body106_0000 暂时改成 f_body066_0010。
    // DAT 的三位主模型号仍应以库内原文件名判定，最后统一写成目标套装号。
    let Some(file_name) = file_name_from_deploy_path(original_path) else {
        return effective_path.to_string();
    };
    let target_gender = rule.target_file_gender.unwrap_or(gender);
    let Some(rewritten_file_name) = rewrite_armor_core_file_name(
        &file_name,
        gender,
        target_gender,
        part,
        source_model_id,
        &target_set_id,
    ) else {
        return effective_path.to_string();
    };

    let mut components = normalize_deploy_path(effective_path)
        .split('/')
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(file_name_index) = components.len().checked_sub(1) {
        components[file_name_index] = rewritten_file_name;
    }
    components.join("/")
}

fn armor_dat_file_context(
    path: &str,
    source_model_id: &str,
) -> Option<(&'static str, &'static str)> {
    let normalized = normalize_deploy_path(path).to_ascii_lowercase();
    let source_model_id = source_model_id.to_ascii_lowercase();
    let components = normalized.split('/').collect::<Vec<_>>();
    components.windows(4).find_map(|window| {
        if window[0] != "pl" || window[2] != source_model_id {
            return None;
        }
        let gender = match window[1] {
            "f_equip" => "f",
            "m_equip" => "m",
            _ => return None,
        };
        ARMOR_PARTS
            .iter()
            .find(|part| **part == window[3])
            .map(|part| (gender, *part))
    })
}

fn file_name_from_deploy_path(path: &str) -> Option<String> {
    normalize_deploy_path(path)
        .rsplit('/')
        .next()
        .map(str::to_string)
}

fn armor_core_file_name_matches(file_name: &str, gender: &str, part: &str, model_id: &str) -> bool {
    rewrite_armor_core_file_name(file_name, gender, gender, part, model_id, model_id).is_some()
}

fn rewrite_armor_core_file_name(
    file_name: &str,
    source_gender: &str,
    target_gender: &str,
    part: &str,
    source_model_id: &str,
    target_model_id: &str,
) -> Option<String> {
    let prefix = format!("{source_gender}_{part}{source_model_id}");
    if !file_name
        .get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(&prefix))
    {
        return None;
    }
    let suffix = &file_name[prefix.len()..];
    // 防止把 f_body1060 或自定义长编号误识别为 f_body106。
    if suffix
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{target_gender}_{part}{target_model_id}{suffix}"))
}

pub fn rewrite_evam_slinger_id(
    source_bytes: &[u8],
    rewrite: &EvamSlingerIdRewrite,
) -> Result<Vec<u8>, String> {
    let current_id = read_evam_slinger_id(source_bytes)?;
    if current_id != rewrite.source_id {
        return Err(format!(
            "EVAM 当前绑定 ID 为 {current_id}，与预期源 ID {} 不一致。",
            rewrite.source_id
        ));
    }

    let mut output = source_bytes.to_vec();
    output[16..20].copy_from_slice(&rewrite.target_id.to_le_bytes());
    Ok(output)
}

pub fn rewrite_mrl3_texture_paths(
    source_bytes: &[u8],
    rewrites: &BTreeMap<String, String>,
) -> Result<(Vec<u8>, usize), String> {
    if rewrites.is_empty() {
        return Ok((source_bytes.to_vec(), 0));
    }
    let mut output = source_bytes.to_vec();
    let mut rewritten_count = 0;
    for (path_start, original_path) in mrl3_texture_entries(source_bytes)? {
        let path_end = path_start + MRL3_TEXTURE_PATH_CAPACITY;
        let key = normalize_resource_path(&original_path);
        let Some(target_path) = rewrites.get(&key) else {
            continue;
        };
        let target_bytes = target_path.as_bytes();
        if target_bytes.len() >= MRL3_TEXTURE_PATH_CAPACITY {
            return Err(format!("改绑后的 MRL3 贴图路径过长：{target_path}"));
        }

        output[path_start..path_end].fill(0);
        output[path_start..path_start + target_bytes.len()].copy_from_slice(target_bytes);
        rewritten_count += 1;
    }

    Ok((output, rewritten_count))
}

/// 只读提取 MRL3 材质表引用的贴图资源路径，供 MOD 依赖分析使用。
pub fn read_mrl3_texture_paths(source_bytes: &[u8]) -> Result<Vec<String>, String> {
    Ok(mrl3_texture_entries(source_bytes)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

fn mrl3_texture_entries(source_bytes: &[u8]) -> Result<Vec<(usize, String)>, String> {
    if source_bytes.len() < 32 || read_u32(source_bytes, 0)? != 5_001_805 {
        return Err("MRL3 文件头无效或暂不支持。".to_string());
    }

    let texture_count = read_u32(source_bytes, 20)? as usize;
    let texture_table_offset = read_u64(source_bytes, 24)? as usize;
    let table_size = texture_count
        .checked_mul(MRL3_TEXTURE_ENTRY_SIZE)
        .ok_or_else(|| "MRL3 贴图表大小溢出。".to_string())?;
    let table_end = texture_table_offset
        .checked_add(table_size)
        .ok_or_else(|| "MRL3 贴图表偏移溢出。".to_string())?;
    if table_end > source_bytes.len() {
        return Err("MRL3 贴图表超出文件范围。".to_string());
    }

    let mut entries = Vec::with_capacity(texture_count);
    for entry_index in 0..texture_count {
        let path_start =
            texture_table_offset + entry_index * MRL3_TEXTURE_ENTRY_SIZE + MRL3_TEXTURE_PATH_OFFSET;
        let path_end = path_start + MRL3_TEXTURE_PATH_CAPACITY;
        let slot = &source_bytes[path_start..path_end];
        let terminator = slot
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(MRL3_TEXTURE_PATH_CAPACITY);
        let path = std::str::from_utf8(&slot[..terminator])
            .map_err(|_| "MRL3 贴图路径不是有效的 UTF-8/ASCII 文本。".to_string())?;
        entries.push((path_start, path.to_string()));
    }
    Ok(entries)
}

fn build_evam_slinger_rewrites(
    groups: &[ModelRemapGroup],
    replacements: &[ModelReplacement],
) -> Result<HashMap<String, EvamSlingerIdRewrite>, String> {
    let mut rewrites = HashMap::new();

    for group in groups.iter().filter(|group| group.model_kind == "slinger") {
        let Some(selected_target_id) = group.selected_target_id.as_deref() else {
            continue;
        };
        let source_model_id = single_source_model_id(group)?;
        let source_id = slinger_numeric_id(source_model_id)?;
        let target = resolve_group_target(group, selected_target_id)?;
        let target_id = slinger_numeric_id(&target.model_id)?;

        for replacement in replacements.iter().filter(|replacement| {
            replacement.model_kind == "slinger" && replacement.model_id == source_model_id
        }) {
            for association in &replacement.associations {
                if association.recognition_source != "evamBinding" {
                    continue;
                }
                for matched_file in &association.matched_files {
                    let path_key = normalize_deploy_path(matched_file).to_lowercase();
                    let rewrite = EvamSlingerIdRewrite {
                        source_id,
                        target_id,
                    };
                    if let Some(existing) = rewrites.insert(path_key, rewrite.clone()) {
                        if existing != rewrite {
                            return Err(format!(
                                "同一个 EVAM 文件被要求改写为不同的飞翔爪 ID：{matched_file}"
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(rewrites)
}

fn remap_index() -> Result<&'static RemapIndex, String> {
    static INDEX: OnceLock<Result<RemapIndex, String>> = OnceLock::new();
    match INDEX.get_or_init(|| {
        serde_json::from_str(MODEL_INDEX_JSON)
            .map_err(|error| format!("无法解析内置 MHWI 改绑索引：{error}"))
    }) {
        Ok(index) => Ok(index),
        Err(error) => Err(error.clone()),
    }
}

fn armor_menu_order_index() -> Result<&'static ArmorMenuOrderIndex, String> {
    static INDEX: OnceLock<Result<ArmorMenuOrderIndex, String>> = OnceLock::new();
    match INDEX.get_or_init(|| {
        serde_json::from_str(ARMOR_MENU_ORDER_JSON)
            .map_err(|error| format!("无法解析内置 MHWI 防具菜单顺序：{error}"))
    }) {
        Ok(index) => Ok(index),
        Err(error) => Err(error.clone()),
    }
}

fn selection_map(selections: &[ModelRemapSelection]) -> Result<HashMap<&str, &str>, String> {
    let mut selection_by_group = HashMap::new();
    for selection in selections {
        if selection.group_key.trim().is_empty() || selection.target_id.trim().is_empty() {
            return Err("已保存的模型改绑包含空分组或空目标 ID。".to_string());
        }
        if selection_by_group
            .insert(selection.group_key.as_str(), selection.target_id.as_str())
            .is_some()
        {
            return Err(format!(
                "已保存的模型改绑包含重复分组：{}",
                selection.group_key
            ));
        }
    }
    Ok(selection_by_group)
}

fn add_weapon_groups(
    groups: &mut Vec<ModelRemapGroup>,
    warnings: &mut Vec<String>,
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) {
    let mut by_weapon_type = BTreeMap::<String, Vec<&ModelReplacement>>::new();
    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "weapon")
    {
        by_weapon_type
            .entry(replacement.sub_kind.clone())
            .or_default()
            .push(replacement);
    }

    for (weapon_type, replacements) in by_weapon_type {
        let mut main_models = replacements
            .iter()
            .filter(|replacement| replacement.model_part == "main")
            .map(|replacement| replacement.model_id.clone())
            .collect::<Vec<_>>();
        let mut accessory_models = replacements
            .iter()
            .filter(|replacement| replacement.model_part == "accessory")
            .map(|replacement| replacement.model_id.clone())
            .collect::<Vec<_>>();
        sort_and_deduplicate(&mut main_models);
        sort_and_deduplicate(&mut accessory_models);
        if main_models.len() != 1 || accessory_models.len() > 1 {
            warnings.push(format!(
                "{weapon_type}包含多个独立模型根目录，当前不能自动改绑该武器分组。"
            ));
            continue;
        }

        let main_model = main_models[0].clone();
        let accessory_model = accessory_models.first().cloned();
        let group_key = format!(
            "weapon:{weapon_type}:{main_model}|{}",
            accessory_model.as_deref().unwrap_or_default()
        );
        let targets = index
            .weapon_remap_targets
            .iter()
            .filter(|target| {
                target.weapon_type == weapon_type
                    && target.accessory_model_path.is_some() == accessory_model.is_some()
            })
            .map(weapon_target)
            .collect::<Vec<_>>();
        let original_target_id = index
            .weapon_remap_targets
            .iter()
            .find(|target| {
                target.weapon_type == weapon_type
                    && target.main_model_path == main_model
                    && target.accessory_model_path.as_deref() == accessory_model.as_deref()
            })
            .map(|target| target.target_id.clone());
        let source_model_ids = std::iter::once(main_model)
            .chain(accessory_model)
            .collect::<Vec<_>>();
        groups.push(ModelRemapGroup {
            group_key: group_key.clone(),
            model_kind: "weapon".to_string(),
            sub_kind: weapon_type,
            source_model_ids,
            source_game_ids: aggregate_values(&replacements, |replacement| &replacement.game_ids),
            source_display_names: aggregate_values(&replacements, |replacement| {
                &replacement.display_names
            }),
            source_affected_parts: Vec::new(),
            original_target_id,
            selected_target_id: selection_by_group
                .get(group_key.as_str())
                .map(|id| (*id).to_string()),
            allows_manual_target: false,
            targets,
        });
    }
}

fn add_armor_groups(
    groups: &mut Vec<ModelRemapGroup>,
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) -> Result<(), String> {
    let mut by_model = BTreeMap::<String, Vec<&ModelReplacement>>::new();
    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "armor")
    {
        by_model
            .entry(replacement.model_id.clone())
            .or_default()
            .push(replacement);
    }
    let menu_order = armor_menu_order_index()?;
    let mut targets = index
        .armor_remap_targets
        .iter()
        // 改绑入口只展示实际外观装备菜单目标。专用角色套装虽然可能没有常规菜单
        // 顺序（例如阿尔忒弥斯），仍明确允许作为受控例外展示。
        .filter(|entry| {
            menu_order.target_orders.contains_key(&entry.target_id)
                || special_character_armor_target(&entry.model_id).is_some()
        })
        .map(armor_target)
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        let left_order = menu_order
            .target_orders
            .get(&left.target_id)
            .map(|entry| entry.global_order)
            .unwrap_or(usize::MAX);
        let right_order = menu_order
            .target_orders
            .get(&right.target_id)
            .map(|entry| entry.global_order)
            .unwrap_or(usize::MAX);
        left_order
            .cmp(&right_order)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });

    for (model_id, replacements) in by_model {
        let group_key = format!("armor:{model_id}");
        groups.push(ModelRemapGroup {
            group_key: group_key.clone(),
            model_kind: "armor".to_string(),
            sub_kind: "防具套装".to_string(),
            source_model_ids: vec![model_id.clone()],
            source_game_ids: aggregate_values(&replacements, |replacement| &replacement.game_ids),
            source_display_names: aggregate_values(&replacements, |replacement| {
                &replacement.display_names
            }),
            source_affected_parts: aggregate_values(&replacements, |replacement| {
                &replacement.affected_parts
            }),
            original_target_id: Some(format!("armor:{model_id}")),
            selected_target_id: selection_by_group
                .get(group_key.as_str())
                .map(|id| (*id).to_string()),
            allows_manual_target: false,
            targets: targets.clone(),
        });
    }
    Ok(())
}

fn add_palico_armor_groups(
    groups: &mut Vec<ModelRemapGroup>,
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) {
    let mut by_model = BTreeMap::<String, Vec<&ModelReplacement>>::new();
    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "palicoArmor")
    {
        by_model
            .entry(replacement.model_id.clone())
            .or_default()
            .push(replacement);
    }
    let targets = index
        .palico_armor_remap_targets
        .iter()
        .map(simple_target)
        .collect::<Vec<_>>();

    for (model_id, replacements) in by_model {
        let group_key = format!("palicoArmor:{model_id}");
        groups.push(ModelRemapGroup {
            group_key: group_key.clone(),
            model_kind: "palicoArmor".to_string(),
            sub_kind: "随从防具".to_string(),
            source_model_ids: vec![model_id.clone()],
            source_game_ids: aggregate_values(&replacements, |replacement| &replacement.game_ids),
            source_display_names: aggregate_values(&replacements, |replacement| {
                &replacement.display_names
            }),
            source_affected_parts: replacements
                .iter()
                .map(|replacement| replacement.sub_kind.clone())
                .collect::<Vec<_>>(),
            original_target_id: Some(format!("palicoArmor:{model_id}")),
            selected_target_id: selection_by_group
                .get(group_key.as_str())
                .map(|id| (*id).to_string()),
            allows_manual_target: false,
            targets: targets.clone(),
        });
    }
}

fn add_hair_groups(
    groups: &mut Vec<ModelRemapGroup>,
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) {
    let targets = index
        .hair_models
        .iter()
        .filter(|entry| entry.category == "player")
        .map(|entry| ModelRemapTarget {
            target_id: format!("hair:{}", entry.model_id),
            model_id: entry.model_id.clone(),
            model_paths: vec![entry.model_path.clone()],
            game_ids: entry.game_ids.clone(),
            display_names: entry.display_names.clone(),
            affected_parts: Vec::new(),
        })
        .collect::<Vec<_>>();

    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "hair")
    {
        let group_key = format!("hair:{}", replacement.model_id);
        groups.push(ModelRemapGroup {
            group_key: group_key.clone(),
            model_kind: "hair".to_string(),
            sub_kind: "发型".to_string(),
            source_model_ids: vec![replacement.model_id.clone()],
            source_game_ids: replacement.game_ids.clone(),
            source_display_names: replacement.display_names.clone(),
            source_affected_parts: Vec::new(),
            original_target_id: Some(group_key.clone()),
            selected_target_id: selection_by_group
                .get(group_key.as_str())
                .map(|id| (*id).to_string()),
            allows_manual_target: false,
            targets: targets.clone(),
        });
    }
}

fn add_slinger_groups(
    groups: &mut Vec<ModelRemapGroup>,
    warnings: &mut Vec<String>,
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) -> Result<(), String> {
    let inference = infer_paired_slinger_targets(replacements, selection_by_group, index);
    warnings.extend(inference.warnings);
    warnings.extend(inference.blocking_errors.iter().cloned());
    let inferred_targets = inference.targets;
    let mut base_targets = index
        .slinger_remap_targets
        .iter()
        .map(simple_target)
        .collect::<Vec<_>>();
    for target in SPECIAL_CHARACTER_ARMOR_TARGETS {
        let Some(special_target) = special_character_slinger_target(&target) else {
            continue;
        };
        if !base_targets
            .iter()
            .any(|candidate| candidate.target_id == special_target.target_id)
        {
            base_targets.push(special_target);
        }
    }
    base_targets.sort_by(|left, right| left.target_id.cmp(&right.target_id));

    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "slinger")
    {
        let group_key = format!("slinger:{}", replacement.model_id);
        let selected_target_id = selection_by_group
            .get(group_key.as_str())
            .map(|id| (*id).to_string())
            .or_else(|| inferred_targets.get(&replacement.model_id).cloned());
        let mut targets = base_targets.clone();
        if let Some(selected_id) = selected_target_id.as_deref() {
            if !targets.iter().any(|target| target.target_id == selected_id) {
                targets.push(manual_slinger_target(selected_id)?);
            }
        }
        groups.push(ModelRemapGroup {
            group_key: group_key.clone(),
            model_kind: "slinger".to_string(),
            sub_kind: "投射器".to_string(),
            source_model_ids: vec![replacement.model_id.clone()],
            source_game_ids: replacement.game_ids.clone(),
            source_display_names: replacement.display_names.clone(),
            source_affected_parts: Vec::new(),
            original_target_id: Some(group_key),
            selected_target_id,
            allows_manual_target: true,
            targets,
        });
    }
    Ok(())
}

fn infer_paired_slinger_targets(
    replacements: &[ModelReplacement],
    selection_by_group: &HashMap<&str, &str>,
    index: &RemapIndex,
) -> PairedSlingerInference {
    let mut inference = PairedSlingerInference {
        targets: HashMap::new(),
        warnings: Vec::new(),
        blocking_errors: Vec::new(),
    };
    let slinger_replacements = replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "slinger")
        .collect::<Vec<_>>();
    let mut armor_files = BTreeMap::<String, Vec<&str>>::new();

    for replacement in replacements
        .iter()
        .filter(|replacement| replacement.model_kind == "armor")
    {
        armor_files
            .entry(replacement.model_id.clone())
            .or_default()
            .extend(replacement.matched_files.iter().map(String::as_str));
    }

    let mut proposals = BTreeMap::<String, BTreeMap<String, Vec<String>>>::new();
    for (source_armor_id, matched_files) in armor_files {
        let armor_group_key = format!("armor:{source_armor_id}");
        let Some(target_armor_id) = selection_by_group
            .get(armor_group_key.as_str())
            .and_then(|target_id| target_id.strip_prefix("armor:"))
        else {
            continue;
        };
        if source_armor_id.eq_ignore_ascii_case(target_armor_id) {
            continue;
        }

        let mut gender_has_evam = BTreeMap::<&str, bool>::new();
        for path in matched_files {
            let Some(gender) = armor_path_gender(path, &source_armor_id) else {
                continue;
            };
            let has_evam = is_armor_evam_path(path, &source_armor_id);
            gender_has_evam
                .entry(gender)
                .and_modify(|current| *current |= has_evam)
                .or_insert(has_evam);
        }

        for (gender, has_evam) in gender_has_evam {
            if has_evam {
                inference.warnings.push(format!(
                    "{}的{}资源自带 EVAM；改绑到 {} 时将保留 MOD 指定的飞翔爪绑定。",
                    source_armor_id,
                    gender_label(gender),
                    target_armor_id
                ));
                continue;
            }

            let Some(source_slinger_id) = armor_slinger_model_id(index, &source_armor_id, gender)
            else {
                inference.warnings.push(format!(
                    "原版 EVAM 表缺少 {} 的{}绑定，无法自动判断配套飞翔爪。",
                    source_armor_id,
                    gender_label(gender)
                ));
                continue;
            };
            let Some(source_slinger_id) = source_slinger_id else {
                continue;
            };
            let paired_slingers = slinger_replacements
                .iter()
                .filter(|replacement| {
                    slinger_matches_original_binding(&replacement.model_id, source_slinger_id)
                        && !replacement
                            .associations
                            .iter()
                            .any(|association| association.recognition_source == "evamBinding")
                })
                .collect::<Vec<_>>();
            if paired_slingers.is_empty() {
                continue;
            }

            // 固定资源性别的角色套装必须使用其实际 EVAM 绑定；没有固定目录的
            // 专用套装则保留来源 MOD 的性别，避免错误移动资源根。
            let target_gender = special_character_armor_target(target_armor_id)
                .and_then(|target| target.resource_gender)
                .unwrap_or(gender);
            let Some(target_slinger_id) =
                armor_slinger_model_id(index, target_armor_id, target_gender)
            else {
                inference.warnings.push(format!(
                    "原版 EVAM 表缺少目标防具 {} 的{}绑定，配套飞翔爪 {} 不会自动改绑。",
                    target_armor_id,
                    gender_label(target_gender),
                    source_slinger_id
                ));
                continue;
            };
            let Some(target_slinger_id) = target_slinger_id else {
                inference.warnings.push(format!(
                    "目标防具 {} 的{}原版 EVAM 明确没有飞翔爪，{} 将保持原路径且不会被该防具调用。",
                    target_armor_id,
                    gender_label(target_gender),
                    source_slinger_id
                ));
                continue;
            };

            for replacement in paired_slingers {
                let slinger_group_key = format!("slinger:{}", replacement.model_id);
                if selection_by_group.contains_key(slinger_group_key.as_str()) {
                    continue;
                }
                proposals
                    .entry(replacement.model_id.clone())
                    .or_default()
                    .entry(target_slinger_id.to_string())
                    .or_default()
                    .push(format!("{} {}", source_armor_id, gender_label(gender)));
            }
        }
    }

    let mut candidate_targets = BTreeMap::<String, (String, Vec<String>)>::new();
    let mut target_sources = BTreeMap::<String, Vec<String>>::new();
    for (source_slinger_id, targets) in proposals {
        if targets.len() > 1 {
            let details = targets
                .iter()
                .map(|(target, contexts)| format!("{target}（{}）", contexts.join("、")))
                .collect::<Vec<_>>()
                .join("；");
            inference.blocking_errors.push(format!(
                "配套飞翔爪 {source_slinger_id} 因男女原版绑定不同，需要同时部署到多个目标：{details}。当前不能安全地自动复制同一份资源，请分别处理男女版本。"
            ));
            continue;
        }

        let (target_slinger_id, contexts) = targets.into_iter().next().unwrap();
        if source_slinger_id.eq_ignore_ascii_case(&target_slinger_id) {
            continue;
        }
        target_sources
            .entry(target_slinger_id.clone())
            .or_default()
            .push(source_slinger_id.clone());
        candidate_targets.insert(source_slinger_id, (target_slinger_id, contexts));
    }

    let mut colliding_sources = HashSet::new();
    for (target_slinger_id, mut source_slinger_ids) in target_sources {
        source_slinger_ids.sort();
        source_slinger_ids.dedup();
        if source_slinger_ids.len() > 1 {
            colliding_sources.extend(source_slinger_ids.iter().cloned());
            inference.blocking_errors.push(format!(
                "多个配套飞翔爪会在防具改绑后写入同一目标 {}：{}。当前不能安全合并不同资源。",
                target_slinger_id,
                source_slinger_ids.join("、")
            ));
        }
    }

    for (source_slinger_id, (target_slinger_id, contexts)) in candidate_targets {
        if colliding_sources.contains(&source_slinger_id) {
            continue;
        }
        inference.warnings.push(format!(
            "根据原版 EVAM，防具改绑将自动同步配套飞翔爪：{} -> {}（{}）。",
            source_slinger_id,
            target_slinger_id,
            contexts.join("、")
        ));
        inference
            .targets
            .insert(source_slinger_id, format!("slinger:{target_slinger_id}"));
    }

    inference
}

fn armor_slinger_binding<'a>(
    index: &'a RemapIndex,
    armor_model_id: &str,
    gender: &str,
) -> Option<&'a ArmorSlingerBindingEntry> {
    index.armor_slinger_bindings.iter().find(|binding| {
        binding.armor_model_id.eq_ignore_ascii_case(armor_model_id) && binding.gender == gender
    })
}

/// 从生成索引取得原版绑定；索引尚未重建时，回退到已核实的角色套装绑定。
/// 外层 None 表示没有可确认记录，内层 None 表示原版明确不绑定投射器。
fn armor_slinger_model_id<'a>(
    index: &'a RemapIndex,
    armor_model_id: &str,
    gender: &str,
) -> Option<Option<&'a str>> {
    if let Some(binding) = armor_slinger_binding(index, armor_model_id, gender) {
        return Some(binding.slinger_model_id.as_deref());
    }
    special_character_armor_target(armor_model_id)
        .filter(|target| {
            target
                .resource_gender
                .map(|fixed| fixed == gender)
                .unwrap_or(true)
        })
        .map(|target| target.slinger_model_id.map(Some).unwrap_or(None))
}

fn armor_path_gender(path: &str, armor_model_id: &str) -> Option<&'static str> {
    let normalized = normalize_deploy_path(path).to_ascii_lowercase();
    let components = normalized.split('/').collect::<Vec<_>>();
    let armor_model_id = armor_model_id.to_ascii_lowercase();
    components.windows(3).find_map(|window| {
        if window[0] != "pl" || window[2] != armor_model_id {
            return None;
        }
        match window[1] {
            "f_equip" => Some("female"),
            "m_equip" => Some("male"),
            _ => None,
        }
    })
}

fn is_armor_evam_path(path: &str, armor_model_id: &str) -> bool {
    let normalized = normalize_deploy_path(path).to_ascii_lowercase();
    let components = normalized.split('/').collect::<Vec<_>>();
    let armor_model_id = armor_model_id.to_ascii_lowercase();
    components.windows(6).any(|window| {
        window[0] == "pl"
            && matches!(window[1], "f_equip" | "m_equip")
            && window[2] == armor_model_id
            && window[3] == "arm"
            && window[4] == "mod"
            && window[5].ends_with(".evam")
    })
}

fn slinger_matches_original_binding(actual_model_id: &str, original_model_id: &str) -> bool {
    actual_model_id.eq_ignore_ascii_case(original_model_id)
        || original_model_id
            .strip_suffix("_0000")
            .is_some_and(|legacy_id| actual_model_id.eq_ignore_ascii_case(legacy_id))
}

fn gender_label(gender: &str) -> &'static str {
    match gender {
        "female" => "女性",
        "male" => "男性",
        _ => "未知性别",
    }
}

fn weapon_target(entry: &WeaponRemapTargetEntry) -> ModelRemapTarget {
    ModelRemapTarget {
        target_id: entry.target_id.clone(),
        model_id: entry.main_model_path.clone(),
        model_paths: entry.model_paths.clone(),
        game_ids: entry.game_ids.clone(),
        display_names: entry.display_names.clone(),
        affected_parts: Vec::new(),
    }
}

fn armor_target(entry: &ArmorRemapTargetEntry) -> ModelRemapTarget {
    ModelRemapTarget {
        target_id: entry.target_id.clone(),
        model_id: entry.model_id.clone(),
        model_paths: vec![entry.model_id.clone()],
        game_ids: entry.game_ids.clone(),
        display_names: entry.display_names.clone(),
        affected_parts: entry.affected_parts.clone(),
    }
}

fn simple_target(entry: &SimpleRemapTargetEntry) -> ModelRemapTarget {
    ModelRemapTarget {
        target_id: entry.target_id.clone(),
        model_id: entry.model_id.clone(),
        model_paths: vec![entry.model_id.clone()],
        game_ids: entry.game_ids.clone(),
        display_names: entry.display_names.clone(),
        affected_parts: entry.affected_parts.clone(),
    }
}

fn special_character_slinger_target(
    target: &SpecialCharacterArmorTarget,
) -> Option<ModelRemapTarget> {
    let slinger_model_id = target.slinger_model_id?;
    Some(ModelRemapTarget {
        target_id: format!("slinger:{slinger_model_id}"),
        model_id: slinger_model_id.to_string(),
        model_paths: vec![format!("wp/slg/{slinger_model_id}")],
        game_ids: vec![slinger_model_id.to_string()],
        display_names: vec![format!("【{}】服装关联投射器", target.character_name)],
        affected_parts: Vec::new(),
    })
}

fn manual_slinger_target(target_id: &str) -> Result<ModelRemapTarget, String> {
    let model_id = target_id
        .strip_prefix("slinger:")
        .ok_or_else(|| "投射器目标 ID 必须以 slinger: 开头。".to_string())?;
    if !is_slinger_model_id(model_id) {
        return Err(format!("投射器模型 ID 无效：{model_id}"));
    }
    Ok(ModelRemapTarget {
        target_id: target_id.to_string(),
        model_id: model_id.to_string(),
        model_paths: vec![model_id.to_string()],
        game_ids: Vec::new(),
        display_names: Vec::new(),
        affected_parts: Vec::new(),
    })
}

fn resolve_group_target(
    group: &ModelRemapGroup,
    target_id: &str,
) -> Result<ModelRemapTarget, String> {
    if let Some(target) = group
        .targets
        .iter()
        .find(|target| target.target_id == target_id)
    {
        return Ok(target.clone());
    }
    if group.model_kind == "slinger" && group.allows_manual_target {
        return manual_slinger_target(target_id);
    }
    Err(format!(
        "目标 {target_id} 不适用于分组 {}。",
        group.group_key
    ))
}

fn path_rules_for_group(
    group: &ModelRemapGroup,
    target: &ModelRemapTarget,
) -> Result<Vec<PathRemapRule>, String> {
    match group.model_kind.as_str() {
        "weapon" => {
            if group.source_model_ids.len() != target.model_paths.len() {
                return Err(format!(
                    "武器主模型/附件结构不兼容：{} -> {}",
                    group.group_key, target.target_id
                ));
            }
            Ok(group
                .source_model_ids
                .iter()
                .zip(&target.model_paths)
                .map(|(source, target)| {
                    path_rule(
                        source,
                        target,
                        vec![FileNameRemapRule::ModelToken {
                            rename_outside_root: true,
                        }],
                    )
                })
                .collect())
        }
        "armor" => {
            let source_id = single_source_model_id(group)?;
            let target_id = &target.model_id;
            let source_set_id = armor_set_base_id(source_id)
                .ok_or_else(|| format!("防具模型 ID 不符合 plNNN_NNNN 格式：{source_id}"))?;
            let target_set_id = armor_set_base_id(target_id)
                .ok_or_else(|| format!("防具目标模型 ID 不符合 plNNN_NNNN 格式：{target_id}"))?;
            let forced_target_gender =
                special_character_armor_target(target_id).and_then(|target| target.resource_gender);
            Ok(["female", "male"]
                .into_iter()
                .map(|source_gender| {
                    let target_gender = forced_target_gender.unwrap_or(source_gender);
                    let mut file_name_rules = vec![
                        FileNameRemapRule::ModelToken {
                            rename_outside_root: false,
                        },
                        FileNameRemapRule::ArmorEpvBaseId {
                            source_set_id: source_set_id.clone(),
                            target_set_id: target_set_id.clone(),
                        },
                    ];
                    if source_gender != target_gender {
                        file_name_rules.push(FileNameRemapRule::ArmorFileGenderPrefix {
                            source_gender: armor_file_gender(source_gender).to_string(),
                            target_gender: armor_file_gender(target_gender).to_string(),
                        });
                    }
                    path_rule(
                        &format!("pl/{}/{}", armor_equip_directory(source_gender), source_id),
                        &format!("pl/{}/{}", armor_equip_directory(target_gender), target_id),
                        file_name_rules,
                    )
                })
                .collect())
        }
        "palicoArmor" => {
            let source_id = single_source_model_id(group)?;
            Ok(vec![path_rule(
                &format!("otomo/equip/{source_id}"),
                &format!("otomo/equip/{}", target.model_id),
                vec![FileNameRemapRule::ModelToken {
                    rename_outside_root: false,
                }],
            )])
        }
        "hair" => {
            let source_id = single_source_model_id(group)?;
            Ok(vec![path_rule(
                &format!("pl/hair/{source_id}"),
                &format!("pl/hair/{}", target.model_id),
                vec![FileNameRemapRule::ModelToken {
                    rename_outside_root: false,
                }],
            )])
        }
        "slinger" => {
            let source_id = single_source_model_id(group)?;
            Ok(vec![path_rule(
                &format!("wp/slg/{source_id}"),
                &format!("wp/slg/{}", target.model_id),
                vec![FileNameRemapRule::ModelToken {
                    rename_outside_root: false,
                }],
            )])
        }
        other => Err(format!("不支持该模型改绑类别：{other}")),
    }
}

fn path_rule(
    source_root: &str,
    target_root: &str,
    file_name_rules: Vec<FileNameRemapRule>,
) -> PathRemapRule {
    let source_token = source_root
        .rsplit('/')
        .next()
        .unwrap_or(source_root)
        .to_string();
    let target_token = target_root
        .rsplit('/')
        .next()
        .unwrap_or(target_root)
        .to_string();
    PathRemapRule {
        source_numeric_token: numeric_model_token(&source_token),
        target_numeric_token: numeric_model_token(&target_token),
        source_root: source_root.to_string(),
        target_root: target_root.to_string(),
        source_token,
        target_token,
        file_name_rules,
    }
}

fn apply_path_rule(path: &str, rule: &PathRemapRule) -> String {
    let mut components = path.split('/').map(str::to_string).collect::<Vec<_>>();
    let source_components = rule.source_root.split('/').collect::<Vec<_>>();
    let target_components = rule.target_root.split('/').collect::<Vec<_>>();
    let mut root_was_replaced = false;

    if let Some(start) = find_component_sequence(&components, &source_components) {
        components.splice(
            start..start + source_components.len(),
            target_components
                .iter()
                .map(|component| (*component).to_string()),
        );
        root_was_replaced = true;
    }

    if let Some(file_name_index) = components.len().checked_sub(1) {
        for file_name_rule in &rule.file_name_rules {
            match file_name_rule {
                FileNameRemapRule::ModelToken {
                    rename_outside_root,
                } if root_was_replaced || *rename_outside_root => {
                    let file_name = &mut components[file_name_index];
                    *file_name = replace_ascii_case_insensitive(
                        file_name,
                        &rule.source_token,
                        &rule.target_token,
                    );
                    if root_was_replaced {
                        if let (Some(source_numeric), Some(target_numeric)) = (
                            rule.source_numeric_token.as_deref(),
                            rule.target_numeric_token.as_deref(),
                        ) {
                            *file_name = replace_numeric_model_token(
                                file_name,
                                source_numeric,
                                target_numeric,
                            );
                        }
                    }
                }
                FileNameRemapRule::ArmorEpvBaseId {
                    source_set_id,
                    target_set_id,
                } if root_was_replaced => {
                    if let Some(file_name) =
                        rewrite_armor_epv_file_name(&components, source_set_id, target_set_id)
                    {
                        components[file_name_index] = file_name;
                    }
                }
                FileNameRemapRule::ArmorFileGenderPrefix {
                    source_gender,
                    target_gender,
                } if root_was_replaced => {
                    let file_name = &mut components[file_name_index];
                    *file_name =
                        rewrite_armor_file_gender_prefix(file_name, source_gender, target_gender);
                }
                _ => {}
            }
        }
    }

    components.join("/")
}

fn find_component_sequence(components: &[String], expected: &[&str]) -> Option<usize> {
    components.windows(expected.len()).position(|window| {
        window
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
    })
}

fn replace_ascii_case_insensitive(value: &str, from: &str, to: &str) -> String {
    let value_lower = value.to_ascii_lowercase();
    let from_lower = from.to_ascii_lowercase();
    let mut result = String::new();
    let mut cursor = 0;
    while let Some(relative_start) = value_lower[cursor..].find(&from_lower) {
        let start = cursor + relative_start;
        result.push_str(&value[cursor..start]);
        result.push_str(to);
        cursor = start + from.len();
    }
    result.push_str(&value[cursor..]);
    result
}

fn rewrite_armor_file_gender_prefix(
    file_name: &str,
    source_gender: &str,
    target_gender: &str,
) -> String {
    let source_prefix = format!("{source_gender}_");
    if file_name
        .get(..source_prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(&source_prefix))
    {
        format!("{target_gender}_{}", &file_name[source_prefix.len()..])
    } else {
        file_name.to_string()
    }
}

fn replace_numeric_model_token(value: &str, from: &str, to: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = String::new();
    let mut cursor = 0;
    while let Some(relative_start) = value[cursor..].find(from) {
        let start = cursor + relative_start;
        let end = start + from.len();
        let before_is_digit = start > 0 && bytes[start - 1].is_ascii_digit();
        let after_is_digit = end < bytes.len() && bytes[end].is_ascii_digit();
        if before_is_digit || after_is_digit {
            result.push_str(&value[cursor..end]);
        } else {
            result.push_str(&value[cursor..start]);
            result.push_str(to);
        }
        cursor = end;
    }
    result.push_str(&value[cursor..]);
    result
}

fn numeric_model_token(model_id: &str) -> Option<String> {
    let start = model_id.find(|character: char| character.is_ascii_digit())?;
    let token = &model_id[start..];
    (!token.is_empty()
        && token
            .chars()
            .all(|character| character.is_ascii_digit() || character == '_'))
    .then(|| token.to_string())
}

fn armor_set_base_id(model_id: &str) -> Option<String> {
    let value = model_id.strip_prefix("pl")?;
    let (set_id, variant_id) = value.split_once('_')?;
    (set_id.len() == 3
        && variant_id.len() == 4
        && set_id.chars().all(|character| character.is_ascii_digit())
        && variant_id
            .chars()
            .all(|character| character.is_ascii_digit()))
    .then(|| set_id.to_string())
}

fn rewrite_armor_epv_file_name(
    components: &[String],
    source_set_id: &str,
    target_set_id: &str,
) -> Option<String> {
    let file_name_index = components.len().checked_sub(1)?;
    let epv_directory_index = file_name_index.checked_sub(1)?;
    let part_directory_index = epv_directory_index.checked_sub(1)?;
    let file_name = components.get(file_name_index)?;
    let part = components.get(part_directory_index)?;

    if !components[epv_directory_index].eq_ignore_ascii_case("epv")
        || !ARMOR_PARTS
            .iter()
            .any(|expected_part| part.eq_ignore_ascii_case(expected_part))
    {
        return None;
    }

    for gender in ["f", "m"] {
        let expected = format!("{gender}_{part}{source_set_id}.epv3");
        if file_name.eq_ignore_ascii_case(&expected) {
            return Some(format!("{gender}_{part}{target_set_id}.epv3"));
        }
    }

    None
}

pub fn is_armor_epv_deploy_path(path: &str) -> bool {
    let normalized = normalize_deploy_path(path);
    let components = normalized
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components.len() < 6 {
        return false;
    }

    let suffix = &components[components.len() - 6..];
    suffix[0].eq_ignore_ascii_case("pl")
        && matches!(
            suffix[1].to_ascii_lowercase().as_str(),
            "f_equip" | "m_equip"
        )
        && armor_set_base_id(suffix[2]).is_some()
        && ARMOR_PARTS
            .iter()
            .any(|part| suffix[3].eq_ignore_ascii_case(part))
        && suffix[4].eq_ignore_ascii_case("epv")
        && has_extension(suffix[5], "epv3")
}

fn build_texture_path_rewrites(
    original_paths: &[String],
    effective_paths: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let mut rewrites = BTreeMap::new();
    for (original, effective) in original_paths.iter().zip(effective_paths) {
        if original.eq_ignore_ascii_case(effective) || !has_extension(original, "tex") {
            continue;
        }
        let source = deploy_path_to_resource_path(original)?;
        let target = deploy_path_to_resource_path(effective)?;
        rewrites.insert(normalize_resource_path(&source), target.replace('/', "\\"));
    }
    Ok(rewrites)
}

fn deploy_path_to_resource_path(path: &str) -> Result<String, String> {
    let normalized = normalize_deploy_path(path);
    let (root, nativepc_relative_path) = normalized
        .split_once('/')
        .ok_or_else(|| format!("MRL3 贴图不在 nativePC 下：{path}"))?;
    if !root.eq_ignore_ascii_case("nativePC") {
        return Err(format!("MRL3 贴图不在 nativePC 下：{path}"));
    }
    let (without_extension, extension) = nativepc_relative_path
        .rsplit_once('.')
        .ok_or_else(|| format!("贴图路径不是 .tex 文件：{path}"))?;
    if !extension.eq_ignore_ascii_case("tex") {
        return Err(format!("贴图路径不是 .tex 文件：{path}"));
    }
    Ok(without_extension.to_string())
}

fn normalize_deploy_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalize_resource_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn has_extension(path: &str, extension: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, actual)| actual.eq_ignore_ascii_case(extension))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "MRL3 文件头不完整。".to_string())?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "armor.am_dat 文件内容不完整。".to_string())?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "MRL3 文件头不完整。".to_string())?;
    Ok(u64::from_le_bytes(value.try_into().unwrap()))
}

fn single_source_model_id(group: &ModelRemapGroup) -> Result<&str, String> {
    if group.source_model_ids.len() != 1 {
        return Err(format!(
            "改绑分组必须恰好包含一个源模型 ID：{}",
            group.group_key
        ));
    }
    Ok(&group.source_model_ids[0])
}

fn aggregate_values<F>(replacements: &[&ModelReplacement], values: F) -> Vec<String>
where
    F: Fn(&ModelReplacement) -> &Vec<String>,
{
    let mut result = replacements
        .iter()
        .flat_map(|replacement| values(replacement).iter().cloned())
        .collect::<Vec<_>>();
    sort_and_deduplicate(&mut result);
    result
}

fn sort_and_deduplicate(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

fn is_slinger_model_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("slg") else {
        return false;
    };
    let mut parts = suffix.split('_');
    let Some(base) = parts.next() else {
        return false;
    };
    if base.len() != 3 || !base.chars().all(|character| character.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(variant), None) => {
            variant.len() == 4 && variant.chars().all(|character| character.is_ascii_digit())
        }
        _ => false,
    }
}

fn slinger_numeric_id(value: &str) -> Result<u32, String> {
    if !is_slinger_model_id(value) {
        return Err(format!("投射器模型 ID 无效：{value}"));
    }
    value
        .strip_prefix("slg")
        .and_then(|suffix| suffix.split('_').next())
        .and_then(|numeric| numeric.parse::<u32>().ok())
        .ok_or_else(|| format!("无法读取投射器模型编号：{value}"))
}

fn model_kind_order(model_kind: &str) -> usize {
    match model_kind {
        "weapon" => 0,
        "armor" => 1,
        "palicoArmor" => 2,
        "slinger" => 3,
        "hair" => 4,
        _ => usize::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::model_recognition::ModelAssociation;

    fn replacement(kind: &str, sub_kind: &str, part: &str, model_id: &str) -> ModelReplacement {
        ModelReplacement {
            model_kind: kind.to_string(),
            sub_kind: sub_kind.to_string(),
            model_part: part.to_string(),
            model_id: model_id.to_string(),
            game_ids: Vec::new(),
            variant_ids: Vec::new(),
            display_names: vec![model_id.to_string()],
            affected_parts: Vec::new(),
            associations: Vec::new(),
            matched_files: Vec::new(),
            recognition_source: "idTable".to_string(),
        }
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

    fn dat_armor_replacement() -> ModelReplacement {
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        // 与实际模型索引一致：冰狼来源槽位为游戏 300、外观 250。
        armor.game_ids = vec!["300".to_string()];
        armor.variant_ids = vec!["250".to_string()];
        armor
    }

    fn dat_alias_armor_replacement() -> ModelReplacement {
        let mut armor = replacement("armor", "防具套装", "set", "pl106_0000");
        // 浴场自身的来源槽位为游戏 512、外观 462；冰狼会被 DAT 额外别名到 106。
        armor.game_ids = vec!["512".to_string()];
        armor.variant_ids = vec!["462".to_string()];
        armor
    }

    fn armor_dat_bytes(entries: &[(u16, u16, u8, u16)]) -> Vec<u8> {
        let mut bytes = vec![0; ARMOR_DAT_HEADER_SIZE + entries.len() * ARMOR_DAT_ENTRY_SIZE];
        bytes[4..6].copy_from_slice(&ARMOR_DAT_MAGIC.to_le_bytes());
        bytes[6..10].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (index, (set_id, set_group, equip_slot, model_id)) in entries.iter().enumerate() {
            let offset = ARMOR_DAT_HEADER_SIZE + index * ARMOR_DAT_ENTRY_SIZE;
            bytes[offset + 7..offset + 9].copy_from_slice(&set_id.to_le_bytes());
            bytes[offset + 10] = *equip_slot;
            bytes[offset + 13..offset + 15].copy_from_slice(&model_id.to_le_bytes());
            bytes[offset + 53..offset + 55].copy_from_slice(&set_group.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn voice_recognition_never_creates_a_remap_group() {
        let replacements = vec![replacement(
            "voice",
            "人物语音 · 女性",
            "bank",
            "pl_act_vo_f_07_m",
        )];
        let (groups, _) = build_model_remap_groups(&replacements, &[]).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn armor_remap_targets_follow_the_game_menu_order() {
        let replacements = vec![replacement("armor", "防具套装", "set", "pl001_0000")];
        let (groups, _) = build_model_remap_groups(&replacements, &[]).unwrap();
        let target_ids = groups[0]
            .targets
            .iter()
            .take(5)
            .map(|target| target.target_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            target_ids,
            [
                "armor:pl067_0000",
                "armor:pl066_0000",
                "armor:pl066_0010",
                "armor:pl018_0000",
                "armor:pl036_0000",
            ]
        );
        for hidden_target in [
            "armor:pl019_0000",
            "armor:pl057_0000",
            "armor:pl057_0010",
            "armor:pl068_0000",
            "armor:pl068_0010",
            "armor:pl068_0020",
        ] {
            assert!(!groups[0]
                .targets
                .iter()
                .any(|target| target.target_id == hidden_target));
        }
        assert!(groups[0]
            .targets
            .iter()
            .any(|target| target.target_id == "armor:pl132_0010"));
    }

    #[test]
    fn remaps_armor_root_and_verified_file_tokens() {
        let replacements = vec![replacement("armor", "防具套装", "set", "pl105_0000")];
        let files = vec![
            "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string(),
            "nativePC/pl/f_equip/HXS/body_BML.tex".to_string(),
        ];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl001_0000".to_string(),
        }];
        let effective = build_effective_remap_files(&files, &replacements, &selections).unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl001_0000/body/mod/f_body001_0000.mod3"
        );
        assert_eq!(effective[1].deploy_relative_path, files[1]);
    }

    #[test]
    fn dat_armor_remap_standardizes_verified_core_files_and_excludes_dat() {
        let replacements = vec![dat_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl105_0000/head/mod/f_head106_0000.mod3",
            "nativePC/pl/f_equip/pl105_0000/body/mod/f_body106_0000.mod3",
            "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm106_0000.mod3",
            "nativePC/pl/f_equip/pl105_0000/wst/mod/f_wst106_0000.mod3",
            "nativePC/pl/f_equip/pl105_0000/leg/mod/f_leg106_0000.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[
            (250, 300, 0, 106),
            (250, 300, 1, 106),
            (250, 300, 2, 106),
            (250, 300, 3, 106),
            (250, 300, 4, 106),
        ]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl067_0000".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        for (file, part) in effective.iter().zip(["head", "body", "arm", "wst", "leg"]) {
            assert_eq!(
                file.deploy_relative_path,
                format!("nativePC/pl/f_equip/pl067_0000/{part}/mod/f_{part}067_0000.mod3")
            );
        }
        assert!(effective[5].automatic_exclusion_reason.is_some());
    }

    #[test]
    fn dat_alias_remap_excludes_dat_and_normalizes_same_number_resource_root() {
        let replacements = vec![dat_alias_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl106_0000/head/mod/f_head106_0000.mod3",
            "nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3",
            "nativePC/pl/f_equip/pl106_0000/arm/mod/f_arm106_0000.mod3",
            "nativePC/pl/f_equip/pl106_0000/wst/mod/f_wst106_0000.mod3",
            "nativePC/pl/f_equip/pl106_0000/leg/mod/f_leg106_0000.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[
            // 浴场自己的表项，主模型号与目录同为 106。
            (462, 512, 0, 106),
            (462, 512, 1, 106),
            (462, 512, 2, 106),
            (462, 512, 3, 106),
            (462, 512, 4, 106),
            // 冰狼额外被映射到同一份 pl106 资源。
            (250, 300, 0, 106),
            (250, 300, 1, 106),
            (250, 300, 2, 106),
            (250, 300, 3, 106),
            (250, 300, 4, 106),
        ]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl106_0000".to_string(),
            target_id: "armor:pl066_0010".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        for (file, part) in effective.iter().zip(["head", "body", "arm", "wst", "leg"]) {
            assert_eq!(
                file.deploy_relative_path,
                format!("nativePC/pl/f_equip/pl066_0010/{part}/mod/f_{part}066_0000.mod3")
            );
        }
        assert!(effective[5].automatic_exclusion_reason.is_some());
    }

    #[test]
    fn dat_alias_remap_uses_special_character_resource_gender() {
        let replacements = vec![dat_alias_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[(462, 512, 1, 106), (250, 300, 1, 106)]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl106_0000".to_string(),
            target_id: "armor:pl118_0000".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/m_equip/pl118_0000/body/mod/m_body118_0000.mod3"
        );
        assert!(effective[1].automatic_exclusion_reason.is_some());
    }

    #[test]
    fn dat_self_mapping_without_foreign_alias_keeps_dat() {
        let replacements = vec![dat_alias_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[(462, 512, 1, 106)]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl106_0000".to_string(),
            target_id: "armor:pl067_0000".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        assert!(effective[1].automatic_exclusion_reason.is_none());
    }

    #[test]
    fn dat_armor_remap_only_normalizes_parts_with_verified_core_files() {
        let replacements = vec![dat_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl105_0000/head/mod/f_head106_0000.mod3",
            "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[(250, 300, 0, 106), (250, 300, 1, 106)]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl067_0000".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl067_0000/head/mod/f_head067_0000.mod3"
        );
        assert_eq!(
            effective[1].deploy_relative_path,
            "nativePC/pl/f_equip/pl067_0000/body/mod/f_body067_0000.mod3"
        );
        assert!(effective[2].automatic_exclusion_reason.is_some());
    }

    #[test]
    fn dat_armor_remap_keeps_dat_when_no_cross_slot_core_file_is_verified() {
        let replacements = vec![dat_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl105_0000/body/mod/custom_body.mod3",
            "nativePC/common/equip/armor.am_dat",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let dat = armor_dat_bytes(&[(250, 300, 1, 106)]);
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl067_0000".to_string(),
        }];

        let effective = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&dat),
        )
        .unwrap();

        assert!(effective[1].automatic_exclusion_reason.is_none());
    }

    #[test]
    fn dat_armor_remap_rejects_invalid_dat_before_changing_paths() {
        let replacements = vec![dat_armor_replacement()];
        let files = [
            "nativePC/pl/f_equip/pl105_0000/body/mod/f_body106_0000.mod3".to_string(),
            "nativePC/common/equip/armor.am_dat".to_string(),
        ];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl067_0000".to_string(),
        }];

        let error = build_effective_remap_files_with_armor_dat(
            &files,
            &replacements,
            &selections,
            Some(&[0; ARMOR_DAT_HEADER_SIZE]),
        )
        .unwrap_err();
        assert!(error.contains("文件头无效"));
    }

    #[test]
    fn remaps_armor_epv_files_by_set_id_for_each_part() {
        let replacements = vec![replacement("armor", "防具套装", "set", "pl105_0000")];
        let files = [
            "nativePC/pl/f_equip/pl105_0000/head/epv/f_head105.epv3",
            "nativePC/pl/f_equip/pl105_0000/body/epv/f_body105.epv3",
            "nativePC/pl/f_equip/pl105_0000/arm/epv/f_arm105.epv3",
            "nativePC/pl/f_equip/pl105_0000/wst/epv/f_wst105.epv3",
            "nativePC/pl/f_equip/pl105_0000/leg/epv/f_leg105.epv3",
            "nativePC/pl/m_equip/pl105_0000/body/epv/m_body105.epv3",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl001_0000".to_string(),
        }];

        let effective = build_effective_remap_files(&files, &replacements, &selections).unwrap();
        let expected = [
            "nativePC/pl/f_equip/pl001_0000/head/epv/f_head001.epv3",
            "nativePC/pl/f_equip/pl001_0000/body/epv/f_body001.epv3",
            "nativePC/pl/f_equip/pl001_0000/arm/epv/f_arm001.epv3",
            "nativePC/pl/f_equip/pl001_0000/wst/epv/f_wst001.epv3",
            "nativePC/pl/f_equip/pl001_0000/leg/epv/f_leg001.epv3",
            "nativePC/pl/m_equip/pl001_0000/body/epv/m_body001.epv3",
        ];

        for (file, expected_path) in effective.iter().zip(expected) {
            assert_eq!(file.deploy_relative_path, expected_path);
        }
    }

    #[test]
    fn special_character_targets_force_their_resource_gender_and_file_prefix() {
        let female_source = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3";
        let male_source = "nativePC/pl/m_equip/pl105_0000/body/epv/m_body105.epv3";
        let replacements = vec![replacement("armor", "防具套装", "set", "pl105_0000")];

        let geralt = build_effective_remap_files(
            &[female_source.to_string()],
            &replacements,
            &[ModelRemapSelection {
                group_key: "armor:pl105_0000".to_string(),
                target_id: "armor:pl118_0000".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            geralt[0].deploy_relative_path,
            "nativePC/pl/m_equip/pl118_0000/body/mod/m_body118_0000.mod3"
        );

        let ciri = build_effective_remap_files(
            &[male_source.to_string()],
            &replacements,
            &[ModelRemapSelection {
                group_key: "armor:pl105_0000".to_string(),
                target_id: "armor:pl119_0000".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            ciri[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl119_0000/body/epv/f_body119.epv3"
        );

        let bayek = build_effective_remap_files(
            &[female_source.to_string()],
            &replacements,
            &[ModelRemapSelection {
                group_key: "armor:pl105_0000".to_string(),
                target_id: "armor:pl120_0000".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            bayek[0].deploy_relative_path,
            "nativePC/pl/m_equip/pl120_0000/body/mod/m_body120_0000.mod3"
        );

        let claire = build_effective_remap_files(
            &[male_source.to_string()],
            &replacements,
            &[ModelRemapSelection {
                group_key: "armor:pl105_0000".to_string(),
                target_id: "armor:pl131_0000".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            claire[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl131_0000/body/epv/f_body131.epv3"
        );

        let artemis = build_effective_remap_files(
            &[male_source.to_string()],
            &replacements,
            &[ModelRemapSelection {
                group_key: "armor:pl105_0000".to_string(),
                target_id: "armor:pl132_0010".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(
            artemis[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl132_0010/body/epv/f_body132.epv3"
        );
    }

    #[test]
    fn special_character_targets_without_fixed_resource_gender_preserve_source_paths() {
        let female_source = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3";
        let male_source = "nativePC/pl/m_equip/pl105_0000/body/mod/m_body105_0000.mod3";
        let replacements = vec![replacement("armor", "防具套装", "set", "pl105_0000")];

        for (target_id, source_path, expected_path) in [
            (
                "armor:pl069_0000",
                female_source,
                "nativePC/pl/f_equip/pl069_0000/body/mod/f_body069_0000.mod3",
            ),
            (
                "armor:pl070_0000",
                male_source,
                "nativePC/pl/m_equip/pl070_0000/body/mod/m_body070_0000.mod3",
            ),
            (
                "armor:pl133_0000",
                male_source,
                "nativePC/pl/m_equip/pl133_0000/body/mod/m_body133_0000.mod3",
            ),
        ] {
            let effective = build_effective_remap_files(
                &[source_path.to_string()],
                &replacements,
                &[ModelRemapSelection {
                    group_key: "armor:pl105_0000".to_string(),
                    target_id: target_id.to_string(),
                }],
            )
            .unwrap();
            assert_eq!(effective[0].deploy_relative_path, expected_path);
        }
    }

    #[test]
    fn special_character_target_infers_fixed_slinger_binding_when_index_is_stale() {
        let armor_path = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string();
        let slinger_path = "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3".to_string();
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        armor.matched_files = vec![armor_path.clone()];
        let mut slinger = replacement("slinger", "投射器", "model", "slg000_0000");
        slinger.matched_files = vec![slinger_path.clone()];
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl118_0000".to_string(),
        }];

        let (groups, warnings) = build_model_remap_groups(&replacements, &selections).unwrap();
        let slinger_group = groups
            .iter()
            .find(|group| group.group_key == "slinger:slg000_0000")
            .unwrap();
        assert_eq!(
            slinger_group.selected_target_id.as_deref(),
            Some("slinger:slg118_0000")
        );
        assert!(slinger_group
            .targets
            .iter()
            .any(|target| target.target_id == "slinger:slg118_0000"));
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("slg000_0000 -> slg118_0000")));

        let effective =
            build_effective_remap_files(&[armor_path, slinger_path], &replacements, &selections)
                .unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/m_equip/pl118_0000/body/mod/m_body118_0000.mod3"
        );
        assert_eq!(
            effective[1].deploy_relative_path,
            "nativePC/wp/slg/slg118_0000/mod/slg118_0000.mod3"
        );
    }

    #[test]
    fn claire_special_target_uses_her_verified_shared_leon_slinger_binding() {
        let armor_path = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string();
        let slinger_path = "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3".to_string();
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        armor.matched_files = vec![armor_path.clone()];
        let mut slinger = replacement("slinger", "投射器", "model", "slg000_0000");
        slinger.matched_files = vec![slinger_path.clone()];
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl131_0000".to_string(),
        }];

        let (groups, _) = build_model_remap_groups(&replacements, &selections).unwrap();
        let slinger_group = groups
            .iter()
            .find(|group| group.group_key == "slinger:slg000_0000")
            .unwrap();
        assert_eq!(
            slinger_group.selected_target_id.as_deref(),
            Some("slinger:slg130_0000")
        );

        let effective =
            build_effective_remap_files(&[armor_path, slinger_path], &replacements, &selections)
                .unwrap();
        assert_eq!(
            effective[1].deploy_relative_path,
            "nativePC/wp/slg/slg130_0000/mod/slg130_0000.mod3"
        );
    }

    #[test]
    fn special_character_target_warning_explains_the_full_special_target_table() {
        let warning = special_character_armor_target_warning("armor:pl118_0000").unwrap();
        assert!(warning.contains("人物语音不随防具改绑自动部署"));
        assert!(warning.contains("m_face/face004"));
        assert!(special_character_armor_target_warning("armor:pl069_0000")
            .unwrap()
            .contains("保留来源 MOD 的男女资源根"));
        assert!(special_character_armor_target_warning("armor:pl133_0000")
            .unwrap()
            .contains("slg133_0000"));
        assert!(special_character_armor_target_warning("armor:pl132_0010")
            .unwrap()
            .contains("f_face/face005"));
        assert!(special_character_armor_target_warning("armor:pl105_0000").is_none());
    }

    #[test]
    fn armor_epv_uses_target_set_id_without_target_variant() {
        let replacements = vec![replacement("armor", "防具套装", "set", "pl106_0000")];
        let file = "nativePC/pl/f_equip/pl106_0000/body/epv/f_body106.epv3".to_string();
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl106_0000".to_string(),
            target_id: "armor:pl027_0010".to_string(),
        }];

        let effective = build_effective_remap_files(&[file], &replacements, &selections).unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl027_0010/body/epv/f_body027.epv3"
        );
    }

    #[test]
    fn armor_epv_rewrite_keeps_nonstandard_epv_file_names() {
        let replacements = vec![replacement("armor", "防具套装", "set", "pl105_0000")];
        let file = "nativePC/pl/f_equip/pl105_0000/body/epv/f_body105_alternate.epv3".to_string();
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl001_0000".to_string(),
        }];

        let effective = build_effective_remap_files(&[file], &replacements, &selections).unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl001_0000/body/epv/f_body105_alternate.epv3"
        );
    }

    #[test]
    fn recognizes_armor_epv_paths_for_remap_confirmation() {
        assert!(is_armor_epv_deploy_path(
            "nativePC/pl/f_equip/pl106_0000/body/epv/f_body106.epv3"
        ));
        assert!(is_armor_epv_deploy_path(
            "nativePC/pl/m_equip/pl027_0010/leg/epv/custom_leg_effect.epv3"
        ));
        assert!(!is_armor_epv_deploy_path(
            "nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3"
        ));
        assert!(!is_armor_epv_deploy_path(
            "nativePC/vfx/efx/EXX/body_effect.epv3"
        ));
    }

    #[test]
    fn armor_remap_uses_original_evam_table_for_paired_slinger_without_mod_evam() {
        let armor_path = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string();
        let slinger_path = "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3".to_string();
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        armor.matched_files = vec![armor_path.clone()];
        let mut slinger = replacement("slinger", "投射器", "model", "slg000_0000");
        slinger.matched_files = vec![slinger_path.clone()];
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl106_0000".to_string(),
        }];

        let (groups, warnings) = build_model_remap_groups(&replacements, &selections).unwrap();
        let slinger_group = groups
            .iter()
            .find(|group| group.group_key == "slinger:slg000_0000")
            .unwrap();
        assert_eq!(
            slinger_group.selected_target_id.as_deref(),
            Some("slinger:slg106_0000")
        );
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("slg000_0000 -> slg106_0000")));

        let effective =
            build_effective_remap_files(&[armor_path, slinger_path], &replacements, &selections)
                .unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl106_0000/body/mod/f_body106_0000.mod3"
        );
        assert_eq!(
            effective[1].deploy_relative_path,
            "nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3"
        );
    }

    #[test]
    fn armor_remap_preserves_custom_slinger_when_mod_provides_evam() {
        let evam_path = "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm105_0000.evam".to_string();
        let slinger_path = "nativePC/wp/slg/slg128_0000/mod/slg128_0000.mod3".to_string();
        let mut armor = replacement("armor", "护手", "model", "pl105_0000");
        armor.matched_files = vec![evam_path.clone()];
        let mut slinger = replacement("slinger", "投射器", "model", "slg128_0000");
        slinger.matched_files = vec![slinger_path.clone()];
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl106_0000".to_string(),
        }];

        let (groups, warnings) = build_model_remap_groups(&replacements, &selections).unwrap();
        let slinger_group = groups
            .iter()
            .find(|group| group.group_key == "slinger:slg128_0000")
            .unwrap();
        assert_eq!(slinger_group.selected_target_id, None);
        assert!(warnings.iter().any(|warning| warning.contains("自带 EVAM")));

        let effective = build_effective_remap_files(
            &[evam_path, slinger_path.clone()],
            &replacements,
            &selections,
        )
        .unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/pl/f_equip/pl106_0000/arm/mod/f_arm106_0000.evam"
        );
        assert_eq!(effective[1].deploy_relative_path, slinger_path);
        assert!(effective[0].evam_slinger_rewrite.is_none());
    }

    #[test]
    fn armor_remap_does_not_reuse_slinger_bound_by_another_mod_evam() {
        let armor_path = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string();
        let other_evam_path =
            "nativePC/pl/f_equip/pl001_0000/arm/mod/f_arm001_0000.evam".to_string();
        let slinger_path = "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3".to_string();
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        armor.matched_files = vec![armor_path.clone()];
        let mut slinger = replacement("slinger", "投射器", "model", "slg000_0000");
        slinger.matched_files = vec![slinger_path.clone()];
        slinger.associations.push(ModelAssociation {
            model_kind: "armor".to_string(),
            model_id: "pl001_0000".to_string(),
            display_names: vec!["pl001_0000".to_string()],
            matched_files: vec![other_evam_path.clone()],
            recognition_source: "evamBinding".to_string(),
        });
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl106_0000".to_string(),
        }];

        let effective = build_effective_remap_files(
            &[armor_path, slinger_path.clone(), other_evam_path],
            &replacements,
            &selections,
        )
        .unwrap();
        assert_eq!(effective[1].deploy_relative_path, slinger_path);
        assert!(effective[2].evam_slinger_rewrite.is_none());
    }

    #[test]
    fn armor_remap_rejects_one_slinger_that_needs_two_gender_targets() {
        let female_path = "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3".to_string();
        let male_path = "nativePC/pl/m_equip/pl105_0000/body/mod/m_body105_0000.mod3".to_string();
        let mut armor = replacement("armor", "防具套装", "set", "pl105_0000");
        armor.matched_files = vec![female_path.clone(), male_path.clone()];
        let slinger = replacement("slinger", "投射器", "model", "slg000_0000");
        let replacements = vec![armor, slinger];
        let selections = vec![ModelRemapSelection {
            group_key: "armor:pl105_0000".to_string(),
            target_id: "armor:pl019_0000".to_string(),
        }];

        let (groups, warnings) = build_model_remap_groups(&replacements, &selections).unwrap();
        assert!(groups
            .iter()
            .any(|group| group.group_key == "armor:pl105_0000"));
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("需要同时部署到多个目标")));

        let error =
            build_effective_remap_files(&[female_path, male_path], &replacements, &selections)
                .unwrap_err();
        assert!(error.contains("需要同时部署到多个目标"));
        assert!(error.contains("slg019_0000"));
        assert!(error.contains("slg057_0000"));
    }

    #[test]
    fn remaps_longsword_main_and_companion_texture_tokens() {
        let replacements = vec![replacement("weapon", "太刀", "main", "wp/swo/swo022")];
        let (groups, _) = build_model_remap_groups(&replacements, &[]).unwrap();
        let target = groups[0]
            .targets
            .iter()
            .find(|target| target.model_id == "wp/swo/swo001")
            .unwrap();
        let files = vec![
            "nativePC/wp/swo/swo022/mod/saya022.mrl3".to_string(),
            "nativePC/wp/tietu/01/swo022_BML.tex".to_string(),
        ];
        let selections = vec![ModelRemapSelection {
            group_key: groups[0].group_key.clone(),
            target_id: target.target_id.clone(),
        }];
        let effective = build_effective_remap_files(&files, &replacements, &selections).unwrap();
        assert_eq!(
            effective[0].deploy_relative_path,
            "nativePC/wp/swo/swo001/mod/saya001.mrl3"
        );
        assert_eq!(
            effective[1].deploy_relative_path,
            "nativePC/wp/tietu/01/swo001_BML.tex"
        );
    }

    #[test]
    fn remaps_hair_palico_armor_and_manual_slinger_targets() {
        let cases = [
            (
                replacement("hair", "发型", "model", "hair100"),
                "hair:hair100",
                "hair:hair120",
                "nativePC/pl/hair/hair100/mod/hair100.mod3",
                "nativePC/pl/hair/hair120/mod/hair120.mod3",
            ),
            (
                replacement("palicoArmor", "随从防具", "model", "ot001"),
                "palicoArmor:ot001",
                "palicoArmor:ot028",
                "nativePC/otomo/equip/ot001/helm/mod/ot001_helm.mod3",
                "nativePC/otomo/equip/ot028/helm/mod/ot028_helm.mod3",
            ),
            (
                replacement("slinger", "投射器", "model", "slg000_0000"),
                "slinger:slg000_0000",
                "slinger:slg106_0000",
                "nativePC/wp/slg/slg000_0000/mod/slg000_0000.mod3",
                "nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3",
            ),
        ];

        for (replacement, group_key, target_id, source_path, expected_path) in cases {
            let effective = build_effective_remap_files(
                &[source_path.to_string()],
                &[replacement],
                &[ModelRemapSelection {
                    group_key: group_key.to_string(),
                    target_id: target_id.to_string(),
                }],
            )
            .unwrap();
            assert_eq!(effective[0].deploy_relative_path, expected_path);
        }
    }

    #[test]
    fn remaps_matching_evam_binding_with_the_slinger_model() {
        let evam_path = "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm105_0000.evam".to_string();
        let mut slinger = replacement("slinger", "投射器", "model", "slg128_0000");
        slinger.associations.push(ModelAssociation {
            model_kind: "armor".to_string(),
            model_id: "pl105_0000".to_string(),
            display_names: vec!["【冰狼】服装".to_string()],
            matched_files: vec![evam_path.clone()],
            recognition_source: "evamBinding".to_string(),
        });
        let paths = vec![
            "nativePC/wp/slg/slg128_0000/mod/slg128_0000.mod3".to_string(),
            evam_path,
        ];
        let selections = vec![ModelRemapSelection {
            group_key: "slinger:slg128_0000".to_string(),
            target_id: "slinger:slg106_0000".to_string(),
        }];

        let files = build_effective_remap_files(&paths, &[slinger], &selections).unwrap();
        assert_eq!(
            files[0].deploy_relative_path,
            "nativePC/wp/slg/slg106_0000/mod/slg106_0000.mod3"
        );
        let rewrite = files[1].evam_slinger_rewrite.as_ref().unwrap();
        assert_eq!(rewrite.source_id, 128);
        assert_eq!(rewrite.target_id, 106);

        let output = rewrite_evam_slinger_id(&evam_bytes(128), rewrite).unwrap();
        assert_eq!(u32::from_le_bytes(output[16..20].try_into().unwrap()), 106);
    }

    #[test]
    fn rewrites_only_exact_mrl3_texture_resources() {
        let texture_table_offset = 40usize;
        let mut bytes = vec![0u8; texture_table_offset + MRL3_TEXTURE_ENTRY_SIZE * 2];
        bytes[0..4].copy_from_slice(&5_001_805u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&(texture_table_offset as u64).to_le_bytes());
        let first_path = texture_table_offset + MRL3_TEXTURE_PATH_OFFSET;
        let first_value = b"wp\\tietu\\01\\swo022_BML";
        bytes[first_path..first_path + first_value.len()].copy_from_slice(first_value);
        let second_path = texture_table_offset + MRL3_TEXTURE_ENTRY_SIZE + MRL3_TEXTURE_PATH_OFFSET;
        let second_value = b"wp\\tietu\\01\\unrelated";
        bytes[second_path..second_path + second_value.len()].copy_from_slice(second_value);
        let rewrites = BTreeMap::from([(
            "wp/tietu/01/swo022_bml".to_string(),
            "wp\\tietu\\01\\swo001_BML".to_string(),
        )]);

        let (rewritten, count) = rewrite_mrl3_texture_paths(&bytes, &rewrites).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            read_mrl3_texture_paths(&bytes).unwrap(),
            [
                "wp\\tietu\\01\\swo022_BML".to_string(),
                "wp\\tietu\\01\\unrelated".to_string(),
            ]
        );
        let first = &rewritten[first_path..first_path + first_value.len()];
        assert_eq!(first, b"wp\\tietu\\01\\swo001_BML");
        assert_eq!(
            &rewritten[second_path..second_path + second_value.len()],
            b"wp\\tietu\\01\\unrelated"
        );
    }
}
