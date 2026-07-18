use futures_util::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    operations::OperationReporter,
    services::{game, mod_library, model_recognition, nexus},
    storage::config,
};

use super::{cleanup, source_search, AgentActionPlan, AgentCoordinator};

const DEFAULT_RESULT_LIMIT: usize = 100;
const MAX_RESULT_LIMIT: usize = 100;

pub(crate) struct ToolExecution {
    pub content: String,
    pub plan: Option<AgentActionPlan>,
    pub cleanup_review: Option<cleanup::AgentCleanupReview>,
}

impl ToolExecution {
    fn query(content: String) -> Self {
        Self {
            content,
            plan: None,
            cleanup_review: None,
        }
    }

    fn plan(plan: AgentActionPlan) -> Result<Self, String> {
        // 完整计划通过本地 Channel 交给界面；只把最小回执发回模型，避免再次上传目标清单。
        let content = serde_json::to_string(&json!({
            "ok": true,
            "planId": plan.plan_id,
            "title": plan.title,
            "targetCount": plan.target_count,
            "warningCount": plan.warnings.len(),
            "awaitingUserConfirmation": true
        }))
        .map_err(|error| format!("无法序列化 AI 操作计划：{error}"))?;
        Ok(Self {
            content,
            plan: Some(plan),
            cleanup_review: None,
        })
    }

