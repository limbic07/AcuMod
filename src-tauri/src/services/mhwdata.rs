//! 固定版 MHWorldData 的本地受控查询服务。
//!
//! 这里不使用 `.acukb` 的实体关系/全文检索格式，也不生成事实摘要。数据库中的
//! `entities` 只是把上游 CSV 行定位到稳定 ID 的查询索引，`records` 保留原始行。

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    operations::OperationReporter,
    services::knowledge::{
        KnowledgeEntityAlias, KnowledgeEntityLookupResponse, KnowledgeEntityMatch,
        KnowledgePackSummary, KnowledgeRelationMatch, KnowledgeRelationResponse,
    },
};

const DATABASE_APPLICATION_ID: i64 = 0x414D_4844;
const DATABASE_SCHEMA_VERSION: u32 = 1;
const MAX_DATABASE_SIZE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_QUERY_CHARS: usize = 200;
const MAX_RESULTS: usize = 50;
const MAX_STRUCTURED_DATA_BYTES: usize = 256 * 1024;
const POINTER_SCHEMA_VERSION: u32 = 1;
const SOURCE_TITLE: &str = "MHWorldData 固定快照";
const SOURCE_URL: &str = "https://github.com/gatheringhallstudios/MHWorldData";

#[derive(Clone)]
struct DatabaseManifest {
    display_name: String,
    content_baseline_version: String,
    runtime_game_version: String,
    source_commit: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveDatabasePointer {
    schema_version: u32,
    file_name: String,
    sha256: String,
    installed_at_unix_seconds: u64,
}

/// 把已解包的固定版数据库安全安装到知识根目录，并原子切换活动版本。
pub(crate) fn install_database_into(
    root: &Path,
    source_path: &Path,
    progress: &OperationReporter,
) -> Result<(), String> {
    validate_database_file(source_path)?;
    let database_root = database_root(root);
    fs::create_dir_all(&database_root)
        .map_err(|error| format!("无法创建 MHWData 数据目录：{error}"))?;
    let temporary_path = database_root.join(format!("install-{}.tmp", unix_nanos_now()?));
    progress.report("正在复制 MHWData 数值数据库", 0, Some(2), None);
    let (sha256, _) = match copy_and_hash(source_path, &temporary_path) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(&temporary_path);
            return Err(error);
        }
    };
    if let Err(error) = validate_database_file(&temporary_path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("复制后的 MHWData 数据库校验失败：{error}"));
    }

    let target_name = format!("mhwdata-{}.acumhwdb", &sha256[..16]);
    let target_path = database_root.join(&target_name);
    if target_path.exists() {
        let _ = fs::remove_file(&temporary_path);
    } else if let Err(error) = fs::rename(&temporary_path, &target_path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("无法写入 MHWData 数据库：{error}"));
    }
    progress.report("正在切换 MHWData 数值数据库", 1, Some(2), None);
    write_pointer(
        root,
        &ActiveDatabasePointer {
            schema_version: POINTER_SCHEMA_VERSION,
            file_name: target_name,
            sha256,
            installed_at_unix_seconds: unix_seconds_now()?,
        },
    )?;
    progress.report("MHWData 数值数据库已安装", 2, Some(2), None);
    Ok(())
}

/// 在写入用户数据目录前校验 ZIP 解包出来的数据库文件。
pub(crate) fn validate_bundle_database(path: &Path) -> Result<(), String> {
    validate_database_file(path).map(|_| ())
}

/// 删除当前 MHWData 数值数据库；调用方必须已取得用户的二次确认。
pub(crate) fn delete_database(root: &Path, progress: &OperationReporter) -> Result<(), String> {
    let Some(_) = read_pointer(root)? else {
        return Ok(());
    };
    progress.report("正在删除 MHWData 数值数据库", 0, Some(2), None);
    let root = database_root(root);
    for entry in
        fs::read_dir(&root).map_err(|error| format!("无法读取 MHWData 数据目录：{error}"))?
    {
        let path = entry
            .map_err(|error| format!("无法读取 MHWData 数据目录条目：{error}"))?
            .path();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("无法检查 MHWData 数据库文件：{error}"))?;
        // 只删除本 service 生成的受控文件名，且拒绝跟随链接。
        if metadata.is_file() && !metadata.file_type().is_symlink() && is_safe_file_name(file_name)
        {
            fs::remove_file(&path).map_err(|error| format!("无法删除 MHWData 数据库：{error}"))?;
        }
    }
    let pointer_path = root.join("active.json");
    if pointer_path.exists() {
        fs::remove_file(pointer_path)
            .map_err(|error| format!("无法删除 MHWData 活动索引：{error}"))?;
    }
    progress.report("MHWData 数值数据库已删除", 2, Some(2), None);
    Ok(())
}

