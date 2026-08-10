use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

const EFFECT_REMAP_INDEX_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/effect-remap-index.json"
));

/// 用户确认后的特效部署路径选择；只影响部署副本，不会改写 MOD 库原文件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRemapSelection {
    pub group_key: String,
    pub target_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRemapTarget {
    pub target_id: String,
    pub target_label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRemapGroup {
    pub group_key: String,
    pub weapon_type: String,
    pub source_slot: String,
    pub source_label: String,
    pub selected_target_id: Option<String>,
    pub targets: Vec<EffectRemapTarget>,
    pub evidence_url: String,
    pub note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectRemapIndex {
    entries: Vec<EffectRemapEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectRemapEntry {
    id: String,
    weapon_type: String,
    source_slot: String,
    source_root: String,
    required_files: Vec<String>,
    targets: Vec<EffectRemapTargetEntry>,
    evidence_url: String,
    note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectRemapTargetEntry {
    target_id: String,
    target_label: String,
    target_root: String,
}

pub fn build_effect_remap_groups(
    deploy_paths: &[String],
    selections: &[EffectRemapSelection],
) -> Result<(Vec<EffectRemapGroup>, Vec<String>), String> {
    let index = effect_remap_index()?;
    let path_set = deploy_paths
        .iter()
        .map(|path| normalize_path(path))
        .collect::<HashSet<_>>();
    let selection_map = selection_map(selections)?;
    let mut groups = Vec::new();
    let mut warnings = Vec::new();

    for entry in &index.entries {
        if !entry
            .required_files
            .iter()
            .all(|path| path_set.contains(&normalize_path(path)))
        {
            continue;
        }
        let selected_target_id = selection_map.get(&entry.id).cloned();
        if let Some(target_id) = selected_target_id.as_deref() {
            if !entry
                .targets
                .iter()
                .any(|target| target.target_id == target_id)
            {
                warnings.push(format!(
                    "已保存的特效目标不再受当前兼容索引支持：{target_id}"
                ));
            }
        }
        groups.push(EffectRemapGroup {
            group_key: entry.id.clone(),
            weapon_type: entry.weapon_type.clone(),
            source_slot: entry.source_slot.clone(),
            source_label: format!("{} 本地特效槽 {}", entry.weapon_type, entry.source_slot),
            selected_target_id,
            targets: entry
                .targets
                .iter()
                .map(|target| EffectRemapTarget {
                    target_id: target.target_id.clone(),
                    target_label: target.target_label.clone(),
                })
                .collect(),
            evidence_url: entry.evidence_url.clone(),
            note: entry.note.clone(),
        });
    }
    let known_groups = groups
        .iter()
        .map(|group| group.group_key.as_str())
        .collect::<HashSet<_>>();
    for selection in selections {
        if !known_groups.contains(selection.group_key.as_str()) {
            warnings.push(format!(
                "已保存的特效分组不在当前 MOD 中：{}",
                selection.group_key
            ));
        }
    }
    Ok((groups, warnings))
}

/// 根据受审计索引重定向部署路径。索引外路径与所有二进制内容保持不变。
pub fn build_effective_effect_remap_paths(
    deploy_paths: &[String],
    selections: &[EffectRemapSelection],
) -> Result<Vec<String>, String> {
    let (groups, warnings) = build_effect_remap_groups(deploy_paths, selections)?;
    if !warnings.is_empty() {
        return Err(warnings.join(" "));
    }
    let index = effect_remap_index()?;
    let selection_map = selection_map(selections)?;
    let mut paths = deploy_paths
        .iter()
        .map(|path| normalize_path(path))
        .collect::<Vec<_>>();

    for group in groups {
        let Some(target_id) = selection_map.get(&group.group_key) else {
            continue;
        };
        let entry = index
            .entries
            .iter()
            .find(|entry| entry.id == group.group_key)
            .ok_or_else(|| format!("未找到特效兼容规则：{}", group.group_key))?;
        let target = entry
            .targets
            .iter()
            .find(|target| target.target_id == *target_id)
            .ok_or_else(|| format!("不支持的特效目标：{target_id}"))?;
        let source_root = format!("{}/", normalize_path(&entry.source_root));
        let target_root = format!("{}/", normalize_path(&target.target_root));
        for path in &mut paths {
            if path.starts_with(&source_root) {
                *path = format!("{}{}", target_root, &path[source_root.len()..]);
                *path = path.replace(&entry.source_slot, &target.target_id);
            }
        }
    }
    ensure_unique_paths(&paths)?;
    Ok(paths)
}

pub fn updated_effect_remap_selections(
    selections: &[EffectRemapSelection],
    group_key: &str,
    target_id: Option<String>,
) -> Vec<EffectRemapSelection> {
    let mut updated = selections
        .iter()
        .filter(|selection| selection.group_key != group_key)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(target_id) = target_id.filter(|value| !value.trim().is_empty()) {
        updated.push(EffectRemapSelection {
            group_key: group_key.to_string(),
            target_id,
        });
    }
    updated.sort_by(|left, right| left.group_key.cmp(&right.group_key));
    updated
}

fn effect_remap_index() -> Result<&'static EffectRemapIndex, String> {
    static INDEX: std::sync::OnceLock<Result<EffectRemapIndex, String>> =
        std::sync::OnceLock::new();
    INDEX
        .get_or_init(|| {
            serde_json::from_str(EFFECT_REMAP_INDEX_JSON)
                .map_err(|error| format!("无法读取特效兼容索引：{error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn selection_map(selections: &[EffectRemapSelection]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for selection in selections {
        if map
            .insert(selection.group_key.clone(), selection.target_id.clone())
            .is_some()
        {
            return Err(format!("特效分组重复保存：{}", selection.group_key));
        }
    }
    Ok(map)
}

fn ensure_unique_paths(paths: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for path in paths {
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err(format!("特效改绑会产生重复部署路径：{path}"));
        }
    }
    Ok(())
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_remaps_indexed_local_effect_slot() {
        let paths = vec![
            "wp/swo/swo010/mod/swo010.evwp".to_string(),
            "wp/swo/swo010/epv/swo010.epv3".to_string(),
            "vfx/efx/cm/cm_all/cm_critical_000.efx".to_string(),
        ];
        let remapped = build_effective_effect_remap_paths(
            &paths,
            &[EffectRemapSelection {
                group_key: "local-swo010-to-swo001".to_string(),
                target_id: "swo001".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(remapped[0], "wp/swo/swo001/mod/swo001.evwp");
        assert_eq!(remapped[1], "wp/swo/swo001/epv/swo001.epv3");
        assert_eq!(remapped[2], "vfx/efx/cm/cm_all/cm_critical_000.efx");
    }
}
