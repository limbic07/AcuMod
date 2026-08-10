use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::operations::OperationReporter;
use crate::services::mod_library::extract_archive_with_bundled_7zip;

const INDEX_SCHEMA_VERSION: u32 = 1;
const PACK_SCHEMA_VERSION: u32 = 1;
const PACK_APPLICATION_ID: i64 = 0x4143_554B;
const MAX_PACK_SIZE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_QUERY_CHARS: usize = 200;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_STRUCTURED_DATA_BYTES: usize = 256 * 1024;
const MAX_FTS_FALLBACK_TERMS: usize = 48;

const FTS_SEARCH_SQL: &str = "SELECT f.result_id, f.result_kind, f.domain, f.title, snippet(knowledge_fts, 4, '', '', '…', 36), COALESCE(e.game_version, d.game_version, ''), COALESCE(e.confidence, d.confidence, 0.5), s.title, s.url FROM knowledge_fts f LEFT JOIN entities e ON f.result_kind = 'entity' AND e.id = f.result_id LEFT JOIN documents d ON f.result_kind = 'document' AND d.id = f.result_id LEFT JOIN sources s ON s.id = COALESCE(e.source_id, d.source_id) WHERE knowledge_fts MATCH ?1 ORDER BY bm25(knowledge_fts) LIMIT ?2";
const LIKE_SEARCH_SQL: &str = "SELECT f.result_id, f.result_kind, f.domain, f.title, substr(f.body, 1, 240), COALESCE(e.game_version, d.game_version, ''), COALESCE(e.confidence, d.confidence, 0.5), s.title, s.url FROM knowledge_fts f LEFT JOIN entities e ON f.result_kind = 'entity' AND e.id = f.result_id LEFT JOIN documents d ON f.result_kind = 'document' AND d.id = f.result_id LEFT JOIN sources s ON s.id = COALESCE(e.source_id, d.source_id) WHERE (f.title LIKE ?1 ESCAPE '\\' OR f.body LIKE ?1 ESCAPE '\\') ORDER BY CASE WHEN f.title LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END, length(f.title), f.title, f.result_id LIMIT ?2";
const EMBEDDED_ALIAS_SEARCH_SQL: &str = "SELECT e.id, 'entity', e.domain, e.canonical_name, e.summary, e.game_version, e.confidence, s.title, s.url FROM entities e INNER JOIN aliases a ON a.entity_id = e.id LEFT JOIN sources s ON s.id = e.source_id WHERE length(a.alias) >= 2 AND instr(?1, a.alias) > 0 GROUP BY e.id ORDER BY MAX(length(a.alias)) DESC, length(e.canonical_name), e.canonical_name, e.id LIMIT ?2";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStatus {
    pub packs: Vec<KnowledgePackSummary>,
    pub active_pack_count: usize,
    pub total_size_bytes: u64,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePackSummary {
    pub pack_id: String,
    pub display_name: String,
    pub kind: String,
    pub version: String,
    pub game_version: String,
    pub locale: String,
    pub description: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub installed_at_unix_seconds: u64,
    pub entity_count: usize,
    pub relation_count: usize,
    pub document_count: usize,
    pub source_count: usize,
    pub active: bool,
    pub healthy: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBundleInstallResult {
    pub message: String,
    pub status: KnowledgeStatus,
    pub installed_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeInstallResult {
    message: String,
    installed_pack: KnowledgePackSummary,
    status: KnowledgeStatus,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchResponse {
    pub query: String,
    pub searched_pack_count: usize,
    pub matches: Vec<KnowledgeSearchMatch>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchMatch {
    pub result_id: String,
    pub result_kind: String,
    pub domain: String,
    pub title: String,
    pub snippet: String,
    pub game_version: String,
    pub confidence: f64,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
    pub pack_id: String,
    pub pack_version: String,
    pub pack_kind: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntityAlias {
    pub locale: String,
    pub alias: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntityMatch {
    pub entity_id: String,
    pub kind: String,
    pub domain: String,
    pub canonical_name: String,
    pub name_zh_hans: Option<String>,
    pub name_zh_hant: Option<String>,
    pub summary: String,
    pub game_version: String,
    pub confidence: f64,
    pub data: Value,
    pub aliases: Vec<KnowledgeEntityAlias>,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
    pub pack_id: String,
    pub pack_version: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEntityLookupResponse {
    pub query: String,
    pub searched_pack_count: usize,
    pub matches: Vec<KnowledgeEntityMatch>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRelationMatch {
    pub relation_id: String,
    pub subject_id: String,
    pub subject_name: String,
    pub predicate: String,
    pub object_id: String,
    pub object_name: String,
    pub game_version: String,
    pub confidence: f64,
    pub data: Value,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
    pub pack_id: String,
    pub pack_version: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRelationResponse {
    pub entity_id: String,
    pub direction: String,
    pub searched_pack_count: usize,
    pub relations: Vec<KnowledgeRelationMatch>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeIndex {
    schema_version: u32,
    packs: Vec<InstalledPackRecord>,
}

impl Default for KnowledgeIndex {
    fn default() -> Self {
        Self {
            schema_version: INDEX_SCHEMA_VERSION,
            packs: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledPackRecord {
    pack_id: String,
    display_name: String,
    kind: String,
    version: String,
    game_version: String,
    locale: String,
    description: String,
    relative_path: String,
    sha256: String,
    size_bytes: u64,
    installed_at_unix_seconds: u64,
    entity_count: usize,
    relation_count: usize,
    document_count: usize,
    source_count: usize,
    active: bool,
}

#[derive(Clone)]
struct ValidatedPack {
    pack_id: String,
    display_name: String,
    kind: String,
    version: String,
    game_version: String,
    locale: String,
    description: String,
    entity_count: usize,
    relation_count: usize,
    document_count: usize,
    source_count: usize,
}

/// 读取知识包状态时只依赖本地索引，不访问网络或 MOD/游戏目录。
pub fn get_status() -> Result<KnowledgeStatus, String> {
    let root = knowledge_root()?;
    get_status_from(&root)
}

fn get_status_from(root: &Path) -> Result<KnowledgeStatus, String> {
    let index = load_index(&root)?;
    status_from_index(&root, &index)
}

/// 从一个 ZIP 中发现、校验并安装完整知识包集合。
pub fn install_bundle(
    app: tauri::AppHandle,
    source_path: String,
    progress: &OperationReporter,
) -> Result<KnowledgeBundleInstallResult, String> {
    let root = knowledge_root()?;
    let archive_path = normalized_bundle_path(&source_path)?;
    let archive_metadata =
        fs::metadata(&archive_path).map_err(|error| format!("无法读取知识包 ZIP：{error}"))?;
    if archive_metadata.len() == 0 {
        return Err("知识包 ZIP 为空。".to_string());
    }

    let staging_root = root.join("staging");
    fs::create_dir_all(&staging_root)
        .map_err(|error| format!("无法创建知识包暂存目录：{error}"))?;
    let extraction_root = staging_root.join(format!("bundle-{}", unix_nanos_now()?));
    fs::create_dir_all(&extraction_root)
        .map_err(|error| format!("无法创建知识包解包目录：{error}"))?;

    progress.report("正在解包知识包 ZIP", 0, Some(100), None);
    if let Err(error) =
        extract_archive_with_bundled_7zip(&app, &archive_path, &extraction_root, progress)
    {
        let _ = fs::remove_dir_all(&extraction_root);
        return Err(error);
    }

    let result = install_extracted_bundle(&root, &extraction_root, progress);
    if let Err(error) = fs::remove_dir_all(&extraction_root) {
        if result.is_ok() {
            return Err(format!("知识包已安装，但无法清理解包暂存目录：{error}"));
        }
    }
    result
}

fn install_extracted_bundle(
    root: &Path,
    extraction_root: &Path,
    progress: &OperationReporter,
) -> Result<KnowledgeBundleInstallResult, String> {
    let mut package_paths = Vec::new();
    collect_knowledge_packages(extraction_root, &mut package_paths)?;
    package_paths.sort();

    if package_paths.len() != 4 {
        return Err(format!(
            "知识包 ZIP 必须包含四个 `.acukb` 文件，当前找到 {} 个。",
            package_paths.len()
        ));
    }

    let mut validated_packs = Vec::new();
    let mut kinds = HashSet::new();
    for path in &package_paths {
        let metadata = fs::metadata(path).map_err(|error| format!("无法读取知识包：{error}"))?;
        if metadata.len() == 0 || metadata.len() > MAX_PACK_SIZE_BYTES {
            return Err(format!(
                "知识包 {} 为空或超过 4 GB 安全上限。",
                display_path(path)
            ));
        }
        let validated = validate_pack(path)
            .map_err(|error| format!("知识包 {} 校验失败：{error}", display_path(path)))?;
        if !matches!(
            validated.kind.as_str(),
            "mhw-game-facts" | "mhw-modding" | "mhw-game-guides" | "acumod-help"
        ) {
            return Err(format!(
                "知识包 {} 的类型 `{}` 不是完整知识包支持的四类之一。",
                display_path(path),
                validated.kind
            ));
        }
        if !kinds.insert(validated.kind.clone()) {
            return Err(format!("知识包 ZIP 中存在重复类型 `{}`。", validated.kind));
        }
        validated_packs.push((path.clone(), validated));
    }

    let required_kinds = [
        "mhw-game-facts",
        "mhw-modding",
        "mhw-game-guides",
        "acumod-help",
    ];
    if required_kinds.iter().any(|kind| !kinds.contains(*kind)) {
        return Err(
            "知识包 ZIP 缺少完整知识库所需的游戏事实、MOD 技术、攻略或 Acumod 说明包。".to_string(),
        );
    }

    let total = validated_packs.len();
    let mut status = None;
    for (index, (path, _)) in validated_packs.into_iter().enumerate() {
        progress.report(
            "正在安装整套知识包",
            index,
            Some(total),
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string),
        );
        let result = install_pack_into(root, path.to_string_lossy().into_owned(), progress)?;
        status = Some(result.status);
    }

    Ok(KnowledgeBundleInstallResult {
        message: format!("已安装整套知识包，共 {} 个。", total),
        status: status.ok_or_else(|| "没有可安装的知识包。".to_string())?,
        installed_count: total,
    })
}

fn collect_knowledge_packages(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|error| format!("无法读取知识包解包目录：{error}"))?
    {
        let path = entry
            .map_err(|error| format!("无法读取知识包解包条目：{error}"))?
            .path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("无法检查知识包解包条目：{error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "知识包 ZIP 含有不允许的符号链接：{}",
                display_path(&path)
            ));
        }
        if metadata.is_dir() {
            collect_knowledge_packages(&path, output)?;
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("acukb"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn install_pack_into(
    root: &Path,
    source_path: String,
    progress: &OperationReporter,
) -> Result<KnowledgeInstallResult, String> {
    let source_path = normalized_source_path(&source_path)?;
    let source_metadata =
        fs::metadata(&source_path).map_err(|error| format!("无法读取知识包：{error}"))?;
    if !source_metadata.is_file() {
        return Err("请选择一个 `.acukb` 文件。".to_string());
    }
    if source_metadata.len() == 0 || source_metadata.len() > MAX_PACK_SIZE_BYTES {
        return Err("知识包为空或超过 4 GB 安全上限。".to_string());
    }

    let staging_root = root.join("staging");
    fs::create_dir_all(&staging_root)
        .map_err(|error| format!("无法创建知识包暂存目录：{error}"))?;
    let staging_path = staging_root.join(format!("import-{}.acukb", unix_nanos_now()?));
    let copy_result = copy_and_hash(&source_path, &staging_path, source_metadata.len(), progress);
    let (sha256, size_bytes) = match copy_result {
        Ok(result) => result,
        Err(error) => {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
    };

    progress.report("正在校验知识包结构", 0, None, None);
    let validated = match validate_pack(&staging_path) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&staging_path);
            return Err(error);
        }
    };

    let pack_directory = root.join("packs").join(&validated.pack_id);
    fs::create_dir_all(&pack_directory).map_err(|error| format!("无法创建知识包目录：{error}"))?;
    let destination_name = format!(
        "{}-{}.acukb",
        validated.version,
        sha256.chars().take(12).collect::<String>()
    );
    let destination_path = pack_directory.join(&destination_name);
    let destination_created = if destination_path.exists() {
        fs::remove_file(&staging_path)
            .map_err(|error| format!("无法清理重复知识包暂存文件：{error}"))?;
        false
    } else {
        fs::rename(&staging_path, &destination_path)
            .map_err(|error| format!("无法启用已校验的知识包：{error}"))?;
        true
    };

    let relative_path = relative_pack_path(&validated.pack_id, &destination_name);
    let installed_at_unix_seconds = unix_seconds_now()?;
    let mut index = load_index(&root)?;
    for pack in &mut index.packs {
        if pack.pack_id == validated.pack_id {
            pack.active = false;
        }
    }
    index.packs.retain(|pack| {
        !(pack.pack_id == validated.pack_id
            && pack.version == validated.version
            && pack.sha256 == sha256)
    });
    let record = InstalledPackRecord {
        pack_id: validated.pack_id,
        display_name: validated.display_name,
        kind: validated.kind,
        version: validated.version,
        game_version: validated.game_version,
        locale: validated.locale,
        description: validated.description,
        relative_path,
        sha256,
        size_bytes,
        installed_at_unix_seconds,
        entity_count: validated.entity_count,
        relation_count: validated.relation_count,
        document_count: validated.document_count,
        source_count: validated.source_count,
        active: true,
    };
    index.packs.push(record.clone());
    index.packs.sort_by(|left, right| {
        left.pack_id
            .cmp(&right.pack_id)
            .then_with(|| {
                right
                    .installed_at_unix_seconds
                    .cmp(&left.installed_at_unix_seconds)
            })
            .then_with(|| left.sha256.cmp(&right.sha256))
    });
    if let Err(error) = save_index(root, &index) {
        // 索引没有切换成功时，新文件不能留在正式 packs 目录中；旧活动版本
        // 仍由原索引管理，因此这里回退文件即可恢复导入前状态。
        if destination_created {
            let _ = fs::remove_file(&destination_path);
        }
        return Err(error);
    }
    progress.report(
        "知识包已启用",
        1,
        Some(1),
        Some(record.display_name.clone()),
    );

    let installed_pack = summary_from_record(root, &record);
    let status = status_from_index(root, &index)?;
    Ok(KnowledgeInstallResult {
        message: format!(
            "已安装知识包“{}” {}。",
            installed_pack.display_name, installed_pack.version
        ),
        installed_pack,
        status,
    })
}

/// 删除操作只接收经过校验的包 ID，所有目标路径都从受控索引恢复。
pub fn delete_pack(pack_id: &str, progress: &OperationReporter) -> Result<KnowledgeStatus, String> {
    let root = knowledge_root()?;
    delete_pack_from(&root, pack_id, progress)
}

fn delete_pack_from(
    root: &Path,
    pack_id: &str,
    progress: &OperationReporter,
) -> Result<KnowledgeStatus, String> {
    validate_identifier(pack_id, "知识包 ID")?;
    let mut index = load_index(root)?;
    let selected = index
        .packs
        .iter()
        .filter(|pack| pack.pack_id == pack_id)
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("没有找到要删除的知识包。".to_string());
    }
    let delete_staging = root
        .join("staging")
        .join(format!("delete-{}", unix_nanos_now()?));
    fs::create_dir_all(&delete_staging)
        .map_err(|error| format!("无法创建知识包删除暂存目录：{error}"))?;
    let mut moved_files = Vec::new();
    for (position, pack) in selected.iter().enumerate() {
        progress.report(
            "正在删除知识包版本",
            position,
            Some(selected.len()),
            Some(pack.version.clone()),
        );
        let path = installed_pack_path(root, &pack.relative_path)?;
        if path.exists() {
            let backup_path = delete_staging.join(format!("{position}.acukb"));
            if let Err(error) = fs::rename(&path, &backup_path) {
                restore_moved_pack_files(&moved_files);
                let _ = fs::remove_dir_all(&delete_staging);
                return Err(format!("无法暂存待删除知识包：{error}"));
            }
            moved_files.push((path, backup_path));
        }
    }
    index.packs.retain(|pack| pack.pack_id != pack_id);
    if let Err(error) = save_index(root, &index) {
        restore_moved_pack_files(&moved_files);
        let _ = fs::remove_dir_all(&delete_staging);
        return Err(error);
    }
    // 索引切换后，暂存文件已经不可达。清理失败只会留下安全的 staging
    // 残留，不得把已经成功的删除误报为失败。
    let _ = fs::remove_dir_all(&delete_staging);
    let pack_directory = root.join("packs").join(pack_id);
    if pack_directory.is_dir()
        && fs::read_dir(&pack_directory)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    {
        let _ = fs::remove_dir(&pack_directory);
    }
    progress.report("知识包已删除", selected.len(), Some(selected.len()), None);
    status_from_index(root, &index)
}

/// 泛用查询只接收文本、领域和数量，不暴露任意 SQL 能力。
pub fn search(
    query: &str,
    domains: Option<&[String]>,
    limit: usize,
) -> Result<KnowledgeSearchResponse, String> {
    let root = knowledge_root()?;
    search_from(&root, query, domains, limit)
}

fn search_from(
    root: &Path,
    query: &str,
    domains: Option<&[String]>,
    limit: usize,
) -> Result<KnowledgeSearchResponse, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("知识库查询内容不能为空。".to_string());
    }
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(format!(
            "知识库查询不能超过 {MAX_SEARCH_QUERY_CHARS} 个字符。"
        ));
    }
    let normalized_domains = normalize_domains(domains)?;
    let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
    let index = load_index(root)?;
    let active_packs = index
        .packs
        .iter()
        .filter(|pack| pack.active)
        .filter(|pack| {
            normalized_domains.is_empty() || normalized_domains.contains(pack.kind.as_str())
        })
        .collect::<Vec<_>>();
    let mut matches_by_pack = Vec::new();
    let mut warnings = Vec::new();
    for pack in &active_packs {
        let pack_path = installed_pack_path(root, &pack.relative_path)?;
        if !pack_path.is_file() {
            warnings.push(format!("知识包“{}”文件缺失，已跳过。", pack.display_name));
            continue;
        }
        match search_pack(&pack_path, pack, query, limit) {
            Ok(pack_matches) => matches_by_pack.push(pack_matches),
            Err(error) => warnings.push(format!("知识包“{}”查询失败：{error}", pack.display_name)),
        }
    }
    // 跨知识域的问题不能让实体数量巨大的事实包占满全局上限，导致攻略或技术资料完全不可见。
    // 每个包先按自身相关性排序，再以轮转方式合并；指定单一领域时自然退化为原有顺序。
    let max_pack_matches = matches_by_pack.iter().map(Vec::len).max().unwrap_or(0);
    let mut matches = Vec::with_capacity(limit);
    for index in 0..max_pack_matches {
        for pack_matches in &matches_by_pack {
            if matches.len() >= limit {
                break;
            }
            if let Some(item) = pack_matches.get(index) {
                matches.push(item.clone());
            }
        }
        if matches.len() >= limit {
            break;
        }
    }
    Ok(KnowledgeSearchResponse {
        query: query.to_string(),
        searched_pack_count: active_packs.len(),
        matches,
        warnings,
    })
}

/// 按名称、别名或稳定 ID 查询游戏实体，返回后续关系查询需要的精确实体 ID。
pub fn lookup_game_entities(
    query: &str,
    kinds: Option<&[String]>,
    limit: usize,
) -> Result<KnowledgeEntityLookupResponse, String> {
    let root = knowledge_root()?;
    lookup_game_entities_from(&root, query, kinds, limit)
}

fn lookup_game_entities_from(
    root: &Path,
    query: &str,
    kinds: Option<&[String]>,
    limit: usize,
) -> Result<KnowledgeEntityLookupResponse, String> {
    let query = normalized_query(query, "游戏实体查询")?;
    let kind_filter = normalize_fixed_values(kinds, "实体类型", 16)?;
    let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
    let query_variants = entity_query_variants(&query);
    let index = load_index(root)?;
    let active_packs = index
        .packs
        .iter()
        .filter(|pack| pack.active && pack.kind == "mhw-game-facts")
        .collect::<Vec<_>>();
    let mut matches = Vec::new();
    let mut matched_entity_ids = HashSet::new();
    let mut warnings = Vec::new();
    for pack in &active_packs {
        let pack_path = installed_pack_path(root, &pack.relative_path)?;
        if !pack_path.is_file() {
            warnings.push(format!("知识包“{}”文件缺失，已跳过。", pack.display_name));
            continue;
        }
        for lookup_query in &query_variants {
            match lookup_entity_pack(
                &pack_path,
                pack,
                lookup_query,
                &kind_filter,
                limit.saturating_sub(matches.len()),
            ) {
                Ok(mut pack_matches) => {
                    pack_matches.retain(|item| matched_entity_ids.insert(item.entity_id.clone()));
                    matches.append(&mut pack_matches);
                }
                Err(error) => warnings.push(format!(
                    "知识包“{}”实体查询失败：{error}",
                    pack.display_name
                )),
            }
            if matches.len() >= limit {
                break;
            }
        }
        if matches.len() >= limit {
            break;
        }
    }
    matches.truncate(limit);
    Ok(KnowledgeEntityLookupResponse {
        query,
        searched_pack_count: active_packs.len(),
        matches,
        warnings,
    })
}

fn lookup_entity_pack(
    path: &Path,
    pack: &InstalledPackRecord,
    query: &str,
    kind_filter: &str,
    limit: usize,
) -> Result<Vec<KnowledgeEntityMatch>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let connection = open_read_only(path)?;
    let like_query = format!("%{}%", escape_like(query));
    let mut statement = connection
        .prepare(
            "WITH ranked AS (
                SELECT e.id,
                       MIN(CASE
                           WHEN e.id = ?2 COLLATE NOCASE
                             OR e.canonical_name = ?2 COLLATE NOCASE
                             OR e.name_zh_hans = ?2
                             OR e.name_zh_hant = ?2
                             OR a.alias = ?2 COLLATE NOCASE THEN 0
                           ELSE 1
                       END) AS match_rank
                FROM entities e
                LEFT JOIN aliases a ON a.entity_id = e.id
                WHERE (?3 = '' OR instr(?3, char(31) || e.kind || char(31)) > 0)
                  AND (e.id LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                    OR e.canonical_name LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                    OR e.name_zh_hans LIKE ?1 ESCAPE '\\'
                    OR e.name_zh_hant LIKE ?1 ESCAPE '\\'
                    OR a.alias LIKE ?1 ESCAPE '\\' COLLATE NOCASE)
                GROUP BY e.id
                ORDER BY match_rank, length(e.canonical_name), e.canonical_name, e.id
                LIMIT ?4
            )
            SELECT e.id, e.kind, e.domain, e.canonical_name, e.name_zh_hans,
                   e.name_zh_hant, e.summary, e.game_version, e.confidence,
                   e.data_json, s.title, s.url
            FROM ranked r
            JOIN entities e ON e.id = r.id
            LEFT JOIN sources s ON s.id = e.source_id
            ORDER BY r.match_rank, length(e.canonical_name), e.canonical_name, e.id",
        )
        .map_err(|error| format!("无法准备游戏实体查询：{error}"))?;
    let rows = statement
        .query_map(
            params![like_query, query, kind_filter, limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, f64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .map_err(|error| format!("游戏实体查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取游戏实体：{error}"))?;

    let mut alias_statement = connection
        .prepare("SELECT locale, alias FROM aliases WHERE entity_id = ?1 ORDER BY locale, alias")
        .map_err(|error| format!("无法准备实体别名查询：{error}"))?;
    rows.into_iter()
        .map(|row| {
            let aliases = alias_statement
                .query_map([row.0.as_str()], |alias_row| {
                    Ok(KnowledgeEntityAlias {
                        locale: alias_row.get(0)?,
                        alias: alias_row.get(1)?,
                    })
                })
                .map_err(|error| format!("实体 {} 的别名查询失败：{error}", row.0))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("无法读取实体 {} 的别名：{error}", row.0))?;
            let data = serde_json::from_str(&row.9)
                .map_err(|error| format!("实体 {} 的结构化数据无效：{error}", row.0))?;
            Ok(KnowledgeEntityMatch {
                entity_id: row.0,
                kind: row.1,
                domain: row.2,
                canonical_name: row.3,
                name_zh_hans: row.4,
                name_zh_hant: row.5,
                summary: row.6,
                game_version: row.7,
                confidence: row.8,
                data,
                aliases,
                source_title: row.10,
                source_url: row.11,
                pack_id: pack.pack_id.clone(),
                pack_version: pack.version.clone(),
            })
        })
        .collect()
}

/// 查询一个精确游戏实体的入向、出向或双向关系；调用方不能提交 SQL。
pub fn get_game_entity_relations(
    entity_id: &str,
    predicates: Option<&[String]>,
    direction: &str,
    limit: usize,
) -> Result<KnowledgeRelationResponse, String> {
    let root = knowledge_root()?;
    get_game_entity_relations_from(&root, entity_id, predicates, direction, limit)
}

fn get_game_entity_relations_from(
    root: &Path,
    entity_id: &str,
    predicates: Option<&[String]>,
    direction: &str,
    limit: usize,
) -> Result<KnowledgeRelationResponse, String> {
    validate_entity_id(entity_id)?;
    let direction = normalize_relation_direction(direction)?;
    let predicate_filter = normalize_relation_predicates(predicates)?;
    let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
    let index = load_index(root)?;
    let active_packs = index
        .packs
        .iter()
        .filter(|pack| pack.active && pack.kind == "mhw-game-facts")
        .collect::<Vec<_>>();
    let mut relations = Vec::new();
    let mut warnings = Vec::new();
    for pack in &active_packs {
        let pack_path = installed_pack_path(root, &pack.relative_path)?;
        if !pack_path.is_file() {
            warnings.push(format!("知识包“{}”文件缺失，已跳过。", pack.display_name));
            continue;
        }
        match relation_pack(
            &pack_path,
            pack,
            entity_id,
            &predicate_filter,
            direction,
            limit.saturating_sub(relations.len()),
        ) {
            Ok(mut pack_relations) => relations.append(&mut pack_relations),
            Err(error) => warnings.push(format!(
                "知识包“{}”关系查询失败：{error}",
                pack.display_name
            )),
        }
        if relations.len() >= limit {
            break;
        }
    }
    relations.truncate(limit);
    Ok(KnowledgeRelationResponse {
        entity_id: entity_id.to_string(),
        direction: direction.to_string(),
        searched_pack_count: active_packs.len(),
        relations,
        warnings,
    })
}

fn relation_pack(
    path: &Path,
    pack: &InstalledPackRecord,
    entity_id: &str,
    predicate_filter: &str,
    direction: &str,
    limit: usize,
) -> Result<Vec<KnowledgeRelationMatch>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let connection = open_read_only(path)?;
    let mut statement = connection
        .prepare(
            "SELECT r.id, r.subject_id,
                    COALESCE(subject.name_zh_hans, subject.name_zh_hant, subject.canonical_name),
                    r.predicate, r.object_id,
                    COALESCE(object.name_zh_hans, object.name_zh_hant, object.canonical_name),
                    r.game_version, r.confidence, r.data_json, source.title, source.url
             FROM relations r
             JOIN entities subject ON subject.id = r.subject_id
             JOIN entities object ON object.id = r.object_id
             LEFT JOIN sources source ON source.id = r.source_id
             WHERE ((?2 = 'outgoing' AND r.subject_id = ?1)
                 OR (?2 = 'incoming' AND r.object_id = ?1)
                 OR (?2 = 'both' AND (r.subject_id = ?1 OR r.object_id = ?1)))
               AND (?3 = '' OR instr(?3, char(31) || r.predicate || char(31)) > 0)
             ORDER BY CASE WHEN r.subject_id = ?1 THEN 0 ELSE 1 END,
                      r.predicate, r.subject_id, r.object_id, r.id
             LIMIT ?4",
        )
        .map_err(|error| format!("无法准备游戏关系查询：{error}"))?;
    let rows = statement
        .query_map(
            params![entity_id, direction, predicate_filter, limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, f64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            },
        )
        .map_err(|error| format!("游戏关系查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取游戏关系：{error}"))?;
    rows.into_iter()
        .map(|row| {
            let data = serde_json::from_str(&row.8)
                .map_err(|error| format!("关系 {} 的结构化数据无效：{error}", row.0))?;
            Ok(KnowledgeRelationMatch {
                relation_id: row.0,
                subject_id: row.1,
                subject_name: row.2,
                predicate: row.3,
                object_id: row.4,
                object_name: row.5,
                game_version: row.6,
                confidence: row.7,
                data,
                source_title: row.9,
                source_url: row.10,
                pack_id: pack.pack_id.clone(),
                pack_version: pack.version.clone(),
            })
        })
        .collect()
}

fn restore_moved_pack_files(moved_files: &[(PathBuf, PathBuf)]) {
    for (original, backup) in moved_files.iter().rev() {
        if backup.exists() && !original.exists() {
            let _ = fs::rename(backup, original);
        }
    }
}

fn validate_pack(path: &Path) -> Result<ValidatedPack, String> {
    let connection = open_read_only(path)?;
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| format!("无法读取知识包标识：{error}"))?;
    if application_id != PACK_APPLICATION_ID {
        return Err("文件不是 Acumod `.acukb` 知识包。".to_string());
    }
    let schema_version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("无法读取知识包 schema：{error}"))?;
    if schema_version != PACK_SCHEMA_VERSION {
        return Err(format!(
            "知识包 schema {schema_version} 不受支持，当前仅支持 {PACK_SCHEMA_VERSION}。"
        ));
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
        .map_err(|error| format!("知识包完整性检查失败：{error}"))?;
    if integrity != "ok" {
        return Err(format!("知识包已损坏：{integrity}"));
    }
    validate_schema_objects(&connection)?;
    validate_pack_rows(&connection)?;

    let manifest = connection
        .query_row(
            "SELECT pack_id, display_name, kind, version, game_version, locale, min_app_version, description FROM pack_manifest",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .map_err(|error| format!("无法读取知识包 manifest：{error}"))?;
    let manifest_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM pack_manifest", [], |row| row.get(0))
        .map_err(|error| format!("无法核对知识包 manifest：{error}"))?;
    if manifest_count != 1 {
        return Err("知识包必须且只能包含一条 manifest。".to_string());
    }
    validate_identifier(&manifest.0, "知识包 ID")?;
    validate_short_text(&manifest.1, "知识包名称", 120)?;
    validate_pack_kind(&manifest.2)?;
    validate_version(&manifest.3, "知识包版本")?;
    validate_version(&manifest.4, "游戏版本")?;
    validate_short_text(&manifest.5, "语言", 40)?;
    validate_version(&manifest.6, "最低应用版本")?;
    ensure_supported_app_version(&manifest.6)?;
    if manifest.7.chars().count() > 500 {
        return Err("知识包说明过长。".to_string());
    }

    Ok(ValidatedPack {
        pack_id: manifest.0,
        display_name: manifest.1,
        kind: manifest.2,
        version: manifest.3,
        game_version: manifest.4,
        locale: manifest.5,
        description: manifest.7,
        entity_count: table_count(&connection, "entities")?,
        relation_count: table_count(&connection, "relations")?,
        document_count: table_count(&connection, "documents")?,
        source_count: table_count(&connection, "sources")?,
    })
}

fn validate_schema_objects(connection: &Connection) -> Result<(), String> {
    let required = [
        "pack_manifest",
        "sources",
        "entities",
        "aliases",
        "relations",
        "documents",
        "knowledge_fts",
    ];
    let mut statement = connection
        .prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .map_err(|error| format!("无法检查知识包 schema：{error}"))?;
    let objects = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("无法读取知识包 schema：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法解析知识包 schema：{error}"))?;
    let table_names = objects
        .iter()
        .filter(|(kind, _)| kind == "table")
        .map(|(_, name)| name.as_str())
        .collect::<HashSet<_>>();
    if let Some(missing) = required.iter().find(|name| !table_names.contains(**name)) {
        return Err(format!("知识包缺少必要数据表：{missing}"));
    }
    for (kind, name) in objects {
        if kind != "table" && kind != "index" {
            return Err(format!("知识包包含不允许的 schema 对象：{kind} {name}"));
        }
        if kind == "table"
            && !required.contains(&name.as_str())
            && !name.starts_with("knowledge_fts_")
        {
            return Err(format!("知识包包含未知数据表：{name}"));
        }
    }
    Ok(())
}

fn validate_pack_rows(connection: &Connection) -> Result<(), String> {
    let invalid_confidence_count: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM entities WHERE confidence < 0 OR confidence > 1)
              + (SELECT COUNT(*) FROM relations WHERE confidence < 0 OR confidence > 1)
              + (SELECT COUNT(*) FROM documents WHERE confidence < 0 OR confidence > 1)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法校验知识可信度：{error}"))?;
    if invalid_confidence_count > 0 {
        return Err("知识包包含超出 0 到 1 范围的可信度。".to_string());
    }

    validate_json_rows(connection, "entities")?;
    validate_json_rows(connection, "relations")?;

    // 不依赖知识包是否声明外键；固定查询主动拒绝悬空引用。
    let orphan_count: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM aliases a LEFT JOIN entities e ON e.id = a.entity_id WHERE e.id IS NULL)
              + (SELECT COUNT(*) FROM relations r LEFT JOIN entities e ON e.id = r.subject_id WHERE e.id IS NULL)
              + (SELECT COUNT(*) FROM relations r LEFT JOIN entities e ON e.id = r.object_id WHERE e.id IS NULL)
              + (SELECT COUNT(*) FROM entities e LEFT JOIN sources s ON s.id = e.source_id WHERE e.source_id IS NOT NULL AND s.id IS NULL)
              + (SELECT COUNT(*) FROM relations r LEFT JOIN sources s ON s.id = r.source_id WHERE r.source_id IS NOT NULL AND s.id IS NULL)
              + (SELECT COUNT(*) FROM documents d LEFT JOIN sources s ON s.id = d.source_id WHERE d.source_id IS NOT NULL AND s.id IS NULL)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法校验知识包引用关系：{error}"))?;
    if orphan_count > 0 {
        return Err("知识包包含悬空的实体、关系、别名或来源引用。".to_string());
    }
    Ok(())
}

fn validate_json_rows(connection: &Connection, table: &str) -> Result<(), String> {
    let sql = match table {
        "entities" => "SELECT id, data_json FROM entities",
        "relations" => "SELECT id, data_json FROM relations",
        _ => return Err("内部结构化知识表名称无效。".to_string()),
    };
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("无法准备结构化知识校验：{error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("无法读取结构化知识：{error}"))?;
    for row in rows {
        let (id, data_json) = row.map_err(|error| format!("无法解析结构化知识行：{error}"))?;
        if data_json.len() > MAX_STRUCTURED_DATA_BYTES {
            return Err(format!("结构化知识 {id} 超过单条大小上限。"));
        }
        serde_json::from_str::<Value>(&data_json)
            .map_err(|error| format!("结构化知识 {id} 不是有效 JSON：{error}"))?;
    }
    Ok(())
}

fn search_pack(
    path: &Path,
    pack: &InstalledPackRecord,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeSearchMatch>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let connection = open_read_only(path)?;
    // 自然问句往往把完整游戏名和提问词连在一起，无法直接命中 FTS。
    // 先从句子中找出完整出现的本地别名，避免“两字片段”被装备、素材等大量结果淹没。
    let embedded_alias_matches = search_embedded_alias_rows(&connection, pack, query, limit)?;
    let use_fts = query.chars().count() >= 3;
    let (sql, parameter) = if use_fts {
        (FTS_SEARCH_SQL, quoted_fts_query(query))
    } else {
        (LIKE_SEARCH_SQL, format!("%{}%", escape_like(query)))
    };
    let direct_matches = search_rows(&connection, pack, sql, &parameter, limit)?;
    let mut matches = Vec::new();
    let mut seen_result_ids = HashSet::new();
    append_unique_matches(
        &mut matches,
        &mut seen_result_ids,
        embedded_alias_matches,
        limit,
    );
    append_unique_matches(&mut matches, &mut seen_result_ids, direct_matches, limit);
    if !use_fts || matches.len() >= limit {
        return Ok(matches);
    }

    // 长问句通常不会原样出现在资料中。先用二字中文窗口做受控 LIKE 回退，
    // 覆盖“黑龙如何解锁”中的“黑龙”这类短术语；再补三字 FTS 片段和英文术语。
    // 两条路径都使用固定 SQL 与绑定参数，用户文本不会成为 FTS 或 SQL 语法。
    // 直接命中只代表至少有一份资料包含完整问句，不代表问句中的每个术语
    // 都在同一份资料里。继续补充分词结果，才能覆盖 MOD3、MRL3、TEX
    // 这类需要合并多份技术说明才能回答的问题。
    for term in fallback_like_terms(query) {
        let parameter = format!("%{}%", escape_like(&term));
        let term_matches = search_rows(&connection, pack, LIKE_SEARCH_SQL, &parameter, limit)?;
        append_unique_matches(&mut matches, &mut seen_result_ids, term_matches, limit);
        if matches.len() >= limit {
            return Ok(matches);
        }
    }
    if let Some(fallback_query) = fallback_fts_query(query) {
        let fts_matches = search_rows(&connection, pack, FTS_SEARCH_SQL, &fallback_query, limit)?;
        append_unique_matches(&mut matches, &mut seen_result_ids, fts_matches, limit);
    }
    Ok(matches)
}

fn search_embedded_alias_rows(
    connection: &Connection,
    pack: &InstalledPackRecord,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeSearchMatch>, String> {
    let mut matches = Vec::new();
    let mut seen_result_ids = HashSet::new();
    for candidate in embedded_alias_search_queries(query) {
        let mut statement = connection
            .prepare(EMBEDDED_ALIAS_SEARCH_SQL)
            .map_err(|error| format!("无法准备实体别名查询：{error}"))?;
        let rows = statement
            .query_map(params![candidate, limit as i64], |row| {
                Ok(KnowledgeSearchMatch {
                    result_id: row.get(0)?,
                    result_kind: row.get(1)?,
                    domain: row.get(2)?,
                    title: row.get(3)?,
                    snippet: row.get(4)?,
                    game_version: row.get(5)?,
                    confidence: row.get(6)?,
                    source_title: row.get(7)?,
                    source_url: row.get(8)?,
                    pack_id: pack.pack_id.clone(),
                    pack_version: pack.version.clone(),
                    pack_kind: pack.kind.clone(),
                })
            })
            .map_err(|error| format!("实体别名查询失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("无法读取实体别名查询结果：{error}"))?;
        append_unique_matches(&mut matches, &mut seen_result_ids, rows, limit);
        if matches.len() >= limit {
            break;
        }
    }
    Ok(matches)
}

fn embedded_alias_search_queries(query: &str) -> Vec<String> {
    let compact = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if compact == query {
        vec![query.to_string()]
    } else {
        // 先查紧凑写法，覆盖“大剑 I”与“大剑I”这类等价输入。
        vec![compact, query.to_string()]
    }
}

fn append_unique_matches(
    matches: &mut Vec<KnowledgeSearchMatch>,
    seen_result_ids: &mut HashSet<String>,
    candidates: Vec<KnowledgeSearchMatch>,
    limit: usize,
) {
    for candidate in candidates {
        if matches.len() >= limit {
            break;
        }
        if seen_result_ids.insert(candidate.result_id.clone()) {
            matches.push(candidate);
        }
    }
}

fn search_rows(
    connection: &Connection,
    pack: &InstalledPackRecord,
    sql: &str,
    parameter: &str,
    limit: usize,
) -> Result<Vec<KnowledgeSearchMatch>, String> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("无法准备知识查询：{error}"))?;
    let rows = statement
        .query_map(params![parameter, limit as i64], |row| {
            Ok(KnowledgeSearchMatch {
                result_id: row.get(0)?,
                result_kind: row.get(1)?,
                domain: row.get(2)?,
                title: row.get(3)?,
                snippet: row.get(4)?,
                game_version: row.get(5)?,
                confidence: row.get(6)?,
                source_title: row.get(7)?,
                source_url: row.get(8)?,
                pack_id: pack.pack_id.clone(),
                pack_version: pack.version.clone(),
                pack_kind: pack.kind.clone(),
            })
        })
        .map_err(|error| format!("知识查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取知识查询结果：{error}"))?;
    Ok(rows)
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("无法打开知识包：{error}"))?;
    connection
        .execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;")
        .map_err(|error| format!("无法启用知识包只读保护：{error}"))?;
    Ok(connection)
}

fn table_count(connection: &Connection, table_name: &str) -> Result<usize, String> {
    let sql = match table_name {
        "entities" => "SELECT COUNT(*) FROM entities",
        "relations" => "SELECT COUNT(*) FROM relations",
        "documents" => "SELECT COUNT(*) FROM documents",
        "sources" => "SELECT COUNT(*) FROM sources",
        _ => return Err("内部知识表名称无效。".to_string()),
    };
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(|error| format!("无法统计知识包数据：{error}"))?;
    usize::try_from(count).map_err(|_| "知识包数据量超出支持范围。".to_string())
}

fn copy_and_hash(
    source: &Path,
    destination: &Path,
    total_bytes: u64,
    progress: &OperationReporter,
) -> Result<(String, u64), String> {
    let mut input = File::open(source).map_err(|error| format!("无法打开知识包：{error}"))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| format!("无法创建知识包暂存文件：{error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| format!("读取知识包失败：{error}"))?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("写入知识包暂存文件失败：{error}"))?;
        hasher.update(&buffer[..count]);
        copied += count as u64;
        progress.report(
            "正在复制并校验知识包",
            usize::try_from(copied).unwrap_or(usize::MAX),
            Some(usize::try_from(total_bytes).unwrap_or(usize::MAX)),
            None,
        );
    }
    output
        .sync_all()
        .map_err(|error| format!("无法刷新知识包暂存文件：{error}"))?;
    if copied != total_bytes {
        return Err("知识包在导入过程中发生变化，请重试。".to_string());
    }
    let digest = hasher.finalize();
    let sha256 = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((sha256, copied))
}

fn status_from_index(root: &Path, index: &KnowledgeIndex) -> Result<KnowledgeStatus, String> {
    let mut packs = index
        .packs
        .iter()
        .map(|record| summary_from_record(root, record))
        .collect::<Vec<_>>();
    packs.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| {
                right
                    .installed_at_unix_seconds
                    .cmp(&left.installed_at_unix_seconds)
            })
    });
    let active_pack_count = packs
        .iter()
        .filter(|pack| pack.active && pack.healthy)
        .count();
    let total_size_bytes = packs.iter().map(|pack| pack.size_bytes).sum();
    let message = if active_pack_count == 0 {
        "尚未安装可用知识包；AcuAI 仍可使用传统 MOD 管理工具。".to_string()
    } else {
        format!("已启用 {active_pack_count} 个知识包。")
    };
    Ok(KnowledgeStatus {
        packs,
        active_pack_count,
        total_size_bytes,
        message,
    })
}

fn summary_from_record(root: &Path, record: &InstalledPackRecord) -> KnowledgePackSummary {
    let path = installed_pack_path(root, &record.relative_path);
    let (healthy, error) = match path {
        Ok(path) if path.is_file() => (true, None),
        Ok(_) => (false, Some("知识包文件缺失。".to_string())),
        Err(error) => (false, Some(error)),
    };
    KnowledgePackSummary {
        pack_id: record.pack_id.clone(),
        display_name: record.display_name.clone(),
        kind: record.kind.clone(),
        version: record.version.clone(),
        game_version: record.game_version.clone(),
        locale: record.locale.clone(),
        description: record.description.clone(),
        sha256: record.sha256.clone(),
        size_bytes: record.size_bytes,
        installed_at_unix_seconds: record.installed_at_unix_seconds,
        entity_count: record.entity_count,
        relation_count: record.relation_count,
        document_count: record.document_count,
        source_count: record.source_count,
        active: record.active,
        healthy,
        error,
    }
}

fn load_index(root: &Path) -> Result<KnowledgeIndex, String> {
    let index_path = root.join("index.json");
    if !index_path.exists() {
        return Ok(KnowledgeIndex::default());
    }
    let content =
        fs::read_to_string(&index_path).map_err(|error| format!("无法读取知识包索引：{error}"))?;
    let index = serde_json::from_str::<KnowledgeIndex>(&content)
        .map_err(|error| format!("知识包索引已损坏：{error}"))?;
    if index.schema_version != INDEX_SCHEMA_VERSION {
        return Err(format!(
            "知识包索引版本 {} 不受支持。",
            index.schema_version
        ));
    }
    Ok(index)
}

fn save_index(root: &Path, index: &KnowledgeIndex) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| format!("无法创建知识数据目录：{error}"))?;
    let temporary_path = root.join("index.json.tmp");
    let index_path = root.join("index.json");
    let backup_path = root.join("index.json.bak");
    let content = serde_json::to_string_pretty(index)
        .map_err(|error| format!("无法序列化知识包索引：{error}"))?;
    fs::write(&temporary_path, format!("{content}\n"))
        .map_err(|error| format!("无法写入知识包索引：{error}"))?;
    if index_path.exists() {
        if backup_path.exists() {
            fs::remove_file(&backup_path)
                .map_err(|error| format!("无法清理知识包索引备份：{error}"))?;
        }
        fs::rename(&index_path, &backup_path)
            .map_err(|error| format!("无法备份知识包索引：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary_path, &index_path) {
        if backup_path.exists() {
            let _ = fs::rename(&backup_path, &index_path);
        }
        return Err(format!("无法启用知识包索引：{error}"));
    }
    // 新索引已经生效，备份清理失败不能再向调用方报告“导入失败”，否则
    // 调用方会回滚文件而索引仍指向新版本。残留备份会在下次保存时清理。
    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }
    Ok(())
}

