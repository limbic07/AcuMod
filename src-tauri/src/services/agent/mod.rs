pub mod cleanup;
mod deepseek;
mod source_search;
mod tools;

use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    env,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use keyring::Entry;
use serde::Serialize;
use serde_json::Value;
use tauri::{ipc::Channel, AppHandle};

use crate::{
    operations::{run_blocking_operation, OperationReporter},
    services::mod_library::{self, BatchModAction, ModWorkspaceSnapshot},
    storage::config::{self, DeepSeekModel},
};

const KEYRING_SERVICE: &str = "Acumen MOD Manager";
const KEYRING_USER: &str = "deepseek-api-key";
const MAX_USER_MESSAGE_CHARS: usize = 4_000;
const ACTION_PLAN_TTL_SECONDS: u64 = 5 * 60;
const MAX_PLAN_TARGETS: usize = 500;
static NEXT_TURN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PLAN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct AgentCoordinator {
    inner: Arc<AgentCoordinatorInner>,
}

#[derive(Default)]
struct AgentCoordinatorInner {
    active: AtomicBool,
    history: Mutex<Vec<deepseek::DeepSeekMessage>>,
    plans: Mutex<HashMap<String, StoredAgentActionPlan>>,
    cleanup_audits: Mutex<HashMap<String, cleanup::AgentCleanupAudit>>,
    cleanup_reviews: Mutex<HashMap<String, cleanup::AgentCleanupReview>>,
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

/// 等待用户确认的受控操作计划。这里只暴露展示信息，不暴露文件路径或底层命令。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionPlan {
    pub plan_id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub state_version: String,
    pub expires_at_unix_seconds: u64,
    pub target_count: usize,
    pub targets: Vec<AgentActionTarget>,
    pub warnings: Vec<String>,
    pub destructive: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionTarget {
    pub mod_id: String,
    pub name: String,
    pub detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActionResult {
    pub plan_id: String,
    pub status: String,
    pub title: String,
    pub message: String,
    pub succeeded_count: usize,
    pub failed_count: usize,
    pub warnings: Vec<String>,
    pub archive_import: Option<mod_library::ModArchiveImportOutcome>,
}

#[derive(Clone)]
enum AgentPlanAction {
    BatchMods {
        action: BatchModAction,
        mod_ids: Vec<String>,
    },
    ConflictOrder {
        group_id: String,
        participant_order: Vec<String>,
    },
    ModelRemap {
        mod_id: String,
        group_key: String,
        target_id: Option<String>,
    },
    CleanupExclude {
        batch_id: String,
        selections: Vec<mod_library::ModCleanupSelection>,
    },
    CleanupRestore {
        candidate_ids: Vec<String>,
    },
}

#[derive(Clone)]
struct StoredAgentActionPlan {
    public: AgentActionPlan,
    action: AgentPlanAction,
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
    pub plan: Option<AgentActionPlan>,
    pub cleanup_review: Option<cleanup::AgentCleanupReview>,
    pub knowledge_evidence: Vec<AgentKnowledgeEvidence>,
}

/// AcuAI 回答实际使用的本地知识证据。前端只展示来源元数据，不回传包内全文。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentKnowledgeEvidence {
    pub evidence_id: String,
    pub title: String,
    pub game_version: String,
    pub confidence: f64,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
    pub pack_id: String,
    pub pack_version: String,
}

/// 本轮可被 Rust 逐字段核验的结构化事实，不会发送给前端或写入会话历史。
#[derive(Clone)]
pub(crate) struct AgentKnowledgeClaim {
    pub evidence_id: String,
    pub data: Value,
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
            plan: None,
            cleanup_review: None,
            knowledge_evidence: Vec::new(),
        });
    }

    pub(crate) fn emit_plan(&mut self, plan: AgentActionPlan) {
        self.sequence += 1;
        let _ = self.channel.send(AgentEvent {
            turn_id: self.turn_id.clone(),
            sequence: self.sequence,
            kind: "planReady".to_string(),
            text: None,
            tool_name: None,
            message: Some("操作计划已生成，请确认后执行。".to_string()),
            plan: Some(plan),
            cleanup_review: None,
            knowledge_evidence: Vec::new(),
        });
    }

    pub(crate) fn emit_cleanup_review(&mut self, review: cleanup::AgentCleanupReview) {
        self.sequence += 1;
        let _ = self.channel.send(AgentEvent {
            turn_id: self.turn_id.clone(),
            sequence: self.sequence,
            kind: "cleanupReviewReady".to_string(),
            text: None,
            tool_name: None,
            message: Some(review.message.clone()),
            plan: None,
            cleanup_review: Some(review),
            knowledge_evidence: Vec::new(),
        });
    }

    /// 只发送本轮工具实际返回过的证据，避免模型回答脱离可追溯来源。
    pub(crate) fn emit_knowledge_evidence(&mut self, evidence: Vec<AgentKnowledgeEvidence>) {
        if evidence.is_empty() {
            return;
        }
        self.sequence += 1;
        let _ = self.channel.send(AgentEvent {
            turn_id: self.turn_id.clone(),
            sequence: self.sequence,
            kind: "knowledgeEvidenceReady".to_string(),
            text: None,
            tool_name: None,
            message: Some("已附上本次回答使用的知识来源。".to_string()),
            plan: None,
            cleanup_review: None,
            knowledge_evidence: evidence,
        });
    }
}

