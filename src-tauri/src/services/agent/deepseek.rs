use std::{collections::BTreeMap, time::Duration, time::Instant};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::storage::config::DeepSeekModel;

use super::{
    tools, AgentConnectionResult, AgentCoordinator, AgentEventSender, AgentKnowledgeClaim,
    AgentKnowledgeEvidence,
};

const DEEPSEEK_CHAT_URL: &str = "https://api.deepseek.com/chat/completions";
const MAX_TOOL_ROUNDS: usize = 24;
const MAX_HISTORY_MESSAGES: usize = 48;
const MAX_KNOWLEDGE_ANSWER_REPAIR_ATTEMPTS: usize = 1;
const MAX_KNOWLEDGE_TOOL_REQUIREMENT_ATTEMPTS: usize = 1;
const KNOWLEDGE_EVIDENCE_MARKER_PREFIX: &str = "[[evidence:";
const KNOWLEDGE_CLAIM_MARKER_PREFIX: &str = "[[claim:";
const KNOWLEDGE_SAFE_FAILURE_REPLY: &str = "当前已安装的知识包没有返回可核验资料，因此我不会根据记忆补充这条结论。请安装对应知识包，或补充更明确的游戏版本、任务、装备或 MOD 信息后重试。";
const TASK_UNLOCK_PROMPT: &str = "任务解锁问题必须先定位任务实体，再读取 requiresQuest 与 requiresCondition。requiresQuest 仅表示已唯一核验的前置任务；requiresCondition 包含等级、捕获、发现、NPC 和活动开放等来源条件。当前覆盖本体与冰原已分配、可选任务中已核验的部分，以及少量人工逐项核对的特别任务；活动、斗技场/挑战和交货任务仍可能缺失。无结果时说明当前包没有已核验资料，绝不能根据任务编号、地图或剧情印象推断。";
const SYSTEM_PROMPT: &str = r#"你是 Acumen MOD Manager 内置助手 AcuAI，面向简体中文的 Monster Hunter: World 用户。
你只能使用 Acumod 提供的本地 MOD、冲突、游戏目录状态、模型改绑、MHW 术语和受控 MOD 来源工具。涉及当前本地状态时必须先调用工具，不得凭空猜测。
只读工具可以直接调用。启用、禁用、卸载、冲突优先级和模型改绑只能调用对应的 create_*_plan 工具生成待确认计划，绝不能声称已经执行，也不能绕过计划直接修改数据。
创建计划前必须先用查询工具取得稳定 MOD ID 和当前状态。名称匹配不唯一、目标不完整或用户意图含糊时先追问，禁止自行选择。冲突顺序必须提交组内全部成员，数组越靠前优先级越高。模型改绑必须先查询精确 groupKey 和 targetId；人物语音只支持识别，不能改绑。
用户确认或取消由 Acumod 界面处理，不需要再次调用工具。任意文件操作和其它未列出的写操作仍未开放。
用户表达“搜索、寻找、推荐、帮我找”等获取 MOD 的意图时，先用 lookup_mhw_terms 核对可能误译的游戏术语，再调用 search_mod_sources。候选必须显示来源、来源类型、访问方式和可点击链接，不得编造链接。所有来源（包括 Nexus Mods、踩蘑菇、3DM、GitHub 等）都只通过系统浏览器打开；用户在原页面自行下载后，再使用 Acumod 的本地文件导入。哔哩哔哩只作为视频或动态分享来源，绝不能声称视频本身是可安装文件，也不能绕过站点登录、会员或下载权限。
用户要求扫描或清理无用文件时，必须调用 scan_mod_cleanup_candidates。Rust 已盘点全部可部署文件并完成本地确定性分流，工具只返回证据冲突或不足的模糊文件组；不要要求查看本地规则已确定保留的标准游戏资源。必须复用首次返回的 auditId 按 nextOffset 读取全部页面，再为每个 groupId 提交 remove、review 或 keep 分类；同组文件共享目录、扩展名和规则证据。存在任何保留证据、位于 plugins 等运行目录或用途不确定时优先选择 review 或 keep，不能仅按扩展名建议清理。只有路径和规则证据不足以判断安全纯文本时，才可用 read_mod_cleanup_text 读取一个代表文件，不能为同组每个文件重复读取。最后一次调用 submit_mod_cleanup_review 必须携带 auditId 并覆盖全部 groupId，清理选择和确认由界面处理。若扫描工具已经直接生成审查结果，则不要重复提交；若 total 和 localSuggestedCount 都为 0，则直接说明没有候选，不要提交空审查。
用户要求恢复清理项时，先调用 get_mod_cleanup_exclusions；恢复操作仍需要生成待确认计划，不能声称已恢复。
用户询问 MHW 游戏事实、机制、任务前置、素材路线、配装或战斗建议，以及 MOD 文件格式、路径、依赖和工作原理时，必须查询知识库。MOD 技术与攻略正文使用 search_knowledge；优先传入 2 至 12 个字的关键术语或术语组合，而不是整段用户问句。服务端会在长问句精确查询失败时安全退化为术语检索，但这不能替代主动提炼目标。涉及精确装备、素材、怪物、任务、技能、数值或名称消歧时，先调用 lookup_game_entities，随后按需要用 get_game_entity_relations 查询制作、升级、掉落、任务、技能和解锁关系。比较两个到四个已消歧实体时，必须调用 compare_game_entities，不得把模糊名称或不同类别实体自行配对。任务、地点或报酬问题先定位任务实体，再读取 hasQuestFacts、occursAt 和 rewardsItem；任务资料中的目标、星级和类别可作为补充证据。怪物弱点、肉质、可捕获性或掉落问题先定位怪物实体，再读取 hasMonsterFacts；生态资料中的 weaknesses、hitzones、traps 和 rewards 是补充证据。素材获取问题先定位素材实体，再查入向 dropsItem、rewardsItem 和 gathersItem；装备属性、技能或制作问题先定位装备实体，再查 hasWeaponFacts、hasArmorFacts、hasDecorationFacts、grantsSkill 和 requiresMaterial。当前事实包尚未覆盖完整解锁链，因此查询不到明确前置关系时只能说明资料缺口，不能根据任务编号、地图或剧情印象推断。开放推荐同时检索 mhw-game-facts 与 mhw-game-guides，并把可核验事实与条件性建议分开；凡是在推荐中具体点名的装备、技能、素材、怪物、任务或数值，都必须在本轮通过实体或关系工具再次核验并附结构化 claim，不能只凭攻略摘要列出。回答必须标明适用游戏版本并引用工具返回的来源；若实体或关系的 gameVersion 为 unverified，必须明确它只是在开发快照中交叉核对，不能当作 15.23 最终事实。知识包无结果、版本不符或来源不足时明确说明缺口，不能用模型记忆或普通联网搜索补写精确事实。
用户询问 Acumod 的导入、启用、禁用、冲突、排序、分支组、模型改绑、知识包或 AcuAI 使用方式时，必须优先查询 acumod-help；帮助包缺失时明确说明缺少 Acumod 使用说明，不要根据界面印象编造当前行为。只有用户明确要求执行启用、禁用、卸载、排序或改绑时，才按受控操作流程生成计划。
用户询问本地某个已安装 MOD 的文件结构、每个文件作用、资源依赖或整体工作方式时，必须先用 search_local_mods 取得唯一稳定 ID，再调用 analyze_installed_mod。工具已经区分二进制解析证据、路径规则和未知项；回答不得提高其可信度，也不得把同目录关联说成已解析的内部引用。用户要求完整逐文件分析时必须按 nextOffset 读取全部页；只询问整体原理时优先使用组件、依赖和知识证据摘要，避免无意义地复述全部路径。
工具结果中的稳定 ID 和状态是事实来源。不要编造 MOD、游戏术语、文件 ID 或冲突。
游戏事实、攻略或 MOD 技术问题在没有本轮知识或本地分析证据时，AcuAI 会拒绝展示回答；此时必须调用合适工具或明确说明知识包缺失。
当用户明确要求“所有”“全部”或完整列表时，必须检查工具返回的 nextOffset；只要 nextOffset 不是 null，就继续分页查询，最终逐项列出全部结果并说明总数，不能只展示部分结果或自行补写未查询条目。
本轮调用过知识工具后，每个包含具体游戏事实、技术判断或攻略建议的段落末尾必须附上至少一个工具结果中的内部证据标记，格式为 `[[evidence:<完整 evidenceId>]]`。标记会在展示前由 Acumod 移除，不能编造、不能引用本轮未返回的 ID。实体、关系或本地 MOD 分析工具还会返回可核验字段；回答中使用具体数字、名称、关系或文件统计时，至少附一个字段标记，格式为 `[[claim:<完整 evidenceId>|/JSON/Pointer|JSON值]]`，例如 `[[claim:mhw-game-facts:dev:game-weapon-fact:mhwdata:2001|/data/attack|624]]`。字段标记中的值必须原样出现在正文，Acumod 会校验后移除。若资料不足，应明确说明缺口并仍为该判断附上相关来源标记。
回答使用清晰的 Markdown，优先使用短段落、列表和必要的表格，不展示工具 JSON、内部函数名、准备调用工具的说明或推理过程。需要调用工具时，直接调用且 content 留空。"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DeepSeekMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<DeepSeekToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl DeepSeekMessage {
    fn system(content: impl Into<String>) -> Self {
        Self::plain("system", content)
    }

    fn user(content: impl Into<String>) -> Self {
        Self::plain("user", content)
    }

    fn assistant(content: Option<String>, tool_calls: Vec<DeepSeekToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls,
            tool_call_id: None,
        }
    }

    fn tool(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id),
        }
    }

    fn plain(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeepSeekToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: DeepSeekFunctionCall,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeepSeekFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Default, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<StreamToolCallDelta>,
}

