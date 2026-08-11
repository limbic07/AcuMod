use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    operations::{run_blocking_operation, OperationReporter},
    services::{game, knowledge, mhwdata, mod_analysis, mod_library, model_recognition},
    storage::config,
};

use super::{
    cleanup, source_search, AgentActionPlan, AgentCoordinator, AgentKnowledgeClaim,
    AgentKnowledgeEvidence,
};

const DEFAULT_RESULT_LIMIT: usize = 100;
const MAX_RESULT_LIMIT: usize = 100;
const MAX_CLAIM_HINTS_PER_EVIDENCE: usize = 16;
const MAX_CLAIM_HINTS_PER_TOOL_RESULT: usize = 60;

pub(crate) struct ToolExecution {
    pub content: String,
    pub plan: Option<AgentActionPlan>,
    pub cleanup_review: Option<cleanup::AgentCleanupReview>,
    /// 标记本轮已实际查询知识包或执行本地 MOD 分析，即使查询结果为空。
    pub knowledge_query_performed: bool,
    pub knowledge_evidence: Vec<AgentKnowledgeEvidence>,
    pub knowledge_claims: Vec<AgentKnowledgeClaim>,
}

impl ToolExecution {
    fn query(content: String) -> Self {
        Self {
            content,
            plan: None,
            cleanup_review: None,
            knowledge_query_performed: false,
            knowledge_evidence: Vec::new(),
            knowledge_claims: Vec::new(),
        }
    }

    fn knowledge_query(
        content: String,
        knowledge_evidence: Vec<AgentKnowledgeEvidence>,
        knowledge_claims: Vec<AgentKnowledgeClaim>,
    ) -> Self {
        Self {
            content: with_claim_hints(content, &knowledge_claims),
            plan: None,
            cleanup_review: None,
            knowledge_query_performed: true,
            knowledge_evidence,
            knowledge_claims,
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
            knowledge_query_performed: false,
            knowledge_evidence: Vec::new(),
            knowledge_claims: Vec::new(),
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
            knowledge_query_performed: false,
            knowledge_evidence: Vec::new(),
            knowledge_claims: Vec::new(),
        })
    }
}

/// 将可核验的标量字段显式交给模型，避免它猜测相对于证据快照的 JSON 指针。
fn with_claim_hints(content: String, claims: &[AgentKnowledgeClaim]) -> String {
    if claims.is_empty() {
        return content;
    }
    let Ok(mut value) = serde_json::from_str::<Value>(&content) else {
        return content;
    };
    let Some(object) = value.as_object_mut() else {
        return content;
    };
    let mut total = 0;
    let hints = claims
        .iter()
        .map(|claim| {
            let mut fields = Vec::new();
            collect_claim_hint_fields(
                &claim.data,
                "",
                &mut fields,
                &mut total,
                MAX_CLAIM_HINTS_PER_EVIDENCE,
            );
            json!({ "evidenceId": claim.evidence_id, "fields": fields })
        })
        .collect::<Vec<_>>();
    object.insert("claimHints".to_string(), Value::Array(hints));
    serde_json::to_string(&value).unwrap_or(content)
}