impl AgentCoordinator {
    fn begin_turn(&self) -> Result<ActiveTurnGuard, String> {
        self.inner
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "AcuAI 正在回答上一条消息，请稍候。".to_string())?;
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
            return Err("AcuAI 正在回答，完成后才能清空对话。".to_string());
        }
        self.inner
            .history
            .lock()
            .map_err(|_| "AI 会话状态不可用，请重启 Acumod 后重试。".to_string())?
            .clear();
        self.inner
            .plans
            .lock()
            .map_err(|_| "AI 操作计划状态不可用，请重启 Acumod 后重试。".to_string())?
            .clear();
        self.inner
            .cleanup_audits
            .lock()
            .map_err(|_| "AI 清理审查状态不可用，请重启 Acumod 后重试。".to_string())?
            .clear();
        self.inner
            .cleanup_reviews
            .lock()
            .map_err(|_| "AI 清理候选状态不可用，请重启 Acumod 后重试。".to_string())?
            .clear();
        Ok(())
    }

    fn store_plan(&self, plan: StoredAgentActionPlan) -> Result<AgentActionPlan, String> {
        let now = unix_seconds_now()?;
        let public = plan.public.clone();
        let mut plans = self
            .inner
            .plans
            .lock()
            .map_err(|_| "AI 操作计划状态不可用，请重启 Acumod 后重试。".to_string())?;
        plans.retain(|_, stored| stored.public.expires_at_unix_seconds > now);
        plans.insert(public.plan_id.clone(), plan);
        Ok(public)
    }

    fn take_plan(&self, plan_id: &str) -> Result<StoredAgentActionPlan, String> {
        self.inner
            .plans
            .lock()
            .map_err(|_| "AI 操作计划状态不可用，请重启 Acumod 后重试。".to_string())?
            .remove(plan_id)
            .ok_or_else(|| "操作计划不存在、已取消或已经执行。".to_string())
    }

    fn cancel_plan(&self, plan_id: &str) -> Result<AgentActionResult, String> {
        let plan = self.take_plan(plan_id)?;
        Ok(AgentActionResult {
            plan_id: plan.public.plan_id,
            status: "cancelled".to_string(),
            title: plan.public.title,
            message: "已取消操作计划，未修改任何 MOD。".to_string(),
            succeeded_count: 0,
            failed_count: 0,
            warnings: Vec::new(),
            archive_import: None,
        })
    }

    pub(crate) fn store_cleanup_review(
        &self,
        review: cleanup::AgentCleanupReview,
    ) -> Result<(), String> {
        self.inner
            .cleanup_reviews
            .lock()
            .map_err(|_| "AI 清理候选状态不可用，请重启 Acumod 后重试。".to_string())?
            .insert(review.review_id.clone(), review);
        Ok(())
    }

    pub(crate) fn store_cleanup_audit(
        &self,
        audit: cleanup::AgentCleanupAudit,
    ) -> Result<(), String> {
        let mut audits = self
            .inner
            .cleanup_audits
            .lock()
            .map_err(|_| "AI 清理审查状态不可用，请重启 Acumod 后重试。".to_string())?;
        // 一次会话只需要保留当前少量审查快照，避免重复扫描产生的清单长期占用内存。
        if audits.len() >= 4 {
            audits.clear();
        }
        audits.insert(audit.audit_id.clone(), audit);
        Ok(())
    }

    pub(crate) fn get_cleanup_audit(
        &self,
        audit_id: &str,
    ) -> Result<cleanup::AgentCleanupAudit, String> {
        self.inner
            .cleanup_audits
            .lock()
            .map_err(|_| "AI 清理审查状态不可用，请重启 Acumod 后重试。".to_string())?
            .get(audit_id)
            .cloned()
            .ok_or_else(|| "清理审查不存在或已经失效，请重新扫描。".to_string())
    }

    pub(crate) fn take_cleanup_audit(
        &self,
        audit_id: &str,
    ) -> Result<cleanup::AgentCleanupAudit, String> {
        self.inner
            .cleanup_audits
            .lock()
            .map_err(|_| "AI 清理审查状态不可用，请重启 Acumod 后重试。".to_string())?
            .remove(audit_id)
            .ok_or_else(|| "清理审查不存在或已经提交，请重新扫描。".to_string())
    }

    fn take_cleanup_review(&self, review_id: &str) -> Result<cleanup::AgentCleanupReview, String> {
        self.inner
            .cleanup_reviews
            .lock()
            .map_err(|_| "AI 清理候选状态不可用，请重启 Acumod 后重试。".to_string())?
            .remove(review_id)
            .ok_or_else(|| "清理候选不存在或已经生成过计划。".to_string())
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
    match deepseek::run_turn(
        &app,
        &coordinator,
        &key,
        model,
        history,
        message,
        &mut sender,
    )
    .await
    {
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

pub(crate) fn create_batch_action_plan(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    action_name: &str,
    mod_ids: Vec<String>,
) -> Result<AgentActionPlan, String> {
    let action = match action_name {
        "enable" => BatchModAction::Enable,
        "disable" => BatchModAction::Disable,
        "uninstall" => BatchModAction::Uninstall,
        _ => return Err("AI 提交了不支持的 MOD 操作类型。".to_string()),
    };
    let mod_ids = normalized_plan_mod_ids(mod_ids)?;
    let snapshot = load_workspace_snapshot(app)?;
    let mut targets = Vec::with_capacity(mod_ids.len());
    let mut warnings = Vec::new();
    let mut no_op_count = 0;

    for mod_id in &mod_ids {
        let item = snapshot
            .installed_mods
            .mods
            .iter()
            .find(|item| item.id == *mod_id)
            .ok_or_else(|| format!("计划中的 MOD 已不存在：{mod_id}"))?;
        let detail = match action {
            BatchModAction::Enable if item.enabled => {
                no_op_count += 1;
                "当前已启用，执行时会跳过".to_string()
            }
            BatchModAction::Enable => {
                let preview = mod_library::preview_enable_mod(app, mod_id.clone())?;
                warnings.extend(
                    preview
                        .warnings
                        .into_iter()
                        .map(|warning| format!("{}：{warning}", item.name)),
                );
                if preview.requires_overwrite_confirmation {
                    warnings.push(format!(
                        "{} 启用时会覆盖游戏目录中未被 Acumod 记录的文件。",
                        item.name
                    ));
                }
                if !preview.conflicts.is_empty() {
                    warnings.push(format!(
                        "{} 会与 {} 个已启用 MOD 发生文件冲突，后启用者优先。",
                        item.name,
                        preview.conflicts.len()
                    ));
                }
                format!("将启用并部署 {} 个文件", preview.file_count)
            }
            BatchModAction::Disable if !item.enabled => {
                no_op_count += 1;
                "当前未启用，执行时会跳过".to_string()
            }
            BatchModAction::Disable => {
                let preview = mod_library::preview_disable_mod(app, mod_id.clone())?;
                warnings.extend(
                    preview
                        .warnings
                        .into_iter()
                        .map(|warning| format!("{}：{warning}", item.name)),
                );
                format!("将禁用并移除 {} 个部署文件", preview.file_count)
            }
            BatchModAction::Uninstall => {
                let preview = mod_library::preview_uninstall_mod(app, mod_id.clone())?;
                warnings.extend(
                    preview
                        .warnings
                        .into_iter()
                        .map(|warning| format!("{}：{warning}", item.name)),
                );
                format!(
                    "将卸载并删除本地库中的 {} 个文件",
                    preview.library_file_count
                )
            }
        };
        targets.push(AgentActionTarget {
            mod_id: item.id.clone(),
            name: item.name.clone(),
            detail,
        });
    }

    if no_op_count > 0 {
        warnings.push(format!(
            "有 {no_op_count} 个 MOD 已处于目标状态，执行时会安全跳过。"
        ));
    }
    if matches!(action, BatchModAction::Uninstall) {
        warnings.insert(
            0,
            "卸载会删除 Acumod 本地库副本；如 MOD 已启用，也会先移除游戏目录中的部署文件。"
                .to_string(),
        );
    }

    let action_label = batch_action_label(action);
    let plan_action = AgentPlanAction::BatchMods { action, mod_ids };
    let plan = build_stored_plan(
        app,
        format!("批量{action_label} MOD"),
        "batchModAction",
        format!("将对 {} 个 MOD 执行{action_label}。", targets.len()),
        targets,
        warnings,
        matches!(action, BatchModAction::Uninstall),
        plan_action,
    )?;
    coordinator.store_plan(plan)
}

pub(crate) fn create_conflict_order_plan(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    group_id: String,
    participant_order: Vec<String>,
) -> Result<AgentActionPlan, String> {
    let snapshot = load_workspace_snapshot(app)?;
    let group = snapshot
        .conflict_report
        .groups
        .iter()
        .find(|group| group.group_id == group_id)
        .ok_or_else(|| "冲突组已经不存在，请重新查询冲突。".to_string())?;
    let participant_order = normalized_plan_mod_ids(participant_order)?;
    let current_ids = group
        .participants
        .iter()
        .map(|participant| participant.mod_id.clone())
        .collect::<HashSet<_>>();
    let planned_ids = participant_order.iter().cloned().collect::<HashSet<_>>();
    if current_ids != planned_ids {
        return Err("必须提供冲突组全部成员且不能包含其它 MOD。".to_string());
    }

    let targets = participant_order
        .iter()
        .enumerate()
        .map(|(index, mod_id)| {
            let participant = group
                .participants
                .iter()
                .find(|participant| participant.mod_id == *mod_id)
                .expect("participant set was validated");
            AgentActionTarget {
                mod_id: mod_id.clone(),
                name: participant.name.clone(),
                detail: format!("优先级第 {} 位", index + 1),
            }
        })
        .collect::<Vec<_>>();
    let preview = mod_library::preview_apply_conflict_order(app, group_id.clone())?;
    let mut warnings = preview.warnings;
    if preview.requires_overwrite_confirmation {
        warnings.push("应用顺序时会覆盖游戏目录中未被 Acumod 记录的文件。".to_string());
    }
    let plan = build_stored_plan(
        app,
        "调整冲突优先级".to_string(),
        "conflictOrder",
        format!(
            "将按从上到下的优先级更新 {} 个 MOD，并重新部署 {} 个冲突文件。",
            targets.len(),
            preview.applicable_file_count
        ),
        targets,
        warnings,
        false,
        AgentPlanAction::ConflictOrder {
            group_id,
            participant_order,
        },
    )?;
    coordinator.store_plan(plan)
}

pub(crate) fn create_model_remap_plan(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    mod_id: String,
    group_key: String,
    target_id: Option<String>,
) -> Result<AgentActionPlan, String> {
    let details = mod_library::get_mod_remap_details(app, mod_id.clone())?;
    if details.enabled {
        return Err("模型改绑前必须先禁用该 MOD。".to_string());
    }
    let group = details
        .groups
        .iter()
        .find(|group| group.group_key == group_key)
        .ok_or_else(|| "模型替换分组已经变化，请重新查询可选目标。".to_string())?;
    let preview =
        mod_library::preview_mod_remap(app, mod_id.clone(), group_key.clone(), target_id.clone())?;
    let detail = if target_id.is_none() {
        "恢复为导入时的替换模型".to_string()
    } else {
        format!("改为替换 {}", preview.target_label)
    };
    let target = AgentActionTarget {
        mod_id: details.mod_id.clone(),
        name: details.name,
        detail,
    };
    let plan = build_stored_plan(
        app,
        "修改替换模型".to_string(),
        "modelRemap",
        format!(
            "将修改“{}”分组，重写 {} 个本地 MOD 文件。",
            group.sub_kind, preview.changed_file_count
        ),
        vec![target],
        preview.warnings,
        false,
        AgentPlanAction::ModelRemap {
            mod_id,
            group_key,
            target_id,
        },
    )?;
    coordinator.store_plan(plan)
}

pub fn create_agent_cleanup_plan(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    review_id: String,
    candidate_ids: Vec<String>,
) -> Result<AgentActionPlan, String> {
    let review = coordinator.take_cleanup_review(&review_id)?;
    let selected_ids = candidate_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<HashSet<_>>();
    if selected_ids.is_empty() {
        return Err("请至少选择一个要排除的文件。".to_string());
    }
    let review_ids = review
        .items
        .iter()
        .map(|item| item.candidate_id.clone())
        .collect::<HashSet<_>>();
    if !selected_ids.is_subset(&review_ids) {
        return Err("选择包含不属于当前清理结果的文件。".to_string());
    }

    let selected_items = review
        .items
        .into_iter()
        .filter(|item| selected_ids.contains(&item.candidate_id))
        .collect::<Vec<_>>();
    let selections = selected_items
        .iter()
        .map(|item| mod_library::ModCleanupSelection {
            candidate_id: item.candidate_id.clone(),
            mod_id: item.mod_id.clone(),
            library_relative_path: item.library_relative_path.clone(),
            reason: item.reason.clone(),
        })
        .collect::<Vec<_>>();
    let targets = selected_items
        .iter()
        .map(|item| AgentActionTarget {
            mod_id: item.mod_id.clone(),
            name: item.mod_name.clone(),
            detail: format!("排除部署：{}", item.deploy_relative_path),
        })
        .collect::<Vec<_>>();
    let deployed_count = selected_items
        .iter()
        .filter(|item| item.currently_deployed)
        .count();
    let plan = build_stored_plan(
        app,
        "应用 MOD 文件清理".to_string(),
        "cleanupExclusions",
        format!(
            "将记录 {} 个部署排除项，其中 {deployed_count} 个当前已部署文件会从游戏目录移除或由冲突中的其它 MOD 接管。",
            targets.len()
        ),
        targets,
        vec![
            "本地 MOD 库中的原始文件不会删除；清理结果可以恢复。".to_string(),
            "清理仅改变后续部署内容，不依赖 AI 才能启用或禁用 MOD。".to_string(),
        ],
        false,
        AgentPlanAction::CleanupExclude {
            batch_id: review_id,
            selections,
        },
    )?;
    coordinator.store_plan(plan)
}

pub(crate) fn create_cleanup_restore_plan(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    scope: &str,
    mod_id: Option<String>,
) -> Result<AgentActionPlan, String> {
    let exclusions = mod_library::list_mod_cleanup_exclusions(app)?;
    let selected = match scope {
        "lastBatch" => {
            let latest_batch_id = exclusions
                .latest_batch_id
                .as_ref()
                .ok_or_else(|| "当前没有可以恢复的清理批次。".to_string())?;
            exclusions
                .groups
                .iter()
                .flat_map(|group| {
                    group
                        .exclusions
                        .iter()
                        .filter(|exclusion| exclusion.batch_id == *latest_batch_id)
                        .map(move |exclusion| (group, exclusion))
                })
                .collect::<Vec<_>>()
        }
        "mod" => {
            let mod_id =
                mod_id.ok_or_else(|| "恢复指定 MOD 时必须提供稳定 MOD ID。".to_string())?;
            let group = exclusions
                .groups
                .iter()
                .find(|group| group.mod_id == mod_id)
                .ok_or_else(|| "该 MOD 当前没有部署排除项。".to_string())?;
            group
                .exclusions
                .iter()
                .map(|exclusion| (group, exclusion))
                .collect::<Vec<_>>()
        }
        "all" => exclusions
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .exclusions
                    .iter()
                    .map(move |exclusion| (group, exclusion))
            })
            .collect::<Vec<_>>(),
        _ => return Err("未知的清理恢复范围。".to_string()),
    };
    if selected.is_empty() {
        return Err("当前没有符合范围的部署排除项。".to_string());
    }
    let candidate_ids = selected
        .iter()
        .map(|(_, exclusion)| exclusion.candidate_id.clone())
        .collect::<Vec<_>>();
    let targets = selected
        .into_iter()
        .map(|(group, exclusion)| AgentActionTarget {
            mod_id: group.mod_id.clone(),
            name: group.mod_name.clone(),
            detail: format!("恢复部署：{}", exclusion.deploy_relative_path),
        })
        .collect::<Vec<_>>();
    let plan = build_stored_plan(
        app,
        "恢复 MOD 清理项".to_string(),
        "cleanupRestore",
        format!(
            "将恢复 {} 个部署排除项；已启用 MOD 会按当前冲突优先级重新部署对应文件。",
            targets.len()
        ),
        targets,
        Vec::new(),
        false,
        AgentPlanAction::CleanupRestore { candidate_ids },
    )?;
    coordinator.store_plan(plan)
}