/// 取得固定数值数据库的状态行；未安装时不返回伪造包记录。
pub(crate) fn status_summary(root: &Path) -> Result<Option<KnowledgePackSummary>, String> {
    let Some(pointer) = read_pointer(root)? else {
        return Ok(None);
    };
    let path = active_database_path(root, &pointer)?;
    let metadata = match fs::metadata(&path) {
        Ok(value) => value,
        Err(error) => {
            return Ok(Some(unhealthy_summary(
                &pointer,
                format!("MHWData 活动文件缺失：{error}"),
            )));
        }
    };
    let manifest = match validate_database_file(&path) {
        Ok(value) => value,
        Err(error) => return Ok(Some(unhealthy_summary(&pointer, error))),
    };
    if sha256_file(&path)? != pointer.sha256 {
        return Ok(Some(unhealthy_summary(
            &pointer,
            "MHWData 文件摘要与活动索引不一致。".to_string(),
        )));
    }
    Ok(Some(KnowledgePackSummary {
        pack_id: "mhwdata".to_string(),
        display_name: manifest.display_name,
        kind: "mhwdata".to_string(),
        version: manifest.content_baseline_version,
        game_version: manifest.runtime_game_version,
        locale: "zh-Hans".to_string(),
        description: format!(
            "固定提交 {} 的原始 MHWData CSV 受控查询数据库；不包含全文 RAG 或模型生成事实。",
            manifest.source_commit
        ),
        sha256: pointer.sha256,
        size_bytes: metadata.len(),
        installed_at_unix_seconds: pointer.installed_at_unix_seconds,
        entity_count: count_table(&path, "entities")?,
        relation_count: count_table(&path, "record_entities")?,
        document_count: 0,
        source_count: count_table(&path, "source_tables")?,
        active: true,
        healthy: true,
        error: None,
    }))
}

/// 按名称、别名或稳定 ID 查询固定版 MHWData 实体。调用方不能提交 SQL。
pub(crate) fn lookup_game_entities(
    root: &Path,
    query: &str,
    kinds: Option<&[String]>,
    limit: usize,
) -> Result<KnowledgeEntityLookupResponse, String> {
    let query = normalized_query(query, "游戏实体查询")?;
    let kinds = normalized_kinds(kinds)?;
    let limit = limit.clamp(1, MAX_RESULTS);
    let (path, manifest) = active_database(root)?;
    let connection = open_read_only(&path)?;
    let mut matches = Vec::new();
    let mut matched_ids = HashSet::new();
    for variant in query_variants(&query) {
        let remaining = limit.saturating_sub(matches.len());
        if remaining == 0 {
            break;
        }
        let mut variant_matches =
            lookup_variant(&connection, &manifest, &variant, &kinds, remaining)?;
        variant_matches.retain(|entry| matched_ids.insert(entry.entity_id.clone()));
        matches.append(&mut variant_matches);
    }
    Ok(KnowledgeEntityLookupResponse {
        query,
        searched_pack_count: 1,
        matches,
        warnings: Vec::new(),
    })
}

