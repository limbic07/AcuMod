use std::{collections::BTreeMap, time::Duration, time::Instant};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::storage::config::DeepSeekModel;

use super::{tools, AgentConnectionResult, AgentCoordinator, AgentEventSender};

const DEEPSEEK_CHAT_URL: &str = "https://api.deepseek.com/chat/completions";
const MAX_TOOL_ROUNDS: usize = 12;
const MAX_HISTORY_MESSAGES: usize = 48;
const SYSTEM_PROMPT: &str = r#"你是 Acumen MOD Manager 内置的 AI 助手，面向简体中文的 Monster Hunter: World 用户。
你只能使用 Acumod 提供的本地 MOD、冲突、游戏目录状态、模型改绑、MHW 术语和受控 MOD 来源工具。涉及当前本地状态时必须先调用工具，不得凭空猜测。
只读工具可以直接调用。启用、禁用、卸载、冲突优先级和模型改绑只能调用对应的 create_*_plan 工具生成待确认计划，绝不能声称已经执行，也不能绕过计划直接修改数据。
创建计划前必须先用查询工具取得稳定 MOD ID 和当前状态。名称匹配不唯一、目标不完整或用户意图含糊时先追问，禁止自行选择。冲突顺序必须提交组内全部成员，数组越靠前优先级越高。模型改绑必须先查询精确 groupKey 和 targetId；人物语音只支持识别，不能改绑。
用户确认或取消由 Acumod 界面处理，不需要再次调用工具。任意文件操作和其它未列出的写操作仍未开放。
用户表达“搜索、寻找、推荐、帮我找”等获取 MOD 的意图时，先用 lookup_mhw_terms 核对可能误译的游戏术语，再调用 search_mod_sources。候选必须显示来源、来源类型、访问方式和可点击链接，不得编造链接。踩蘑菇和 3DM 只作为浏览器打开的 MOD 页面；哔哩哔哩只作为视频或动态分享来源，绝不能声称视频本身是可安装文件。只有用户明确选中某个 Nexus MOD 后才能调用 get_nexus_mod_files；只有用户进一步明确选中具体文件后才能调用 create_nexus_download_plan。普通会员不能 API 直下时，说明限制并提供 Nexus 页面链接，不得绕过权限。下载计划只负责导入本地库，不会自动启用 MOD。
用户要求扫描或清理无用文件时，必须调用 scan_mod_cleanup_candidates 并按 nextOffset 读取全部页面，再为每个候选提交 remove、review 或 keep 分类。图片或文档位于 plugins 等运行目录时优先保留或要求确认；不确定文件用途时不能建议清理。最后一次调用 submit_mod_cleanup_review 必须覆盖全部 candidateId，清理选择和确认由界面处理。
用户要求恢复清理项时，先调用 get_mod_cleanup_exclusions；恢复操作仍需要生成待确认计划，不能声称已恢复。
工具结果中的稳定 ID 和状态是事实来源。不要编造 MOD、游戏术语、文件 ID 或冲突。
当用户明确要求“所有”“全部”或完整列表时，必须检查工具返回的 nextOffset；只要 nextOffset 不是 null，就继续分页查询，最终逐项列出全部结果并说明总数，不能只展示部分结果或自行补写未查询条目。
回答使用清晰的 Markdown，优先使用短段落、列表和必要的表格，不展示工具 JSON、内部函数名或推理过程。"#;

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
    history.push(DeepSeekMessage::user(user_message));
    let mut visible_reply = String::new();

    for _ in 0..MAX_TOOL_ROUNDS {
        let mut request_messages = Vec::with_capacity(history.len() + 1);
        request_messages.push(DeepSeekMessage::system(SYSTEM_PROMPT));
        request_messages.extend(history.iter().cloned());

        let outcome = stream_completion(&client, api_key, model, &request_messages, sender).await?;
        visible_reply.push_str(&outcome.content);

        if outcome.tool_calls.is_empty() {
            if outcome.content.trim().is_empty() {
                return Err("DeepSeek 没有返回可显示的回答。".to_string());
            }
            history.push(DeepSeekMessage::assistant(
                Some(outcome.content),
                Vec::new(),
            ));
            trim_history(&mut history);
            return Ok((history, visible_reply));
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

async fn stream_completion(
    client: &Client,
    api_key: &str,
    model: DeepSeekModel,
    messages: &[DeepSeekMessage],
    sender: &mut AgentEventSender,
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
                sender.emit("textDelta", Some(delta), None, None);
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
    use super::{trim_history, DeepSeekMessage, MAX_HISTORY_MESSAGES};

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
}