pub async fn confirm_agent_action_plan(
    app: AppHandle,
    coordinator: AgentCoordinator,
    plan_id: String,
) -> Result<AgentActionResult, String> {
    let stored = coordinator.take_plan(&plan_id)?;
    if stored.public.expires_at_unix_seconds <= unix_seconds_now()? {
        return Err("操作计划已过期，请让 AI 根据当前状态重新生成。".to_string());
    }

    let (kind, title) = match stored.action {
        AgentPlanAction::BatchMods { .. } => ("agentBatchMods", "正在执行 AI 批量操作"),
        AgentPlanAction::ConflictOrder { .. } => ("agentConflictOrder", "正在应用 AI 冲突优先级"),
        AgentPlanAction::ModelRemap { .. } => ("agentModelRemap", "正在应用 AI 模型改绑"),
        AgentPlanAction::CleanupExclude { .. } => ("agentCleanupApply", "正在应用 MOD 文件清理"),
        AgentPlanAction::CleanupRestore { .. } => ("agentCleanupRestore", "正在恢复 MOD 清理项"),
    };
    let worker_app = app.clone();
    run_blocking_operation(app, kind, title, move |progress| {
        let current_version = state_version_for_action(&worker_app, &stored.action)?;
        validate_plan_state_version(&stored.public.state_version, &current_version)?;
        execute_stored_plan(&worker_app, stored, &progress)
    })
    .await
}