fn knowledge_root() -> Result<PathBuf, String> {
    let executable_path =
        env::current_exe().map_err(|error| format!("无法定位 Acumod 程序目录：{error}"))?;
    let executable_directory = executable_path
        .parent()
        .ok_or_else(|| "无法定位 Acumod 程序目录。".to_string())?;
    Ok(executable_directory.join("AcumodData").join("knowledge"))
}

/// MOD 分析缓存与知识包共享数据根目录，但不属于任何可分发知识包。
pub(crate) fn analysis_cache_root() -> Result<PathBuf, String> {
    Ok(knowledge_root()?.join("analysis"))
}

fn normalized_source_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        return Err("请选择知识包文件。".to_string());
    }
    let path = PathBuf::from(value);
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("acukb"))
    {
        return Err("知识包文件扩展名必须是 `.acukb`。".to_string());
    }
    Ok(path)
}

fn normalized_bundle_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        return Err("请选择知识包 ZIP 文件。".to_string());
    }
    let path = PathBuf::from(value);
    if !path.is_file() {
        return Err("知识包导入必须选择一个 ZIP 文件。".to_string());
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Err("知识包文件扩展名必须是 `.zip`。ZIP 内应包含四个 `.acukb` 文件。".to_string());
    }
    Ok(path)
}