/// 读取一个实体已关联的上游 CSV 原始行。`predicate` 是固定 section（如 `monster.hitzones`）。
pub(crate) fn get_game_entity_relations(
    root: &Path,
    entity_id: &str,
    predicates: Option<&[String]>,
    direction: &str,
    limit: usize,
) -> Result<KnowledgeRelationResponse, String> {
    validate_entity_id(entity_id)?;
    let direction = normalize_direction(direction)?;
    let predicates = normalized_predicates(predicates)?;
    let limit = limit.clamp(1, MAX_RESULTS);
    let (path, manifest) = active_database(root)?;
    let connection = open_read_only(&path)?;
    let entity_name: String = connection
        .query_row(
            "SELECT COALESCE(name_zh_hans, name_zh_hant, name_en) FROM entities WHERE id = ?1",
            [entity_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("未找到 MHWData 实体 {entity_id}：{error}"))?;
    let predicate_filter = if predicates.is_empty() {
        String::new()
    } else {
        format!("\u{1f}{}\u{1f}", predicates.join("\u{1f}"))
    };
    let mut statement = connection
        .prepare(
            "SELECT r.id, r.section, r.data_json
             FROM record_entities re
             JOIN records r ON r.id = re.record_id
             WHERE re.entity_id = ?1
               AND (?2 = '' OR instr(?2, char(31) || r.section || char(31)) > 0)
             ORDER BY r.section, r.id
             LIMIT ?3",
        )
        .map_err(|error| format!("无法准备 MHWData 原始行查询：{error}"))?;
    let rows = statement
        .query_map(params![entity_id, predicate_filter, limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| format!("MHWData 原始行查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取 MHWData 原始行：{error}"))?;
    let relations = rows
        .into_iter()
        .map(|(record_id, section, data_json)| {
            if data_json.len() > MAX_STRUCTURED_DATA_BYTES {
                return Err(format!("MHWData 原始行 {record_id} 超过安全大小上限。"));
            }
            let data = serde_json::from_str::<Value>(&data_json)
                .map_err(|error| format!("MHWData 原始行 {record_id} JSON 无效：{error}"))?;
            Ok(KnowledgeRelationMatch {
                relation_id: format!("mhwdata:record:{record_id}"),
                subject_id: entity_id.to_string(),
                subject_name: entity_name.clone(),
                predicate: section.clone(),
                object_id: format!("mhwdata:record:{record_id}"),
                object_name: section,
                game_version: manifest.runtime_game_version.clone(),
                confidence: 1.0,
                data,
                source_title: Some(SOURCE_TITLE.to_string()),
                source_url: Some(SOURCE_URL.to_string()),
                pack_id: "mhwdata".to_string(),
                pack_version: manifest.content_baseline_version.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(KnowledgeRelationResponse {
        entity_id: entity_id.to_string(),
        direction: direction.to_string(),
        searched_pack_count: 1,
        relations,
        warnings: vec![
            "返回的是与实体关联的 MHWorldData 原始 CSV 行；predicate 为固定 section，不是可执行查询语句。".to_string(),
        ],
    })
}

/// 读取一个防具套装的五个部位及其制作原始行。
///
/// 这是对 `armorSet.base -> armor.crafting` 固定关联的受控展开，避免模型为了
/// 回答“整套需要哪些材料”而遗漏部位，调用方不能提交 SQL 或任意字段名。
pub(crate) fn get_armor_set_crafting(
    root: &Path,
    armor_set_id: &str,
) -> Result<KnowledgeRelationResponse, String> {
    validate_entity_id(armor_set_id)?;
    let (path, manifest) = active_database(root)?;
    let connection = open_read_only(&path)?;
    let set_data_json: String = connection
        .query_row(
            "SELECT data_json FROM entities WHERE id = ?1 AND kind = 'armorSet'",
            [armor_set_id],
            |row| row.get(0),
        )
        .map_err(|_| format!("未找到防具套装实体 {armor_set_id}。"))?;
    let set_data = serde_json::from_str::<Value>(&set_data_json)
        .map_err(|error| format!("防具套装 {armor_set_id} 的原始数据无效：{error}"))?;
    let slots = ["head", "chest", "arms", "waist", "legs"];
    let mut relations = Vec::with_capacity(slots.len());

    for slot in slots {
        let armor_name = set_data
            .get(slot)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("防具套装 {armor_set_id} 缺少 {slot} 部位名称。"))?;
        let (armor_id, armor_name_display, record_id, crafting_json): (String, String, i64, String) = connection
            .query_row(
                "SELECT e.id, COALESCE(e.name_zh_hans, e.name_zh_hant, e.name_en), r.id, r.data_json
                 FROM entities e
                 JOIN record_entities re ON re.entity_id = e.id
                 JOIN records r ON r.id = re.record_id
                 WHERE e.kind = 'armor' AND e.name_en = ?1 AND r.section = 'armor.crafting'
                 ORDER BY r.id
                 LIMIT 1",
                [armor_name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| format!("防具套装 {armor_set_id} 的 {slot} 部位缺少 armor.crafting 原始行。"))?;
        if crafting_json.len() > MAX_STRUCTURED_DATA_BYTES {
            return Err(format!(
                "MHWData 防具制作原始行 {record_id} 超过安全大小上限。"
            ));
        }
        let mut data = serde_json::from_str::<Value>(&crafting_json)
            .map_err(|error| format!("MHWData 防具制作原始行 {record_id} JSON 无效：{error}"))?;
        append_localized_materials(&connection, &mut data)?;
        relations.push(KnowledgeRelationMatch {
            relation_id: format!("mhwdata:record:{record_id}"),
            subject_id: armor_id,
            subject_name: armor_name_display,
            predicate: "armor.crafting".to_string(),
            object_id: format!("mhwdata:record:{record_id}"),
            object_name: slot.to_string(),
            game_version: manifest.runtime_game_version.clone(),
            confidence: 1.0,
            data,
            source_title: Some(SOURCE_TITLE.to_string()),
            source_url: Some(SOURCE_URL.to_string()),
            pack_id: "mhwdata".to_string(),
            pack_version: manifest.content_baseline_version.clone(),
        });
    }

    Ok(KnowledgeRelationResponse {
        entity_id: armor_set_id.to_string(),
        direction: "outgoing".to_string(),
        searched_pack_count: 1,
        relations,
        warnings: vec![
            "返回的是 MHWData armorSet.base 所列五个部位的 armor.crafting 原始行；materials 仅为同库物品名称桥。".to_string(),
        ],
    })
}

/// 在不改变上游制作行的前提下，附加同一数据库中已经核对的中文物品名称。
fn append_localized_materials(connection: &Connection, data: &mut Value) -> Result<(), String> {
    let mut materials = Vec::new();
    for index in 1..=4 {
        let name_en = data
            .get(format!("item{index}_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let quantity = data
            .get(format!("item{index}_qty"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (Some(name_en), Some(quantity)) = (name_en, quantity) else {
            continue;
        };
        let name_zh_hans = connection
            .query_row(
                "SELECT name_zh_hans FROM entities WHERE kind = 'item' AND name_en = ?1 LIMIT 1",
                [name_en],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("无法读取制作材料 {name_en} 的名称桥：{error}"))?
            .flatten();
        materials.push(json!({
            "nameEn": name_en,
            "nameZhHans": name_zh_hans,
            "quantity": quantity,
        }));
    }
    let object = data
        .as_object_mut()
        .ok_or_else(|| "MHWData 防具制作原始行不是 JSON 对象。".to_string())?;
    object.insert("materials".to_string(), Value::Array(materials));
    Ok(())
}

fn lookup_variant(
    connection: &Connection,
    manifest: &DatabaseManifest,
    query: &str,
    kinds: &str,
    limit: usize,
) -> Result<Vec<KnowledgeEntityMatch>, String> {
    let like_query = format!("%{}%", escape_like(query));
    let normalized_query = query.to_lowercase();
    let mut statement = connection
        .prepare(
            "WITH ranked AS (
                SELECT e.id,
                       MIN(CASE WHEN e.id = ?2 COLLATE NOCASE
                                      OR e.name_en = ?2 COLLATE NOCASE
                                      OR e.name_zh_hans = ?2
                                      OR e.name_zh_hant = ?2
                                      OR a.normalized_alias = ?3 THEN 0 ELSE 1 END) AS match_rank
                FROM entities e
                LEFT JOIN aliases a ON a.entity_id = e.id
                WHERE (?4 = '' OR instr(?4, char(31) || e.kind || char(31)) > 0)
                  AND (e.id LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                    OR e.name_en LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                    OR e.name_zh_hans LIKE ?1 ESCAPE '\\'
                    OR e.name_zh_hant LIKE ?1 ESCAPE '\\'
                    OR a.normalized_alias LIKE ?5 ESCAPE '\\')
                GROUP BY e.id
                ORDER BY match_rank, length(COALESCE(e.name_zh_hans, e.name_zh_hant, e.name_en)), e.id
                LIMIT ?6
             )
             SELECT e.id, e.kind, e.name_en, e.name_zh_hans, e.name_zh_hant, e.data_json
             FROM ranked r JOIN entities e ON e.id = r.id
             ORDER BY r.match_rank, length(COALESCE(e.name_zh_hans, e.name_zh_hant, e.name_en)), e.id",
        )
        .map_err(|error| format!("无法准备 MHWData 实体查询：{error}"))?;
    let rows = statement
        .query_map(
            params![
                like_query,
                query,
                normalized_query,
                kinds,
                format!("%{}%", escape_like(&normalized_query)),
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|error| format!("MHWData 实体查询失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法读取 MHWData 实体：{error}"))?;
    let mut aliases_statement = connection
        .prepare("SELECT locale, alias FROM aliases WHERE entity_id = ?1 ORDER BY locale, alias")
        .map_err(|error| format!("无法准备 MHWData 别名查询：{error}"))?;
    rows.into_iter()
        .map(|row| {
            if row.5.len() > MAX_STRUCTURED_DATA_BYTES {
                return Err(format!(
                    "MHWData 实体 {} 的原始数据超过安全大小上限。",
                    row.0
                ));
            }
            let aliases = aliases_statement
                .query_map([row.0.as_str()], |alias_row| {
                    Ok(KnowledgeEntityAlias {
                        locale: alias_row.get(0)?,
                        alias: alias_row.get(1)?,
                    })
                })
                .map_err(|error| format!("MHWData 实体 {} 的别名查询失败：{error}", row.0))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("无法读取 MHWData 实体 {} 的别名：{error}", row.0))?;
            let data = serde_json::from_str::<Value>(&row.5)
                .map_err(|error| format!("MHWData 实体 {} JSON 无效：{error}", row.0))?;
            Ok(KnowledgeEntityMatch {
                entity_id: row.0,
                kind: row.1,
                domain: "mhwdata".to_string(),
                canonical_name: row
                    .3
                    .clone()
                    .or(row.4.clone())
                    .unwrap_or_else(|| row.2.clone()),
                name_zh_hans: row.3,
                name_zh_hant: row.4,
                summary: "MHWorldData 原始 CSV 基础行；请继续按需要读取固定 section 的关联原始行。"
                    .to_string(),
                game_version: manifest.runtime_game_version.clone(),
                confidence: 1.0,
                data,
                aliases,
                source_title: Some(SOURCE_TITLE.to_string()),
                source_url: Some(SOURCE_URL.to_string()),
                pack_id: "mhwdata".to_string(),
                pack_version: manifest.content_baseline_version.clone(),
            })
        })
        .collect()
}

fn active_database(root: &Path) -> Result<(PathBuf, DatabaseManifest), String> {
    let pointer = read_pointer(root)?.ok_or_else(|| {
        "尚未安装 MHWData 数值数据库。请在知识库设置中安装包含 `.acumhwdb` 的整套知识包。"
            .to_string()
    })?;
    let path = active_database_path(root, &pointer)?;
    let manifest = validate_database_file(&path)?;
    if sha256_file(&path)? != pointer.sha256 {
        return Err("MHWData 文件摘要与活动索引不一致；请重新安装知识包。".to_string());
    }
    Ok((path, manifest))
}

fn validate_database_file(path: &Path) -> Result<DatabaseManifest, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("无法读取 MHWData 数据库：{error}"))?;
    if metadata.len() == 0 || metadata.len() > MAX_DATABASE_SIZE_BYTES {
        return Err("MHWData 数据库为空或超过 4 GB 安全上限。".to_string());
    }
    let connection = open_read_only(path)?;
    let application_id: i64 = connection
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(|error| format!("无法读取 MHWData 数据库标识：{error}"))?;
    if application_id != DATABASE_APPLICATION_ID {
        return Err("文件不是 Acumod MHWData 数值数据库。".to_string());
    }
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| format!("无法读取 MHWData 数据库 schema：{error}"))?;
    if version != DATABASE_SCHEMA_VERSION {
        return Err(format!("MHWData 数据库 schema {version} 不受支持。"));
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
        .map_err(|error| format!("MHWData 数据库完整性检查失败：{error}"))?;
    if integrity != "ok" {
        return Err(format!("MHWData 数据库已损坏：{integrity}"));
    }
    validate_schema(&connection)?;
    let manifest_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM mhwdata_manifest", [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("无法读取 MHWData manifest：{error}"))?;
    if manifest_count != 1 {
        return Err("MHWData 数据库必须且只能包含一条 manifest。".to_string());
    }
    let manifest = connection
        .query_row(
            "SELECT id, display_name, format_version, content_baseline_version, runtime_game_version, source_commit FROM mhwdata_manifest",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|error| format!("无法解析 MHWData manifest：{error}"))?;
    if manifest.0 != "mhwdata" || manifest.2 != DATABASE_SCHEMA_VERSION as i64 {
        return Err("MHWData manifest 标识或版本无效。".to_string());
    }
    for value in [&manifest.1, &manifest.3, &manifest.4, &manifest.5] {
        if value.trim().is_empty() || value.chars().count() > 160 {
            return Err("MHWData manifest 包含无效文本。".to_string());
        }
    }
    Ok(DatabaseManifest {
        display_name: manifest.1,
        content_baseline_version: manifest.3,
        runtime_game_version: manifest.4,
        source_commit: manifest.5,
    })
}

fn validate_schema(connection: &Connection) -> Result<(), String> {
    let required = [
        "mhwdata_manifest",
        "source_tables",
        "entities",
        "aliases",
        "records",
        "record_entities",
    ];
    let mut statement = connection
        .prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .map_err(|error| format!("无法检查 MHWData schema：{error}"))?;
    let objects = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("无法读取 MHWData schema：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("无法解析 MHWData schema：{error}"))?;
    if objects
        .iter()
        .any(|(kind, _)| kind == "trigger" || kind == "view")
    {
        return Err("MHWData 数据库不允许触发器或视图。".to_string());
    }
    for table in required {
        if !objects
            .iter()
            .any(|(kind, name)| kind == "table" && name == table)
        {
            return Err(format!("MHWData 数据库缺少表：{table}"));
        }
    }
    Ok(())
}

fn count_table(path: &Path, table: &str) -> Result<usize, String> {
    let connection = open_read_only(path)?;
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value.max(0) as usize)
        .map_err(|error| format!("无法读取 MHWData {table} 数量：{error}"))
}

fn database_root(root: &Path) -> PathBuf {
    root.join("mhwdata")
}

fn pointer_path(root: &Path) -> PathBuf {
    database_root(root).join("active.json")
}

fn read_pointer(root: &Path) -> Result<Option<ActiveDatabasePointer>, String> {
    let path = pointer_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(&path).map_err(|error| format!("无法读取 MHWData 活动索引：{error}"))?;
    let pointer = serde_json::from_str::<ActiveDatabasePointer>(&contents)
        .map_err(|error| format!("MHWData 活动索引格式无效：{error}"))?;
    if pointer.schema_version != POINTER_SCHEMA_VERSION
        || !is_safe_file_name(&pointer.file_name)
        || pointer.sha256.len() != 64
        || !pointer
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("MHWData 活动索引包含不安全字段。".to_string());
    }
    Ok(Some(pointer))
}

fn active_database_path(root: &Path, pointer: &ActiveDatabasePointer) -> Result<PathBuf, String> {
    if !is_safe_file_name(&pointer.file_name) {
        return Err("MHWData 活动索引包含不安全路径。".to_string());
    }
    Ok(database_root(root).join(&pointer.file_name))
}

fn write_pointer(root: &Path, pointer: &ActiveDatabasePointer) -> Result<(), String> {
    let root = database_root(root);
    fs::create_dir_all(&root).map_err(|error| format!("无法创建 MHWData 索引目录：{error}"))?;
    let destination = root.join("active.json");
    let temporary = root.join(format!("active-{}.tmp", unix_nanos_now()?));
    let contents = serde_json::to_vec_pretty(pointer)
        .map_err(|error| format!("无法序列化 MHWData 活动索引：{error}"))?;
    fs::write(&temporary, contents)
        .map_err(|error| format!("无法写入 MHWData 临时索引：{error}"))?;
    if destination.exists() {
        let backup = root.join(format!("active-{}.bak", unix_nanos_now()?));
        fs::rename(&destination, &backup)
            .map_err(|error| format!("无法备份 MHWData 旧索引：{error}"))?;
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::rename(&backup, &destination);
            let _ = fs::remove_file(&temporary);
            return Err(format!("无法切换 MHWData 活动索引：{error}"));
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("无法写入 MHWData 活动索引：{error}"))?;
    }
    Ok(())
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("无法以只读方式打开 MHWData 数据库：{error}"))?;
    connection
        .execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;")
        .map_err(|error| format!("无法启用 MHWData 只读保护：{error}"))?;
    Ok(connection)
}

fn copy_and_hash(source: &Path, target: &Path) -> Result<(String, u64), String> {
    let mut input =
        File::open(source).map_err(|error| format!("无法读取 MHWData 数据库：{error}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| format!("无法创建 MHWData 临时数据库：{error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 MHWData 数据库：{error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("无法写入 MHWData 临时数据库：{error}"))?;
        digest.update(&buffer[..read]);
        copied += read as u64;
        if copied > MAX_DATABASE_SIZE_BYTES {
            return Err("MHWData 数据库超过 4 GB 安全上限。".to_string());
        }
    }
    output
        .flush()
        .map_err(|error| format!("无法刷新 MHWData 临时数据库：{error}"))?;
    let sha256 = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((sha256, copied))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("无法读取 MHWData 数据库：{error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 MHWData 数据库：{error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

fn unhealthy_summary(pointer: &ActiveDatabasePointer, error: String) -> KnowledgePackSummary {
    KnowledgePackSummary {
        pack_id: "mhwdata".to_string(),
        display_name: "MHWData 游戏数值数据库".to_string(),
        kind: "mhwdata".to_string(),
        version: "未知".to_string(),
        game_version: "未知".to_string(),
        locale: "zh-Hans".to_string(),
        description: "固定版 MHWData 本地数据库。".to_string(),
        sha256: pointer.sha256.clone(),
        size_bytes: 0,
        installed_at_unix_seconds: pointer.installed_at_unix_seconds,
        entity_count: 0,
        relation_count: 0,
        document_count: 0,
        source_count: 0,
        active: true,
        healthy: false,
        error: Some(error),
    }
}

fn normalized_query(value: &str, label: &str) -> Result<String, String> {
    let query = value.trim();
    if query.is_empty() {
        return Err(format!("{label}内容不能为空。"));
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(format!("{label}不能超过 {MAX_QUERY_CHARS} 个字符。"));
    }
    Ok(query.to_string())
}

fn normalized_kinds(values: Option<&[String]>) -> Result<String, String> {
    let mut kinds = Vec::new();
    for kind in values.unwrap_or_default() {
        let kind = kind.trim();
        if kind.is_empty()
            || kind.len() > 80
            || !kind.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(format!("实体类型“{kind}”格式无效。"));
        }
        if !kinds.iter().any(|existing| existing == kind) {
            kinds.push(kind.to_string());
        }
    }
    if kinds.len() > 16 {
        return Err("实体类型最多可指定 16 项。".to_string());
    }
    Ok(if kinds.is_empty() {
        String::new()
    } else {
        format!("\u{1f}{}\u{1f}", kinds.join("\u{1f}"))
    })
}

fn normalized_predicates(values: Option<&[String]>) -> Result<Vec<String>, String> {
    let mut predicates = Vec::new();
    for predicate in values.unwrap_or_default() {
        let predicate = predicate.trim();
        if predicate.is_empty()
            || predicate.len() > 80
            || !predicate.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(format!("MHWData section“{predicate}”格式无效。"));
        }
        if !predicates.iter().any(|existing| existing == predicate) {
            predicates.push(predicate.to_string());
        }
    }
    if predicates.len() > 24 {
        return Err("MHWData section 最多可指定 24 项。".to_string());
    }
    Ok(predicates)
}

fn validate_entity_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > 240 || value.chars().any(char::is_control)
    {
        return Err("游戏实体 ID 格式无效。".to_string());
    }
    Ok(())
}

fn normalize_direction(value: &str) -> Result<&str, String> {
    match value.trim() {
        "outgoing" => Ok("outgoing"),
        "incoming" => Ok("incoming"),
        "both" | "" => Ok("both"),
        _ => Err("关系方向只支持 outgoing、incoming 或 both。".to_string()),
    }
}

fn query_variants(query: &str) -> Vec<String> {
    let characters = query.chars().collect::<Vec<_>>();
    let mut compact = String::with_capacity(query.len());
    let mut changed = false;
    for (index, character) in characters.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|value| characters.get(value));
        let next = characters.get(index + 1);
        let can_compact = character.is_whitespace()
            && previous.is_some_and(|value| matches!(value, '\u{4e00}'..='\u{9fff}'))
            && next.is_some_and(|value| value.is_ascii_digit() || matches!(value, 'I' | 'V' | 'X'));
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

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn is_safe_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.ends_with(".acumhwdb")
        && !value.contains(['/', '\\', ':'])
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn unix_seconds_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|error| format!("系统时间异常：{error}"))
}

fn unix_nanos_now() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .map_err(|error| format!("系统时间异常：{error}"))
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use crate::operations::OperationReporter;

    use super::{
        delete_database, get_armor_set_crafting, get_game_entity_relations, install_database_into,
        lookup_game_entities, status_summary, unix_nanos_now,
    };

    #[test]
    fn installed_database_returns_raw_weapon_rows() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../references/knowledge/build/acumod-mhwdata-15.10.acumhwdb");
        assert!(
            source.is_file(),
            "请先运行 npm.cmd run knowledge:build-mhwdata。"
        );
        let root =
            env::temp_dir().join(format!("acumod-mhwdata-test-{}", unix_nanos_now().unwrap()));
        let reporter = OperationReporter::default();
        install_database_into(&root, &source, &reporter).unwrap();

        let kinds = vec!["weapon".to_string()];
        let response =
            lookup_game_entities(&root, "Defender Great Sword I", Some(&kinds), 4).unwrap();
        let weapon = response
            .matches
            .into_iter()
            .find(|entry| entry.entity_id == "mhwdata:weapon:2001")
            .unwrap();
        assert_eq!(weapon.data["attack"], "624");
        let sections = vec!["weapon.sharpness".to_string()];
        let rows = get_game_entity_relations(&root, &weapon.entity_id, Some(&sections), "both", 4)
            .unwrap();
        assert!(!rows.relations.is_empty());
        assert_eq!(rows.relations[0].predicate, "weapon.sharpness");
        assert_eq!(
            status_summary(&root).unwrap().unwrap().game_version,
            "15.23"
        );

        let armor_set_kinds = vec!["armorSet".to_string()];
        let black_dragon_sets =
            lookup_game_entities(&root, "黑龙套", Some(&armor_set_kinds), 4).unwrap();
        let alpha_set = black_dragon_sets
            .matches
            .into_iter()
            .find(|entry| entry.canonical_name == "黑龙α+套装")
            .expect("黑龙套中文别名应定位到 α+ 套装");
        let recipes = get_armor_set_crafting(&root, &alpha_set.entity_id).unwrap();
        assert_eq!(recipes.relations.len(), 5);
        assert_eq!(recipes.relations[0].subject_name, "黑龙头部α+");
        assert_eq!(recipes.relations[0].data["item1_qty"], "3");
        assert_eq!(
            recipes.relations[0].data["materials"][0]["nameZhHans"],
            "黑龙的重壳"
        );

        delete_database(&root, &reporter).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