pub fn cancel_agent_action_plan(
    coordinator: &AgentCoordinator,
    plan_id: String,
) -> Result<AgentActionResult, String> {
    coordinator.cancel_plan(&plan_id)
}

fn execute_stored_plan(
    app: &AppHandle,
    stored: StoredAgentActionPlan,
    progress: &OperationReporter,
) -> Result<AgentActionResult, String> {
    let public = stored.public;
    match stored.action {
        AgentPlanAction::BatchMods { action, mod_ids } => {
            let result =
                mod_library::batch_update_mods_with_progress(app, action, mod_ids, progress)?;
            let mut warnings = result.warnings;
            warnings.extend(
                result
                    .items
                    .iter()
                    .filter(|item| item.status == "failed")
                    .map(|item| format!("{}：{}", item.name, item.message)),
            );
            Ok(AgentActionResult {
                plan_id: public.plan_id,
                status: if result.failed_count == 0 {
                    "completed".to_string()
                } else {
                    "partiallyFailed".to_string()
                },
                title: public.title,
                message: result.message,
                succeeded_count: result.succeeded_count,
                failed_count: result.failed_count,
                warnings,
                archive_import: None,
            })
        }
        AgentPlanAction::ConflictOrder {
            group_id,
            participant_order,
        } => {
            mod_library::set_conflict_participant_order(app, group_id.clone(), participant_order)?;
            let preview = mod_library::preview_apply_conflict_order(app, group_id.clone())?;
            let result = mod_library::apply_conflict_order_with_progress(
                app,
                group_id,
                preview.requires_overwrite_confirmation,
                progress,
            )?;
            Ok(AgentActionResult {
                plan_id: public.plan_id,
                status: "completed".to_string(),
                title: public.title,
                message: result.message,
                succeeded_count: 1,
                failed_count: 0,
                warnings: result.warnings,
                archive_import: None,
            })
        }
        AgentPlanAction::ModelRemap {
            mod_id,
            group_key,
            target_id,
        } => {
            let result = mod_library::apply_mod_remap_with_progress(
                app, mod_id, group_key, target_id, progress,
            )?;
            Ok(AgentActionResult {
                plan_id: public.plan_id,
                status: "completed".to_string(),
                title: public.title,
                message: result.message,
                succeeded_count: 1,
                failed_count: 0,
                warnings: Vec::new(),
                archive_import: None,
            })
        }
        AgentPlanAction::CleanupExclude {
            batch_id,
            selections,
        } => {
            let result = mod_library::apply_mod_cleanup_exclusions_with_progress(
                app, batch_id, selections, progress,
            )?;
            Ok(AgentActionResult {
                plan_id: public.plan_id,
                status: "completed".to_string(),
                title: public.title,
                message: result.message,
                succeeded_count: result.exclusion_count,
                failed_count: 0,
                warnings: result.warnings,
                archive_import: None,
            })
        }
        AgentPlanAction::CleanupRestore { candidate_ids } => {
            let result = mod_library::restore_mod_cleanup_exclusions_with_progress(
                app,
                candidate_ids,
                progress,
            )?;
            Ok(AgentActionResult {
                plan_id: public.plan_id,
                status: "completed".to_string(),
                title: public.title,
                message: result.message,
                succeeded_count: result.restored_exclusion_count,
                failed_count: 0,
                warnings: result.warnings,
                archive_import: None,
            })
        }
    }
}