#[derive(Deserialize)]
struct StreamToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<StreamFunctionDelta>,
}

#[derive(Deserialize)]
struct StreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

struct StreamOutcome {
    content: String,
    tool_calls: Vec<DeepSeekToolCall>,
}

#[derive(Deserialize)]
struct CompletionResponse {
    #[serde(default)]
    choices: Vec<CompletionChoice>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

pub(crate) async fn test_connection(
    api_key: &str,
    model: DeepSeekModel,
) -> Result<AgentConnectionResult, String> {
    let client = build_client()?;
    let started_at = Instant::now();
    let response = client
        .post(DEEPSEEK_CHAT_URL)
        .bearer_auth(api_key)
        .json(&json!({
            "model": model.api_name(),
            "messages": [
                { "role": "system", "content": "你是连接测试助手。" },
                { "role": "user", "content": "只回复：连接正常" }
            ],
            "thinking": { "type": "disabled" },
            "stream": false,
            "max_tokens": 16
        }))
        .send()
        .await
        .map_err(map_request_error)?;
    let response = ensure_success(response).await?;
    let body = response
        .json::<CompletionResponse>()
        .await
        .map_err(|error| format!("无法解析 DeepSeek 连接测试结果：{error}"))?;
    let has_content = body
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .is_some_and(|content| !content.trim().is_empty());
    if !has_content {
        return Err("DeepSeek 已响应，但没有返回可用文本。".to_string());
    }

    Ok(AgentConnectionResult {
        model,
        model_api_name: model.api_name().to_string(),
        elapsed_millis: started_at.elapsed().as_millis(),
        message: "DeepSeek V4 连接正常。".to_string(),
    })
}

pub(crate) async fn run_turn(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    api_key: &str,
    model: DeepSeekModel,
    mut history: Vec<DeepSeekMessage>,
    user_message: String,
    sender: &mut AgentEventSender,
) -> Result<(Vec<DeepSeekMessage>, String), String> {
    let client = build_client()?;
    let knowledge_evidence_required = requires_knowledge_evidence(&user_message);
    history.push(DeepSeekMessage::user(user_message));
    let mut visible_reply = String::new();
    let mut knowledge_evidence = Vec::<AgentKnowledgeEvidence>::new();
    let mut knowledge_claims = Vec::<AgentKnowledgeClaim>::new();
    let mut knowledge_query_performed = false;
    let mut knowledge_answer_repair_attempts = 0;
    let mut knowledge_tool_requirement_attempts = 0;

    for _ in 0..MAX_TOOL_ROUNDS {
        let mut request_messages = Vec::with_capacity(history.len() + 1);
        request_messages.push(DeepSeekMessage::system(format!(
            "{TASK_UNLOCK_PROMPT}\n{SYSTEM_PROMPT}"
        )));
        request_messages.extend(history.iter().cloned());

        // 知识工具返回后，最终正文必须先通过本轮证据引用校验；否则不能在流中提前展示。
        let stream_text = knowledge_evidence.is_empty() && !knowledge_evidence_required;
        let outcome = stream_completion(
            &client,
            api_key,
            model,
            &request_messages,
            sender,
            stream_text,
        )
        .await?;

        if outcome.tool_calls.is_empty() {
            if outcome.content.trim().is_empty() {
                return Err("DeepSeek 没有返回可显示的回答。".to_string());
            }
            if knowledge_evidence_required && !knowledge_query_performed {
                if knowledge_tool_requirement_attempts < MAX_KNOWLEDGE_TOOL_REQUIREMENT_ATTEMPTS {
                    knowledge_tool_requirement_attempts += 1;
                    history.push(DeepSeekMessage::assistant(
                        Some(outcome.content),
                        Vec::new(),
                    ));
                    history.push(DeepSeekMessage::user(
                        "系统要求：上一条属于游戏事实、攻略或 MOD 技术问题。请先调用合适的知识查询或本地 MOD 分析工具，再基于返回证据重写；知识包没有资料时，明确说明缺口。",
                    ));
                    continue;
                }
                return Err("该问题需要查询本地知识包或已安装 MOD 分析结果，但 AcuAI 未能取得可核验证据。请确认已安装对应知识包后重试。".to_string());
            }
            // 校验集与界面来源列表使用同一批证据，避免回答引用用户无法追溯的截断结果。
            let final_evidence = deduplicate_knowledge_evidence(knowledge_evidence.clone());
            let final_claims = deduplicate_knowledge_claims(&knowledge_claims, &final_evidence);
            if knowledge_evidence_required && final_evidence.is_empty() {
                // 查询成功但没有命中证据时，不能把模型的自由回答当作知识事实。
                // 由 Rust 生成确定性的缺口说明，保证知识包缺失或资料不足时不猜测。
                sender.emit(
                    "textDelta",
                    Some(KNOWLEDGE_SAFE_FAILURE_REPLY.to_string()),
                    None,
                    None,
                );
                visible_reply.push_str(KNOWLEDGE_SAFE_FAILURE_REPLY);
                history.push(DeepSeekMessage::assistant(
                    Some(KNOWLEDGE_SAFE_FAILURE_REPLY.to_string()),
                    Vec::new(),
                ));
                sender.emit_knowledge_evidence(Vec::new());
                trim_history(&mut history);
                return Ok((history, visible_reply));
            }
            let final_content = if final_evidence.is_empty() {
                outcome.content
            } else {
                match validate_and_strip_knowledge_markers(
                    &outcome.content,
                    &final_evidence,
                    &final_claims,
                ) {
                    Ok(content) => content,
                    Err(error)
                        if knowledge_answer_repair_attempts
                            < MAX_KNOWLEDGE_ANSWER_REPAIR_ATTEMPTS =>
                    {
                        knowledge_answer_repair_attempts += 1;
                        history.push(DeepSeekMessage::assistant(
                            Some(outcome.content),
                            Vec::new(),
                        ));
                        history.push(DeepSeekMessage::user(format!(
                            "系统校验未通过：{error}。请只重写上一条知识回答；保留原有 Markdown 内容，并在每个事实或建议段落末尾附上本轮工具结果中的 [[evidence:<完整 evidenceId>]] 标记。若使用了实体、关系或本地 MOD 分析的具体字段，也必须附 [[claim:<完整 evidenceId>|/JSON/Pointer|JSON值]] 标记。"
                        )));
                        continue;
                    }
                    Err(error) => {
                        return Err(format!("知识回答引用校验失败：{error}"));
                    }
                }
            };
            if !stream_text {
                sender.emit("textDelta", Some(final_content.clone()), None, None);
            }
            visible_reply.push_str(&final_content);
            history.push(DeepSeekMessage::assistant(Some(final_content), Vec::new()));
            sender.emit_knowledge_evidence(final_evidence);
            trim_history(&mut history);
            return Ok((history, visible_reply));
        }

        // 工具轮次的 content 可能是模型误发的准备说明，绝不能混入最终用户回复。
        if !outcome.content.trim().is_empty() {
            sender.emit("textReset", None, None, None);
        }
        history.push(DeepSeekMessage::assistant(
            (!outcome.content.is_empty()).then_some(outcome.content),
            outcome.tool_calls.clone(),
        ));
        for call in outcome.tool_calls {
            let label = tools::tool_label(&call.function.name);
            sender.emit(
                "toolStarted",
                None,
                Some(call.function.name.clone()),
                Some(format!("正在{label}")),
            );
            let tool_result = match tools::execute_tool(
                app,
                coordinator,
                &call.function.name,
                &call.function.arguments,
            )
            .await
            {
                Ok(result) => {
                    sender.emit(
                        "toolFinished",
                        None,
                        Some(call.function.name.clone()),
                        Some(format!("已完成{label}")),
                    );
                    if let Some(plan) = result.plan {
                        sender.emit_plan(plan);
                    }
                    if let Some(review) = result.cleanup_review {
                        sender.emit_cleanup_review(review);
                    }
                    knowledge_query_performed |= result.knowledge_query_performed;
                    knowledge_evidence.extend(result.knowledge_evidence);
                    knowledge_claims.extend(result.knowledge_claims);
                    result.content
                }
                Err(error) => {
                    sender.emit(
                        "toolFinished",
                        None,
                        Some(call.function.name.clone()),
                        Some(format!("{label}失败，正在整理原因")),
                    );
                    json!({ "ok": false, "error": error }).to_string()
                }
            };
            history.push(DeepSeekMessage::tool(call.id, tool_result));
        }
    }

    Err("AI 工具调用轮次过多，请缩小问题范围后重试。".to_string())
}

/// 只拦截需要知识证据的事实、攻略、MOD 技术或 Acumod 使用说明问句，避免影响搜索与传统管理操作。
fn requires_knowledge_evidence(user_message: &str) -> bool {
    let message = user_message.trim().to_lowercase();
    if message.is_empty() {
        return false;
    }
    if ["搜索", "寻找", "帮我找", "下载", "链接", "nexus"]
        .iter()
        .any(|term| message.contains(term))
    {
        return false;
    }
    let asks_how_to = ["如何", "怎么", "怎样"]
        .iter()
        .any(|term| message.contains(term));
    let has_acumod_term = ["acumod", "acuai", "mod管理器", "冲突管理", "知识包"]
        .iter()
        .any(|term| message.contains(term));
    let asks_acumod_help = has_acumod_term
        && [
            "什么",
            "作用",
            "如何",
            "怎么",
            "怎样",
            "为什么",
            "区别",
            "规则",
            "流程",
            "哪里",
        ]
        .iter()
        .any(|term| message.contains(term));
    if [
        "启用", "禁用", "卸载", "删除", "排序", "改绑", "恢复", "导入", "打开",
    ]
    .iter()
    .any(|term| message.contains(term))
        && !asks_how_to
        && !asks_acumod_help
    {
        return false;
    }

    // 多轮追问经常只保留“那……呢”“这个呢”等指代词；仍需重新查询，
    // 避免模型仅凭上一轮记忆继续扩展事实。搜索请求和实际操作已在上方豁免。
    let is_contextual_follow_up = message.len() <= 32
        && (["那", "这个", "它", "该", "上述", "前面", "刚才"]
            .iter()
            .any(|term| message.starts_with(term))
            || message.ends_with('呢'));
    if is_contextual_follow_up {
        return true;
    }

    let asks_for_explanation = [
        "什么",
        "为何",
        "为什么",
        "如何",
        "怎么",
        "作用",
        "区别",
        "关系",
        "依赖",
        "前置",
        "解锁",
        "弱点",
        "掉落",
        "配装",
        "推荐",
        "建议",
        "路线",
        "适合",
        "怎么过",
        "怎么打",
        "打法",
        "准备",
        "需要什么",
        "有没有",
        "想要",
        "装备",
        "哪里",
        "获得",
        "攻击力",
        "数值",
        "属性",
        "素材",
    ]
    .iter()
    .any(|term| message.contains(term));
    if !asks_for_explanation {
        return false;
    }

    let has_domain_term = [
        "mhw",
        "冰原",
        "聚魔",
        "怪物",
        "龙",
        "兽",
        "任务",
        "素材",
        "矿石",
        "骨",
        "爪",
        "鳞",
        "玉",
        "宝珠",
        "武器",
        "大剑",
        "太刀",
        "片手剑",
        "双剑",
        "大锤",
        "狩猎笛",
        "长枪",
        "铳枪",
        "斩斧",
        "盾斧",
        "操虫棍",
        "轻弩",
        "重弩",
        "弓",
        "防具",
        "装备",
        "黑龙",
        "飞翔爪",
        "技能",
        "装饰珠",
        "护石",
        "配装",
        "猎虫",
        "随从",
        "猫饭",
        "地图",
        "营地",
        "调和",
        "mod3",
        "mrl3",
        "tex",
        "dds",
        "evam",
        "evwp",
        "epv",
        "efx",
        "timl",
        "ctc",
        "ccl",
        "sobj",
        "sobjl",
        "lmt",
        "gmd",
        "nativepc",
        "插件",
        "飞翔爪",
        "文件格式",
        "acumod",
        "acuai",
        "mod管理器",
        "冲突管理",
        "知识包",
        "mod库",
        "分支组",
        "批量操作",
        "游戏目录检测",
    ]
    .iter()
    .any(|term| message.contains(term));
    has_domain_term || (message.contains("mod") && message.contains("文件"))
}

fn deduplicate_knowledge_evidence(
    evidence: Vec<AgentKnowledgeEvidence>,
) -> Vec<AgentKnowledgeEvidence> {
    let mut seen = std::collections::HashSet::new();
    evidence
        .into_iter()
        .filter(|item| seen.insert(item.evidence_id.clone()))
        .take(20)
        .collect()
}

fn deduplicate_knowledge_claims(
    claims: &[AgentKnowledgeClaim],
    evidence: &[AgentKnowledgeEvidence],
) -> Vec<AgentKnowledgeClaim> {
    let allowed = evidence
        .iter()
        .map(|item| item.evidence_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::new();
    claims
        .iter()
        .filter(|item| allowed.contains(item.evidence_id.as_str()))
        .filter(|item| seen.insert(item.evidence_id.clone()))
        .cloned()
        .take(20)
        .collect()
}

fn validate_and_strip_knowledge_markers(
    content: &str,
    evidence: &[AgentKnowledgeEvidence],
    claims: &[AgentKnowledgeClaim],
) -> Result<String, String> {
    let content = validate_and_strip_knowledge_evidence_markers(content, evidence)?;
    validate_and_strip_knowledge_claim_markers(&content, claims)
}

fn validate_and_strip_knowledge_evidence_markers(
    content: &str,
    evidence: &[AgentKnowledgeEvidence],
) -> Result<String, String> {
    let allowed = evidence
        .iter()
        .map(|item| item.evidence_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut output = String::with_capacity(content.len());
    let mut remaining = content;
    let mut marker_count = 0;

    while let Some(start) = remaining.find(KNOWLEDGE_EVIDENCE_MARKER_PREFIX) {
        output.push_str(&remaining[..start]);
        let marker = &remaining[start + KNOWLEDGE_EVIDENCE_MARKER_PREFIX.len()..];
        let Some(end) = marker.find("]]") else {
            return Err("存在未闭合的内部证据标记".to_string());
        };
        let evidence_id = marker[..end].trim();
        if evidence_id.is_empty() {
            return Err("存在空的内部证据标记".to_string());
        }
        if !allowed.contains(evidence_id) {
            return Err(format!("引用了本轮未返回的证据：{evidence_id}"));
        }
        marker_count += 1;
        remaining = &marker[end + 2..];
    }
    output.push_str(remaining);

    if marker_count == 0 {
        return Err("没有引用本轮知识工具返回的证据".to_string());
    }
    let output = output.trim().to_string();
    if output.is_empty() {
        return Err("移除内部证据标记后没有可展示的正文".to_string());
    }
    Ok(output)
}

fn validate_and_strip_knowledge_claim_markers(
    content: &str,
    claims: &[AgentKnowledgeClaim],
) -> Result<String, String> {
    if claims.is_empty() {
        return Ok(content.to_string());
    }
    let allowed = claims
        .iter()
        .map(|item| (item.evidence_id.as_str(), &item.data))
        .collect::<std::collections::HashMap<_, _>>();
    let mut output = String::with_capacity(content.len());
    let mut remaining = content;
    let mut validated_values = Vec::new();
    let mut marker_count = 0;

    while let Some(start) = remaining.find(KNOWLEDGE_CLAIM_MARKER_PREFIX) {
        output.push_str(&remaining[..start]);
        let marker = &remaining[start + KNOWLEDGE_CLAIM_MARKER_PREFIX.len()..];
        let Some(end) = marker.find("]]") else {
            return Err("存在未闭合的内部字段标记".to_string());
        };
        let parts = marker[..end].splitn(3, '|').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err("内部字段标记必须包含证据、JSON 指针和值".to_string());
        }
        let evidence_id = parts[0].trim();
        let pointer = parts[1].trim();
        if evidence_id.is_empty() || !pointer.starts_with('/') {
            return Err("内部字段标记的证据或 JSON 指针无效".to_string());
        }
        let Some(data) = allowed.get(evidence_id) else {
            return Err(format!("字段标记引用了本轮不可核验的证据：{evidence_id}"));
        };
        let expected = serde_json::from_str::<Value>(parts[2].trim())
            .map_err(|_| "内部字段标记的 JSON 值无效".to_string())?;
        let Some(actual) = data.pointer(pointer) else {
            return Err(format!("字段标记引用了不存在的字段：{pointer}"));
        };
        if actual != &expected {
            return Err(format!("字段标记的值与本轮证据不一致：{pointer}"));
        }
        let display = claim_display_value(actual)?;
        validated_values.push(display);
        marker_count += 1;
        remaining = &marker[end + 2..];
    }
    output.push_str(remaining);

    if marker_count == 0 {
        return Err("没有核验实体、关系或本地分析中的具体字段".to_string());
    }
    let output = output.trim().to_string();
    for value in validated_values {
        if !output.contains(&value) {
            return Err(format!("已核验字段值未出现在展示正文中：{value}"));
        }
    }
    Ok(output)
}

fn claim_display_value(value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) if !value.is_empty() => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err("内部字段标记暂只支持非空字符串、数字或布尔值".to_string()),
    }
}