    fn cleanup_review(review: cleanup::AgentCleanupReview) -> Result<Self, String> {
        let content = serde_json::to_string(&json!({
            "ok": true,
            "reviewId": review.review_id,
            "candidateCount": review.candidate_count,
            "awaitingUserSelection": true
        }))
        .map_err(|error| format!("无法序列化 AI 清理结果：{error}"))?;
        Ok(Self {
            content,
            plan: None,
            cleanup_review: Some(review),
        })
    }
}

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
                "name": "scan_mod_cleanup_candidates",
                "description": "盘点全部已安装 MOD 的可部署文件。本地规则先处理确定保留和确定排除项，只返回需要 AcuAI 判断的模糊文件组。首次调用省略 auditId，后续分页必须复用返回的 auditId。扫描不修改任何文件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "auditId": { "type": "string", "description": "后续分页复用首次扫描返回的审查 ID" },
                        "offset": { "type": "integer", "minimum": 0 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    },
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_mod_cleanup_text",
                "description": "仅在无法根据文件组元数据判断时，读取当前清理审查中一个候选文件最多 32 KB 的安全文本。Rust 会拒绝二进制类型并隐藏疑似凭据和本地路径。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "auditId": { "type": "string" },
                        "candidateId": { "type": "string" }
                    },
                    "required": ["auditId", "candidateId"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "submit_mod_cleanup_review",
                "description": "提交全部模糊文件组的结构化分类，并与本地规则建议合并后展示给用户逐项选择。必须覆盖审查中的每个 groupId，不能漏项或重复。不会执行清理。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "auditId": { "type": "string" },
                        "classifications": {
                            "type": "array",
                            "maxItems": 2000,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "groupId": { "type": "string" },
                                    "recommendation": { "type": "string", "enum": ["remove", "review", "keep"] },
                                    "reason": { "type": "string" },
                                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                                },
                                "required": ["groupId", "recommendation", "reason", "confidence"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["auditId", "classifications"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_mod_cleanup_exclusions",
                "description": "查询当前部署排除记录，用于回答或生成恢复计划。本地 MOD 库原始文件仍然存在。",
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
                "name": "create_cleanup_restore_plan",
                "description": "为部署排除项生成恢复计划。scope=lastBatch 恢复最近一次，scope=mod 恢复指定稳定 MOD ID，scope=all 恢复全部。只创建计划。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "scope": { "type": "string", "enum": ["lastBatch", "mod", "all"] },
                        "modId": { "type": "string" }
                    },
                    "required": ["scope"],
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
        json!({
            "type": "function",
            "function": {
                "name": "get_mod_remap_options",
                "description": "按稳定 MOD ID 查询可改绑分组和目标。模型改绑前必须先调用；query 可按游戏名称或资源 ID 缩小目标。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modId": { "type": "string" },
                        "groupKey": { "type": "string", "description": "已知分组时传入；可省略" },
                        "query": { "type": "string", "description": "目标游戏名称或资源 ID；可省略" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 30 }
                    },
                    "required": ["modId"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_mod_action_plan",
                "description": "为已经用稳定 ID 明确选中的 MOD 创建批量启用、禁用或卸载计划。只创建计划，不直接执行。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["enable", "disable", "uninstall"] },
                        "modIds": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "maxItems": 500
                        }
                    },
                    "required": ["action", "modIds"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_conflict_order_plan",
                "description": "为一个已查询的冲突组创建完整优先级计划。orderedModIds 必须包含组内全部稳定 MOD ID，数组越靠前优先级越高。只创建计划。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "groupId": { "type": "string" },
                        "orderedModIds": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 2,
                            "maxItems": 500
                        }
                    },
                    "required": ["groupId", "orderedModIds"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_model_remap_plan",
                "description": "为已禁用 MOD 的一个已查询模型分组创建改绑计划。恢复导入时目标用 restoreDefault=true；否则传精确 targetId。只创建计划。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modId": { "type": "string" },
                        "groupKey": { "type": "string" },
                        "targetId": { "type": "string" },
                        "restoreDefault": { "type": "boolean" }
                    },
                    "required": ["modId", "groupKey"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_mod_sources",
                "description": "联网搜索 MHW MOD 候选页面。返回经过 Rust 页面规则校验的 Nexus Mods、踩蘑菇、3DM、哔哩哔哩、Mod DB、GitHub 或 CurseForge 链接，并标注来源类型和访问方式；不会下载文件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "包含 MHW 游戏术语、MOD 类型和用户偏好的具体搜索条件" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_nexus_mod_files",
                "description": "读取一个已选 Nexus MHW MOD 的官方元数据和文件列表。需要先配置 Nexus Personal API Key；不会下载。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modId": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["modId"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_nexus_download_plan",
                "description": "为用户明确选择的 Nexus MOD 文件生成下载并导入 Acumod 本地库的待确认计划。仅 Nexus Premium API 直接下载可用；只生成计划。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modId": { "type": "integer", "minimum": 1 },
                        "fileId": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["modId", "fileId"],
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
        "get_mod_remap_options" => "查询模型改绑目标",
        "create_mod_action_plan" => "生成 MOD 操作计划",
        "create_conflict_order_plan" => "生成冲突优先级计划",
        "create_model_remap_plan" => "生成模型改绑计划",
        "scan_mod_cleanup_candidates" => "扫描可清理文件",
        "read_mod_cleanup_text" => "读取候选文本",
        "submit_mod_cleanup_review" => "整理清理建议",
        "get_mod_cleanup_exclusions" => "查询清理记录",
        "create_cleanup_restore_plan" => "生成清理恢复计划",
        "search_mod_sources" => "联网搜索 MOD",
        "get_nexus_mod_files" => "读取 Nexus 文件列表",
        "create_nexus_download_plan" => "生成 Nexus 下载计划",
        _ => "处理 AI 请求",
    }
}

pub(crate) async fn execute_tool(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    name: &str,
    arguments: &str,
) -> Result<ToolExecution, String> {
    match name {
        "search_local_mods" => {
            let args = parse_arguments::<SearchLocalModsArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            search_local_mods(snapshot, args).map(ToolExecution::query)
        }
        "get_mod_details" => {
            let args = parse_arguments::<GetModDetailsArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            get_mod_details(snapshot, args).map(ToolExecution::query)
        }
        "get_enabled_conflicts" => {
            let args = parse_arguments::<ConflictArgs>(arguments)?;
            let snapshot = load_snapshot(app).await?;
            get_enabled_conflicts(snapshot, args).map(ToolExecution::query)
        }
        "get_game_directory_status" => {
            parse_arguments::<EmptyArgs>(arguments)?;
            let status = game::get_game_directory_status(app)?;
            Ok(ToolExecution::query(
                json!({
                    "ok": true,
                    "isConfigured": status.is_configured,
                    "isValid": status.is_valid,
                    "message": status.message
                })
                .to_string(),
            ))
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
            Ok(ToolExecution::query(
                json!({ "ok": true, "terms": terms }).to_string(),
            ))
        }
        "get_mod_remap_options" => {
            let args = parse_arguments::<GetRemapOptionsArgs>(arguments)?;
            let worker_app = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                get_mod_remap_options(&worker_app, args)
            })
            .await
            .map_err(|error| format!("模型改绑目标查询任务失败：{error}"))??;
            Ok(ToolExecution::query(result))
        }
        "create_mod_action_plan" => {
            let args = parse_arguments::<CreateModActionPlanArgs>(arguments)?;
            let worker_app = app.clone();
            let worker_coordinator = coordinator.clone();
            let plan = tauri::async_runtime::spawn_blocking(move || {
                super::create_batch_action_plan(
                    &worker_app,
                    &worker_coordinator,
                    &args.action,
                    args.mod_ids,
                )
            })
            .await
            .map_err(|error| format!("MOD 操作计划生成任务失败：{error}"))??;
            ToolExecution::plan(plan)
        }
        "create_conflict_order_plan" => {
            let args = parse_arguments::<CreateConflictOrderPlanArgs>(arguments)?;
            let worker_app = app.clone();
            let worker_coordinator = coordinator.clone();
            let plan = tauri::async_runtime::spawn_blocking(move || {
                super::create_conflict_order_plan(
                    &worker_app,
                    &worker_coordinator,
                    args.group_id,
                    args.ordered_mod_ids,
                )
            })
            .await
            .map_err(|error| format!("冲突优先级计划生成任务失败：{error}"))??;
            ToolExecution::plan(plan)
        }
        "create_model_remap_plan" => {
            let args = parse_arguments::<CreateModelRemapPlanArgs>(arguments)?;
            if args.restore_default && args.target_id.is_some() {
                return Err("恢复默认和指定目标不能同时设置。".to_string());
            }
            if !args.restore_default && args.target_id.is_none() {
                return Err("请指定模型目标，或明确选择恢复导入时目标。".to_string());
            }
            let worker_app = app.clone();
            let worker_coordinator = coordinator.clone();
            let target_id = (!args.restore_default).then_some(args.target_id).flatten();
            let plan = tauri::async_runtime::spawn_blocking(move || {
                super::create_model_remap_plan(
                    &worker_app,
                    &worker_coordinator,
                    args.mod_id,
                    args.group_key,
                    target_id,
                )
            })
            .await
            .map_err(|error| format!("模型改绑计划生成任务失败：{error}"))??;
            ToolExecution::plan(plan)
        }
        "scan_mod_cleanup_candidates" => {
            let args = parse_arguments::<CleanupScanArgs>(arguments)?;
            let audit = if let Some(audit_id) = args.audit_id.as_deref() {
                coordinator.get_cleanup_audit(audit_id)?
            } else {
                if args.offset.unwrap_or(0) != 0 {
                    return Err("首次清理扫描必须从 offset 0 开始。".to_string());
                }
                let worker_app = app.clone();
                let audit =
                    tauri::async_runtime::spawn_blocking(move || cleanup::scan_audit(&worker_app))
                        .await
                        .map_err(|error| format!("清理候选扫描任务失败：{error}"))??;
                coordinator.store_cleanup_audit(audit.clone())?;
                audit
            };
            if audit.ai_groups.is_empty() && audit.scan.local_remove_count > 0 {
                let review = cleanup::create_review(coordinator, &audit.audit_id, Vec::new())?;
                return ToolExecution::cleanup_review(review);
            }
            let limit = args.limit.unwrap_or(100).clamp(1, 100);
            let (start, end, next_offset) =
                pagination_bounds(audit.ai_groups.len(), args.offset.unwrap_or(0), limit);
            let page = &audit.ai_groups[start..end];
            Ok(ToolExecution::query(
                json!({
                    "ok": true,
                    "auditId": audit.audit_id,
                    "ruleVersion": audit.scan.rule_version,
                    "scannedFileCount": audit.scan.scanned_file_count,
                    "localKeepCount": audit.scan.local_keep_count,
                    "localSuggestedCount": audit.scan.local_remove_count,
                    "aiFileCount": audit.scan.ai_review_count,
                    "total": audit.ai_groups.len(),
                    "offset": start,
                    "returned": page.len(),
                    "nextOffset": next_offset,
                    "groups": page,
                    "message": audit.scan.message
                })
                .to_string(),
            ))
        }
        "submit_mod_cleanup_review" => {
            let args = parse_arguments::<CleanupReviewArgs>(arguments)?;
            let review = cleanup::create_review(coordinator, &args.audit_id, args.classifications)?;
            ToolExecution::cleanup_review(review)
        }
        "read_mod_cleanup_text" => {
            let args = parse_arguments::<CleanupTextArgs>(arguments)?;
            let preview =
                cleanup::read_text_preview(app, coordinator, &args.audit_id, &args.candidate_id)?;
            Ok(ToolExecution::query(
                serde_json::to_string(&json!({
                    "ok": true,
                    "candidateId": preview.candidate_id,
                    "libraryRelativePath": preview.library_relative_path,
                    "content": preview.content,
                    "truncated": preview.truncated
                }))
                .map_err(|error| format!("无法序列化候选文本：{error}"))?,
            ))
        }
        "get_mod_cleanup_exclusions" => {
            parse_arguments::<EmptyArgs>(arguments)?;
            let worker_app = app.clone();
            let exclusions = tauri::async_runtime::spawn_blocking(move || {
                mod_library::list_mod_cleanup_exclusions(&worker_app)
            })
            .await
            .map_err(|error| format!("清理记录查询任务失败：{error}"))??;
            Ok(ToolExecution::query(
                serde_json::to_string(&json!({ "ok": true, "exclusions": exclusions }))
                    .map_err(|error| format!("无法序列化清理记录：{error}"))?,
            ))
        }
        "create_cleanup_restore_plan" => {
            let args = parse_arguments::<CleanupRestorePlanArgs>(arguments)?;
            let worker_app = app.clone();
            let worker_coordinator = coordinator.clone();
            let plan = tauri::async_runtime::spawn_blocking(move || {
                super::create_cleanup_restore_plan(
                    &worker_app,
                    &worker_coordinator,
                    &args.scope,
                    args.mod_id,
                )
            })
            .await
            .map_err(|error| format!("清理恢复计划生成任务失败：{error}"))??;
            ToolExecution::plan(plan)
        }
        "search_mod_sources" => {
            let args = parse_arguments::<SearchModSourcesArgs>(arguments)?;
            let key = super::require_deepseek_api_key()?;
            let model = config::load(app)?.deep_seek_model;
            let mut results = source_search::search(&key, model, &args.query).await?;
            let nexus_configured = nexus::credential_status()?.configured;
            let mut verified_nexus_count = 0_usize;
            let mut values = Vec::with_capacity(results.len());
            let nexus_summaries = if nexus_configured {
                join_all(results.iter().map(|result| async move {
                    match result.nexus_mod_id {
                        Some(mod_id) => nexus::get_mod_summary(mod_id).await.ok(),
                        None => None,
                    }
                }))
                .await
            } else {
                vec![None; results.len()]
            };
            for (result, summary) in results.iter_mut().zip(nexus_summaries) {
                let mut nexus_verified = false;
                let mut updated_at_unix_seconds = 0_u64;
                if let Some(summary) = summary {
                    result.title = summary.name;
                    result.author = summary.author;
                    result.summary = summary.summary;
                    result.url = summary.page_url;
                    updated_at_unix_seconds = summary.updated_at_unix_seconds;
                    nexus_verified = true;
                    verified_nexus_count += 1;
                }
                values.push(json!({
                    "title": result.title,
                    "url": result.url,
                    "source": result.source,
                    "author": result.author,
                    "summary": result.summary,
                    "nexusModId": result.nexus_mod_id,
                    "nexusVerified": nexus_verified,
                    "updatedAtUnixSeconds": updated_at_unix_seconds,
                    "sourceKind": result.source_kind,
                    "sourceKindLabel": result.source_kind_label,
                    "accessMode": result.access_mode,
                    "accessModeLabel": result.access_mode_label,
                    "accessNote": result.access_note
                }));
            }
            Ok(ToolExecution::query(
                json!({
                    "ok": true,
                    "resultCount": values.len(),
                    "nexusApiKeyConfigured": nexus_configured,
                    "verifiedNexusCount": verified_nexus_count,
                    "results": values
                })
                .to_string(),
            ))
        }
        "get_nexus_mod_files" => {
            let args = parse_arguments::<NexusModArgs>(arguments)?;
            let files = nexus::get_mod_files(args.mod_id).await?;
            Ok(ToolExecution::query(
                serde_json::to_string(&json!({ "ok": true, "nexus": files }))
                    .map_err(|error| format!("无法序列化 Nexus 文件列表：{error}"))?,
            ))
        }
        "create_nexus_download_plan" => {
            let args = parse_arguments::<NexusDownloadPlanArgs>(arguments)?;
            let target = nexus::get_download_target(args.mod_id, args.file_id).await?;
            let plan = super::create_nexus_download_plan(app, coordinator, target)?;
            ToolExecution::plan(plan)
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetRemapOptionsArgs {
    mod_id: String,
    group_key: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateModActionPlanArgs {
    action: String,
    mod_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateConflictOrderPlanArgs {
    group_id: String,
    ordered_mod_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateModelRemapPlanArgs {
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
    #[serde(default)]
    restore_default: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupScanArgs {
    audit_id: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupReviewArgs {
    audit_id: String,
    classifications: Vec<cleanup::CleanupClassification>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupTextArgs {
    audit_id: String,
    candidate_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupRestorePlanArgs {
    scope: String,
    mod_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchModSourcesArgs {
    query: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NexusModArgs {
    mod_id: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NexusDownloadPlanArgs {
    mod_id: u64,
    file_id: u64,
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

fn get_mod_remap_options(app: &AppHandle, args: GetRemapOptionsArgs) -> Result<String, String> {
    let details = mod_library::get_mod_remap_details(app, args.mod_id)?;
    let query = args.query.unwrap_or_default().trim().to_lowercase();
    let limit = args.limit.unwrap_or(30).clamp(1, 30);
    let groups = details
        .groups
        .into_iter()
        .filter(|group| {
            args.group_key
                .as_ref()
                .is_none_or(|group_key| group.group_key == *group_key)
        })
        .map(|group| {
            let matches = group
                .targets
                .iter()
                .filter(|target| {
                    query.is_empty()
                        || target.target_id.to_lowercase().contains(&query)
                        || target.model_id.to_lowercase().contains(&query)
                        || target
                            .game_ids
                            .iter()
                            .chain(target.display_names.iter())
                            .any(|value| value.to_lowercase().contains(&query))
                })
                .collect::<Vec<_>>();
            let total = matches.len();
            let targets = matches
                .into_iter()
                .take(limit)
                .map(|target| {
                    json!({
                        "targetId": target.target_id,
                        "modelId": target.model_id,
                        "gameIds": target.game_ids,
                        "displayNames": target.display_names,
                        "affectedParts": target.affected_parts
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "groupKey": group.group_key,
                "modelKind": group.model_kind,
                "subKind": group.sub_kind,
                "sourceModelIds": group.source_model_ids,
                "sourceDisplayNames": group.source_display_names,
                "selectedTargetId": group.selected_target_id,
                "originalTargetId": group.original_target_id,
                "allowsManualTarget": group.allows_manual_target,
                "targetCount": total,
                "returned": targets.len(),
                "truncated": total > targets.len(),
                "targets": targets
            })
        })
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Err("没有找到匹配的模型替换分组。".to_string());
    }
    Ok(json!({
        "ok": true,
        "modId": details.mod_id,
        "name": details.name,
        "enabled": details.enabled,
        "groups": groups,
        "warnings": details.warnings
    })
    .to_string())
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