fn build_stored_plan(
    app: &AppHandle,
    title: String,
    kind: &str,
    summary: String,
    targets: Vec<AgentActionTarget>,
    warnings: Vec<String>,
    destructive: bool,
    action: AgentPlanAction,
) -> Result<StoredAgentActionPlan, String> {
    let now = unix_seconds_now()?;
    let state_version = state_version_for_action(app, &action)?;
    Ok(StoredAgentActionPlan {
        public: AgentActionPlan {
            plan_id: format!(
                "agent-plan-{}",
                NEXT_PLAN_ID.fetch_add(1, Ordering::Relaxed)
            ),
            kind: kind.to_string(),
            title,
            summary,
            state_version,
            expires_at_unix_seconds: now + ACTION_PLAN_TTL_SECONDS,
            target_count: targets.len(),
            targets,
            warnings,
            destructive,
        },
        action,
    })
}

fn state_version_for_action(app: &AppHandle, action: &AgentPlanAction) -> Result<String, String> {
    let material = match action {
        AgentPlanAction::BatchMods { mod_ids, .. } => {
            let snapshot = load_workspace_snapshot(app)?;
            let mut states = snapshot
                .installed_mods
                .mods
                .iter()
                .map(|item| format!("{}:{}", item.id, item.enabled))
                .collect::<Vec<_>>();
            states.sort();
            for mod_id in mod_ids {
                if !snapshot
                    .installed_mods
                    .mods
                    .iter()
                    .any(|item| item.id == *mod_id)
                {
                    return Err(format!("计划中的 MOD 已不存在：{mod_id}"));
                }
            }
            let mut conflict_orders = snapshot
                .conflict_report
                .groups
                .iter()
                .map(|group| {
                    let order = group
                        .participants
                        .iter()
                        .map(|participant| format!("{}:{}", participant.mod_id, participant.order))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("{}={order}", group.group_id)
                })
                .collect::<Vec<_>>();
            conflict_orders.sort();
            format!(
                "mods:{}|conflicts:{}",
                states.join(";"),
                conflict_orders.join(";")
            )
        }
        AgentPlanAction::ConflictOrder { group_id, .. } => {
            let snapshot = load_workspace_snapshot(app)?;
            let group = snapshot
                .conflict_report
                .groups
                .iter()
                .find(|group| group.group_id == *group_id)
                .ok_or_else(|| "冲突组已经不存在。".to_string())?;
            let participants = group
                .participants
                .iter()
                .map(|participant| {
                    format!(
                        "{}:{}:{}",
                        participant.mod_id, participant.enabled, participant.order
                    )
                })
                .collect::<Vec<_>>()
                .join(";");
            format!("conflict:{group_id}|{participants}")
        }
        AgentPlanAction::ModelRemap {
            mod_id, group_key, ..
        } => {
            let details = mod_library::get_mod_remap_details(app, mod_id.clone())?;
            let group = details
                .groups
                .iter()
                .find(|group| group.group_key == *group_key)
                .ok_or_else(|| "模型替换分组已经不存在。".to_string())?;
            format!(
                "remap:{}:{}:{}:{}",
                details.mod_id,
                details.enabled,
                group.group_key,
                group.selected_target_id.as_deref().unwrap_or("default")
            )
        }
        AgentPlanAction::CleanupExclude { selections, .. } => {
            let mod_ids = selections
                .iter()
                .map(|selection| selection.mod_id.clone())
                .collect::<Vec<_>>();
            let candidates = mod_library::scan_mod_cleanup_candidates_for_mod_ids(app, &mod_ids)?
                .candidates
                .into_iter()
                .map(|candidate| (candidate.candidate_id.clone(), candidate))
                .collect::<HashMap<_, _>>();
            let mut states = Vec::new();
            for selection in selections {
                let candidate = candidates
                    .get(&selection.candidate_id)
                    .ok_or_else(|| "清理候选已经变化，请重新扫描。".to_string())?;
                if candidate.mod_id != selection.mod_id
                    || conflict_path_key_for_agent(&candidate.library_relative_path)
                        != conflict_path_key_for_agent(&selection.library_relative_path)
                {
                    return Err("清理候选路径已经变化，请重新扫描。".to_string());
                }
                states.push(format!(
                    "{}:{}:{}",
                    candidate.candidate_id, candidate.mod_id, candidate.currently_deployed
                ));
            }
            states.sort();
            format!("cleanup-exclude:{}", states.join(";"))
        }
        AgentPlanAction::CleanupRestore { candidate_ids } => {
            let exclusions = mod_library::list_mod_cleanup_exclusions(app)?;
            let available = exclusions
                .groups
                .iter()
                .flat_map(|group| {
                    group
                        .exclusions
                        .iter()
                        .map(|exclusion| exclusion.candidate_id.clone())
                })
                .collect::<HashSet<_>>();
            if !candidate_ids.iter().all(|id| available.contains(id)) {
                return Err("部署排除记录已经变化，请重新生成恢复计划。".to_string());
            }
            let mut ids = candidate_ids.clone();
            ids.sort();
            format!("cleanup-restore:{}", ids.join(";"))
        }
    };
    let mut hasher = DefaultHasher::new();
    material.hash(&mut hasher);
    Ok(format!("state-{:016x}", hasher.finish()))
}