async fn stream_completion(
    client: &Client,
    api_key: &str,
    model: DeepSeekModel,
    messages: &[DeepSeekMessage],
    sender: &mut AgentEventSender,
    stream_text: bool,
) -> Result<StreamOutcome, String> {
    let response = client
        .post(DEEPSEEK_CHAT_URL)
        .bearer_auth(api_key)
        .json(&json!({
            "model": model.api_name(),
            "messages": messages,
            "tools": tools::tool_definitions(),
            "tool_choice": "auto",
            "thinking": { "type": "disabled" },
            "stream": true
        }))
        .send()
        .await
        .map_err(map_request_error)?;
    let response = ensure_success(response).await?;
    let mut stream = response.bytes_stream().eventsource();
    let mut content = String::new();
    let mut tool_calls = BTreeMap::<usize, ToolCallAccumulator>::new();

    while let Some(event) = stream.next().await {
        let event = event.map_err(|error| format!("DeepSeek 流式响应中断：{error}"))?;
        if event.data.trim() == "[DONE]" {
            break;
        }
        let chunk = serde_json::from_str::<StreamChunk>(&event.data)
            .map_err(|error| format!("无法解析 DeepSeek 流式响应：{error}"))?;
        for choice in chunk.choices {
            if let Some(delta) = choice.delta.content.filter(|value| !value.is_empty()) {
                content.push_str(&delta);
                if stream_text {
                    sender.emit("textDelta", Some(delta), None, None);
                }
            }
            for delta in choice.delta.tool_calls {
                let accumulated = tool_calls.entry(delta.index).or_default();
                if let Some(id) = delta.id {
                    accumulated.id.push_str(&id);
                }
                if let Some(function) = delta.function {
                    if let Some(name) = function.name {
                        accumulated.name.push_str(&name);
                    }
                    if let Some(arguments) = function.arguments {
                        accumulated.arguments.push_str(&arguments);
                    }
                }
            }
        }
    }

    let tool_calls = tool_calls
        .into_values()
        .map(|call| {
            if call.id.is_empty() || call.name.is_empty() {
                return Err("DeepSeek 返回了不完整的工具调用。".to_string());
            }
            Ok(DeepSeekToolCall {
                id: call.id,
                call_type: "function".to_string(),
                function: DeepSeekFunctionCall {
                    name: call.name,
                    arguments: if call.arguments.trim().is_empty() {
                        "{}".to_string()
                    } else {
                        call.arguments
                    },
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(StreamOutcome {
        content,
        tool_calls,
    })
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .user_agent(concat!("Acumen-MOD-Manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("无法初始化 DeepSeek 网络客户端：{error}"))
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body).ok().and_then(|value| {
        value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(sanitize_error_detail)
    });
    let summary = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "DeepSeek 访问密钥无效或没有访问权限。",
        StatusCode::PAYMENT_REQUIRED => "DeepSeek 账户余额不足。",
        StatusCode::TOO_MANY_REQUESTS => "DeepSeek 请求过于频繁，请稍后重试。",
        status if status.is_server_error() => "DeepSeek 服务暂时不可用，请稍后重试。",
        _ => "DeepSeek 请求失败。",
    };
    Err(match detail {
        Some(detail) if !detail.is_empty() => format!("{summary} {detail}"),
        _ => summary.to_string(),
    })
}

fn map_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "连接 DeepSeek 超时，请检查网络后重试。".to_string()
    } else if error.is_connect() {
        "无法连接 DeepSeek，请检查网络设置。".to_string()
    } else {
        format!("DeepSeek 网络请求失败：{error}")
    }
}

fn sanitize_error_detail(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect::<String>()
}

fn trim_history(history: &mut Vec<DeepSeekMessage>) {
    while history.len() > MAX_HISTORY_MESSAGES {
        history.remove(0);
    }
    // 不能让裁剪后的历史以孤立的 assistant/tool 消息开头。
    while history
        .first()
        .is_some_and(|message| message.role != "user")
    {
        history.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        requires_knowledge_evidence, trim_history, validate_and_strip_knowledge_markers,
        AgentKnowledgeClaim, AgentKnowledgeEvidence, DeepSeekMessage, MAX_HISTORY_MESSAGES,
    };

    fn knowledge_evidence(id: &str) -> AgentKnowledgeEvidence {
        AgentKnowledgeEvidence {
            evidence_id: id.to_string(),
            title: "测试知识".to_string(),
            game_version: "15.23".to_string(),
            confidence: 1.0,
            source_title: Some("测试来源".to_string()),
            source_url: None,
            pack_id: "mhw-game-facts".to_string(),
            pack_version: "dev".to_string(),
        }
    }

    #[test]
    fn knowledge_answer_requires_and_hides_current_turn_evidence_markers() {
        let evidence = vec![knowledge_evidence("mhw-game-facts:dev:game-quest:51612")];
        let claims = vec![AgentKnowledgeClaim {
            evidence_id: "mhw-game-facts:dev:game-quest:51612".to_string(),
            data: serde_json::json!({ "data": { "attack": 624 } }),
        }];
        let content = "攻击力是 624。[[evidence:mhw-game-facts:dev:game-quest:51612]][[claim:mhw-game-facts:dev:game-quest:51612|/data/attack|624]]";
        assert_eq!(
            validate_and_strip_knowledge_markers(content, &evidence, &claims).unwrap(),
            "攻击力是 624。"
        );

        assert!(validate_and_strip_knowledge_markers("没有引用", &evidence, &claims).is_err());
        assert!(validate_and_strip_knowledge_markers(
            "伪造来源。[[evidence:mhw-game-facts:dev:game-quest:00000]][[claim:mhw-game-facts:dev:game-quest:51612|/data/attack|624]]",
            &evidence,
            &claims,
        )
        .is_err());
        assert!(validate_and_strip_knowledge_markers(
            "错误数值 625。[[evidence:mhw-game-facts:dev:game-quest:51612]][[claim:mhw-game-facts:dev:game-quest:51612|/data/attack|625]]",
            &evidence,
            &claims,
        )
        .is_err());
        assert!(validate_and_strip_knowledge_markers(
            "没有展示数值。[[evidence:mhw-game-facts:dev:game-quest:51612]][[claim:mhw-game-facts:dev:game-quest:51612|/data/attack|624]]",
            &evidence,
            &claims,
        )
        .is_err());
        assert_eq!(
            validate_and_strip_knowledge_markers(
                "攻略建议。[[evidence:mhw-game-facts:dev:game-quest:51612]]",
                &evidence,
                &[],
            )
            .unwrap(),
            "攻略建议。"
        );
    }

    #[test]
    fn history_trimming_keeps_a_user_message_at_the_boundary() {
        let mut history = vec![DeepSeekMessage::plain("assistant", "orphan")];
        for index in 0..MAX_HISTORY_MESSAGES {
            history.push(DeepSeekMessage::user(format!("user-{index}")));
        }

        trim_history(&mut history);

        assert!(history.len() <= MAX_HISTORY_MESSAGES);
        assert_eq!(
            history.first().map(|message| message.role.as_str()),
            Some("user")
        );
    }

    #[test]
    fn knowledge_questions_require_tool_evidence_but_operations_do_not() {
        assert!(requires_knowledge_evidence(
            "冰鱼龙弱什么属性，应该打哪里？"
        ));
        assert!(requires_knowledge_evidence("冰原中期大剑怎么配装？"));
        assert!(requires_knowledge_evidence(
            "我刚到冰原中期，想要一套能打黑龙的装备。"
        ));
        assert!(requires_knowledge_evidence("黑龙怎么打？"));
        assert!(requires_knowledge_evidence("那技能呢？"));
        assert!(requires_knowledge_evidence("铁矿石从哪里获得？"));
        assert!(requires_knowledge_evidence(
            "MOD 里的 EVAM 和 EPV 文件有什么作用？"
        ));
        assert!(requires_knowledge_evidence("紧急任务狩猎毒妖鸟怎么解锁？"));
        assert!(requires_knowledge_evidence("Acumod如何安装知识包？"));
        assert!(requires_knowledge_evidence("冲突管理的优先级规则是什么？"));
        assert!(!requires_knowledge_evidence("帮我找一个太刀外观 MOD"));
        assert!(!requires_knowledge_evidence("启用太刀分类的所有 MOD"));
        assert!(!requires_knowledge_evidence("那就启用它"));
        assert!(!requires_knowledge_evidence("你好"));
    }

    #[test]
    fn empty_knowledge_results_use_a_non_speculative_failure_message() {
        assert!(super::KNOWLEDGE_SAFE_FAILURE_REPLY.contains("不会根据记忆补充"));
        assert!(super::KNOWLEDGE_SAFE_FAILURE_REPLY.contains("安装对应知识包"));
    }
}