fn display_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("未知文件")
        .to_string()
}

fn relative_pack_path(pack_id: &str, file_name: &str) -> String {
    format!("packs/{pack_id}/{file_name}")
}

fn installed_pack_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let normalized = relative_path.replace('\\', "/");
    let segments = normalized.split('/').collect::<Vec<_>>();
    if segments.len() != 3
        || segments[0] != "packs"
        || segments.iter().any(|segment| {
            segment.is_empty() || *segment == "." || *segment == ".." || segment.contains(':')
        })
    {
        return Err("知识包索引包含不安全路径。".to_string());
    }
    Ok(root.join(segments[0]).join(segments[1]).join(segments[2]))
}

fn normalize_domains(domains: Option<&[String]>) -> Result<HashSet<&str>, String> {
    let mut result = HashSet::new();
    for domain in domains.unwrap_or_default() {
        let domain = domain.trim();
        validate_pack_kind(domain)?;
        result.insert(domain);
    }
    Ok(result)
}

fn normalized_query(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label}内容不能为空。"));
    }
    if value.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(format!("{label}不能超过 {MAX_SEARCH_QUERY_CHARS} 个字符。"));
    }
    Ok(value.to_string())
}

fn entity_query_variants(query: &str) -> Vec<String> {
    let characters = query.chars().collect::<Vec<_>>();
    let mut compact = String::with_capacity(query.len());
    let mut changed = false;
    for (index, character) in characters.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|value| characters.get(value));
        let next = characters.get(index + 1);
        // 中文装备名后常把等级写成“ I”或“ 1”，而原始文本通常没有空格。
        // 只在下一段是完整的罗马/阿拉伯等级时压缩，英文单词间的空格保持不变。
        let can_compact = character.is_whitespace()
            && previous.is_some_and(|value| matches!(value, '\u{4e00}'..='\u{9fff}'))
            && next.is_some_and(|value| value.is_ascii_digit() || is_roman_numeral(*value))
            && numeric_suffix_ends_at(&characters, index + 1);
        if can_compact {
            changed = true;
            continue;
        }
        compact.push(*character);
    }
    if changed && !compact.is_empty() {
        vec![query.to_string(), compact]
    } else {
        vec![query.to_string()]
    }
}