fn load_workspace_snapshot(app: &AppHandle) -> Result<ModWorkspaceSnapshot, String> {
    mod_library::get_mod_workspace_snapshot_with_progress(app, &OperationReporter::default())
}

fn normalized_plan_mod_ids(mod_ids: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mod_ids = mod_ids
        .into_iter()
        .map(|mod_id| mod_id.trim().to_string())
        .filter(|mod_id| !mod_id.is_empty() && seen.insert(mod_id.clone()))
        .collect::<Vec<_>>();
    if mod_ids.is_empty() {
        return Err("操作计划至少需要一个 MOD。".to_string());
    }
    if mod_ids.len() > MAX_PLAN_TARGETS {
        return Err(format!("一次操作计划最多包含 {MAX_PLAN_TARGETS} 个 MOD。"));
    }
    Ok(mod_ids)
}

fn batch_action_label(action: BatchModAction) -> &'static str {
    match action {
        BatchModAction::Enable => "启用",
        BatchModAction::Disable => "禁用",
        BatchModAction::Uninstall => "卸载",
    }
}

fn validate_plan_state_version(expected: &str, current: &str) -> Result<(), String> {
    if expected == current {
        Ok(())
    } else {
        Err("MOD 状态在计划生成后发生变化，请重新生成操作计划。".to_string())
    }
}

fn conflict_path_key_for_agent(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn unix_seconds_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("系统时间不可用：{error}"))
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
    use super::{
        api_key_hint, normalized_plan_mod_ids, validate_api_key, validate_plan_state_version,
        validate_user_message,
    };

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

    #[test]
    fn action_plan_ids_are_trimmed_and_deduplicated_without_reordering() {
        assert_eq!(
            normalized_plan_mod_ids(vec![
                " mod-b ".to_string(),
                "mod-a".to_string(),
                "mod-b".to_string(),
            ])
            .unwrap(),
            vec!["mod-b", "mod-a"]
        );
    }

    #[test]
    fn changed_state_version_rejects_a_stale_plan() {
        assert!(validate_plan_state_version("state-old", "state-old").is_ok());
        assert!(validate_plan_state_version("state-old", "state-new").is_err());
    }
}