fn collect_claim_hint_fields(
    value: &Value,
    pointer: &str,
    fields: &mut Vec<Value>,
    total: &mut usize,
    per_evidence_limit: usize,
) {
    if *total >= MAX_CLAIM_HINTS_PER_TOOL_RESULT || fields.len() >= per_evidence_limit {
        return;
    }
    match value {
        Value::String(value) if !value.is_empty() => {
            fields.push(json!({ "pointer": pointer, "value": value }));
            *total += 1;
        }
        Value::Number(value) => {
            fields.push(json!({ "pointer": pointer, "value": value }));
            *total += 1;
        }
        Value::Bool(value) => {
            fields.push(json!({ "pointer": pointer, "value": value }));
            *total += 1;
        }
        Value::Array(values) => {
            for (index, item) in values.iter().enumerate() {
                collect_claim_hint_fields(
                    item,
                    &format!("{pointer}/{index}"),
                    fields,
                    total,
                    per_evidence_limit,
                );
                if *total >= MAX_CLAIM_HINTS_PER_TOOL_RESULT || fields.len() >= per_evidence_limit {
                    break;
                }
            }
        }
        Value::Object(values) => {
            for (key, item) in values {
                let key = key.replace('~', "~0").replace('/', "~1");
                collect_claim_hint_fields(
                    item,
                    &format!("{pointer}/{key}"),
                    fields,
                    total,
                    per_evidence_limit,
                );
                if *total >= MAX_CLAIM_HINTS_PER_TOOL_RESULT || fields.len() >= per_evidence_limit {
                    break;
                }
            }
        }
        Value::Null | Value::String(_) => {}
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
                "name": "analyze_installed_mod",
                "description": "分析一个已安装 MOD 的全部文件作用、资源组件和依赖关系。必须先用 search_local_mods 取得稳定 MOD ID；支持按路径、作用、组件或替换目标过滤并分页。报告由本地路径规则、二进制解析器和已安装 MOD 技术知识共同生成。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "modId": { "type": "string", "description": "search_local_mods 返回的稳定 MOD ID" },
                        "query": { "type": "string", "description": "可选：文件路径、扩展名、作用、组件或替换目标" },
                        "offset": { "type": "integer", "minimum": 0 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    },
                    "required": ["modId"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_knowledge",
                "description": "查询已安装的 MOD 制作技术、游戏攻略和 Acumod 使用说明文本包。精确游戏数值、掉率、肉质、装备属性和任务报酬必须改用 lookup_game_entities 与 get_game_entity_relations；本工具不查询 MHWData 数值数据库。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "使用精确 MHW 术语描述要查的事实、关系或技术问题" },
                        "domains": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["mhw-modding", "mhw-game-guides", "acumod-help"] },
                            "description": "可选文本知识领域；不传时查询全部活动文本包"
                        },
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
                "name": "lookup_game_entities",
                "description": "在固定版 MHWorldData 本地数据库中按简体、繁体、英文名、别名或稳定 ID 查询游戏实体。返回上游 CSV 的基础行；精确数值、装备、素材、怪物、任务和技能问题必须先用此工具消歧。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "实体名称、资源 ID 或常用别名" },
                        "kinds": {
                            "type": "array",
                            "items": { "type": "string" },
                            "maxItems": 16,
                            "description": "可选实体类型，例如 weapon、armor、item、monster、quest、skill"
                        },
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
                "name": "compare_game_entities",
                "description": "按已查询到的稳定 MHWData 实体 ID 批量读取 2 至 4 个基础 CSV 行，用于比较武器、防具、护石、怪物或素材。必须先调用 lookup_game_entities 消歧；不会自行推断哪个更适合玩家。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "entityIds": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 2,
                            "maxItems": 4
                        }
                    },
                    "required": ["entityIds"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_game_entity_relations",
                "description": "读取精确实体关联的 MHWorldData 原始 CSV 行。predicates 是固定 section：武器用 weapon.sharpness、weapon.crafting；防具用 armor.skills、armor.crafting；怪物用 monster.weaknesses、monster.hitzones、monster.rewards；任务用 quest.monsters、quest.rewards；技能用 skill.levels；素材可先查 item 再读取关联的 crafting、rewards 或 location.items 行。没有返回的字段不得推断。必须使用 lookup_game_entities 返回的稳定 entityId。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "entityId": { "type": "string", "description": "lookup_game_entities 返回的稳定实体 ID" },
                        "predicates": {
                            "type": "array",
                            "items": { "type": "string" },
                            "maxItems": 24,
                            "description": "可选 MHWData section 过滤；不确定时省略"
                        },
                        "direction": {
                            "type": "string",
                            "enum": ["outgoing", "incoming", "both"],
                            "description": "为兼容调用保留；MHWData 始终返回与该实体关联的原始行"
                        },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                    },
                    "required": ["entityId"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_armor_set_crafting",
                "description": "读取一个已消歧防具套装的完整五部位制作配方。仅接受 lookup_game_entities 返回的 armorSet 稳定 ID；返回每件的 MHWData armor.crafting 原始行和同库中文材料名称桥。用户问“整套需要多少材料”时优先使用。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "armorSetId": { "type": "string", "description": "lookup_game_entities 返回的 armorSet 实体 ID" }
                    },
                    "required": ["armorSetId"],
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
        "analyze_installed_mod" => "分析 MOD 文件",
        "search_knowledge" => "查询 MHW 知识库",
        "lookup_game_entities" => "查询游戏实体",
        "get_game_entity_relations" => "查询游戏实体关系",
        "get_armor_set_crafting" => "查询整套防具制作配方",
        "search_mod_sources" => "联网搜索 MOD",
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
                let audit = run_blocking_operation(
                    app.clone(),
                    "agentCleanupScan",
                    "正在扫描可清理 MOD 文件",
                    move |progress| cleanup::scan_audit(&worker_app, &progress),
                )
                .await?;
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
        "analyze_installed_mod" => {
            let args = parse_arguments::<AnalyzeInstalledModArgs>(arguments)?;
            let worker_app = app.clone();
            let report = tauri::async_runtime::spawn_blocking(move || {
                mod_analysis::analyze_mod(&worker_app, &args.mod_id, &OperationReporter::default())
                    .map(|report| (report, args))
            })
            .await
            .map_err(|error| format!("MOD 文件分析任务失败：{error}"))??;
            let evidence = mod_analysis_evidence(&report.0);
            let claims = mod_analysis_claims(&report.0, &evidence);
            mod_analysis_tool_result(report.0, report.1, &evidence)
                .map(|content| ToolExecution::knowledge_query(content, evidence, claims))
        }
        "search_knowledge" => {
            let args = parse_arguments::<SearchKnowledgeArgs>(arguments)?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                knowledge::search(
                    &args.query,
                    args.domains.as_deref(),
                    args.limit.unwrap_or(20),
                )
            })
            .await
            .map_err(|error| format!("知识库查询任务失败：{error}"))??;
            let evidence = search_evidence(&result);
            Ok(ToolExecution::knowledge_query(
                serde_json::to_string(&json!({ "ok": true, "knowledge": result }))
                    .map_err(|error| format!("无法序列化知识库结果：{error}"))?,
                evidence,
                Vec::new(),
            ))
        }
        "lookup_game_entities" => {
            let args = parse_arguments::<LookupGameEntitiesArgs>(arguments)?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                let root = knowledge::knowledge_root()?;
                mhwdata::lookup_game_entities(
                    &root,
                    &args.query,
                    args.kinds.as_deref(),
                    args.limit.unwrap_or(20),
                )
            })
            .await
            .map_err(|error| format!("游戏实体查询任务失败：{error}"))??;
            let evidence = entity_evidence(&result);
            let claims = entity_claims(&result);
            Ok(ToolExecution::knowledge_query(
                serde_json::to_string(&json!({ "ok": true, "entities": result }))
                    .map_err(|error| format!("无法序列化游戏实体结果：{error}"))?,
                evidence,
                claims,
            ))
        }
        "compare_game_entities" => {
            let args = parse_arguments::<CompareGameEntitiesArgs>(arguments)?;
            let entity_ids = normalized_comparison_entity_ids(args.entity_ids)?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                let mut entities = Vec::with_capacity(entity_ids.len());
                let mut warnings = Vec::new();
                for entity_id in entity_ids {
                    let root = knowledge::knowledge_root()?;
                    let response = mhwdata::lookup_game_entities(&root, &entity_id, None, 4)?;
                    warnings.extend(response.warnings);
                    let entity = response
                        .matches
                        .into_iter()
                        .find(|item| item.entity_id == entity_id)
                        .ok_or_else(|| format!("未找到用于比较的精确游戏实体：{entity_id}"))?;
                    entities.push(entity);
                }
                Ok::<_, String>((entities, warnings))
            })
            .await
            .map_err(|error| format!("游戏实体比较任务失败：{error}"))??;
            let evidence = entity_evidence_matches(&result.0);
            let claims = entity_claims_matches(&result.0);
            Ok(ToolExecution::knowledge_query(
                serde_json::to_string(&json!({
                    "ok": true,
                    "entities": result.0,
                    "warnings": result.1,
                }))
                .map_err(|error| format!("无法序列化游戏实体比较结果：{error}"))?,
                evidence,
                claims,
            ))
        }
        "get_game_entity_relations" => {
            let args = parse_arguments::<GetGameEntityRelationsArgs>(arguments)?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                let root = knowledge::knowledge_root()?;
                mhwdata::get_game_entity_relations(
                    &root,
                    &args.entity_id,
                    args.predicates.as_deref(),
                    args.direction.as_deref().unwrap_or("both"),
                    args.limit.unwrap_or(30),
                )
            })
            .await
            .map_err(|error| format!("游戏实体关系查询任务失败：{error}"))??;
            let evidence = relation_evidence(&result);
            let claims = relation_claims(&result);
            Ok(ToolExecution::knowledge_query(
                serde_json::to_string(&json!({ "ok": true, "relations": result }))
                    .map_err(|error| format!("无法序列化游戏关系结果：{error}"))?,
                evidence,
                claims,
            ))
        }
        "get_armor_set_crafting" => {
            let args = parse_arguments::<GetArmorSetCraftingArgs>(arguments)?;
            let result = tauri::async_runtime::spawn_blocking(move || {
                let root = knowledge::knowledge_root()?;
                mhwdata::get_armor_set_crafting(&root, &args.armor_set_id)
            })
            .await
            .map_err(|error| format!("防具套装制作查询任务失败：{error}"))??;
            let evidence = relation_evidence(&result);
            let claims = relation_claims(&result);
            Ok(ToolExecution::knowledge_query(
                serde_json::to_string(&json!({ "ok": true, "armorSetCrafting": result }))
                    .map_err(|error| format!("无法序列化防具套装制作结果：{error}"))?,
                evidence,
                claims,
            ))
        }
        "search_mod_sources" => {
            let args = parse_arguments::<SearchModSourcesArgs>(arguments)?;
            let key = super::require_deepseek_api_key()?;
            let model = config::load(app)?.deep_seek_model;
            let results = source_search::search(&key, model, &args.query).await?;
            let mut values = Vec::with_capacity(results.len());
            for result in results {
                values.push(json!({
                    "title": result.title,
                    "url": result.url,
                    "source": result.source,
                    "author": result.author,
                    "summary": result.summary,
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
                    "results": values
                })
                .to_string(),
            ))
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
struct SearchKnowledgeArgs {
    query: String,
    domains: Option<Vec<String>>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LookupGameEntitiesArgs {
    query: String,
    kinds: Option<Vec<String>>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompareGameEntitiesArgs {
    entity_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetGameEntityRelationsArgs {
    entity_id: String,
    predicates: Option<Vec<String>>,
    direction: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetArmorSetCraftingArgs {
    armor_set_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnalyzeInstalledModArgs {
    mod_id: String,
    query: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T, String> {
    serde_json::from_str(arguments).map_err(|error| format!("AI 工具参数格式无效：{error}"))
}

fn normalized_comparison_entity_ids(entity_ids: Vec<String>) -> Result<Vec<String>, String> {
    if !(2..=4).contains(&entity_ids.len()) {
        return Err("游戏实体比较需要 2 到 4 个稳定实体 ID。".to_string());
    }
    let mut result = Vec::with_capacity(entity_ids.len());
    let mut seen = HashSet::new();
    for entity_id in entity_ids {
        let entity_id = entity_id.trim();
        if entity_id.is_empty() || entity_id.chars().count() > 240 {
            return Err("游戏实体比较包含无效的实体 ID。".to_string());
        }
        if !seen.insert(entity_id.to_string()) {
            return Err("游戏实体比较不能包含重复实体 ID。".to_string());
        }
        result.push(entity_id.to_string());
    }
    Ok(result)
}

fn search_evidence(result: &knowledge::KnowledgeSearchResponse) -> Vec<AgentKnowledgeEvidence> {
    result
        .matches
        .iter()
        .map(|item| AgentKnowledgeEvidence {
            evidence_id: format!("{}:{}:{}", item.pack_id, item.pack_version, item.result_id),
            title: item.title.clone(),
            game_version: item.game_version.clone(),
            confidence: item.confidence,
            source_title: item.source_title.clone(),
            source_url: item.source_url.clone(),
            pack_id: item.pack_id.clone(),
            pack_version: item.pack_version.clone(),
        })
        .collect()
}

fn entity_evidence(
    result: &knowledge::KnowledgeEntityLookupResponse,
) -> Vec<AgentKnowledgeEvidence> {
    entity_evidence_matches(&result.matches)
}

fn entity_evidence_matches(
    matches: &[knowledge::KnowledgeEntityMatch],
) -> Vec<AgentKnowledgeEvidence> {
    matches
        .iter()
        .map(|item| AgentKnowledgeEvidence {
            evidence_id: format!("{}:{}:{}", item.pack_id, item.pack_version, item.entity_id),
            title: item
                .name_zh_hans
                .clone()
                .unwrap_or_else(|| item.canonical_name.clone()),
            game_version: item.game_version.clone(),
            confidence: item.confidence,
            source_title: item.source_title.clone(),
            source_url: item.source_url.clone(),
            pack_id: item.pack_id.clone(),
            pack_version: item.pack_version.clone(),
        })
        .collect()
}

fn entity_claims(result: &knowledge::KnowledgeEntityLookupResponse) -> Vec<AgentKnowledgeClaim> {
    entity_claims_matches(&result.matches)
}

fn entity_claims_matches(matches: &[knowledge::KnowledgeEntityMatch]) -> Vec<AgentKnowledgeClaim> {
    matches
        .iter()
        .map(|item| AgentKnowledgeClaim {
            evidence_id: format!("{}:{}:{}", item.pack_id, item.pack_version, item.entity_id),
            data: json!({
                "entityId": item.entity_id,
                "canonicalName": item.canonical_name,
                "nameZhHans": item.name_zh_hans,
                "nameZhHant": item.name_zh_hant,
                "data": item.data,
            }),
        })
        .collect()
}

fn relation_evidence(result: &knowledge::KnowledgeRelationResponse) -> Vec<AgentKnowledgeEvidence> {
    result
        .relations
        .iter()
        .map(|item| AgentKnowledgeEvidence {
            evidence_id: format!(
                "{}:{}:{}",
                item.pack_id, item.pack_version, item.relation_id
            ),
            title: format!(
                "{} - {} - {}",
                item.subject_name, item.predicate, item.object_name
            ),
            game_version: item.game_version.clone(),
            confidence: item.confidence,
            source_title: item.source_title.clone(),
            source_url: item.source_url.clone(),
            pack_id: item.pack_id.clone(),
            pack_version: item.pack_version.clone(),
        })
        .collect()
}

fn relation_claims(result: &knowledge::KnowledgeRelationResponse) -> Vec<AgentKnowledgeClaim> {
    result
        .relations
        .iter()
        .map(|item| AgentKnowledgeClaim {
            evidence_id: format!(
                "{}:{}:{}",
                item.pack_id, item.pack_version, item.relation_id
            ),
            data: json!({
                "subjectId": item.subject_id,
                "subjectName": item.subject_name,
                "predicate": item.predicate,
                "objectId": item.object_id,
                "objectName": item.object_name,
                "data": item.data,
            }),
        })
        .collect()
}

fn local_mod_analysis_evidence_id(report: &mod_analysis::ModAnalysisReport) -> String {
    format!(
        "acumod-local-analysis:{}:{}:{}",
        report.analyzer_version, report.mod_id, report.content_sha256
    )
}

fn mod_analysis_evidence(report: &mod_analysis::ModAnalysisReport) -> Vec<AgentKnowledgeEvidence> {
    let mut evidence = vec![AgentKnowledgeEvidence {
        // 本地分析报告是文件、组件和依赖结论的第一手证据；它不依赖知识包是否存在。
        evidence_id: local_mod_analysis_evidence_id(report),
        title: format!("{} 的本地文件分析", report.mod_name),
        game_version: "本地只读分析".to_string(),
        confidence: 1.0,
        source_title: Some("Acumod 本地 MOD 分析器".to_string()),
        source_url: None,
        pack_id: "acumod-local-analysis".to_string(),
        pack_version: report.analyzer_version.to_string(),
    }];
    evidence.extend(
        report
            .knowledge_evidence
            .iter()
            .map(|item| AgentKnowledgeEvidence {
                evidence_id: format!("{}:{}:{}", item.pack_id, item.pack_version, item.result_id),
                title: item.title.clone(),
                game_version: item.game_version.clone(),
                confidence: item.confidence,
                source_title: item.source_title.clone(),
                source_url: item.source_url.clone(),
                pack_id: item.pack_id.clone(),
                pack_version: item.pack_version.clone(),
            }),
    );
    evidence
}

fn mod_analysis_claims(
    report: &mod_analysis::ModAnalysisReport,
    evidence: &[AgentKnowledgeEvidence],
) -> Vec<AgentKnowledgeClaim> {
    let Some(local_evidence) = evidence
        .iter()
        .find(|item| item.pack_id == "acumod-local-analysis")
    else {
        return Vec::new();
    };
    vec![AgentKnowledgeClaim {
        evidence_id: local_evidence.evidence_id.clone(),
        data: json!({
            "modId": report.mod_id,
            "modName": report.mod_name,
            "fileCount": report.file_count,
            "recognizedFileCount": report.recognized_file_count,
            "unknownFileCount": report.unknown_file_count,
            "componentCount": report.component_count,
            "components": report.components,
            "dependencies": report.edges,
            "warnings": report.warnings,
        }),
    }]
}

fn mod_analysis_tool_result(
    report: mod_analysis::ModAnalysisReport,
    args: AnalyzeInstalledModArgs,
    evidence: &[AgentKnowledgeEvidence],
) -> Result<String, String> {
    let query = args.query.unwrap_or_default().trim().to_lowercase();
    let matching_files = report
        .files
        .iter()
        .filter(|file| {
            query.is_empty()
                || file
                    .effective_deploy_relative_path
                    .to_lowercase()
                    .contains(&query)
                || file.role.to_lowercase().contains(&query)
                || file.role_label.to_lowercase().contains(&query)
                || file.component_label.to_lowercase().contains(&query)
                || file
                    .replacement_targets
                    .iter()
                    .any(|target| target.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    let total = matching_files.len();
    let limit = args.limit.unwrap_or(50).clamp(1, 100);
    let (start, end, next_offset) = pagination_bounds(total, args.offset.unwrap_or(0), limit);
    let page = &matching_files[start..end];
    let page_file_ids = page
        .iter()
        .map(|file| file.file_id.as_str())
        .collect::<HashSet<_>>();
    let files = page
        .iter()
        .map(|file| {
            json!({
                "fileId": file.file_id,
                "deployPath": file.effective_deploy_relative_path,
                "originalDeployPath": (file.source_deploy_relative_path != file.effective_deploy_relative_path).then_some(&file.source_deploy_relative_path),
                "role": file.role_label,
                "component": file.component_label,
                "replacementTargets": file.replacement_targets,
                "references": file.references,
                "confidence": file.confidence,
                "excludedFromDeployment": file.excluded_from_deployment,
                "evidence": file.evidence
            })
        })
        .collect::<Vec<_>>();
    let edges = report
        .edges
        .iter()
        .filter(|edge| {
            page_file_ids.contains(edge.from_file_id.as_str())
                || edge
                    .to_file_id
                    .as_deref()
                    .is_some_and(|target| page_file_ids.contains(target))
        })
        .take(300)
        .map(|edge| {
            json!({
                "fromFileId": edge.from_file_id,
                "toFileId": edge.to_file_id,
                "targetReference": edge.target_reference,
                "relation": edge.relation_label,
                "evidence": edge.evidence,
                "confidence": edge.confidence
            })
        })
        .collect::<Vec<_>>();
    let components = report
        .components
        .iter()
        .take(100)
        .map(|component| {
            json!({
                "componentId": component.component_id,
                "kind": component.kind,
                "label": component.label,
                "fileCount": component.file_count,
                "roles": component.roles,
                "replacementTargets": component.replacement_targets,
                "confidence": component.confidence
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "ok": true,
        "modId": report.mod_id,
        "modName": report.mod_name,
        "analyzerVersion": report.analyzer_version,
        "cacheHit": report.cache_hit,
        "summary": {
            "fileCount": report.file_count,
            "recognizedFileCount": report.recognized_file_count,
            "unknownFileCount": report.unknown_file_count,
            "componentCount": report.component_count,
            "dependencyCount": report.edges.len(),
            "message": report.message
        },
        "filter": query,
        "total": total,
        "offset": start,
        "returned": files.len(),
        "nextOffset": next_offset,
        "components": components,
        "files": files,
        "dependencies": edges,
        // 与 ToolExecution 的证据集合一致，使模型能够引用本地文件分析和技术知识来源。
        "knowledgeEvidence": evidence,
        "warnings": report.warnings
    }))
    .map_err(|error| format!("无法序列化 MOD 文件分析结果：{error}"))
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
    use super::{
        mod_analysis_evidence, mod_analysis_tool_result, normalized_comparison_entity_ids,
        normalized_limit, pagination_bounds, tool_definitions, with_claim_hints,
        AgentKnowledgeClaim, AnalyzeInstalledModArgs, DEFAULT_RESULT_LIMIT, MAX_RESULT_LIMIT,
    };
    use crate::services::mod_analysis::{ModAnalysisReport, ModKnowledgeEvidence};

    fn empty_mod_report() -> ModAnalysisReport {
        ModAnalysisReport {
            schema_version: 1,
            analyzer_version: 2,
            mod_id: "mod-1".to_string(),
            mod_name: "测试 MOD".to_string(),
            inventory_sha256: "inventory".to_string(),
            content_sha256: "content".to_string(),
            knowledge_signature: "knowledge".to_string(),
            file_count: 0,
            total_size_bytes: 0,
            recognized_file_count: 0,
            unknown_file_count: 0,
            component_count: 0,
            files: Vec::new(),
            components: Vec::new(),
            edges: Vec::new(),
            knowledge_evidence: Vec::new(),
            warnings: Vec::new(),
            cache_hit: false,
            message: "测试".to_string(),
        }
    }

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

    #[test]
    fn local_mod_analysis_is_always_an_agent_evidence_source() {
        let mut report = empty_mod_report();
        report.knowledge_evidence.push(ModKnowledgeEvidence {
            result_id: "modding-mod3".to_string(),
            title: "MOD3 模型资源".to_string(),
            snippet: "测试".to_string(),
            game_version: "15.23".to_string(),
            confidence: 0.98,
            source_title: Some("测试资料".to_string()),
            source_url: None,
            pack_id: "mhw-modding".to_string(),
            pack_version: "dev".to_string(),
        });
        let evidence = mod_analysis_evidence(&report);
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence[0].evidence_id,
            "acumod-local-analysis:2:mod-1:content"
        );
        assert_eq!(
            evidence[0].source_title.as_deref(),
            Some("Acumod 本地 MOD 分析器")
        );
        assert_eq!(evidence[1].evidence_id, "mhw-modding:dev:modding-mod3");

        let content = mod_analysis_tool_result(
            report,
            AnalyzeInstalledModArgs {
                mod_id: "mod-1".to_string(),
                query: None,
                offset: None,
                limit: None,
            },
            &evidence,
        )
        .unwrap();
        let tool_result = serde_json::from_str::<serde_json::Value>(&content).unwrap();
        assert_eq!(
            tool_result["knowledgeEvidence"][0]["evidenceId"],
            "acumod-local-analysis:2:mod-1:content"
        );
    }

    #[test]
    fn structured_claim_hints_expose_bounded_json_pointers() {
        let content = with_claim_hints(
            r#"{"ok":true}"#.to_string(),
            &[AgentKnowledgeClaim {
                evidence_id: "test:entity".to_string(),
                data: serde_json::json!({
                    "nameZhHans": "测试大剑",
                    "data": { "attack": 624, "isCraftable": true }
                }),
            }],
        );
        let value = serde_json::from_str::<serde_json::Value>(&content).unwrap();
        let fields = value["claimHints"][0]["fields"].as_array().unwrap();
        assert!(fields
            .iter()
            .any(|field| { field["pointer"] == "/data/attack" && field["value"] == 624 }));
        assert!(fields
            .iter()
            .any(|field| { field["pointer"] == "/data/isCraftable" && field["value"] == true }));
    }

    #[test]
    fn comparison_requires_unique_stable_entity_ids() {
        assert_eq!(
            normalized_comparison_entity_ids(vec![
                "game-weapon:0:136".to_string(),
                "game-weapon:0:137".to_string(),
            ])
            .unwrap(),
            ["game-weapon:0:136", "game-weapon:0:137"]
        );
        assert!(normalized_comparison_entity_ids(vec!["one".to_string()]).is_err());
        assert!(
            normalized_comparison_entity_ids(vec!["one".to_string(), "one".to_string()]).is_err()
        );
        assert!(tool_definitions()
            .iter()
            .any(|item| item["function"]["name"] == "compare_game_entities"));
    }
}
