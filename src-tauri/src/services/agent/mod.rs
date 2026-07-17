mod deepseek;
mod tools;

use std::{
    env,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use keyring::Entry;
use serde::Serialize;
use tauri::{ipc::Channel, AppHandle};

use crate::storage::config::{self, DeepSeekModel};

const KEYRING_SERVICE: &str = "Acumen MOD Manager";
const KEYRING_USER: &str = "deepseek-api-key";
const MAX_USER_MESSAGE_CHARS: usize = 4_000;
static NEXT_TURN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct AgentCoordinator {
    inner: Arc<AgentCoordinatorInner>,
}

#[derive(Default)]
struct AgentCoordinatorInner {
    active: AtomicBool,
    history: Mutex<Vec<deepseek::DeepSeekMessage>>,
}

struct ActiveTurnGuard {
    inner: Arc<AgentCoordinatorInner>,
}

impl Drop for ActiveTurnGuard {
    fn drop(&mut self) {
        self.inner.active.store(false, Ordering::Release);
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSettings {
    pub model: DeepSeekModel,
    pub model_api_name: String,
    pub api_key_configured: bool,
    pub api_key_hint: Option<String>,
    pub api_key_source: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConnectionResult {
    pub model: DeepSeekModel,
    pub model_api_name: String,
    pub elapsed_millis: u128,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnResult {
    pub turn_id: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub turn_id: String,
    pub sequence: u64,
    pub kind: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub message: Option<String>,
}

pub(crate) struct AgentEventSender {
    turn_id: String,
    sequence: u64,
    channel: Channel<AgentEvent>,
}

impl AgentEventSender {
    fn new(turn_id: String, channel: Channel<AgentEvent>) -> Self {
        Self {
            turn_id,
            sequence: 0,
            channel,
        }
    }

    pub(crate) fn emit(
        &mut self,
        kind: &str,
        text: Option<String>,
        tool_name: Option<String>,
        message: Option<String>,
    ) {
        self.sequence += 1;
        // 关闭悬浮窗口不会取消模型请求；当前轮次仍完成并保留在内存会话中。
        let _ = self.channel.send(AgentEvent {
            turn_id: self.turn_id.clone(),
            sequence: self.sequence,
            kind: kind.to_string(),
            text,
            tool_name,
            message,
        });
    }
}

impl AgentCoordinator {
    fn begin_turn(&self) -> Result<ActiveTurnGuard, String> {
        self.inner
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "AI 助手正在回答上一条消息，请稍候。".to_string())?;
        Ok(ActiveTurnGuard {
            inner: Arc::clone(&self.inner),
        })
    }

    fn history(&self) -> Result<Vec<deepseek::DeepSeekMessage>, String> {
        self.inner
            .history
            .lock()
            .map(|history| history.clone())
            .map_err(|_| "AI 会话状态不可用，请重启 Acumod 后重试。".to_string())
    }

    fn replace_history(&self, history: Vec<deepseek::DeepSeekMessage>) -> Result<(), String> {
        *self
            .inner
            .history
            .lock()
            .map_err(|_| "AI 会话状态不可用，请重启 Acumod 后重试。".to_string())? = history;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        if self.inner.active.load(Ordering::Acquire) {
            return Err("AI 助手正在回答，完成后才能清空对话。".to_string());
        }
        self.inner
            .history
            .lock()
            .map_err(|_| "AI 会话状态不可用，请重启 Acumod 后重试。".to_string())?
            .clear();
        Ok(())
    }
}

pub fn get_agent_settings(app: &AppHandle) -> Result<AgentSettings, String> {
    let model = config::load(app)?.deep_seek_model;
    let credential = load_deepseek_api_key()?;
    Ok(AgentSettings {
        model,
        model_api_name: model.api_name().to_string(),
        api_key_configured: credential.is_some(),
        api_key_hint: credential.as_ref().map(|value| api_key_hint(&value.key)),
        api_key_source: credential.map(|value| value.source),
    })
}

pub fn save_agent_model(app: &AppHandle, model: DeepSeekModel) -> Result<AgentSettings, String> {
    let mut app_config = config::load(app)?;
    app_config.deep_seek_model = model;
    config::save(app, &app_config)?;
    get_agent_settings(app)
}

pub fn set_deepseek_api_key(app: &AppHandle, api_key: String) -> Result<AgentSettings, String> {
    let api_key = validate_api_key(&api_key)?;
    keyring_entry()?
        .set_password(&api_key)
        .map_err(|error| format!("无法把 DeepSeek 访问密钥保存到 Windows 凭据管理器：{error}"))?;
    get_agent_settings(app)
}

pub fn delete_deepseek_api_key(app: &AppHandle) -> Result<AgentSettings, String> {
    let entry = keyring_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => {
            return Err(format!(
                "无法从 Windows 凭据管理器删除 DeepSeek 访问密钥：{error}"
            ));
        }
    }
    get_agent_settings(app)
}

pub async fn test_agent_connection(app: &AppHandle) -> Result<AgentConnectionResult, String> {
    let model = config::load(app)?.deep_seek_model;
    let key = require_deepseek_api_key()?;
    deepseek::test_connection(&key, model).await
}

pub async fn start_agent_turn(
    app: AppHandle,
    coordinator: AgentCoordinator,
    message: String,
    channel: Channel<AgentEvent>,
) -> Result<AgentTurnResult, String> {
    let message = validate_user_message(message)?;
    let _guard = coordinator.begin_turn()?;
    let model = config::load(&app)?.deep_seek_model;
    let key = require_deepseek_api_key()?;
    let turn_id = format!(
        "agent-turn-{}",
        NEXT_TURN_ID.fetch_add(1, Ordering::Relaxed)
    );
    let mut sender = AgentEventSender::new(turn_id.clone(), channel);
    sender.emit(
        "started",
        None,
        None,
        Some(format!("正在连接 {}", model.display_name())),
    );

    let history = coordinator.history()?;
    match deepseek::run_turn(&app, &key, model, history, message, &mut sender).await {
        Ok((history, reply)) => {
            coordinator.replace_history(history)?;
            sender.emit("completed", None, None, Some("回答完成".to_string()));
            Ok(AgentTurnResult {
                turn_id,
                message: reply,
            })
        }
        Err(error) => {
            sender.emit("failed", None, None, Some(error.clone()));
            Err(error)
        }
    }
}

struct StoredCredential {
    key: String,
    source: String,
}

fn keyring_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| format!("无法访问 Windows 凭据管理器：{error}"))
}

fn load_deepseek_api_key() -> Result<Option<StoredCredential>, String> {
    match keyring_entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => {
            return Ok(Some(StoredCredential {
                key,
                source: "credentialManager".to_string(),
            }));
        }
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(error) => {
            return Err(format!("无法读取 Windows 凭据管理器：{error}"));
        }
    }

    Ok(env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .map(|key| StoredCredential {
            key,
            source: "environment".to_string(),
        }))
}

