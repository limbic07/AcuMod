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
    model_remap::{read_armor_dat_summary, read_mrl3_texture_paths},
};

const ANALYSIS_SCHEMA_VERSION: u32 = 1;
const ANALYZER_VERSION: u32 = 4;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_PARSER_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RESOURCE_EDGES: usize = 20_000;
const MAX_KNOWLEDGE_QUERIES: usize = 12;
const SHARP_PLUGIN_LOADER_CORE_NAME: &[u8] = b"SharpPluginLoader.Core";

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

/// MOD 库刷新用的轻量特效汇总：只依据部署相对路径，不读取文件内容或写入清单。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRecognitionSummary {
    pub effect_file_count: usize,
    pub local_weapon_effect_count: usize,
    pub global_weapon_effect_count: usize,
    pub global_hit_effect_count: usize,
    pub global_critical_effect_count: usize,
    pub armor_effect_count: usize,
    pub unclassified_effect_count: usize,
}

/// 根据已导入 MOD 的部署路径重建特效汇总；用于库刷新时让识别结果立即可见。
pub fn summarize_effect_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> EffectRecognitionSummary {
    let mut summary = EffectRecognitionSummary::default();
    for path in paths {
        let extension = file_extension(path);
        if !matches!(extension.as_str(), "efx" | "epv3" | "evwp") {
            continue;
        }
        summary.effect_file_count += 1;
        match classify_effect_role(path, &extension).map(|rule| rule.role) {
            Some("localWeaponEffect") | Some("localWeaponEffectBinding") => {
                summary.local_weapon_effect_count += 1;
            }
            Some("globalWeaponEffectBinding") | Some("globalWeaponEffectMapping") => {
                summary.global_weapon_effect_count += 1;
            }
            Some("globalHitEffect") => summary.global_hit_effect_count += 1,
            Some("globalCriticalEffect") => summary.global_critical_effect_count += 1,
            Some("armorEffectBinding") => summary.armor_effect_count += 1,
            _ => summary.unclassified_effect_count += 1,
        }
    }
    summary
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SharpPluginLoaderAssemblyLocation {
    PluginCandidate,
    LoaderComponent,
}

struct DotnetAssemblySummary {
    has_sharp_plugin_loader_core_metadata_name: bool,
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
    let normalized = normalize_path(path);
    if normalized == "winmm.dll" {
        return role(
            "sharpPluginLoaderProxyCandidate",
            "SPL 启动代理候选",
            "游戏根目录的 winmm.dll 是 SharpPluginLoader 的启动代理位置；仅凭同名文件不能确认不存在其它加载器冲突。",
            0.88,
            Some("SharpPluginLoader"),
        );
    }
    if normalized == "loader-config.json" {
        return role(
            "sharpPluginLoaderConfiguration",
            "SPL 配置",
            "loader-config.json 是 SharpPluginLoader 在游戏根目录自动生成的配置文件。",
            0.98,
            Some("SharpPluginLoader"),
        );
    }
    if extension == "dll" {
        if let Some(location) = sharp_plugin_loader_assembly_location(&normalized) {
            return match location {
                SharpPluginLoaderAssemblyLocation::PluginCandidate => role(
                    "sharpPluginLoaderPluginCandidate",
                    "SPL C# 插件候选",
                    "DLL 位于 nativePC/plugins/CSharp；SPL 会枚举此目录及其普通子目录中的 C# 插件。分析器将只读验证其是否为 .NET 程序集并查找 SPL Core 引用。",
                    0.92,
                    Some("SharpPluginLoader"),
                ),
                SharpPluginLoaderAssemblyLocation::LoaderComponent => role(
                    "sharpPluginLoaderLoaderComponent",
                    "SPL 加载器组件",
                    "DLL 位于 nativePC/plugins/CSharp/Loader；该目录用于加载器自身组件，普通 C# 插件放在这里不会被 SPL 枚举加载。",
                    0.96,
                    Some("SharpPluginLoader"),
                ),
            };
        }
    }
    if normalized == "lua_framework.dll" || normalized.starts_with("lua_framework/") {
        return match extension.as_str() {
            "dll" => role(
                "luaRuntime",
                "Lua 运行框架",
                "文件属于 Lua Framework 的加载器或原生扩展；它是运行时依赖，不是 nativePC 模型资源。",
                0.95,
                Some("Lua Framework"),
            ),
            "lua" => role(
                "luaScript",
                "Lua Framework 脚本",
                "文件位于 Lua Framework 目录，由该运行框架按其自身规则加载；当前不会解析脚本行为。",
                0.95,
                Some("Lua Framework"),
            ),
            "ini" | "cfg" | "json" => role(
                "luaRuntimeConfiguration",
                "Lua Framework 配置",
                "文件位于 Lua Framework 目录，属于运行框架或其脚本的配置/数据。",
                0.9,
                Some("Lua Framework"),
            ),
            _ => role(
                "luaRuntimeResource",
                "Lua Framework 资源",
                "文件位于 Lua Framework 目录，可能是运行框架的脚本、字体或扩展资源；当前不解析其内部用途。",
                0.75,
                Some("Lua Framework"),
            ),
        };
    }
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
    if matches!(extension.as_str(), "efx" | "epv3" | "evwp") {
        if let Some(effect_role) = classify_effect_role(path, &extension) {
            return effect_role;
        }
    }
    match extension.as_str() {
        "am_dat" if normalized.ends_with("common/equip/armor.am_dat") => role(
            "armorMappingTable",
            "全局防具映射表",
            "armor.am_dat 是全局防具数据表；它可把装备记录关联到主模型编号，不能作为普通单套防具文件随意改名部署。",
            0.98,
            Some("armor.am_dat"),
        ),
        "am_dat" => role(
            "armorDataTable",
            "防具数据表",
            "AM_DAT 是防具数据资源；只有规范 armor.am_dat 路径才会使用已验证的防具映射解析器。",
            0.85,
            Some("armor.am_dat"),
        ),
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
        "eq_crt" | "cat_skill" | "deco" | "dglt" | "diot" | "cus_otr" | "dtt_rsz"
        | "rod_inse" | "slt" => role(
            "gameDataTable",
            "游戏数据表",
            "该扩展名在 MHW 资源表中对应装备、技能、装饰珠、怪物或商店等数据类别；当前只确认类别，不读取字段或推断游戏效果。",
            0.9,
            Some("MHW 游戏数据表"),
        ),
        "wwbk" | "wwct" => role(
            "audioMetadata",
            "Wwise 音频元数据",
            "该文件是 Wwise 的列表或容器配置，不等同于 NBNK/NPCK 音频内容；当前不解析事件或声音 ID。",
            0.88,
            Some("Wwise 音频元数据"),
        ),
        "epvsp" => role(
            "effectSoundParameter",
            "特效声音参数",
            "EPVSP 在 MHW 资源表中对应特效声音参数；当前只确认其类别，不能据此推断会播放的声音或触发条件。",
            0.9,
            Some("EPVSP 特效声音参数"),
        ),
        "cms" => role(
            "cameraParameter",
            "镜头设置",
            "CMS 在 MHW 资源表中对应镜头设置；当前不会解析镜头数值、调用时机或影响范围。",
            0.9,
            Some("MHW 镜头与界面资源"),
        ),
        "gui" => role(
            "interfaceResource",
            "界面资源",
            "GUI 是游戏界面资源类别；当前只确认文件类别，不解析控件、文本或调用关系。",
            0.9,
            Some("MHW 镜头与界面资源"),
        ),
        "sdl" => role(
            "schedulerData",
            "调度数据",
            "SDL 在 MHW 资源表中对应调度资源；当前不解析调度事件、条件或运行效果。",
            0.85,
            Some("MHW 镜头与界面资源"),
        ),
        "shlp" => role(
            "shellParameter",
            "弹药/炮弹参数",
            "SHLP 在 MHW 资源表中对应 Shell 参数；当前不读取弹药数值、攻击或动作字段。",
            0.9,
            Some("SHLP 弹药参数"),
        ),
        "otf" => role(
            "fontAsset",
            "字体资源",
            "OTF 是字体文件，可能为插件或界面资源提供字形；不是 MHW 原生模型或贴图。",
            0.9,
            None,
        ),
        "bak" | "old" => role(
            "backupArtifact",
            "备份文件",
            "文件扩展名表明它是备份副本；除非已有插件运行证据，不能当作 MHW 原生运行资源。",
            0.9,
            None,
        ),
        "bat" => role(
            "utilityScript",
            "附带脚本",
            "批处理脚本通常用于安装、转换或维护；AcuMOD 不会执行它，也不把它当作游戏资源。",
            0.95,
            None,
        ),
        "lib" => role(
            "developmentArtifact",
            "开发库文件",
            "LIB 通常是开发或链接产物，不是 MHW 原生运行资源；当前不会根据文件名判断其是否被某个工具使用。",
            0.8,
            None,
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

/// 判断路径是否属于 SPL 约定的 C# 程序集目录。
///
/// Loader 子目录并非普通插件枚举目录，必须单独保留这一层语义，避免把加载器
/// 自身组件或安装位置错误的插件都报告成“会自动加载”。
fn sharp_plugin_loader_assembly_location(
    normalized_path: &str,
) -> Option<SharpPluginLoaderAssemblyLocation> {
    if !normalized_path.ends_with(".dll") {
        return None;
    }
    let parts = path_components(normalized_path);
    let stripped = parts
        .strip_prefix(&["nativepc"])
        .unwrap_or(parts.as_slice());
    let tail = stripped.strip_prefix(&["plugins", "csharp"])?;
    Some(if tail.first() == Some(&"loader") {
        SharpPluginLoaderAssemblyLocation::LoaderComponent
    } else {
        SharpPluginLoaderAssemblyLocation::PluginCandidate
    })
}

/// 特效路径决定影响范围；不能仅按扩展名把全局 EPV 当作单武器资源。
fn classify_effect_role(path: &str, extension: &str) -> Option<RoleRule> {
    let normalized = normalize_path(path);
    if normalized.contains("/vfx/efx/cm/cm_all/") {
        if normalized.contains("cm_critical_") {
            return Some(role(
                "globalCriticalEffect",
                "全局会心命中特效",
                "该 EFX 位于通用命中目录，会影响使用此命中调用的会心效果；不是单把武器特效，禁止自动迁移。",
                0.98,
                Some("MHW 会心 EFX"),
            ));
        }
        return Some(role(
            "globalHitEffect",
            "全局命中特效",
            "该 EFX 位于通用命中目录，可能影响斩击、刺击、弹药、格挡或其它共享命中效果；禁止自动迁移。",
            0.96,
            Some("MHW 通用命中 EFX"),
        ));
    }
    if extension == "efx" && normalized.contains("/vfx/efx/wp_tu/") {
        return Some(role(
            "localWeaponEffect",
            "武器独立特效资源",
            "该 EFX 位于武器本地特效目录；只有索引明确验证目标槽兼容时才可替换，当前分析会保留为待确认资源。",
            0.86,
            Some("MHW 武器独立 EFX"),
        ));
    }
    if extension == "epv3" && normalized.contains("/wp/") && normalized.contains("/epv/") {
        let is_local = normalized.contains("/wp/")
            && normalized
                .split('/')
                .collect::<Vec<_>>()
                .windows(4)
                .any(|parts| parts[0] == "wp" && parts[3] == "epv");
        return Some(if is_local {
            role(
                "localWeaponEffectBinding",
                "武器本地特效触发",
                "该 EPV3 位于具体武器资源旁，通常控制该槽的拔刀或特殊状态效果；替换前仍需验证目标武器槽。",
                0.88,
                Some("MHW 本地 EPV"),
            )
        } else {
            role(
                "globalWeaponEffectBinding",
                "武器类别全局特效触发",
                "该 EPV3 位于武器类别 EPV 目录，可能影响该类别多数武器；禁止自动迁移。",
                0.94,
                Some("MHW 全局 EPV"),
            )
        });
    }
    if extension == "evwp" && normalized.contains("/wp/") {
        return Some(role(
            "globalWeaponEffectMapping",
            "武器全局特效映射",
            "EVWP 决定武器使用的全局 EPV 映射；修改可能影响整个武器类别，禁止自动迁移。",
            0.95,
            Some("MHW EVWP"),
        ));
    }
    if extension == "epv3" && normalized.contains("/pl/") {
        return Some(role(
            "armorEffectBinding",
            "防具特效触发",
            "该 EPV3 随防具部位部署，可能控制常驻、装备或套装触发的视觉效果。",
            0.9,
            Some("MHW 防具 EPV"),
        ));
    }
    None
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
    let normalized = normalize_path(path);
    let from_replacement = || {
        replacements.first().map(|replacement| ComponentDescriptor {
            key: format!("target:{}:{}", replacement.model_kind, replacement.model_id),
            kind: replacement.model_kind.clone(),
            label: replacement_kind_label(&replacement.model_kind).to_string(),
            confidence: 1.0,
        })
    };
    if normalized == "winmm.dll"
        || normalized == "loader-config.json"
        || sharp_plugin_loader_assembly_location(&normalized).is_some()
    {
        return ComponentDescriptor {
            key: "runtime:sharp-plugin-loader".to_string(),
            kind: "sharpPluginLoader".to_string(),
            label: "SharpPluginLoader（SPL）运行环境".to_string(),
            confidence: 0.95,
        };
    }
    if normalized.ends_with("common/equip/armor.am_dat") {
        return ComponentDescriptor {
            key: "global:armor-data".to_string(),
            kind: "globalArmorMapping".to_string(),
            label: "全局防具映射".to_string(),
            confidence: 1.0,
        };
    }
    if normalized == "lua_framework.dll" || normalized.starts_with("lua_framework/") {
        return ComponentDescriptor {
            key: "runtime:lua-framework".to_string(),
            kind: "luaRuntime".to_string(),
            label: "Lua Framework 运行环境".to_string(),
            confidence: 0.95,
        };
    }
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
        } else if role == "armorMappingTable" {
            match read_parser_file(&input_file.source_path, input_file.size_bytes)
                .and_then(|bytes| read_armor_dat_summary(&bytes))
            {
                Ok(summary) => {
                    let references = summary
                        .slots
                        .iter()
                        .map(|slot| {
                            let part = slot
                                .part
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("未知部位({})", slot.equip_slot));
                            format!(
                                "{part}：{} 条记录，{} 个主模型编号",
                                slot.entry_count, slot.unique_model_id_count
                            )
                        })
                        .collect::<Vec<_>>();
                    files[file_index_value].references = references.clone();
                    files[file_index_value].evidence.push(ModAnalysisEvidence {
                        kind: "binaryParser".to_string(),
                        detail: format!(
                            "armor.am_dat 已通过头部和固定记录长度校验，共 {} 条防具映射记录、{} 个部位。",
                            summary.entry_count,
                            summary.slots.len()
                        ),
                        confidence: 1.0,
                    });
                    for (slot, reference) in summary.slots.iter().zip(references) {
                        let part = slot
                            .part
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("未知部位({})", slot.equip_slot));
                        push_edge(
                            &mut edges,
                            &mut seen,
                            ModResourceEdge {
                                from_file_id: files[file_index_value].file_id.clone(),
                                to_file_id: None,
                                target_reference: reference,
                                relation: "mapsArmorSlot".to_string(),
                                relation_label: "影响防具部位映射".to_string(),
                                evidence: format!(
                                    "armor.am_dat 二进制记录表；{part} 部位包含 {} 条记录。",
                                    slot.entry_count
                                ),
                                confidence: 1.0,
                            },
                        );
                    }
                }
                Err(error) => warnings.push(format!(
                    "无法解析 {} 的全局防具映射：{error}",
                    files[file_index_value].effective_deploy_relative_path
                )),
            }
        } else if matches!(
            role.as_str(),
            "sharpPluginLoaderPluginCandidate" | "sharpPluginLoaderLoaderComponent"
        ) {
            match read_parser_file(&input_file.source_path, input_file.size_bytes)
                .and_then(|bytes| read_dotnet_assembly_summary(&bytes))
            {
                Ok(summary) => {
                    files[file_index_value]
                        .references
                        .push(".NET CLI 程序集".to_string());
                    files[file_index_value].evidence.push(ModAnalysisEvidence {
                        kind: "binaryParser".to_string(),
                        detail: "PE 头与 CLR 数据目录有效，已确认是 .NET CLI 程序集。".to_string(),
                        confidence: 1.0,
                    });
                    if summary.has_sharp_plugin_loader_core_metadata_name {
                        files[file_index_value]
                            .references
                            .push("SharpPluginLoader.Core".to_string());
                        files[file_index_value].evidence.push(ModAnalysisEvidence {
                            kind: "assemblyMetadata".to_string(),
                            detail:
                                "程序集元数据的 #Strings 堆包含 SharpPluginLoader.Core 标识符。"
                                    .to_string(),
                            confidence: 0.94,
                        });
                        let target_file_id = files
                            .iter()
                            .find(|candidate| {
                                normalize_path(&candidate.effective_deploy_relative_path)
                                    .ends_with("/sharppluginloader.core.dll")
                            })
                            .map(|candidate| candidate.file_id.clone());
                        push_edge(
                            &mut edges,
                            &mut seen,
                            ModResourceEdge {
                                from_file_id: files[file_index_value].file_id.clone(),
                                to_file_id: target_file_id,
                                target_reference: "SharpPluginLoader.Core".to_string(),
                                relation: "declaresPluginFrameworkDependency".to_string(),
                                relation_label: "声明 SPL 框架依赖".to_string(),
                                evidence: "已验证 .NET 程序集元数据 #Strings 堆中的 SharpPluginLoader.Core 标识符。"
                                    .to_string(),
                                confidence: 0.94,
                            },
                        );
                    }
                    if role == "sharpPluginLoaderPluginCandidate" {
                        let target_file_id = files
                            .iter()
                            .find(|candidate| {
                                let path =
                                    normalize_path(&candidate.effective_deploy_relative_path);
                                path == "loader-config.json" || path == "winmm.dll"
                            })
                            .map(|candidate| candidate.file_id.clone());
                        push_edge(
                            &mut edges,
                            &mut seen,
                            ModResourceEdge {
                                from_file_id: files[file_index_value].file_id.clone(),
                                to_file_id: target_file_id,
                                target_reference: "SharpPluginLoader 运行环境".to_string(),
                                relation: "requiresPluginLoader".to_string(),
                                relation_label: "依赖 SPL 加载器".to_string(),
                                evidence: "C# 程序集位于 SPL 约定的 nativePC/plugins/CSharp 目录。"
                                    .to_string(),
                                confidence: 0.95,
                            },
                        );
                    }
                }
                Err(error) => warnings.push(format!(
                    "无法验证 {} 是否为 SPL 可加载的 .NET 程序集：{error}",
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

/// 只验证 PE/.NET CLI 头，并在程序集元数据 `#Strings` 堆中查找 SPL Core 标识符。
///
/// 这不是反编译器：不读取类型、方法、代码或用户数据，也不执行程序集。目的仅是
/// 区分 CSharp 目录中可由 SPL 处理的托管程序集与误放入的原生 DLL。
fn read_dotnet_assembly_summary(bytes: &[u8]) -> Result<DotnetAssemblySummary, String> {
    if bytes.len() < 0x40 || bytes.get(0..2) != Some(b"MZ") {
        return Err("缺少 PE 的 MZ 文件头".to_string());
    }
    let pe_offset =
        usize::try_from(read_u32_at(bytes, 0x3c)?).map_err(|_| "PE 偏移超出支持范围。")?;
    let coff_header_offset = pe_offset
        .checked_add(4)
        .ok_or_else(|| "PE 偏移溢出。".to_string())?;
    if bytes.get(pe_offset..coff_header_offset) != Some(b"PE\0\0") {
        return Err("缺少 PE 文件签名".to_string());
    }
    let optional_header_offset = coff_header_offset
        .checked_add(20)
        .ok_or_else(|| "PE 可选头偏移溢出。".to_string())?;
    let optional_header_size_offset = coff_header_offset
        .checked_add(16)
        .ok_or_else(|| "PE 可选头大小字段偏移溢出。".to_string())?;
    let optional_header_size = usize::from(read_u16_at(bytes, optional_header_size_offset)?);
    let optional_header_end = optional_header_offset
        .checked_add(optional_header_size)
        .ok_or_else(|| "PE 可选头大小溢出。".to_string())?;
    if optional_header_end > bytes.len() {
        return Err("PE 可选头长度超出文件范围".to_string());
    }
    let data_directory_offset = match read_u16_at(bytes, optional_header_offset)? {
        0x10b => 96usize,
        0x20b => 112usize,
        magic => return Err(format!("不支持的 PE 可选头魔数 {magic:04X}")),
    };
    let cli_directory_offset = optional_header_offset
        .checked_add(data_directory_offset)
        .and_then(|offset| offset.checked_add(14 * 8))
        .ok_or_else(|| "CLR 数据目录偏移溢出。".to_string())?;
    let cli_directory_end = cli_directory_offset
        .checked_add(8)
        .ok_or_else(|| "CLR 数据目录大小溢出。".to_string())?;
    if cli_directory_end > optional_header_end {
        return Err("PE 可选头没有 CLR 数据目录".to_string());
    }
    let cli_rva = read_u32_at(bytes, cli_directory_offset)?;
    let cli_size_offset = cli_directory_offset
        .checked_add(4)
        .ok_or_else(|| "CLR 数据目录大小字段偏移溢出。".to_string())?;
    if cli_rva == 0 || read_u32_at(bytes, cli_size_offset)? == 0 {
        return Err("PE 未声明 CLR 运行时数据目录".to_string());
    }
    let section_count = usize::from(read_u16_at(
        bytes,
        coff_header_offset
            .checked_sub(2)
            .ok_or_else(|| "PE COFF 头偏移无效。".to_string())?,
    )?);
    let section_table_offset = optional_header_end;
    let cli_offset = pe_rva_to_file_offset(bytes, section_table_offset, section_count, cli_rva)?;
    let metadata_rva = read_u32_at(
        bytes,
        cli_offset
            .checked_add(8)
            .ok_or_else(|| "CLR 元数据 RVA 字段偏移溢出。".to_string())?,
    )?;
    let metadata_size = usize::try_from(read_u32_at(
        bytes,
        cli_offset
            .checked_add(12)
            .ok_or_else(|| "CLR 元数据大小字段偏移溢出。".to_string())?,
    )?)
    .map_err(|_| "CLR 元数据大小超出支持范围。".to_string())?;
    if metadata_rva == 0 || metadata_size == 0 {
        return Err("CLR 头未声明程序集元数据。".to_string());
    }
    let metadata_offset =
        pe_rva_to_file_offset(bytes, section_table_offset, section_count, metadata_rva)?;
    let metadata_end = metadata_offset
        .checked_add(metadata_size)
        .ok_or_else(|| "程序集元数据大小溢出。".to_string())?;
    if metadata_end > bytes.len() {
        return Err("程序集元数据超出文件范围。".to_string());
    }
    Ok(DotnetAssemblySummary {
        has_sharp_plugin_loader_core_metadata_name: dotnet_metadata_strings(
            bytes,
            metadata_offset,
            metadata_end,
        )?
        .windows(SHARP_PLUGIN_LOADER_CORE_NAME.len())
        .any(|window| window == SHARP_PLUGIN_LOADER_CORE_NAME),
    })
}

/// 将 PE 的 RVA 映射到文件偏移。托管程序集的 CLR 头和元数据均按此映射定位，
/// 避免把任意 DLL 字节串误当作 .NET 元数据。
fn pe_rva_to_file_offset(
    bytes: &[u8],
    section_table_offset: usize,
    section_count: usize,
    rva: u32,
) -> Result<usize, String> {
    for index in 0..section_count {
        let section_offset = section_table_offset
            .checked_add(
                index
                    .checked_mul(40)
                    .ok_or_else(|| "PE 节区索引溢出。".to_string())?,
            )
            .ok_or_else(|| "PE 节区偏移溢出。".to_string())?;
        let section_end = section_offset
            .checked_add(40)
            .ok_or_else(|| "PE 节区大小溢出。".to_string())?;
        if section_end > bytes.len() {
            return Err("PE 节区表超出文件范围。".to_string());
        }
        let virtual_size = read_u32_at(bytes, section_offset + 8)?;
        let virtual_address = read_u32_at(bytes, section_offset + 12)?;
        let raw_size = read_u32_at(bytes, section_offset + 16)?;
        let raw_offset = read_u32_at(bytes, section_offset + 20)?;
        let section_span = virtual_size.max(raw_size);
        let section_end_rva = virtual_address
            .checked_add(section_span)
            .ok_or_else(|| "PE 节区 RVA 范围溢出。".to_string())?;
        if rva < virtual_address || rva >= section_end_rva {
            continue;
        }
        let relative = rva - virtual_address;
        if relative >= raw_size {
            return Err("PE RVA 指向节区中没有文件数据的区域。".to_string());
        }
        let file_offset = usize::try_from(raw_offset)
            .map_err(|_| "PE 节区文件偏移超出支持范围。".to_string())?
            .checked_add(usize::try_from(relative).map_err(|_| "PE RVA 超出支持范围。")?)
            .ok_or_else(|| "PE 文件偏移溢出。".to_string())?;
        if file_offset >= bytes.len() {
            return Err("PE RVA 映射超出文件范围。".to_string());
        }
        return Ok(file_offset);
    }
    Err("PE 节区表中找不到 RVA 对应的数据。".to_string())
}

/// 读取 CLR 元数据根和 `#Strings` 堆；只读取元数据目录，不解析或执行程序集代码。
fn dotnet_metadata_strings<'a>(
    bytes: &'a [u8],
    metadata_offset: usize,
    metadata_end: usize,
) -> Result<&'a [u8], String> {
    if read_u32_at(bytes, metadata_offset)? != 0x424a_5342 {
        return Err("CLR 元数据缺少 BSJB 签名。".to_string());
    }
    let version_length = usize::try_from(read_u32_at(bytes, metadata_offset + 12)?)
        .map_err(|_| "CLR 元数据版本长度超出支持范围。".to_string())?;
    let stream_count_offset = align_to_four(
        metadata_offset
            .checked_add(16)
            .and_then(|offset| offset.checked_add(version_length))
            .ok_or_else(|| "CLR 元数据版本字段溢出。".to_string())?,
    )?;
    let stream_headers_offset = stream_count_offset
        .checked_add(4)
        .ok_or_else(|| "CLR 元数据流头偏移溢出。".to_string())?;
    if stream_headers_offset > metadata_end {
        return Err("CLR 元数据流头超出范围。".to_string());
    }
    let stream_count = usize::from(read_u16_at(
        bytes,
        stream_count_offset
            .checked_add(2)
            .ok_or_else(|| "CLR 元数据流数量字段偏移溢出。".to_string())?,
    )?);
    let mut header_offset = stream_headers_offset;
    for _ in 0..stream_count {
        let name_offset = header_offset
            .checked_add(8)
            .ok_or_else(|| "CLR 元数据流名称偏移溢出。".to_string())?;
        if name_offset >= metadata_end {
            return Err("CLR 元数据流名称超出范围。".to_string());
        }
        let name_end = bytes[name_offset..metadata_end]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| name_offset + offset)
            .ok_or_else(|| "CLR 元数据流名称未终止。".to_string())?;
        let name = std::str::from_utf8(&bytes[name_offset..name_end])
            .map_err(|_| "CLR 元数据流名称不是 UTF-8。".to_string())?;
        let next_header_offset = align_to_four(
            name_end
                .checked_add(1)
                .ok_or_else(|| "CLR 元数据流名称长度溢出。".to_string())?,
        )?;
        if next_header_offset > metadata_end {
            return Err("CLR 元数据流头填充超出范围。".to_string());
        }
        if name == "#Strings" {
            let stream_relative_offset = usize::try_from(read_u32_at(bytes, header_offset)?)
                .map_err(|_| "CLR 字符串堆偏移超出支持范围。".to_string())?;
            let stream_size = usize::try_from(read_u32_at(bytes, header_offset + 4)?)
                .map_err(|_| "CLR 字符串堆大小超出支持范围。".to_string())?;
            let stream_start = metadata_offset
                .checked_add(stream_relative_offset)
                .ok_or_else(|| "CLR 字符串堆偏移溢出。".to_string())?;
            let stream_end = stream_start
                .checked_add(stream_size)
                .ok_or_else(|| "CLR 字符串堆大小溢出。".to_string())?;
            return bytes
                .get(stream_start..stream_end)
                .filter(|_| stream_end <= metadata_end)
                .ok_or_else(|| "CLR 字符串堆超出元数据范围。".to_string());
        }
        header_offset = next_header_offset;
    }
    Err("CLR 元数据中未找到 #Strings 堆。".to_string())
}

fn align_to_four(value: usize) -> Result<usize, String> {
    value
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| "四字节对齐偏移溢出。".to_string())
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| "PE 字段偏移溢出。".to_string())?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| "PE 字段越过文件末尾".to_string())?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "PE 字段偏移溢出。".to_string())?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| "PE 字段越过文件末尾".to_string())?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
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

    /// 仅写入已由模型改绑模块验证的字段偏移，用于确认分析器不会把 DAT 当普通文件。
    fn armor_dat_bytes(entries: &[(u16, u16, u8, u16)]) -> Vec<u8> {
        let mut bytes = vec![0; 10 + entries.len() * 60];
        bytes[4..6].copy_from_slice(&0x005Fu16.to_le_bytes());
        bytes[6..10].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (index, (set_id, main_model_id, equip_slot, set_group)) in entries.iter().enumerate() {
            let offset = 10 + index * 60;
            bytes[offset + 7..offset + 9].copy_from_slice(&set_id.to_le_bytes());
            bytes[offset + 10] = *equip_slot;
            bytes[offset + 13..offset + 15].copy_from_slice(&main_model_id.to_le_bytes());
            bytes[offset + 53..offset + 55].copy_from_slice(&set_group.to_le_bytes());
        }
        bytes
    }

    /// 仅构造读取 PE/CLR 头和元数据字符串堆所需的最小程序集，不含任何可执行代码。
    fn dotnet_assembly_bytes(has_spl_core_name: bool) -> Vec<u8> {
        const PE_OFFSET: usize = 0x80;
        const COFF_OFFSET: usize = PE_OFFSET + 4;
        const OPTIONAL_OFFSET: usize = COFF_OFFSET + 20;
        const OPTIONAL_SIZE: usize = 0xf0;
        const SECTION_OFFSET: usize = OPTIONAL_OFFSET + OPTIONAL_SIZE;
        const RAW_OFFSET: usize = 0x200;
        const METADATA_OFFSET: usize = 0x280;
        const STRINGS_OFFSET: usize = 0x2c0;

        let mut bytes = vec![0u8; 0x400];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&(PE_OFFSET as u32).to_le_bytes());
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        bytes[COFF_OFFSET + 2..COFF_OFFSET + 4].copy_from_slice(&1u16.to_le_bytes());
        bytes[COFF_OFFSET + 16..COFF_OFFSET + 18]
            .copy_from_slice(&(OPTIONAL_SIZE as u16).to_le_bytes());
        bytes[OPTIONAL_OFFSET..OPTIONAL_OFFSET + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        let cli_directory_offset = OPTIONAL_OFFSET + 112 + 14 * 8;
        bytes[cli_directory_offset..cli_directory_offset + 4]
            .copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[cli_directory_offset + 4..cli_directory_offset + 8]
            .copy_from_slice(&0x48u32.to_le_bytes());

        bytes[SECTION_OFFSET + 8..SECTION_OFFSET + 12].copy_from_slice(&0x400u32.to_le_bytes());
        bytes[SECTION_OFFSET + 12..SECTION_OFFSET + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[SECTION_OFFSET + 16..SECTION_OFFSET + 20].copy_from_slice(&0x400u32.to_le_bytes());
        bytes[SECTION_OFFSET + 20..SECTION_OFFSET + 24]
            .copy_from_slice(&(RAW_OFFSET as u32).to_le_bytes());

        bytes[RAW_OFFSET..RAW_OFFSET + 4].copy_from_slice(&0x48u32.to_le_bytes());
        bytes[RAW_OFFSET + 8..RAW_OFFSET + 12].copy_from_slice(&0x1080u32.to_le_bytes());
        bytes[RAW_OFFSET + 12..RAW_OFFSET + 16].copy_from_slice(&0x80u32.to_le_bytes());

        bytes[METADATA_OFFSET..METADATA_OFFSET + 4].copy_from_slice(&0x424a_5342u32.to_le_bytes());
        bytes[METADATA_OFFSET + 12..METADATA_OFFSET + 16].copy_from_slice(&4u32.to_le_bytes());
        bytes[METADATA_OFFSET + 16..METADATA_OFFSET + 20].copy_from_slice(b"v4\0\0");
        bytes[METADATA_OFFSET + 22..METADATA_OFFSET + 24].copy_from_slice(&1u16.to_le_bytes());
        bytes[METADATA_OFFSET + 24..METADATA_OFFSET + 28].copy_from_slice(&0x40u32.to_le_bytes());
        let string_bytes = if has_spl_core_name {
            b"\0SharpPluginLoader.Core\0".as_slice()
        } else {
            b"\0Example.Plugin\0".as_slice()
        };
        bytes[METADATA_OFFSET + 28..METADATA_OFFSET + 32]
            .copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());
        bytes[METADATA_OFFSET + 32..METADATA_OFFSET + 41].copy_from_slice(b"#Strings\0");
        bytes[STRINGS_OFFSET..STRINGS_OFFSET + string_bytes.len()].copy_from_slice(string_bytes);
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

    #[test]
    fn verifies_spl_assembly_layout_and_metadata_without_executing_plugin_code() {
        let root = unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let proxy = root.join("winmm.dll");
        let config = root.join("loader-config.json");
        let plugin = root.join("example-plugin.dll");
        let core = root.join("SharpPluginLoader.Core.dll");
        fs::write(&proxy, b"SPL proxy candidate").unwrap();
        fs::write(&config, b"{}").unwrap();
        fs::write(&plugin, dotnet_assembly_bytes(true)).unwrap();
        fs::write(&core, dotnet_assembly_bytes(false)).unwrap();
        let file = |source_path: PathBuf, deploy: &str| ModAnalysisInputFile {
            size_bytes: fs::metadata(&source_path).unwrap().len(),
            source_path,
            library_relative_path: format!("content/{deploy}"),
            source_deploy_relative_path: deploy.to_string(),
            effective_deploy_relative_path: deploy.to_string(),
            excluded_from_deployment: false,
        };
        let input = ModAnalysisInput {
            mod_id: "spl-test".to_string(),
            name: "SPL 解析测试".to_string(),
            files: vec![
                file(proxy, "winmm.dll"),
                file(config, "loader-config.json"),
                file(plugin, "nativePC/plugins/CSharp/Example/ExamplePlugin.dll"),
                file(
                    core,
                    "nativePC/plugins/CSharp/Loader/SharpPluginLoader.Core.dll",
                ),
            ],
            model_replacements: Vec::new(),
        };

        let local = analyze_local_files(&input, &OperationReporter::default()).unwrap();
        let plugin = local
            .files
            .iter()
            .find(|file| file.role == "sharpPluginLoaderPluginCandidate")
            .unwrap();
        assert_eq!(
            classify_role("winmm.dll").role,
            "sharpPluginLoaderProxyCandidate"
        );
        assert_eq!(
            classify_role("loader-config.json").role,
            "sharpPluginLoaderConfiguration"
        );
        assert_eq!(
            classify_role("nativePC/plugins/CSharp/Loader/Example.dll").role,
            "sharpPluginLoaderLoaderComponent"
        );
        assert!(plugin
            .references
            .iter()
            .any(|value| value == ".NET CLI 程序集"));
        assert!(plugin
            .references
            .iter()
            .any(|value| value == "SharpPluginLoader.Core"));
        assert_eq!(plugin.component_label, "SharpPluginLoader（SPL）运行环境");
        assert!(local.edges.iter().any(|edge| {
            edge.from_file_id == plugin.file_id
                && edge.relation == "declaresPluginFrameworkDependency"
                && edge.to_file_id.is_some()
        }));
        assert!(local.edges.iter().any(|edge| {
            edge.from_file_id == plugin.file_id
                && edge.relation == "requiresPluginLoader"
                && edge.to_file_id.is_some()
        }));
        fs::remove_dir_all(root).unwrap();
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
        assert_eq!(
            classify_role("nativePC/plugins/CSharp/Example.dll").role,
            "sharpPluginLoaderPluginCandidate"
        );
        assert_eq!(
            classify_role("nativepc/common/equip/armor.am_dat").role,
            "armorMappingTable"
        );
        assert_eq!(
            classify_role("nativepc/common/equip/armor.eq_crt").role,
            "gameDataTable"
        );
        assert_eq!(
            classify_role("lua_framework/scripts/example.lua").role,
            "luaScript"
        );
        assert_eq!(
            classify_role("nativepc/sound/weapon.epvsp").role,
            "effectSoundParameter"
        );
        assert_eq!(
            classify_role("nativepc/ui/menu.gui").role,
            "interfaceResource"
        );
        assert_eq!(
            classify_role("nativepc/wp/two/shell.shlp").role,
            "shellParameter"
        );
        assert_eq!(classify_role("nativepc/a.custom").role, "unknown");
    }

    #[test]
    fn reads_verified_armor_dat_as_global_slot_mappings() {
        let root = unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let dat = root.join("armor.am_dat");
        fs::write(
            &dat,
            armor_dat_bytes(&[(250, 106, 0, 300), (251, 106, 0, 300), (250, 107, 1, 300)]),
        )
        .unwrap();
        let input = ModAnalysisInput {
            mod_id: "dat-test".to_string(),
            name: "DAT 映射测试".to_string(),
            files: vec![ModAnalysisInputFile {
                size_bytes: fs::metadata(&dat).unwrap().len(),
                source_path: dat,
                library_relative_path: "content/nativePC/common/equip/armor.am_dat".to_string(),
                source_deploy_relative_path: "nativePC/common/equip/armor.am_dat".to_string(),
                effective_deploy_relative_path: "nativePC/common/equip/armor.am_dat".to_string(),
                excluded_from_deployment: false,
            }],
            model_replacements: Vec::new(),
        };
        let local = analyze_local_files(&input, &OperationReporter::default()).unwrap();
        let dat = &local.files[0];
        assert_eq!(dat.role, "armorMappingTable");
        assert_eq!(dat.component_label, "全局防具映射");
        assert_eq!(dat.references.len(), 2);
        assert!(dat
            .references
            .iter()
            .any(|reference| reference.starts_with("head：2 条记录")));
        assert!(dat
            .references
            .iter()
            .any(|reference| reference.starts_with("body：1 条记录")));
        assert_eq!(local.edges.len(), 2);
        assert!(local.edges.iter().all(|edge| {
            edge.relation == "mapsArmorSlot" && edge.to_file_id.is_none() && edge.confidence == 1.0
        }));
        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn classifies_global_and_local_effect_paths_without_overstating_scope() {
        assert_eq!(
            super::classify_role("nativePC/vfx/efx/cm/cm_all/cm_critical_000.efx").role,
            "globalCriticalEffect"
        );
        assert_eq!(
            super::classify_role("nativePC/wp/two/epv/bs_two.epv3").role,
            "globalWeaponEffectBinding"
        );
        assert_eq!(
            super::classify_role("nativePC/wp/two/two010/epv/two010.epv3").role,
            "localWeaponEffectBinding"
        );
        assert_eq!(
            super::classify_role("nativePC/vfx/efx/wp_TU/two/two010/two010_003.efx").role,
            "localWeaponEffect"
        );
    }

    #[test]
    fn summarizes_effect_paths_for_library_refresh_without_reading_file_contents() {
        let summary = super::summarize_effect_paths([
            "nativePC/vfx/efx/cm/cm_all/cm_critical_000.efx",
            "nativePC/wp/two/two010/epv/two010.epv3",
            "nativePC/wp/two/epv/bs_two.epv3",
            "nativePC/wp/two/two010/mod/two010.evwp",
            "nativePC/weapon/ignored.mod3",
        ]);
        assert_eq!(summary.effect_file_count, 4);
        assert_eq!(summary.global_critical_effect_count, 1);
        assert_eq!(summary.local_weapon_effect_count, 1);
        assert_eq!(summary.global_weapon_effect_count, 2);
        assert_eq!(summary.unclassified_effect_count, 0);
    }
}