fn is_roman_numeral(value: char) -> bool {
    matches!(value, 'I' | 'V' | 'X')
}

fn numeric_suffix_ends_at(characters: &[char], start: usize) -> bool {
    let mut index = start;
    while characters
        .get(index)
        .is_some_and(|value| value.is_ascii_digit() || is_roman_numeral(*value))
    {
        index += 1;
    }
    characters
        .get(index)
        .is_none_or(|value| !value.is_ascii_alphanumeric())
}

fn normalize_fixed_values(
    values: Option<&[String]>,
    label: &str,
    max_items: usize,
) -> Result<String, String> {
    let mut normalized = Vec::new();
    for value in values.unwrap_or_default() {
        let value = value.trim();
        if value.is_empty()
            || value.len() > 80
            || !value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_' | ':' | '.')
            })
        {
            return Err(format!("{label}“{value}”格式无效。"));
        }
        if !normalized.iter().any(|existing| existing == value) {
            normalized.push(value.to_string());
        }
    }
    if normalized.len() > max_items {
        return Err(format!("{label}最多可指定 {max_items} 项。"));
    }
    if normalized.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("\u{1f}{}\u{1f}", normalized.join("\u{1f}")))
    }
}

/// 关系谓词沿用知识包中的 camelCase 命名；仍只允许受控 ASCII 标识符，不能成为 SQL 片段。
fn normalize_relation_predicates(values: Option<&[String]>) -> Result<String, String> {
    let mut normalized = Vec::new();
    for value in values.unwrap_or_default() {
        let value = value.trim();
        if value.is_empty()
            || value.len() > 80
            || !value.chars().all(|character| {
                character.is_ascii_alphabetic()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '_' | ':' | '.')
            })
        {
            return Err(format!("关系类型“{value}”格式无效。"));
        }
        if !normalized.iter().any(|existing| existing == value) {
            normalized.push(value.to_string());
        }
    }
    if normalized.len() > 24 {
        return Err("关系类型最多可指定 24 项。".to_string());
    }
    if normalized.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("\u{1f}{}\u{1f}", normalized.join("\u{1f}")))
    }
}

