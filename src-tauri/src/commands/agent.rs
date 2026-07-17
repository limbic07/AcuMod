use tauri::{ipc::Channel, State};

use crate::{
    services::agent::{
        self, AgentActionPlan, AgentActionResult, AgentConnectionResult, AgentCoordinator,
        AgentEvent, AgentSettings, AgentTurnResult,
    },
    services::nexus::NexusConnectionResult,
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

/// 将 Nexus Personal API Key 保存到系统凭据库。
#[tauri::command]
pub fn set_nexus_api_key(app: tauri::AppHandle, api_key: String) -> Result<AgentSettings, String> {
    agent::set_nexus_api_key(&app, api_key)
}

/// 删除 Acumod 写入系统凭据库的 Nexus Personal API Key。
#[tauri::command]
pub fn delete_nexus_api_key(app: tauri::AppHandle) -> Result<AgentSettings, String> {
    agent::delete_nexus_api_key(&app)
}

/// 验证 Nexus API Key、账户名称和 API 直接下载权限。
#[tauri::command]
pub async fn test_nexus_connection() -> Result<NexusConnectionResult, String> {
    agent::test_nexus_connection().await
}

/// 开始一次 Agent 对话；写操作只能生成等待确认的受控计划。
#[tauri::command]
pub async fn start_agent_turn(
    app: tauri::AppHandle,
    coordinator: State<'_, AgentCoordinator>,
    message: String,
    on_event: Channel<AgentEvent>,
) -> Result<AgentTurnResult, String> {
    agent::start_agent_turn(app, coordinator.inner().clone(), message, on_event).await
}

/// 重新校验当前状态并执行一份尚未过期的 AI 操作计划。
#[tauri::command]
pub async fn confirm_agent_action_plan(
    app: tauri::AppHandle,
    coordinator: State<'_, AgentCoordinator>,
    plan_id: String,
) -> Result<AgentActionResult, String> {
    agent::confirm_agent_action_plan(app, coordinator.inner().clone(), plan_id).await
}

/// 取消并销毁一份 AI 操作计划，不修改 MOD 状态。
#[tauri::command]
pub fn cancel_agent_action_plan(
    coordinator: State<'_, AgentCoordinator>,
    plan_id: String,
) -> Result<AgentActionResult, String> {
    agent::cancel_agent_action_plan(coordinator.inner(), plan_id)
}

/// 根据用户在清理建议中的逐项选择生成待确认计划，不执行文件操作。
#[tauri::command]
pub async fn create_agent_cleanup_plan(
    app: tauri::AppHandle,
    coordinator: State<'_, AgentCoordinator>,
    review_id: String,
    candidate_ids: Vec<String>,
) -> Result<AgentActionPlan, String> {
    let worker_app = app.clone();
    let worker_coordinator = coordinator.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        agent::create_agent_cleanup_plan(&worker_app, &worker_coordinator, review_id, candidate_ids)
    })
    .await
    .map_err(|error| format!("清理计划生成任务失败：{error}"))?
}

/// 清空当前运行期间的 AI 会话，不修改任何 MOD 状态。
#[tauri::command]
pub fn clear_agent_session(coordinator: State<'_, AgentCoordinator>) -> Result<(), String> {
    coordinator.clear()
}
