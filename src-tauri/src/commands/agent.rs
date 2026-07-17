use tauri::{ipc::Channel, State};

use crate::{
    services::agent::{
        self, AgentConnectionResult, AgentCoordinator, AgentEvent, AgentSettings, AgentTurnResult,
    },
    storage::config::DeepSeekModel,
};

/// 读取 DeepSeek V4 模型和脱敏凭据状态。
#[tauri::command]
pub fn get_agent_settings(app: tauri::AppHandle) -> Result<AgentSettings, String> {
    agent::get_agent_settings(&app)
}

/// 保存非敏感的 DeepSeek V4 模型选择。
#[tauri::command]
pub fn save_agent_model(
    app: tauri::AppHandle,
    model: DeepSeekModel,
) -> Result<AgentSettings, String> {
    agent::save_agent_model(&app, model)
}

/// 将 DeepSeek 访问密钥保存到系统凭据库，普通配置不会保存明文。
#[tauri::command]
pub fn set_deepseek_api_key(
    app: tauri::AppHandle,
    api_key: String,
) -> Result<AgentSettings, String> {
    agent::set_deepseek_api_key(&app, api_key)
}

/// 删除 Acumod 写入系统凭据库的 DeepSeek 访问密钥。
#[tauri::command]
pub fn delete_deepseek_api_key(app: tauri::AppHandle) -> Result<AgentSettings, String> {
    agent::delete_deepseek_api_key(&app)
}

/// 使用当前 Key 和模型发送最小请求，验证 DeepSeek 服务是否可用。
#[tauri::command]
pub async fn test_agent_connection(app: tauri::AppHandle) -> Result<AgentConnectionResult, String> {
    agent::test_agent_connection(&app).await
}

/// 开始一次只读 Agent 对话，并通过有序 Channel 发送文本和工具状态。
#[tauri::command]
pub async fn start_agent_turn(
    app: tauri::AppHandle,
    coordinator: State<'_, AgentCoordinator>,
    message: String,
    on_event: Channel<AgentEvent>,
) -> Result<AgentTurnResult, String> {
    agent::start_agent_turn(app, coordinator.inner().clone(), message, on_event).await
}

/// 清空当前运行期间的 AI 会话，不修改任何 MOD 状态。
#[tauri::command]
pub fn clear_agent_session(coordinator: State<'_, AgentCoordinator>) -> Result<(), String> {
    coordinator.clear()
}