fn validate_entity_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.chars().count() > 240
        || value.chars().any(|character| character.is_control())
    {
        return Err("游戏实体 ID 格式无效。".to_string());
    }
    Ok(())
}

fn normalize_relation_direction(value: &str) -> Result<&'static str, String> {
    match value.trim() {
        "outgoing" => Ok("outgoing"),
        "incoming" => Ok("incoming"),
        "both" | "" => Ok("both"),
        _ => Err("关系方向只支持 outgoing、incoming 或 both。".to_string()),
    }
}

fn validate_pack_kind(value: &str) -> Result<(), String> {
    if matches!(
        value,
        "mhw-modding" | "mhw-game-facts" | "mhw-game-guides" | "acumod-help"
    ) {
        Ok(())
    } else {
        Err(format!("不支持的知识包类型：{value}"))
    }
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err(format!("{label}格式无效。"));
    }
    Ok(())
}

fn validate_version(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 40
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
        })
    {
        return Err(format!("{label}格式无效。"));
    }
    Ok(())
}

fn validate_short_text(value: &str, label: &str, max_chars: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > max_chars {
        return Err(format!("{label}为空或过长。"));
    }
    Ok(())
}

fn ensure_supported_app_version(minimum: &str) -> Result<(), String> {
    let current = parse_numeric_version(env!("CARGO_PKG_VERSION"));
    let minimum = parse_numeric_version(minimum);
    if current < minimum {
        return Err(format!(
            "知识包需要 Acumod {} 或更高版本，当前版本为 {}。",
            minimum
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join("."),
            env!("CARGO_PKG_VERSION")
        ));
    }
    Ok(())
}

fn parse_numeric_version(value: &str) -> Vec<u64> {
    value
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse::<u64>().unwrap_or(u64::MAX))
        .chain(std::iter::repeat(0))
        .take(3)
        .collect()
}