fn require_deepseek_api_key() -> Result<String, String> {
    load_deepseek_api_key()?
        .map(|credential| credential.key)
        .ok_or_else(|| "尚未配置 DeepSeek 访问密钥，请先在设置中保存。".to_string())
}

fn validate_api_key(api_key: &str) -> Result<String, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("DeepSeek 访问密钥不能为空。".to_string());
    }
    if api_key.len() > 512 || api_key.chars().any(char::is_whitespace) {
        return Err("DeepSeek 访问密钥格式无效。".to_string());
    }
    Ok(api_key.to_string())
}

fn validate_user_message(message: String) -> Result<String, String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("请输入要询问的内容。".to_string());
    }
    if message.chars().count() > MAX_USER_MESSAGE_CHARS {
        return Err(format!(
            "单条消息不能超过 {MAX_USER_MESSAGE_CHARS} 个字符。"
        ));
    }
    Ok(message)
}

fn api_key_hint(api_key: &str) -> String {
    let suffix = api_key.chars().rev().take(4).collect::<Vec<_>>();
    let suffix = suffix.into_iter().rev().collect::<String>();
    format!("****{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{api_key_hint, validate_api_key, validate_user_message};

    #[test]
    fn api_key_hint_only_exposes_last_four_characters() {
        assert_eq!(api_key_hint("sk-example-1234"), "****1234");
    }

    #[test]
    fn api_key_validation_rejects_whitespace() {
        assert!(validate_api_key("sk-example key").is_err());
    }

    #[test]
    fn user_message_is_trimmed() {
        assert_eq!(
            validate_user_message("  查询太刀 MOD  ".to_string()).unwrap(),
            "查询太刀 MOD"
        );
    }
}
