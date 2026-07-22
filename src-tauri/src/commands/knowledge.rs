use crate::{
    operations::run_blocking_operation,
    services::knowledge::{
        self, KnowledgeEntityLookupResponse, KnowledgeInstallResult, KnowledgeRelationResponse,
        KnowledgeSearchResponse, KnowledgeStatus,
    },
};

/// 读取已经安装的知识包及其健康状态。
#[tauri::command]
pub fn get_knowledge_status() -> Result<KnowledgeStatus, String> {
    knowledge::get_status()
}

/// 将一个本地 `.acukb` 文件校验后原子安装到软件数据目录。
#[tauri::command]
pub async fn install_knowledge_pack(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<KnowledgeInstallResult, String> {
    run_blocking_operation(
        app,
        "installKnowledgePack",
        "正在安装知识包",
        move |progress| knowledge::install_pack(source_path, &progress),
    )
    .await
}

/// 删除同一包 ID 的全部本地版本，不影响传统 MOD 管理功能。
#[tauri::command]
pub async fn delete_knowledge_pack(
    app: tauri::AppHandle,
    pack_id: String,
) -> Result<KnowledgeStatus, String> {
    run_blocking_operation(
        app,
        "deleteKnowledgePack",
        "正在删除知识包",
        move |progress| knowledge::delete_pack(&pack_id, &progress),
    )
    .await
}

/// 使用固定参数查询活动知识包；调用方不能提交 SQL。
#[tauri::command]
pub async fn search_knowledge(
    query: String,
    domains: Option<Vec<String>>,
    limit: Option<usize>,
) -> Result<KnowledgeSearchResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        knowledge::search(&query, domains.as_deref(), limit.unwrap_or(20))
    })
    .await
    .map_err(|error| format!("知识库查询任务失败：{error}"))?
}

/// 按名称、别名或稳定 ID 查询游戏实体；调用方不能提交 SQL。
#[tauri::command]
pub async fn lookup_game_entities(
    query: String,
    kinds: Option<Vec<String>>,
    limit: Option<usize>,
) -> Result<KnowledgeEntityLookupResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        knowledge::lookup_game_entities(&query, kinds.as_deref(), limit.unwrap_or(20))
    })
    .await
    .map_err(|error| format!("游戏实体查询任务失败：{error}"))?
}

/// 查询精确实体的受控入向、出向或双向关系。
#[tauri::command]
pub async fn get_game_entity_relations(
    entity_id: String,
    predicates: Option<Vec<String>>,
    direction: Option<String>,
    limit: Option<usize>,
) -> Result<KnowledgeRelationResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        knowledge::get_game_entity_relations(
            &entity_id,
            predicates.as_deref(),
            direction.as_deref().unwrap_or("both"),
            limit.unwrap_or(30),
        )
    })
    .await
    .map_err(|error| format!("游戏关系查询任务失败：{error}"))?
}
