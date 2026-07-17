use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    operations::OperationReporter,
    services::{game, mod_library, model_recognition},
};

const DEFAULT_RESULT_LIMIT: usize = 100;
const MAX_RESULT_LIMIT: usize = 100;

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "search_local_mods",
                "description": "按名称、备注、分类、启用状态或替换目标搜索 Acumod 本地 MOD。支持分页；用户要求所有或全部结果时，必须从 offset 0 开始并按 nextOffset 继续查询，直到 nextOffset 为 null。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "名称、分类、备注、游戏内名称或资源 ID；可省略" },
                        "enabled": { "type": "boolean", "description": "只查询已启用或未启用 MOD；可省略" },
                        "category": { "type": "string", "description": "分类名称；可省略" },
                        "offset": { "type": "integer", "minimum": 0, "description": "分页起点；首次查询省略或传 0" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "每页数量，完整列表建议传 100" },
                        "includeDetails": { "type": "boolean", "description": "是否附带替换摘要；完整名称列表不需要开启" }
                    },
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_mod_details",
                "description": "按稳定 MOD ID 查询状态、分类、替换目标和冲突摘要。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modIds": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "maxItems": 10
                        }
                    },
                    "required": ["modIds"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_enabled_conflicts",
                "description": "查询当前已启用 MOD 冲突组和优先级。只有用户明确要求冲突文件时才把 includeFiles 设为 true。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "includeFiles": { "type": "boolean" }
                    },
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_game_directory_status",
                "description": "查询 MHW 游戏目录是否已经配置且有效；不会返回本地绝对路径。",
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "lookup_mhw_terms",
                "description": "查询内置 MHW 简体中文、繁体中文游戏名称和资源 ID。用于核对武器、防具、发型等游戏术语。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 30 }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
    ]
}

pub(crate) fn tool_label(name: &str) -> &'static str {
    match name {
        "search_local_mods" => "查询本地 MOD",
        "get_mod_details" => "读取 MOD 详情",
        "get_enabled_conflicts" => "分析启用冲突",
        "get_game_directory_status" => "检查游戏目录",
        "lookup_mhw_terms" => "查询 MHW 术语",
        _ => "处理只读查询",
    }
}