fn quoted_fts_query(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

fn fallback_fts_query(query: &str) -> Option<String> {
    let terms = fallback_fts_terms(query);
    (terms.len() > 1).then(|| {
        terms
            .iter()
            .map(|term| quoted_fts_query(term))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn fallback_fts_terms(query: &str) -> Vec<String> {
    let characters = query.chars().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0;
    while index < characters.len() {
        if is_cjk_character(characters[index]) {
            let start = index;
            while index < characters.len() && is_cjk_character(characters[index]) {
                index += 1;
            }
            let run = &characters[start..index];
            for window in run.windows(3) {
                let term = window.iter().collect::<String>();
                if seen.insert(term.clone()) {
                    terms.push(term);
                }
            }
            continue;
        }
        if characters[index].is_ascii_alphanumeric() {
            let start = index;
            while index < characters.len() && characters[index].is_ascii_alphanumeric() {
                index += 1;
            }
            let term = characters[start..index].iter().collect::<String>();
            if term.len() >= 2 && seen.insert(term.clone()) {
                terms.push(term);
            }
            continue;
        }
        index += 1;
    }
    if terms.len() <= MAX_FTS_FALLBACK_TERMS {
        return terms;
    }

    // 极长问句不应放大为大量 SQLite 查询项；保留均匀分布的片段，避免只留下开头客套语。
    let last_index = terms.len() - 1;
    (0..MAX_FTS_FALLBACK_TERMS)
        .map(|position| {
            let index = position * last_index / (MAX_FTS_FALLBACK_TERMS - 1);
            terms[index].clone()
        })
        .collect()
}

fn fallback_like_terms(query: &str) -> Vec<String> {
    let characters = query.chars().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0;
    while index < characters.len() {
        if is_cjk_character(characters[index]) {
            let start = index;
            while index < characters.len() && is_cjk_character(characters[index]) {
                index += 1;
            }
            for window in characters[start..index].windows(2) {
                let term = window.iter().collect::<String>();
                if seen.insert(term.clone()) {
                    terms.push(term);
                }
            }
            continue;
        }
        index += 1;
    }
    if terms.len() <= MAX_FTS_FALLBACK_TERMS {
        return terms;
    }
    let last_index = terms.len() - 1;
    (0..MAX_FTS_FALLBACK_TERMS)
        .map(|position| {
            let index = position * last_index / (MAX_FTS_FALLBACK_TERMS - 1);
            terms[index].clone()
        })
        .collect()
}

fn is_cjk_character(value: char) -> bool {
    matches!(value, '\u{4e00}'..='\u{9fff}')
}

fn escape_like(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn unix_seconds_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("系统时间不可用：{error}"))
}

fn unix_nanos_now() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| format!("系统时间不可用：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        delete_pack_from, get_game_entity_relations_from, get_status_from, install_pack_into,
        installed_pack_path, load_index, lookup_game_entities_from, normalize_domains,
        normalize_relation_predicates, normalized_bundle_path, parse_numeric_version,
        quoted_fts_query, search_from, search_pack, validate_identifier, validate_pack,
        validate_version, InstalledPackRecord, PACK_APPLICATION_ID,
    };
    use crate::operations::OperationReporter;
    use rusqlite::{params, Connection};
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_test_path(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "acumod-knowledge-{label}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn knowledge_bundle_import_accepts_only_zip_files() {
        let root = unique_test_path("bundle-extension");
        fs::create_dir_all(&root).unwrap();
        let directory = root.join("packs");
        fs::create_dir_all(&directory).unwrap();
        let acukb = root.join("single.acukb");
        let zip = root.join("complete.zip");
        fs::write(&acukb, b"test").unwrap();
        fs::write(&zip, b"test").unwrap();

        assert!(normalized_bundle_path(directory.to_string_lossy().as_ref()).is_err());
        assert!(normalized_bundle_path(acukb.to_string_lossy().as_ref()).is_err());
        assert_eq!(
            normalized_bundle_path(zip.to_string_lossy().as_ref()).unwrap(),
            zip
        );

        fs::remove_dir_all(root).unwrap();
    }

    fn create_test_pack(path: &Path, version: &str, body: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(&format!(
                "
                PRAGMA application_id = {PACK_APPLICATION_ID};
                PRAGMA user_version = 1;
                CREATE TABLE pack_manifest (pack_id TEXT PRIMARY KEY, display_name TEXT NOT NULL, kind TEXT NOT NULL, version TEXT NOT NULL, game_version TEXT NOT NULL, locale TEXT NOT NULL, min_app_version TEXT NOT NULL, description TEXT NOT NULL);
                CREATE TABLE sources (id TEXT PRIMARY KEY, title TEXT NOT NULL, url TEXT, kind TEXT NOT NULL, game_version TEXT NOT NULL, license_note TEXT NOT NULL);
                CREATE TABLE entities (id TEXT PRIMARY KEY, kind TEXT NOT NULL, domain TEXT NOT NULL, canonical_name TEXT NOT NULL, name_zh_hans TEXT, name_zh_hant TEXT, summary TEXT NOT NULL, game_version TEXT NOT NULL, confidence REAL NOT NULL, source_id TEXT, data_json TEXT NOT NULL);
                CREATE TABLE aliases (entity_id TEXT NOT NULL, locale TEXT NOT NULL, alias TEXT NOT NULL, PRIMARY KEY (entity_id, locale, alias));
                CREATE TABLE relations (id TEXT PRIMARY KEY, subject_id TEXT NOT NULL, predicate TEXT NOT NULL, object_id TEXT NOT NULL, game_version TEXT NOT NULL, confidence REAL NOT NULL, source_id TEXT, data_json TEXT NOT NULL);
                CREATE TABLE documents (id TEXT PRIMARY KEY, namespace TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, game_version TEXT NOT NULL, confidence REAL NOT NULL, source_id TEXT);
                CREATE VIRTUAL TABLE knowledge_fts USING fts5(result_id UNINDEXED, result_kind UNINDEXED, domain UNINDEXED, title, body, tokenize='trigram');
                "
            ))
            .unwrap();
        connection
            .execute(
                "INSERT INTO pack_manifest VALUES ('test-modding', '测试 MOD 技术包', 'mhw-modding', ?1, '15.23', 'zh-Hans', '0.1.0', '测试')",
                [version],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sources VALUES ('source', '测试来源', 'https://example.com', 'test', '15.23', 'test')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO documents VALUES ('slinger', 'mod-slinger', '飞翔爪模型', ?1, '15.23', 1.0, 'source')",
                [body],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_fts VALUES ('slinger', 'document', 'mod-slinger', '飞翔爪模型', ?1)",
                params![body],
            )
            .unwrap();
    }

    fn create_test_game_pack(path: &Path) {
        create_test_pack(path, "0.1.0", "用于测试游戏实体和关系查询。");
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE pack_manifest SET pack_id = 'test-game-facts', display_name = '测试游戏事实包', kind = 'mhw-game-facts'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO entities VALUES (?1, 'weapon', 'game-equipment', ?2, ?3, ?4, ?5, '15.23', 1.0, 'source', ?6)",
                params![
                    "weapon:greatsword:wyvern-ignition",
                    "Wyvern Ignition Impact",
                    "喷气大剑",
                    "噴射大劍",
                    "活动大剑，需要特定素材制作。",
                    r#"{"rarity": 12, "attack": 1008}"#,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO entities VALUES (?1, 'item', 'game-item', ?2, ?3, ?4, ?5, '15.23', 1.0, 'source', ?6)",
                params![
                    "item:elder-dragon-blood",
                    "Elder Dragon Blood",
                    "古龙之血",
                    "古龍之血",
                    "用于多种装备制作的素材。",
                    r#"{"rarity": 7}"#,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO aliases VALUES (?1, 'en', 'Rocket GS')",
                ["weapon:greatsword:wyvern-ignition"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO relations VALUES ('relation:wyvern-ignition:blood', ?1, 'requires_material', ?2, '15.23', 1.0, 'source', ?3)",
                params![
                    "weapon:greatsword:wyvern-ignition",
                    "item:elder-dragon-blood",
                    r#"{"quantity": 2}"#,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_fts VALUES (?1, 'entity', 'game-equipment', '喷气大剑', '噴射大劍 Rocket GS 活动大剑')",
                ["weapon:greatsword:wyvern-ignition"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_fts VALUES (?1, 'entity', 'game-item', '古龙之血', '古龍之血 Elder Dragon Blood')",
                ["item:elder-dragon-blood"],
            )
            .unwrap();
    }

    fn create_test_guide_pack(path: &Path) {
        create_test_pack(
            path,
            "0.1.0",
            "冰原中后期大剑优先集中、弱点特效和体力增强。",
        );
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE pack_manifest SET pack_id = 'test-game-guides', display_name = '测试游戏攻略包', kind = 'mhw-game-guides'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE documents SET id = 'guide-greatsword', namespace = 'guide-build', title = '冰原大剑进阶', body = '冰原中后期大剑优先集中、弱点特效和体力增强。'",
                [],
            )
            .unwrap();
        connection.execute("DELETE FROM knowledge_fts", []).unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_fts VALUES ('guide-greatsword', 'document', 'guide-build', '冰原大剑进阶', '冰原中后期大剑优先集中、弱点特效和体力增强。')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn identifiers_reject_path_material() {
        assert!(validate_identifier("mhw-modding", "ID").is_ok());
        assert!(validate_identifier("../mhw", "ID").is_err());
        assert!(validate_identifier("MHW", "ID").is_err());
    }

    #[test]
    fn relative_pack_paths_cannot_escape_the_knowledge_root() {
        assert!(installed_pack_path(Path::new("root"), "packs/mhw/1.acukb").is_ok());
        assert!(installed_pack_path(Path::new("root"), "packs/../secret").is_err());
        assert!(installed_pack_path(Path::new("root"), "C:/secret.acukb").is_err());
    }

    #[test]
    fn search_domains_are_fixed_pack_kinds() {
        let domains = vec!["mhw-modding".to_string(), "mhw-game-facts".to_string()];
        assert_eq!(normalize_domains(Some(&domains)).unwrap().len(), 2);
        assert!(normalize_domains(Some(&["other".to_string()])).is_err());
        assert!(normalize_domains(Some(&["acumod-help".to_string()])).is_ok());
    }

    #[test]
    fn relation_predicates_accept_camel_case_but_reject_control_characters() {
        let predicates = vec!["hasMonsterFacts".to_string(), "gathersItem".to_string()];
        assert!(normalize_relation_predicates(Some(&predicates)).is_ok());
        assert!(normalize_relation_predicates(Some(&["gathersItem;DROP".to_string()])).is_err());
    }

    #[test]
    fn versions_and_fts_queries_are_normalized() {
        assert!(validate_version("15.23", "版本").is_ok());
        assert!(validate_version("15/23", "版本").is_err());
        assert!(parse_numeric_version("0.1.0") < parse_numeric_version("0.2.0"));
        assert_eq!(quoted_fts_query("冰狼\"套装"), "\"冰狼\"\"套装\"");
    }

    #[test]
    fn validates_and_searches_a_minimal_pack() {
        let path = unique_test_path("minimal").with_extension("acukb");
        create_test_pack(&path, "0.1.0", "飞翔爪资源通常位于 wp/slg。");

        let validated = validate_pack(&path).unwrap();
        assert_eq!(validated.pack_id, "test-modding");
        let record = InstalledPackRecord {
            pack_id: validated.pack_id,
            display_name: validated.display_name,
            kind: validated.kind,
            version: validated.version,
            game_version: validated.game_version,
            locale: validated.locale,
            description: validated.description,
            relative_path: "packs/test-modding/test.acukb".to_string(),
            sha256: "test".to_string(),
            size_bytes: 0,
            installed_at_unix_seconds: 0,
            entity_count: validated.entity_count,
            relation_count: validated.relation_count,
            document_count: validated.document_count,
            source_count: validated.source_count,
            active: true,
        };
        let matches = search_pack(&path, &record, "飞翔爪", 10).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source_title.as_deref(), Some("测试来源"));
        // trigram FTS 对两字中文查询不建立三元分词；此处验证固定 LIKE
        // 回退仍能查到标题，避免“大剑”“太刀”这类常用术语失效。
        let short_matches = search_pack(&path, &record, "模型", 10).unwrap();
        assert_eq!(short_matches.len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn resolves_multilingual_entities_and_traverses_relations() {
        let test_root = unique_test_path("entities");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        let source = test_root.join("game.acukb");
        create_test_game_pack(&source);
        install_pack_into(
            &knowledge_root,
            source.to_string_lossy().into_owned(),
            &OperationReporter::default(),
        )
        .unwrap();

        let simplified = lookup_game_entities_from(&knowledge_root, "喷气大剑", None, 10).unwrap();
        assert_eq!(simplified.matches.len(), 1);
        assert_eq!(
            simplified.matches[0].entity_id,
            "weapon:greatsword:wyvern-ignition"
        );
        assert_eq!(simplified.matches[0].data["attack"], 1008);

        let traditional = lookup_game_entities_from(&knowledge_root, "噴射大劍", None, 10).unwrap();
        assert_eq!(
            traditional.matches[0].entity_id,
            simplified.matches[0].entity_id
        );
        let alias = lookup_game_entities_from(&knowledge_root, "Rocket GS", None, 10).unwrap();
        assert_eq!(alias.matches[0].entity_id, simplified.matches[0].entity_id);
        let item_only = vec!["item".to_string()];
        assert!(
            lookup_game_entities_from(&knowledge_root, "喷气大剑", Some(&item_only), 10,)
                .unwrap()
                .matches
                .is_empty()
        );

        let outgoing = get_game_entity_relations_from(
            &knowledge_root,
            "weapon:greatsword:wyvern-ignition",
            None,
            "outgoing",
            10,
        )
        .unwrap();
        assert_eq!(outgoing.relations.len(), 1);
        assert_eq!(outgoing.relations[0].object_name, "古龙之血");
        assert_eq!(outgoing.relations[0].data["quantity"], 2);

        let incoming = get_game_entity_relations_from(
            &knowledge_root,
            "item:elder-dragon-blood",
            Some(&["requires_material".to_string()]),
            "incoming",
            10,
        )
        .unwrap();
        assert_eq!(incoming.relations.len(), 1);
        assert_eq!(incoming.relations[0].subject_name, "喷气大剑");

        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn guide_search_is_isolated_from_game_facts() {
        let test_root = unique_test_path("guide-search");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        let game_source = test_root.join("game.acukb");
        let guide_source = test_root.join("guide.acukb");
        create_test_game_pack(&game_source);
        create_test_guide_pack(&guide_source);
        let reporter = OperationReporter::default();
        install_pack_into(
            &knowledge_root,
            game_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();
        install_pack_into(
            &knowledge_root,
            guide_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();

        let guides = vec!["mhw-game-guides".to_string()];
        let guide_matches = search_from(&knowledge_root, "冰原大剑", Some(&guides), 10).unwrap();
        assert_eq!(guide_matches.matches.len(), 1);
        assert_eq!(guide_matches.matches[0].pack_id, "test-game-guides");

        let facts = vec!["mhw-game-facts".to_string()];
        let fact_matches = search_from(&knowledge_root, "冰原大剑", Some(&facts), 10).unwrap();
        // 完整实体别名可合法命中游戏事实；隔离要求是不能越过调用方指定的包域。
        assert!(fact_matches
            .matches
            .iter()
            .all(|item| item.pack_kind == "mhw-game-facts"));
        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn search_skips_missing_or_corrupt_pack_without_blocking_valid_pack() {
        let test_root = unique_test_path("damaged-pack");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        let game_source = test_root.join("game.acukb");
        let modding_source = test_root.join("modding.acukb");
        create_test_game_pack(&game_source);
        create_test_pack(&modding_source, "0.1.0", "椋炵繑鐖祫婧愭ā鍨嬫祦绋?");
        let reporter = OperationReporter::default();
        install_pack_into(
            &knowledge_root,
            game_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();
        install_pack_into(
            &knowledge_root,
            modding_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();

        let modding_record = load_index(&knowledge_root)
            .unwrap()
            .packs
            .into_iter()
            .find(|pack| pack.pack_id == "test-modding" && pack.active)
            .expect("测试 MOD 知识包应在索引中激活");
        let modding_path =
            installed_pack_path(&knowledge_root, &modding_record.relative_path).unwrap();
        fs::remove_file(&modding_path).unwrap();

        let missing_response = search_from(&knowledge_root, "Rocket GS", None, 10).unwrap();
        assert_eq!(missing_response.searched_pack_count, 2);
        assert!(missing_response
            .matches
            .iter()
            .any(|item| item.result_id == "weapon:greatsword:wyvern-ignition"));
        assert!(missing_response
            .warnings
            .iter()
            .any(|warning| warning.contains("文件缺失")));

        install_pack_into(
            &knowledge_root,
            modding_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();
        let restored_record = load_index(&knowledge_root)
            .unwrap()
            .packs
            .into_iter()
            .find(|pack| pack.pack_id == "test-modding" && pack.active)
            .expect("重新导入后测试 MOD 知识包应在索引中激活");
        let restored_path =
            installed_pack_path(&knowledge_root, &restored_record.relative_path).unwrap();
        fs::write(&restored_path, b"not a sqlite knowledge pack").unwrap();

        let corrupt_response = search_from(&knowledge_root, "Rocket GS", None, 10).unwrap();
        assert!(corrupt_response
            .matches
            .iter()
            .any(|item| item.result_id == "weapon:greatsword:wyvern-ignition"));
        assert!(corrupt_response
            .warnings
            .iter()
            .any(|warning| warning.contains("查询失败")));

        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    #[ignore = "需要先运行 npm.cmd run knowledge:build-dev，使用真实开发知识包验证安装与检索链路"]
    fn generated_development_packs_install_and_answer_core_queries() {
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 必须位于项目根目录下");
        let build_root = project_root
            .join("references")
            .join("knowledge")
            .join("build");
        let game_facts = build_root.join("acumod-dev-game-facts.acukb");
        let modding = build_root.join("acumod-dev-modding.acukb");
        let guides = build_root.join("acumod-dev-game-guides.acukb");
        let acumod_help = build_root.join("acumod-dev-acumod-help.acukb");
        assert!(game_facts.is_file(), "缺少游戏事实开发包：{game_facts:?}");
        assert!(modding.is_file(), "缺少 MOD 技术开发包：{modding:?}");
        assert!(guides.is_file(), "缺少攻略开发包：{guides:?}");
        assert!(
            acumod_help.is_file(),
            "缺少 Acumod 使用说明开发包：{acumod_help:?}"
        );

        let test_root = unique_test_path("generated-pack-e2e");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        let reporter = OperationReporter::default();
        for pack in [&game_facts, &modding, &guides, &acumod_help] {
            install_pack_into(
                &knowledge_root,
                pack.to_string_lossy().into_owned(),
                &reporter,
            )
            .unwrap();
        }

        assert_eq!(
            get_status_from(&knowledge_root).unwrap().active_pack_count,
            4
        );

        // 真实包验收覆盖核心实体、关系、攻略和 MOD 技术问法，确保安装索引、别名检索、
        // 关系遍历和 FTS/LIKE 回退均能为 AcuAI 返回证据，而不是只检查 SQLite 中存在原始记录。
        let quest =
            lookup_game_entities_from(&knowledge_root, "贼龙与古代树森林", None, 20).unwrap();
        assert!(quest
            .matches
            .iter()
            .any(|item| item.entity_id == "game-quest-fact:mhwdata:101"));
        let quest_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:00101",
            Some(&["hasQuestFacts".to_string(), "occursAt".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(quest_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-quest-fact:mhwdata:101"));
        assert!(quest_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-location:mhwdata:1"));

        let deep_snow_diver =
            lookup_game_entities_from(&knowledge_root, "深雪的潜水员", None, 20).unwrap();
        assert!(deep_snow_diver
            .matches
            .iter()
            .any(|item| item.entity_id == "game-quest:01121"));
        let deep_snow_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:01121",
            Some(&["requiresQuest".to_string(), "requiresCondition".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(deep_snow_relations
            .relations
            .iter()
            .any(|item| item.predicate == "requiresQuest" && item.object_id == "game-quest:01101"));
        assert!(deep_snow_relations
            .relations
            .iter()
            .any(|item| item.predicate == "requiresCondition"
                && item.object_id == "game-unlock-condition:01121:0"));

        let pink_power_grab_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:01272",
            Some(&["requiresCondition".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(pink_power_grab_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-unlock-condition:01272:0"));

        let baptism_by_ice =
            lookup_game_entities_from(&knowledge_root, "Baptism by Ice", None, 20).unwrap();
        assert!(baptism_by_ice
            .matches
            .iter()
            .any(|item| item.entity_id == "game-quest:01101"));
        let baptism_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:01101",
            Some(&["requiresCondition".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(baptism_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-unlock-condition:01101:0"));
        assert!(baptism_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-unlock-condition:01101:1"));

        let black_dragon = lookup_game_entities_from(&knowledge_root, "黑龙", None, 20).unwrap();
        assert!(black_dragon
            .matches
            .iter()
            .any(|item| item.entity_id == "game-quest:51612"));
        assert!(!black_dragon
            .matches
            .iter()
            .any(|item| item.entity_id == "game-quest-fact:mhwdata:67803"));
        let black_dragon_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:51612",
            Some(&["requiresQuest".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(black_dragon_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-quest:51613"));
        let task_domains = vec!["mhw-game-facts".to_string()];
        let black_dragon_question =
            search_from(&knowledge_root, "黑龙如何解锁", Some(&task_domains), 20).unwrap();
        assert!(black_dragon_question
            .matches
            .iter()
            .any(|item| item.result_id == "game-quest:51612"));
        for (query, entity_id) in [
            ("初次洗礼如何解锁", "game-quest:01101"),
            ("紧急任务狩猎毒妖鸟怎么解锁", "game-quest:00205"),
            ("冰鱼龙弱什么属性", "game-monster-fact:mhwdata:111"),
            ("大贼龙的爪从哪里获得", "game-item:305"),
            (
                "防卫队炎刃型大剑 I 的攻击力",
                "game-weapon-fact:mhwdata:2001",
            ),
        ] {
            let natural_question =
                search_from(&knowledge_root, query, Some(&task_domains), 20).unwrap();
            assert!(
                natural_question
                    .matches
                    .iter()
                    .any(|item| item.result_id == entity_id),
                "自然问句“{query}”必须先召回 {entity_id}"
            );
        }

        for (query, entity_id, stage_entity_id) in [
            ("永霜冻土", "game-location:mhwdata:12", "game-stage:ST108"),
            ("虚黑城", "game-location:mhwdata:17", "game-stage:ST417"),
        ] {
            let locations = lookup_game_entities_from(&knowledge_root, query, None, 20).unwrap();
            let location = locations
                .matches
                .iter()
                .find(|item| item.entity_id == entity_id)
                .unwrap_or_else(|| panic!("{query} 必须能查询到 {entity_id}"));
            assert_eq!(location.data["stageEntityId"], stage_entity_id);
        }

        let leather_head =
            lookup_game_entities_from(&knowledge_root, "皮制头饰", None, 20).unwrap();
        assert!(leather_head
            .matches
            .iter()
            .any(|item| item.entity_id == "game-armor-fact:mhwdata:1"));
        let leather_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-armor-fact:mhwdata:1",
            Some(&["grantsSkill".to_string(), "requiresMaterial".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(leather_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-skill:77"));
        assert!(leather_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-item:205"));

        let defender_greatsword =
            lookup_game_entities_from(&knowledge_root, "防卫队炎刃型大剑 I", None, 20).unwrap();
        let defender_greatsword = defender_greatsword
            .matches
            .iter()
            .find(|item| item.entity_id == "game-weapon-fact:mhwdata:2001")
            .expect("防卫队炎刃型大剑 I 必须能按中文名称查询");
        assert_eq!(defender_greatsword.data["attack"], 624);
        let defender_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-weapon-fact:mhwdata:2001",
            Some(&["requiresMaterial".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(defender_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-item:205"));

        // 实体比较工具按稳定 ID 回读，不能把相近名称重新走模糊匹配。
        let compared_weapons = [
            ("game-weapon-fact:mhwdata:2001", 624),
            ("game-weapon-fact:mhwdata:2002", 816),
        ]
        .into_iter()
        .map(|(entity_id, expected_attack)| {
            let response = lookup_game_entities_from(&knowledge_root, entity_id, None, 4).unwrap();
            let entity = response
                .matches
                .iter()
                .find(|item| item.entity_id == entity_id)
                .unwrap_or_else(|| panic!("稳定 ID {entity_id} 必须精确命中"));
            assert_eq!(entity.data["attack"], expected_attack);
            entity.entity_id.clone()
        })
        .collect::<Vec<_>>();
        assert_eq!(
            compared_weapons,
            [
                "game-weapon-fact:mhwdata:2001",
                "game-weapon-fact:mhwdata:2002"
            ]
        );

        let poison_resistance =
            lookup_game_entities_from(&knowledge_root, "毒耐性", None, 20).unwrap();
        let poison_rank_facts = poison_resistance
            .matches
            .iter()
            .find(|item| item.entity_id == "game-skill-fact:1")
            .expect("毒耐性等级资料必须能按中文名称查询");
        assert!(poison_rank_facts.data["ranks"]
            .as_array()
            .is_some_and(|ranks| ranks.len() >= 3));
        let poison_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-skill:1",
            Some(&["hasRankFacts".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(poison_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-skill-fact:1"));
        let antidote_jewel =
            lookup_game_entities_from(&knowledge_root, "耐毒珠 1", None, 20).unwrap();
        assert!(antidote_jewel
            .matches
            .iter()
            .any(|item| item.entity_id == "game-decoration-fact:mhwdata:1"));
        let jewel_relations = get_game_entity_relations_from(
            &knowledge_root,
            "game-decoration-fact:mhwdata:1",
            Some(&["grantsSkill".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(jewel_relations
            .relations
            .iter()
            .any(|item| item.object_id == "game-skill:1"));

        let ice_fish = lookup_game_entities_from(&knowledge_root, "冰鱼龙", None, 20).unwrap();
        let ecology = ice_fish
            .matches
            .iter()
            .find(|item| item.entity_id == "game-monster-fact:mhwdata:111")
            .expect("冰鱼龙生态资料必须可按中文名称查询");
        assert!(ecology.data["weaknesses"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));
        assert!(ecology.data["hitzones"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));

        let iron = lookup_game_entities_from(&knowledge_root, "铁矿石", None, 20).unwrap();
        let iron_item = iron
            .matches
            .iter()
            .find(|item| item.entity_id == "game-item:205")
            .expect("铁矿石必须可按中文名称查询");
        let sources = get_game_entity_relations_from(
            &knowledge_root,
            &iron_item.entity_id,
            Some(&["gathersItem".to_string()]),
            "incoming",
            50,
        )
        .unwrap();
        assert!(sources
            .relations
            .iter()
            .any(|item| item.subject_id == "game-location:mhwdata:3"));

        let great_jagras_claw =
            lookup_game_entities_from(&knowledge_root, "大贼龙的爪", None, 20).unwrap();
        let great_jagras_claw = great_jagras_claw
            .matches
            .iter()
            .find(|item| item.entity_id == "game-item:305")
            .expect("大贼龙的爪必须能按中文名称查询");
        let claw_sources = get_game_entity_relations_from(
            &knowledge_root,
            &great_jagras_claw.entity_id,
            Some(&["dropsItem".to_string()]),
            "incoming",
            20,
        )
        .unwrap();
        assert!(claw_sources
            .relations
            .iter()
            .any(|item| item.subject_id == "game-monster-fact:mhwdata:17"));
        let quest_rewards = get_game_entity_relations_from(
            &knowledge_root,
            "game-quest:00101",
            Some(&["rewardsItem".to_string()]),
            "outgoing",
            20,
        )
        .unwrap();
        assert!(quest_rewards
            .relations
            .iter()
            .any(|item| item.object_id == "game-item:205"));

        let guide_domains = vec!["mhw-game-guides".to_string()];
        let guide_matches =
            search_from(&knowledge_root, "冰原中后期大剑", Some(&guide_domains), 20).unwrap();
        assert!(guide_matches
            .matches
            .iter()
            .any(|item| item.result_id == "guide-greatsword-iceborne-midlate"));
        let natural_build_question = search_from(
            &knowledge_root,
            "我刚打到冰原中期，推荐一套好做又稳定的大剑配装",
            Some(&guide_domains),
            20,
        )
        .unwrap();
        assert!(natural_build_question
            .matches
            .iter()
            .any(|item| item.result_id == "guide-greatsword-iceborne-midlate"));
        let story_matches =
            search_from(&knowledge_root, "本体主线", Some(&guide_domains), 20).unwrap();
        assert!(story_matches
            .matches
            .iter()
            .any(|item| item.result_id == "guide-story-progression-basics"));
        let guiding_lands_matches =
            search_from(&knowledge_root, "聚魔之地", Some(&guide_domains), 20).unwrap();
        assert!(guiding_lands_matches
            .matches
            .iter()
            .any(|item| item.result_id == "guide-guiding-lands-basics"));
        let fatalis_matches =
            search_from(&knowledge_root, "黑龙", Some(&guide_domains), 20).unwrap();
        assert!(fatalis_matches
            .matches
            .iter()
            .any(|item| item.result_id == "guide-fatalis-combat-preparation"));
        let modding_domains = vec!["mhw-modding".to_string()];
        for (query, expected_document) in [
            ("MOD3", "modding-mod3"),
            ("MRL3", "modding-mrl3"),
            ("TEX", "modding-texture-maps"),
            ("EVAM", "modding-evam-slinger"),
            ("slg", "modding-slinger-chain"),
            ("EPV", "modding-epv"),
            ("EVWP", "modding-evwp"),
            ("plugins", "modding-nativepc-plugins"),
            ("CTC", "modding-ctc-ccl"),
            ("NBNK", "modding-audio-containers"),
            ("EFX", "modding-efx"),
            ("TIML", "modding-timl"),
        ] {
            let matches = search_from(&knowledge_root, query, Some(&modding_domains), 20).unwrap();
            assert!(
                matches
                    .matches
                    .iter()
                    .any(|item| item.result_id == expected_document),
                "{query} 必须召回 {expected_document}"
            );
        }
        let natural_mod_question = search_from(
            &knowledge_root,
            "帮我分析这个 MOD 里的 EVAM、slg 和 EPV 文件是怎么共同生效的",
            Some(&modding_domains),
            20,
        )
        .unwrap();
        assert!(natural_mod_question
            .matches
            .iter()
            .any(|item| item.result_id == "modding-evam-slinger"));
        assert!(natural_mod_question
            .matches
            .iter()
            .any(|item| item.result_id == "modding-slinger-chain"));
        assert!(natural_mod_question
            .matches
            .iter()
            .any(|item| item.result_id == "modding-epv"));
        let natural_evwp_question = search_from(
            &knowledge_root,
            "武器 MOD 里的 EVWP 会怎样映射全局 EPV 特效？",
            Some(&modding_domains),
            20,
        )
        .unwrap();
        assert!(natural_evwp_question
            .matches
            .iter()
            .any(|item| item.result_id == "modding-evwp"));
        let natural_material_question = search_from(
            &knowledge_root,
            "外观 MOD 的 MOD3、MRL3 和 TEX 文件为什么要一起部署，缺一个会怎样？",
            Some(&modding_domains),
            20,
        )
        .unwrap();
        for expected_document in ["modding-mod3", "modding-mrl3", "modding-texture-maps"] {
            assert!(
                natural_material_question
                    .matches
                    .iter()
                    .any(|item| item.result_id == expected_document),
                "模型材质自然问句必须召回 {expected_document}"
            );
        }
        let natural_texture_source_question = search_from(
            &knowledge_root,
            "MOD 里的 DDS 和 TEX 文件分别有什么用途？",
            Some(&modding_domains),
            20,
        )
        .unwrap();
        assert!(natural_texture_source_question
            .matches
            .iter()
            .any(|item| item.result_id == "modding-texture-maps"));

        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    #[ignore = "需要先运行 npm.cmd run knowledge:build-modding-dev，使用真实 MOD 技术包验证安装与检索链路"]
    fn generated_modding_pack_installs_and_answers_effect_scope_questions() {
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 必须位于项目根目录下");
        let modding = project_root
            .join("references")
            .join("knowledge")
            .join("build")
            .join("acumod-dev-modding.acukb");
        assert!(modding.is_file(), "缺少 MOD 技术开发包：{modding:?}");

        let test_root = unique_test_path("generated-modding-effect-pack");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        install_pack_into(
            &knowledge_root,
            modding.to_string_lossy().into_owned(),
            &OperationReporter::default(),
        )
        .unwrap();
        let domains = vec!["mhw-modding".to_string()];
        for (query, expected_document) in [
            ("会心特效和武器独立特效有什么区别", "modding-efx-scope"),
            (
                "本地 EPV、全局 EPV 与 EVWP 如何区分",
                "modding-weapon-epv-scope",
            ),
        ] {
            let response = search_from(&knowledge_root, query, Some(&domains), 20).unwrap();
            assert!(
                response
                    .matches
                    .iter()
                    .any(|item| item.result_id == expected_document),
                "特效问句“{query}”必须召回 {expected_document}"
            );
        }
        fs::remove_dir_all(test_root).unwrap();
    }

    #[test]
    fn rejects_invalid_structured_knowledge_before_installation() {
        let path = unique_test_path("invalid-json").with_extension("acukb");
        create_test_game_pack(&path);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE entities SET data_json = '{' WHERE id = 'item:elder-dragon-blood'",
                [],
            )
            .unwrap();
        drop(connection);

        let error = validate_pack(&path).err().unwrap();
        assert!(error.contains("不是有效 JSON"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn installs_switches_searches_rolls_back_and_deletes_packs() {
        let test_root = unique_test_path("lifecycle");
        let knowledge_root = test_root.join("knowledge");
        fs::create_dir_all(&test_root).unwrap();
        let first_source = test_root.join("first.acukb");
        let second_source = test_root.join("second.acukb");
        create_test_pack(&first_source, "0.1.0", "第一版飞翔爪模型资料。");
        create_test_pack(&second_source, "0.2.0", "第二版飞翔爪特效资料。");
        let reporter = OperationReporter::default();

        let first = install_pack_into(
            &knowledge_root,
            first_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();
        assert_eq!(first.status.active_pack_count, 1);
        assert_eq!(first.installed_pack.version, "0.1.0");
        assert_eq!(
            search_from(&knowledge_root, "第一版", None, 10)
                .unwrap()
                .matches
                .len(),
            1
        );

        let second = install_pack_into(
            &knowledge_root,
            second_source.to_string_lossy().into_owned(),
            &reporter,
        )
        .unwrap();
        assert_eq!(second.status.packs.len(), 2);
        assert_eq!(second.status.active_pack_count, 1);
        assert_eq!(second.installed_pack.version, "0.2.0");
        let matches = search_from(&knowledge_root, "第二版", None, 10)
            .unwrap()
            .matches;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pack_version, "0.2.0");

        // 强制索引写入失败，确认删除会把已暂存的包文件恢复，活动版本仍可查询。
        fs::create_dir(knowledge_root.join("index.json.tmp")).unwrap();
        assert!(delete_pack_from(&knowledge_root, "test-modding", &reporter).is_err());
        assert_eq!(
            get_status_from(&knowledge_root).unwrap().active_pack_count,
            1
        );
        assert_eq!(
            search_from(&knowledge_root, "第二版", None, 10)
                .unwrap()
                .matches
                .len(),
            1
        );
        fs::remove_dir(knowledge_root.join("index.json.tmp")).unwrap();

        let status = delete_pack_from(&knowledge_root, "test-modding", &reporter).unwrap();
        assert_eq!(status.active_pack_count, 0);
        assert!(status.packs.is_empty());
        assert!(first_source.is_file());
        assert!(second_source.is_file());
        fs::remove_dir_all(test_root).unwrap();
    }
}
