use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::operations::OperationReporter;

use super::{
    knowledge,
    mod_library::{self, ModAnalysisInput, ModAnalysisInputFile},
    model_recognition::{read_evam_slinger_id, ModelReplacement},
    model_remap::read_mrl3_texture_paths,
};

const ANALYSIS_SCHEMA_VERSION: u32 = 1;
const ANALYZER_VERSION: u32 = 2;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_PARSER_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RESOURCE_EDGES: usize = 20_000;
const MAX_KNOWLEDGE_QUERIES: usize = 12;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModAnalysisReport {
    pub schema_version: u32,
    pub analyzer_version: u32,
    pub mod_id: String,
    pub mod_name: String,
    pub inventory_sha256: String,
    pub content_sha256: String,
    pub knowledge_signature: String,
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub recognized_file_count: usize,
    pub unknown_file_count: usize,
    pub component_count: usize,
    pub files: Vec<AnalyzedModFile>,
    pub components: Vec<ModResourceComponent>,
    pub edges: Vec<ModResourceEdge>,
    pub knowledge_evidence: Vec<ModKnowledgeEvidence>,
    pub warnings: Vec<String>,
    pub cache_hit: bool,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedModFile {
    pub file_id: String,
    pub library_relative_path: String,
    pub source_deploy_relative_path: String,
    pub effective_deploy_relative_path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub role: String,
    pub role_label: String,
    pub component_id: String,
    pub component_label: String,
    pub replacement_targets: Vec<String>,
    pub references: Vec<String>,
    pub evidence: Vec<ModAnalysisEvidence>,
    pub confidence: f64,
    pub excluded_from_deployment: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModAnalysisEvidence {
    pub kind: String,
    pub detail: String,
    pub confidence: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModResourceComponent {
    pub component_id: String,
    pub kind: String,
    pub label: String,
    pub file_count: usize,
    pub file_ids: Vec<String>,
    pub roles: Vec<String>,
    pub replacement_targets: Vec<String>,
    pub confidence: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModResourceEdge {
    pub from_file_id: String,
    pub to_file_id: Option<String>,
    pub target_reference: String,
    pub relation: String,
    pub relation_label: String,
    pub evidence: String,
    pub confidence: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModKnowledgeEvidence {
    pub result_id: String,
    pub title: String,
    pub snippet: String,
    pub game_version: String,
    pub confidence: f64,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
    pub pack_id: String,
    pub pack_version: String,
}

struct RoleRule {
    role: &'static str,
    label: &'static str,
    detail: &'static str,
    confidence: f64,
    knowledge_query: Option<&'static str>,
}

#[derive(Clone)]
struct ComponentDescriptor {
    key: String,
    kind: String,
    label: String,
    confidence: f64,
}

struct LocalAnalysis {
    files: Vec<AnalyzedModFile>,
    components: Vec<ModResourceComponent>,
    edges: Vec<ModResourceEdge>,
    roles_for_knowledge: BTreeSet<String>,
    warnings: Vec<String>,
}

/// 分析入口只接收稳定 MOD ID，文件路径由 mod_library 从 manifest 安全恢复。
pub fn analyze_mod(
    app: &AppHandle,
    mod_id: &str,
    progress: &OperationReporter,
) -> Result<ModAnalysisReport, String> {
    progress.report("正在读取 MOD 分析清单", 0, None, None);
    let input = mod_library::load_mod_analysis_input(app, mod_id)?;
    let cache_root = knowledge::analysis_cache_root()?;
    let (knowledge_signature, has_modding_pack, mut status_warnings) = knowledge_signature()?;
    let inventory_sha256 = inventory_fingerprint(&input, &knowledge_signature)?;
    let cache_path = analysis_cache_path(
        &cache_root,
        &input.mod_id,
        &inventory_sha256,
        &knowledge_signature,
    );
    match read_cached_report(&cache_path) {
        Ok(Some(mut cached)) => {
            cached.cache_hit = true;
            cached.warnings.append(&mut status_warnings);
            cached.warnings.sort();
            cached.warnings.dedup();
            cached.message = format!("已从缓存读取“{}”的 MOD 文件分析。", cached.mod_name);
            progress.report(
                "MOD 文件分析已完成",
                1,
                Some(1),
                Some(cached.mod_name.clone()),
            );
            return Ok(cached);
        }
        Ok(None) => {}
        Err(error) => status_warnings.push(format!("{error} 已忽略该缓存并重新分析。")),
    }

    let content_sha256 = hash_mod_content(&input, progress)?;
    progress.report("正在识别 MOD 文件作用", 0, Some(input.files.len()), None);
    let mut local = analyze_local_files(&input, progress)?;
    local.warnings.append(&mut status_warnings);
    let knowledge_evidence = if has_modding_pack {
        collect_knowledge_evidence(&local.roles_for_knowledge, &mut local.warnings)
    } else {
        local.warnings.push(
            "未安装可用的 MHW MOD 技术知识包；当前报告仅使用本地解析器和路径规则。".to_string(),
        );
        Vec::new()
    };
    local.warnings.sort();
    local.warnings.dedup();
    let total_size_bytes = input.files.iter().map(|file| file.size_bytes).sum();
    let recognized_file_count = local
        .files
        .iter()
        .filter(|file| file.role != "unknown")
        .count();
    let unknown_file_count = local.files.len().saturating_sub(recognized_file_count);
    let component_count = local.components.len();
    let report = ModAnalysisReport {
        schema_version: ANALYSIS_SCHEMA_VERSION,
        analyzer_version: ANALYZER_VERSION,
        mod_id: input.mod_id,
        mod_name: input.name,
        inventory_sha256,
        content_sha256,
        knowledge_signature,
        file_count: local.files.len(),
        total_size_bytes,
        recognized_file_count,
        unknown_file_count,
        component_count,
        files: local.files,
        components: local.components,
        edges: local.edges,
        knowledge_evidence,
        warnings: local.warnings,
        cache_hit: false,
        message: format!(
            "已分析 {recognized_file_count} 个可识别文件，另有 {unknown_file_count} 个文件需要更多格式资料。"
        ),
    };
    write_cached_report(&cache_path, &report)?;
    progress.report(
        "MOD 文件分析已完成",
        1,
        Some(1),
        Some(report.mod_name.clone()),
    );
    Ok(report)
}

fn knowledge_signature() -> Result<(String, bool, Vec<String>), String> {
    let status = knowledge::get_status()?;
    let mut active = status
        .packs
        .iter()
        .filter(|pack| pack.active && pack.healthy)
        .map(|pack| {
            format!(
                "{}:{}:{}:{}",
                pack.kind, pack.pack_id, pack.version, pack.sha256
            )
        })
        .collect::<Vec<_>>();
    active.sort();
    let has_modding_pack = status
        .packs
        .iter()
        .any(|pack| pack.active && pack.healthy && pack.kind == "mhw-modding");
    let signature = if active.is_empty() {
        "none".to_string()
    } else {
        sha256_text(&active.join("\n"))
    };
    let warnings = status
        .packs
        .iter()
        .filter(|pack| pack.active && !pack.healthy)
        .map(|pack| format!("知识包“{}”不可用，分析时已跳过。", pack.display_name))
        .collect();
    Ok((signature, has_modding_pack, warnings))
}

fn inventory_fingerprint(
    input: &ModAnalysisInput,
    knowledge_signature: &str,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(format!("analysis:{ANALYZER_VERSION}\n{}\n", input.mod_id));
    hasher.update(knowledge_signature.as_bytes());
    let replacements = serde_json::to_vec(&input.model_replacements)
        .map_err(|error| format!("无法整理 MOD 替换目标：{error}"))?;
    hasher.update(&replacements);
    for file in &input.files {
        let metadata = fs::metadata(&file.source_path)
            .map_err(|error| format!("无法读取 MOD 文件元数据：{error}"))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        hasher.update(file.library_relative_path.as_bytes());
        hasher.update([0]);
        hasher.update(file.effective_deploy_relative_path.as_bytes());
        hasher.update(file.size_bytes.to_le_bytes());
        hasher.update(modified.to_le_bytes());
        hasher.update([u8::from(file.excluded_from_deployment)]);
    }
    Ok(hex_digest(hasher.finalize()))
}

fn hash_mod_content(
    input: &ModAnalysisInput,
    progress: &OperationReporter,
) -> Result<String, String> {
    let total_bytes = input.files.iter().map(|file| file.size_bytes).sum::<u64>();
    let mut completed_bytes = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    for file in &input.files {
        hasher.update(file.effective_deploy_relative_path.as_bytes());
        hasher.update([0]);
        let mut source = File::open(&file.source_path)
            .map_err(|error| format!("无法读取 MOD 文件进行分析：{error}"))?;
        loop {
            let count = source
                .read(&mut buffer)
                .map_err(|error| format!("读取 MOD 文件内容失败：{error}"))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            completed_bytes = completed_bytes.saturating_add(count as u64);
            progress.report(
                "正在计算 MOD 内容指纹",
                usize::try_from(completed_bytes).unwrap_or(usize::MAX),
                Some(usize::try_from(total_bytes).unwrap_or(usize::MAX)),
                Some(file.effective_deploy_relative_path.clone()),
            );
        }
    }
    Ok(hex_digest(hasher.finalize()))
}

fn analyze_local_files(
    input: &ModAnalysisInput,
    progress: &OperationReporter,
) -> Result<LocalAnalysis, String> {
    let replacement_index = replacement_index(&input.model_replacements);
    let mut files = Vec::with_capacity(input.files.len());
    let mut component_files = BTreeMap::<String, Vec<String>>::new();
    let mut component_descriptors = BTreeMap::<String, ComponentDescriptor>::new();
    let mut roles_for_knowledge = BTreeSet::new();
    let mut warnings = Vec::new();

    for (index, input_file) in input.files.iter().enumerate() {
        let path_key = normalize_path(&input_file.effective_deploy_relative_path);
        let replacements = replacements_for_file(&replacement_index, input_file);
        let role = classify_role(&path_key);
        let component = component_for_path(&path_key, &replacements);
        let file_id = stable_id("file", &input_file.effective_deploy_relative_path);
        let replacement_targets = replacement_labels(&replacements);
        let mut evidence = vec![ModAnalysisEvidence {
            kind: "extensionRule".to_string(),
            detail: role.detail.to_string(),
            confidence: role.confidence,
        }];
        if component.confidence >= 0.8 {
            evidence.push(ModAnalysisEvidence {
                kind: "pathRule".to_string(),
                detail: format!("资源路径归入组件“{}”。", component.label),
                confidence: component.confidence,
            });
        }
        if !replacement_targets.is_empty() {
            evidence.push(ModAnalysisEvidence {
                kind: "modelRecognition".to_string(),
                detail: format!("现有模型识别器命中：{}。", replacement_targets.join("、")),
                confidence: 1.0,
            });
        }
        if let Some(query) = role.knowledge_query {
            roles_for_knowledge.insert(query.to_string());
        }
        component_files
            .entry(component.key.clone())
            .or_default()
            .push(file_id.clone());
        component_descriptors
            .entry(component.key.clone())
            .or_insert_with(|| component.clone());
        files.push(AnalyzedModFile {
            file_id,
            library_relative_path: input_file.library_relative_path.clone(),
            source_deploy_relative_path: input_file.source_deploy_relative_path.clone(),
            effective_deploy_relative_path: input_file.effective_deploy_relative_path.clone(),
            extension: file_extension(&path_key),
            size_bytes: input_file.size_bytes,
            role: role.role.to_string(),
            role_label: role.label.to_string(),
            component_id: String::new(),
            component_label: String::new(),
            replacement_targets,
            references: Vec::new(),
            evidence,
            confidence: role.confidence.max(component.confidence),
            excluded_from_deployment: input_file.excluded_from_deployment,
        });
        progress.report(
            "正在识别 MOD 文件作用",
            index + 1,
            Some(input.files.len()),
            Some(input_file.effective_deploy_relative_path.clone()),
        );
    }

    let mut file_index = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.file_id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut components = Vec::with_capacity(component_files.len());
    for (key, file_ids) in component_files {
        let descriptor = component_descriptors.remove(&key).unwrap();
        let component_id = stable_id("component", &key);
        let mut roles = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for file_id in &file_ids {
            let file = &mut files[*file_index.get(file_id).unwrap()];
            file.component_id = component_id.clone();
            file.component_label = descriptor.label.clone();
            roles.insert(file.role_label.clone());
            targets.extend(file.replacement_targets.iter().cloned());
        }
        let label = targets
            .iter()
            .next()
            .map(|target| format!("{}：{target}", descriptor.label))
            .unwrap_or_else(|| descriptor.label.clone());
        for file_id in &file_ids {
            let file = &mut files[*file_index.get(file_id).unwrap()];
            file.component_label = label.clone();
        }
        components.push(ModResourceComponent {
            component_id,
            kind: descriptor.kind,
            label,
            file_count: file_ids.len(),
            file_ids,
            roles: roles.into_iter().collect(),
            replacement_targets: targets.into_iter().collect(),
            confidence: descriptor.confidence,
        });
    }
    components.sort_by(|left, right| left.label.cmp(&right.label));
    file_index.clear();
    file_index.extend(
        files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.file_id.clone(), index)),
    );
    let edges = build_resource_edges(input, &mut files, &file_index, &mut warnings)?;

    Ok(LocalAnalysis {
        files,
        components,
        edges,
        roles_for_knowledge,
        warnings,
    })
}

fn replacement_index(replacements: &[ModelReplacement]) -> HashMap<String, Vec<&ModelReplacement>> {
    let mut index = HashMap::<String, Vec<&ModelReplacement>>::new();
    for replacement in replacements {
        for path in &replacement.matched_files {
            index
                .entry(normalize_path(path))
                .or_default()
                .push(replacement);
        }
        for association in &replacement.associations {
            for path in &association.matched_files {
                index
                    .entry(normalize_path(path))
                    .or_default()
                    .push(replacement);
            }
        }
    }
    index
}

fn replacements_for_file<'a>(
    index: &'a HashMap<String, Vec<&'a ModelReplacement>>,
    file: &ModAnalysisInputFile,
) -> Vec<&'a ModelReplacement> {
    let mut matches = Vec::new();
    for path in [
        &file.source_deploy_relative_path,
        &file.effective_deploy_relative_path,
    ] {
        if let Some(values) = index.get(&normalize_path(path)) {
            for value in values {
                if !matches.iter().any(|current: &&ModelReplacement| {
                    current.model_kind == value.model_kind && current.model_id == value.model_id
                }) {
                    matches.push(*value);
                }
            }
        }
    }
    matches
}

fn replacement_labels(replacements: &[&ModelReplacement]) -> Vec<String> {
    let mut labels = replacements
        .iter()
        .map(|replacement| {
            replacement
                .display_names
                .first()
                .cloned()
                .unwrap_or_else(|| replacement.model_id.clone())
        })
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn classify_role(path: &str) -> RoleRule {
    let extension = file_extension(path);
    if path_components(path).iter().any(|part| *part == "plugins") {
        return match extension.as_str() {
            "dll" => role(
                "plugin",
                "原生插件",
                "文件位于 plugins，DLL 会由插件加载器载入。",
                0.98,
                Some("plugins"),
            ),
            "lua" | "js" | "json" => role(
                "pluginScript",
                "插件脚本或数据",
                "文件位于 plugins，属于插件运行时脚本或数据。",
                0.9,
                Some("plugins"),
            ),
            "ini" | "cfg" | "toml" | "yaml" | "yml" => role(
                "configuration",
                "插件配置",
                "文件位于 plugins，属于插件配置。",
                0.95,
                Some("plugins"),
            ),
            _ => role(
                "pluginResource",
                "插件资源",
                "文件位于 plugins，具体用途需结合插件资料。",
                0.75,
                Some("plugins"),
            ),
        };
    }
    match extension.as_str() {
        "mod3" => role(
            "model",
            "模型",
            "MOD3 是 MHW 模型资源；具体替换对象由路径和 ID 识别。",
            0.98,
            Some("MOD3"),
        ),
        "mrl3" => role(
            "material",
            "材质",
            "MRL3 保存材质参数及贴图资源引用。",
            1.0,
            Some("MRL3"),
        ),
        "tex" => role(
            "texture",
            "贴图",
            "TEX 是 MHW 模型或特效使用的运行时贴图资源。",
            0.98,
            Some("TEX"),
        ),
        "dds" => role(
            "textureSource",
            "贴图源文件",
            "DDS 通常用于贴图制作或转换；除非位于插件目录或有其它加载证据，不能视为 MHW 原生运行资源。",
            0.7,
            Some("TEX"),
        ),
        "ctc" => role(
            "physics",
            "物理链",
            "CTC 通常描述布料、毛发或挂件的动态骨骼链。",
            0.92,
            Some("CTC"),
        ),
        "ccl" | "col" => role(
            "collision",
            "碰撞数据",
            "文件用于物理链或模型的碰撞约束。",
            0.9,
            Some("CCL"),
        ),
        "efx" => role(
            "effect",
            "特效定义",
            "EFX 位于 MHW 特效资源链中，具体引用仍需格式解析。",
            0.9,
            Some("EFX"),
        ),
        "epv3" => role(
            "effectBinding",
            "装备特效触发",
            "EPV3 通常随装备部位部署，用于关联装备外观与特效。",
            0.9,
            Some("EPV3"),
        ),
        "evam" => role(
            "appearanceBinding",
            "防具飞翔爪绑定",
            "EVAM 可精确读取防具对应的飞翔爪资源 ID。",
            1.0,
            Some("EVAM"),
        ),
        "evwp" => role(
            "weaponParameter",
            "武器位置与全局特效参数",
            "EVWP 可配置武器与挂件位置，并映射武器使用的全局 EPV；当前不解析内部字段。",
            0.92,
            Some("EVWP"),
        ),
        "evhl" | "evbd" => role(
            "eventParameter",
            "外观事件参数",
            "该 EV 文件属于外观资源参数；当前只按扩展名和路径确认类别。",
            0.65,
            Some("EV"),
        ),
        "nbnk" => role(
            "audioBank",
            "音频库",
            "NBNK 是 sound/wwise 下的音频库资源。",
            0.92,
            Some("NBNK"),
        ),
        "npck" => role(
            "audioPackage",
            "音频数据包",
            "NPCK 是 sound/wwise 下的音频数据包。",
            0.92,
            Some("NPCK"),
        ),
        "timl" => role(
            "timeline",
            "时间轴参数",
            "TIML 保存时间轴或动画事件参数。",
            0.82,
            Some("TIML"),
        ),
        "sobj" => role(
            "objectData",
            "游戏对象数据",
            "SOBJ 是游戏对象或参数资源，具体字段需专用解析器。",
            0.75,
            Some("SOBJ"),
        ),
        "sobjl" => role(
            "objectList",
            "游戏对象列表",
            "SOBJL 保存游戏对象资源列表，并可引用同一 nativePC 相对路径下的 SOBJ。",
            0.95,
            Some("SOBJ"),
        ),
        "uvs" => role(
            "uvData",
            "UV 辅助数据",
            "UVS 与模型表面坐标相关，具体字段尚未解析。",
            0.7,
            Some("UVS"),
        ),
        "lmt" => role(
            "animation",
            "动画资源",
            "LMT 是动画资源；当前不会解析具体动作、骨骼或调用关系。",
            0.9,
            Some("LMT"),
        ),
        "gmd" => role(
            "localization",
            "本地化文本",
            "GMD 是游戏本地化文本资源，不是模型或特效文件。",
            0.95,
            Some("GMD"),
        ),
        "ini" | "cfg" | "json" | "xml" | "toml" | "yaml" | "yml" => role(
            "configuration",
            "配置或数据",
            "文本结构表明它是配置或数据文件，是否参与运行由所在目录决定。",
            0.72,
            None,
        ),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => role(
            "image",
            "普通图片",
            "普通图片不是 MHW 原生运行资源，可能是预览图或插件界面资源。",
            0.85,
            None,
        ),
        "txt" | "md" | "pdf" | "html" | "htm" => role(
            "documentation",
            "说明文档",
            "文档通常不参与游戏运行，但可能包含安装或制作说明。",
            0.9,
            None,
        ),
        "zip" | "7z" | "rar" => role(
            "archive",
            "内嵌压缩包",
            "压缩包是附带分支或原始材料，不会作为 MHW 资源直接加载。",
            0.95,
            None,
        ),
        "exe" => role(
            "utility",
            "附带工具",
            "可执行文件不是 nativePC 资源，不能仅凭文件名判断是否需要。",
            0.8,
            None,
        ),
        _ => role(
            "unknown",
            "未知文件",
            "当前路径规则和格式表尚不能确定该文件作用。",
            0.2,
            None,
        ),
    }
}

fn role(
    role: &'static str,
    label: &'static str,
    detail: &'static str,
    confidence: f64,
    knowledge_query: Option<&'static str>,
) -> RoleRule {
    RoleRule {
        role,
        label,
        detail,
        confidence,
        knowledge_query,
    }
}

fn component_for_path(path: &str, replacements: &[&ModelReplacement]) -> ComponentDescriptor {
    let parts = path_components(path);
    let stripped = parts
        .strip_prefix(&["nativepc"])
        .unwrap_or(parts.as_slice());
    let from_replacement = || {
        replacements.first().map(|replacement| ComponentDescriptor {
            key: format!("target:{}:{}", replacement.model_kind, replacement.model_id),
            kind: replacement.model_kind.clone(),
            label: replacement_kind_label(&replacement.model_kind).to_string(),
            confidence: 1.0,
        })
    };
    if stripped.len() >= 3 && stripped[0] == "pl" && matches!(stripped[1], "f_equip" | "m_equip") {
        return ComponentDescriptor {
            key: format!("armor:{}", stripped[2]),
            kind: "armor".to_string(),
            label: "防具外观".to_string(),
            confidence: 0.98,
        };
    }
    if stripped.len() >= 3 && stripped[0] == "wp" && stripped[1] == "slg" {
        return ComponentDescriptor {
            key: format!("slinger:{}", stripped[2]),
            kind: "slinger".to_string(),
            label: "飞翔爪外观".to_string(),
            confidence: 0.98,
        };
    }
    if stripped.len() >= 3 && stripped[0] == "wp" {
        return ComponentDescriptor {
            key: format!("weapon:{}:{}", stripped[1], stripped[2]),
            kind: "weapon".to_string(),
            label: format!("武器资源（{}）", stripped[1]),
            confidence: 0.92,
        };
    }
    if stripped.len() >= 3 && stripped[0] == "pl" && stripped[1] == "hair" {
        return ComponentDescriptor {
            key: format!("hair:{}", stripped[2]),
            kind: "hair".to_string(),
            label: "发型外观".to_string(),
            confidence: 0.98,
        };
    }
    if stripped.first() == Some(&"vfx") {
        let key = stripped
            .iter()
            .take(5)
            .copied()
            .collect::<Vec<_>>()
            .join("/");
        return ComponentDescriptor {
            key: format!("vfx:{key}"),
            kind: "effect".to_string(),
            label: "视觉特效".to_string(),
            confidence: 0.88,
        };
    }
    if stripped.first() == Some(&"sound") {
        return ComponentDescriptor {
            key: format!("sound:{}", stripped.last().copied().unwrap_or("unknown")),
            kind: "audio".to_string(),
            label: "音频资源".to_string(),
            confidence: 0.9,
        };
    }
    if stripped.iter().any(|part| *part == "plugins") {
        return ComponentDescriptor {
            key: format!("plugin:{}", stripped.last().copied().unwrap_or("unknown")),
            kind: "plugin".to_string(),
            label: "插件".to_string(),
            confidence: 0.95,
        };
    }
    if let Some(component) = from_replacement() {
        return component;
    }
    let key = stripped
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .join("/");
    ComponentDescriptor {
        key: format!("generic:{key}"),
        kind: "generic".to_string(),
        label: stripped
            .first()
            .map(|value| format!("{} 资源", value))
            .unwrap_or_else(|| "其它资源".to_string()),
        confidence: 0.45,
    }
}

fn replacement_kind_label(kind: &str) -> &'static str {
    match kind {
        "armor" => "防具外观",
        "weapon" => "武器外观",
        "slinger" => "飞翔爪外观",
        "hair" => "发型外观",
        "voice" => "人物语音",
        "palicoArmor" => "随从防具",
        "palicoWeapon" => "随从武器",
        "insect" => "猎虫",
        "charm" => "挂件",
        "npc" => "NPC 外观",
        _ => "游戏资源",
    }
}

fn build_resource_edges(
    input: &ModAnalysisInput,
    files: &mut [AnalyzedModFile],
    file_index: &HashMap<String, usize>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ModResourceEdge>, String> {
    let mut aliases = HashMap::<String, String>::new();
    for file in files.iter() {
        for path in [
            &file.source_deploy_relative_path,
            &file.effective_deploy_relative_path,
        ] {
            let normalized = normalize_resource_reference(path);
            aliases.insert(normalized.clone(), file.file_id.clone());
            aliases.insert(
                strip_known_resource_extension(&normalized),
                file.file_id.clone(),
            );
        }
    }
    let input_by_effective_path = input
        .files
        .iter()
        .map(|file| (normalize_path(&file.effective_deploy_relative_path), file))
        .collect::<HashMap<_, _>>();
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    for file_index_value in 0..files.len() {
        let role = files[file_index_value].role.clone();
        let path_key = normalize_path(&files[file_index_value].effective_deploy_relative_path);
        let Some(input_file) = input_by_effective_path.get(&path_key).copied() else {
            continue;
        };
        if role == "material" {
            match read_parser_file(&input_file.source_path, input_file.size_bytes)
                .and_then(|bytes| read_mrl3_texture_paths(&bytes))
            {
                Ok(references) => {
                    files[file_index_value].references = references.clone();
                    files[file_index_value].evidence.push(ModAnalysisEvidence {
                        kind: "binaryParser".to_string(),
                        detail: format!("MRL3 材质表包含 {} 条贴图引用。", references.len()),
                        confidence: 1.0,
                    });
                    for reference in references {
                        let key = strip_known_resource_extension(&normalize_resource_reference(
                            &reference,
                        ));
                        let target_file_id = aliases.get(&key).cloned();
                        push_edge(
                            &mut edges,
                            &mut seen,
                            ModResourceEdge {
                                from_file_id: files[file_index_value].file_id.clone(),
                                to_file_id: target_file_id,
                                target_reference: reference,
                                relation: "referencesTexture".to_string(),
                                relation_label: "引用贴图".to_string(),
                                evidence: "MRL3 二进制贴图表".to_string(),
                                confidence: 1.0,
                            },
                        );
                    }
                }
                Err(error) => warnings.push(format!(
                    "无法解析 {} 的 MRL3 贴图引用：{error}",
                    files[file_index_value].effective_deploy_relative_path
                )),
            }
        } else if role == "appearanceBinding" {
            match read_parser_file(&input_file.source_path, input_file.size_bytes)
                .and_then(|bytes| read_evam_slinger_id(&bytes))
            {
                Ok(slinger_id) => {
                    let reference = format!("slg{slinger_id:03}_0000");
                    files[file_index_value].references.push(reference.clone());
                    files[file_index_value].evidence.push(ModAnalysisEvidence {
                        kind: "binaryParser".to_string(),
                        detail: format!("EVAM 明确绑定飞翔爪 ID {slinger_id}。"),
                        confidence: 1.0,
                    });
                    let target_file_id = files
                        .iter()
                        .find(|candidate| {
                            candidate.role == "model"
                                && normalize_path(&candidate.effective_deploy_relative_path)
                                    .contains(&format!("/wp/slg/slg{slinger_id:03}_"))
                        })
                        .map(|candidate| candidate.file_id.clone());
                    push_edge(
                        &mut edges,
                        &mut seen,
                        ModResourceEdge {
                            from_file_id: files[file_index_value].file_id.clone(),
                            to_file_id: target_file_id,
                            target_reference: reference,
                            relation: "bindsSlinger".to_string(),
                            relation_label: "绑定飞翔爪".to_string(),
                            evidence: "EVAM 二进制绑定 ID".to_string(),
                            confidence: 1.0,
                        },
                    );
                }
                Err(error) => warnings.push(format!(
                    "无法解析 {} 的 EVAM 绑定：{error}",
                    files[file_index_value].effective_deploy_relative_path
                )),
            }
        } else if role == "objectList" {
            match read_parser_file(&input_file.source_path, input_file.size_bytes)
                .and_then(|bytes| read_sobjl_object_paths(&bytes))
            {
                Ok(references) => {
                    files[file_index_value].references = references.clone();
                    files[file_index_value].evidence.push(ModAnalysisEvidence {
                        kind: "binaryStringExtraction".to_string(),
                        detail: format!("SOBJL 中提取到 {} 条 SOBJ 相对路径。", references.len()),
                        confidence: 0.85,
                    });
                    for reference in references {
                        let key = normalize_resource_reference(&reference);
                        let target_file_id = aliases.get(&key).cloned();
                        push_edge(
                            &mut edges,
                            &mut seen,
                            ModResourceEdge {
                                from_file_id: files[file_index_value].file_id.clone(),
                                to_file_id: target_file_id,
                                target_reference: reference,
                                relation: "referencesObjectData".to_string(),
                                relation_label: "引用游戏对象".to_string(),
                                evidence: "SOBJL 内嵌 rSetObject 路径字符串".to_string(),
                                confidence: 0.85,
                            },
                        );
                    }
                }
                Err(error) => warnings.push(format!(
                    "无法提取 {} 的 SOBJL 对象路径：{error}",
                    files[file_index_value].effective_deploy_relative_path
                )),
            }
        }
    }

    let files_by_component = files.iter().fold(
        BTreeMap::<String, Vec<&AnalyzedModFile>>::new(),
        |mut grouped, file| {
            grouped
                .entry(file.component_id.clone())
                .or_default()
                .push(file);
            grouped
        },
    );
    for grouped in files_by_component.values() {
        let models = grouped
            .iter()
            .filter(|file| file.role == "model")
            .collect::<Vec<_>>();
        let materials = grouped
            .iter()
            .filter(|file| file.role == "material")
            .collect::<Vec<_>>();
        for model in &models {
            for material in &materials {
                push_edge(
                    &mut edges,
                    &mut seen,
                    ModResourceEdge {
                        from_file_id: model.file_id.clone(),
                        to_file_id: Some(material.file_id.clone()),
                        target_reference: material.effective_deploy_relative_path.clone(),
                        relation: "usesMaterial".to_string(),
                        relation_label: "配套材质".to_string(),
                        evidence: "模型与材质位于同一已识别资源组件".to_string(),
                        confidence: 0.75,
                    },
                );
            }
        }
    }
    edges.retain(|edge| {
        file_index.contains_key(&edge.from_file_id)
            && edge
                .to_file_id
                .as_ref()
                .is_none_or(|target| file_index.contains_key(target))
    });
    Ok(edges)
}

fn push_edge(edges: &mut Vec<ModResourceEdge>, seen: &mut HashSet<String>, edge: ModResourceEdge) {
    if edges.len() >= MAX_RESOURCE_EDGES {
        return;
    }
    let key = format!(
        "{}\0{}\0{}\0{}",
        edge.from_file_id,
        edge.to_file_id.as_deref().unwrap_or_default(),
        edge.relation,
        edge.target_reference
    );
    if seen.insert(key) {
        edges.push(edge);
    }
}

fn read_parser_file(path: &Path, size_bytes: u64) -> Result<Vec<u8>, String> {
    if size_bytes > MAX_PARSER_FILE_BYTES {
        return Err(format!(
            "文件超过 {} MB 解析上限",
            MAX_PARSER_FILE_BYTES / 1024 / 1024
        ));
    }
    fs::read(path).map_err(|error| format!("无法读取文件：{error}"))
}

/// SOBJL 可包含相对于 nativePC 的 rSetObject 路径；只提取完整 ASCII 路径，不解释对象字段。
fn read_sobjl_object_paths(bytes: &[u8]) -> Result<Vec<String>, String> {
    if !bytes
        .windows(b"rSetObject".len())
        .any(|window| window == b"rSetObject")
    {
        return Err("未找到 rSetObject 标记".to_string());
    }
    let mut paths = BTreeSet::new();
    let mut start = 0usize;
    while start < bytes.len() {
        while start < bytes.len() && !(0x20..=0x7e).contains(&bytes[start]) {
            start += 1;
        }
        let end = bytes[start..]
            .iter()
            .position(|byte| !(0x20..=0x7e).contains(byte))
            .map(|offset| start + offset)
            .unwrap_or(bytes.len());
        if let Ok(value) = std::str::from_utf8(&bytes[start..end]) {
            let normalized = normalize_resource_reference(value);
            if normalized.ends_with(".sobj") && normalized.contains('/') {
                paths.insert(normalized);
            }
        }
        start = end.saturating_add(1);
    }
    if paths.is_empty() {
        Err("未找到 SOBJ 相对路径".to_string())
    } else {
        Ok(paths.into_iter().collect())
    }
}

fn collect_knowledge_evidence(
    queries: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Vec<ModKnowledgeEvidence> {
    let domains = vec!["mhw-modding".to_string()];
    let mut evidence = Vec::new();
    let mut seen = HashSet::new();
    for query in queries.iter().take(MAX_KNOWLEDGE_QUERIES) {
        match knowledge::search(query, Some(&domains), 3) {
            Ok(response) => {
                warnings.extend(response.warnings);
                for item in response.matches {
                    let key = format!("{}:{}", item.pack_id, item.result_id);
                    if !seen.insert(key) {
                        continue;
                    }
                    evidence.push(ModKnowledgeEvidence {
                        result_id: item.result_id,
                        title: item.title,
                        snippet: item.snippet,
                        game_version: item.game_version,
                        confidence: item.confidence,
                        source_title: item.source_title,
                        source_url: item.source_url,
                        pack_id: item.pack_id,
                        pack_version: item.pack_version,
                    });
                }
            }
            Err(error) => warnings.push(format!("查询 MOD 技术知识“{query}”失败：{error}")),
        }
    }
    evidence
}

fn analysis_cache_path(
    root: &Path,
    mod_id: &str,
    inventory_sha256: &str,
    knowledge_signature: &str,
) -> PathBuf {
    let mod_key = stable_id("mod", mod_id);
    let knowledge_key = if knowledge_signature == "none" {
        "none".to_string()
    } else {
        knowledge_signature.chars().take(16).collect()
    };
    root.join(mod_key).join(format!(
        "{}-a{ANALYZER_VERSION}-k{knowledge_key}.json",
        inventory_sha256.chars().take(24).collect::<String>()
    ))
}

fn read_cached_report(path: &Path) -> Result<Option<ModAnalysisReport>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("无法读取 MOD 分析缓存：{error}"))?;
    let report = serde_json::from_str::<ModAnalysisReport>(&content)
        .map_err(|error| format!("MOD 分析缓存已损坏：{error}"))?;
    if report.schema_version != ANALYSIS_SCHEMA_VERSION
        || report.analyzer_version != ANALYZER_VERSION
    {
        return Ok(None);
    }
    Ok(Some(report))
}

fn write_cached_report(path: &Path, report: &ModAnalysisReport) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无法确定 MOD 分析缓存目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建 MOD 分析缓存目录：{error}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间不可用：{error}"))?
        .as_nanos();
    let temporary = parent.join(format!(".analysis-{stamp}.tmp"));
    let backup = parent.join(format!(".analysis-{stamp}.backup"));
    let content = serde_json::to_string_pretty(report)
        .map_err(|error| format!("无法序列化 MOD 分析结果：{error}"))?;
    fs::write(&temporary, format!("{content}\n"))
        .map_err(|error| format!("无法写入 MOD 分析缓存：{error}"))?;
    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup).map_err(|error| format!("无法暂存旧 MOD 分析缓存：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法启用 MOD 分析缓存：{error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn path_components(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn normalize_resource_reference(path: &str) -> String {
    let mut normalized = normalize_path(path);
    if let Some(stripped) = normalized.strip_prefix("nativepc/") {
        normalized = stripped.to_string();
    }
    normalized
}

fn strip_known_resource_extension(path: &str) -> String {
    for extension in [".tex", ".dds", ".mod3", ".mrl3"] {
        if let Some(stripped) = path.strip_suffix(extension) {
            return stripped.to_string();
        }
    }
    path.to_string()
}

fn file_extension(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn stable_id(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{prefix}-{}", hex_digest(digest)[..16].to_string())
}

fn sha256_text(value: &str) -> String {
    hex_digest(Sha256::digest(value.as_bytes()))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::mod_library::{
        load_mod_analysis_input_from, ModAnalysisInput, ModAnalysisInputFile,
    };
    use serde_json::Value;
    use std::{env, process};

    fn unique_test_root() -> PathBuf {
        env::temp_dir().join(format!(
            "acumod-mod-analysis-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn mrl3_bytes(reference: &str) -> Vec<u8> {
        let texture_table_offset = 40usize;
        let mut bytes = vec![0u8; texture_table_offset + 272];
        bytes[0..4].copy_from_slice(&5_001_805u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&(texture_table_offset as u64).to_le_bytes());
        let start = texture_table_offset + 16;
        bytes[start..start + reference.len()].copy_from_slice(reference.as_bytes());
        bytes
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

    #[test]
    fn extracts_sobjl_paths_without_claiming_object_contents() {
        let bytes = b"rSetObject\0quest/test_object.sobj\0";
        assert_eq!(
            read_sobjl_object_paths(bytes).unwrap(),
            ["quest/test_object.sobj"]
        );
        assert!(read_sobjl_object_paths(b"quest/test_object.sobj\0").is_err());
    }

    fn find_real_sample_id(installed_root: &Path, name_fragment: &str) -> String {
        fs::read_dir(installed_root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .find_map(|entry| {
                let content = fs::read_to_string(entry.path().join("manifest.json")).ok()?;
                let manifest = serde_json::from_str::<Value>(&content).ok()?;
                manifest["name"]
                    .as_str()
                    .filter(|name| name.contains(name_fragment))
                    .and_then(|_| manifest["id"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| panic!("本地 MOD 库中缺少回归样本：{name_fragment}"))
    }

    #[test]
    fn classifies_common_mhw_resources_without_guessing_unknown_files() {
        assert_eq!(classify_role("nativepc/a.mod3").role, "model");
        assert_eq!(classify_role("nativepc/a.mrl3").role, "material");
        assert_eq!(classify_role("nativepc/a.dds").role, "textureSource");
        assert_eq!(classify_role("nativepc/plugins/a.dll").role, "plugin");
        assert_eq!(classify_role("nativepc/a.custom").role, "unknown");
    }

    #[test]
    fn builds_verified_mrl3_and_evam_resource_edges() {
        let root = unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let model = root.join("model.mod3");
        let material = root.join("model.mrl3");
        let texture = root.join("body_bm.tex");
        let evam = root.join("body.evam");
        let slinger = root.join("slinger.mod3");
        fs::write(&model, b"model").unwrap();
        fs::write(
            &material,
            mrl3_bytes("pl/f_equip/pl105_0000/body/tex/f_body105_BM"),
        )
        .unwrap();
        fs::write(&texture, b"texture").unwrap();
        fs::write(&evam, evam_bytes(128)).unwrap();
        fs::write(&slinger, b"slinger").unwrap();
        let file = |source_path: PathBuf, deploy: &str| ModAnalysisInputFile {
            size_bytes: fs::metadata(&source_path).unwrap().len(),
            source_path,
            library_relative_path: format!("content/{deploy}"),
            source_deploy_relative_path: deploy.to_string(),
            effective_deploy_relative_path: deploy.to_string(),
            excluded_from_deployment: false,
        };
        let input = ModAnalysisInput {
            mod_id: "test-mod".to_string(),
            name: "测试 MOD".to_string(),
            files: vec![
                file(
                    model,
                    "nativePC/pl/f_equip/pl105_0000/body/mod/f_body105_0000.mod3",
                ),
                file(
                    material,
                    "nativePC/pl/f_equip/pl105_0000/body/mrl/f_body105_0000.mrl3",
                ),
                file(
                    texture,
                    "nativePC/pl/f_equip/pl105_0000/body/tex/f_body105_BM.tex",
                ),
                file(
                    evam,
                    "nativePC/pl/f_equip/pl105_0000/arm/mod/f_arm105_0000.evam",
                ),
                file(slinger, "nativePC/wp/slg/slg128_0000/mod/slg128_0000.mod3"),
            ],
            model_replacements: Vec::new(),
        };
        let local = analyze_local_files(&input, &OperationReporter::default()).unwrap();
        let texture_edge = local
            .edges
            .iter()
            .find(|edge| edge.relation == "referencesTexture")
            .unwrap();
        assert!(texture_edge.to_file_id.is_some());
        let slinger_edge = local
            .edges
            .iter()
            .find(|edge| edge.relation == "bindsSlinger")
            .unwrap();
        assert!(slinger_edge.to_file_id.is_some());
        assert_eq!(
            local
                .files
                .iter()
                .find(|file| file.role == "appearanceBinding")
                .unwrap()
                .references,
            ["slg128_0000"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "需要开发机本地 MOD 库中的两个已核对复杂样本"]
    fn analyzes_real_armor_slinger_and_vfx_samples() {
        let installed_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/debug/AcumodData/mods/installed");
        let samples = [
            ("绮梦花嫁", false, 8_usize),
            ("龙神碎羽2版龙珠", true, 10_usize),
        ];
        for (name_fragment, expects_evam, minimum_effect_files) in samples {
            let mod_id = find_real_sample_id(&installed_root, name_fragment);
            let input = load_mod_analysis_input_from(&installed_root, &mod_id).unwrap();
            let local = analyze_local_files(&input, &OperationReporter::default()).unwrap();
            let roles = local
                .files
                .iter()
                .map(|file| file.role.as_str())
                .collect::<HashSet<_>>();
            let component_kinds = local
                .components
                .iter()
                .map(|component| component.kind.as_str())
                .collect::<HashSet<_>>();

            assert!(roles.contains("model"));
            assert!(roles.contains("material"));
            assert!(roles.contains("texture"));
            assert!(roles.contains("effectBinding"));
            assert!(roles.contains("effect"));
            assert!(component_kinds.contains("armor"));
            assert!(component_kinds.contains("slinger"));
            assert!(component_kinds.contains("effect"));
            assert!(
                local
                    .files
                    .iter()
                    .filter(|file| file.role == "effect")
                    .count()
                    >= minimum_effect_files
            );
            assert!(
                local
                    .edges
                    .iter()
                    .any(|edge| edge.relation == "referencesTexture"),
                "{} 应至少解析出一条 MRL3 贴图引用",
                input.name
            );
            if expects_evam {
                assert!(roles.contains("appearanceBinding"));
                assert!(
                    local
                        .edges
                        .iter()
                        .any(|edge| edge.relation == "bindsSlinger"),
                    "{} 应从 EVAM 解析出飞翔爪绑定",
                    input.name
                );
            }
        }
    }
}