pub(crate) async fn execute_tool(
    app: &AppHandle,
    name: &str,
    arguments: &str,
) -> Result<String, String> {
    match name {
        "search_local_mods" => {
            let args = parse_arguments::<SearchLocalModsArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            search_local_mods(snapshot, args)
        }
        "get_mod_details" => {
            let args = parse_arguments::<GetModDetailsArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            get_mod_details(snapshot, args)
        }
        "get_enabled_conflicts" => {
            let args = parse_arguments::<ConflictArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            get_enabled_conflicts(snapshot, args)
        }
        "get_game_directory_status" => {
            parse_arguments::<EmptyArgs>(arguments)?;
            let status = game::get_game_directory_status(app)?;
            Ok(json!({
                "ok": true,
                "isConfigured": status.is_configured,
                "isValid": status.is_valid,
                "message": status.message
            })
            .to_string())
        }
        "lookup_mhw_terms" => {
            let args = parse_arguments::<LookupTermsArgs>(arguments)?;
            let limit = normalized_limit(args.limit);
            let query = args.query;
            let terms = tauri::async_runtime::spawn_blocking(move || {
                model_recognition::search_game_terms(&query, limit)
            })
            .await
            .map_err(|error| format!("MHW 术语查询任务失败：{error}"))??;
            Ok(json!({ "ok": true, "terms": terms }).to_string())
        }
        _ => Err("模型请求了未开放的工具。".to_string()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchLocalModsArgs {
    query: Option<String>,
    enabled: Option<bool>,
    category: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    #[serde(default)]
    include_details: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetModDetailsArgs {
    mod_ids: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConflictArgs {
    #[serde(default)]
    include_files: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LookupTermsArgs {
    query: String,
    limit: Option<usize>,
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T, String> {
    serde_json::from_str(arguments).map_err(|error| format!("AI 工具参数格式无效：{error}"))
}

async fn load_snapshot(app: &AppHandle) -> Result<mod_library::ModWorkspaceSnapshot, String> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        mod_library::get_mod_workspace_snapshot_with_progress(
            &worker_app,
            &OperationReporter::default(),
        )
    })
    .await
    .map_err(|error| format!("本地 MOD 查询任务失败：{error}"))?
}

fn search_local_mods(
    snapshot: mod_library::ModWorkspaceSnapshot,
    args: SearchLocalModsArgs,
) -> Result<String, String> {
    let query = args.query.unwrap_or_default().trim().to_lowercase();
    let category = args.category.unwrap_or_default().trim().to_lowercase();
    let limit = normalized_limit(args.limit);
    let matches = snapshot
        .installed_mods
        .mods
        .iter()
        .filter(|item| args.enabled.is_none_or(|enabled| item.enabled == enabled))
        .filter(|item| {
            category.is_empty()
                || item
                    .categories
                    .iter()
                    .any(|value| value.name.to_lowercase().contains(&category))
        })
        .filter(|item| query.is_empty() || mod_search_text(item).contains(&query))
        .collect::<Vec<_>>();
    let total = matches.len();
    let (start, end, next_offset) = pagination_bounds(total, args.offset.unwrap_or(0), limit);
    let mods = matches
        .into_iter()
        .skip(start)
        .take(end - start)
        .map(|item| {
            let mut result = json!({
                "id": item.id,
                "name": item.name,
                "enabled": item.enabled,
                "partiallyOverridden": item.partially_overridden,
                "categories": item.categories.iter().map(|value| value.name.as_str()).collect::<Vec<_>>()
            });
            if args.include_details {
                result["replacements"] =
                    Value::Array(replacement_summaries(&item.model_replacements));
            }
            result
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "total": total,
        "offset": start,
        "returned": mods.len(),
        "nextOffset": next_offset,
        "truncated": next_offset.is_some(),
        "mods": mods
    })
    .to_string())
}

fn get_mod_details(
    snapshot: mod_library::ModWorkspaceSnapshot,
    args: GetModDetailsArgs,
) -> Result<String, String> {
    if args.mod_ids.is_empty() || args.mod_ids.len() > 10 {
        return Err("一次只能查询 1 到 10 个 MOD。".to_string());
    }
    let mut mods = Vec::new();
    let mut missing_ids = Vec::new();
    for mod_id in args.mod_ids {
        let Some(item) = snapshot
            .installed_mods
            .mods
            .iter()
            .find(|item| item.id == mod_id)
        else {
            missing_ids.push(mod_id);
            continue;
        };
        let conflicts = snapshot
            .conflict_report
            .groups
            .iter()
            .filter(|group| {
                group
                    .participants
                    .iter()
                    .any(|participant| participant.mod_id == item.id)
            })
            .map(|group| {
                json!({
                    "groupId": group.group_id,
                    "conflictFileCount": group.conflict_file_count,
                    "participants": group.participants.iter().map(|participant| participant.name.as_str()).collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        mods.push(json!({
            "id": item.id,
            "name": item.name,
            "originalName": item.original_name,
            "note": item.note,
            "enabled": item.enabled,
            "partiallyOverridden": item.partially_overridden,
            "categories": item.categories.iter().map(|value| value.name.as_str()).collect::<Vec<_>>(),
            "replacements": replacement_summaries(&item.model_replacements),
            "conflicts": conflicts
        }));
    }
    Ok(json!({ "ok": true, "mods": mods, "missingIds": missing_ids }).to_string())
}

fn get_enabled_conflicts(
    snapshot: mod_library::ModWorkspaceSnapshot,
    args: ConflictArgs,
) -> Result<String, String> {
    let groups = snapshot
        .conflict_report
        .groups
        .iter()
        .map(|group| {
            json!({
                "groupId": group.group_id,
                "conflictFileCount": group.conflict_file_count,
                "participants": group.participants.iter().map(|participant| json!({
                    "modId": participant.mod_id,
                    "name": participant.name,
                    "enabled": participant.enabled,
                    "priority": participant.order
                })).collect::<Vec<_>>(),
                "conflictFiles": args.include_files.then(|| group.conflict_files.iter().take(50).collect::<Vec<_>>())
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "conflictCount": snapshot.conflict_report.conflict_count,
        "groups": groups
    })
    .to_string())
}

fn mod_search_text(item: &mod_library::InstalledModSummary) -> String {
    let mut values = vec![
        item.name.as_str(),
        item.original_name.as_str(),
        item.note.as_str(),
    ];
    values.extend(item.categories.iter().map(|value| value.name.as_str()));
    values.extend(
        item.model_replacements
            .iter()
            .flat_map(|replacement| replacement.display_names.iter().map(String::as_str)),
    );
    values.extend(
        item.model_replacements
            .iter()
            .map(|replacement| replacement.model_id.as_str()),
    );
    values.extend(item.model_replacements.iter().flat_map(|replacement| {
        [
            replacement.model_kind.as_str(),
            replacement.sub_kind.as_str(),
        ]
    }));
    values.join(" ").to_lowercase()
}

fn replacement_summaries(replacements: &[model_recognition::ModelReplacement]) -> Vec<Value> {
    replacements
        .iter()
        .take(20)
        .map(|replacement| {
            json!({
                "kind": replacement.model_kind,
                "subKind": replacement.sub_kind,
                "modelId": replacement.model_id,
                "displayNames": replacement.display_names.iter().take(6).collect::<Vec<_>>()
            })
        })
        .collect()
}

fn normalized_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_RESULT_LIMIT)
        .clamp(1, MAX_RESULT_LIMIT)
}

/// 将越界 offset 收敛到结果末尾，并明确给出下一页起点。
fn pagination_bounds(total: usize, offset: usize, limit: usize) -> (usize, usize, Option<usize>) {
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    let next_offset = (end < total).then_some(end);
    (start, end, next_offset)
}

#[cfg(test)]
mod tests {
    use super::{normalized_limit, pagination_bounds, DEFAULT_RESULT_LIMIT, MAX_RESULT_LIMIT};

    #[test]
    fn local_mod_results_can_continue_after_the_first_hundred_items() {
        assert_eq!(pagination_bounds(103, 0, 100), (0, 100, Some(100)));
        assert_eq!(pagination_bounds(103, 100, 100), (100, 103, None));
    }

    #[test]
    fn pagination_handles_out_of_range_offsets_and_limits() {
        assert_eq!(pagination_bounds(10, 99, 100), (10, 10, None));
        assert_eq!(normalized_limit(None), DEFAULT_RESULT_LIMIT);
        assert_eq!(normalized_limit(Some(999)), MAX_RESULT_LIMIT);
    }
}
