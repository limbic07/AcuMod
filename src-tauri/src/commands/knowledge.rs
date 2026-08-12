use crate::{
    operations::run_blocking_operation,
    services::knowledge::{
        self, KnowledgeBundleInstallResult, KnowledgeEntityLookupResponse,
        KnowledgeRelationResponse, KnowledgeSearchResponse, KnowledgeStatus,
    },
    services::mhwdata,
};

/// 读取已经安装的知识包及其健康状态。
#[tauri::command]
pub fn get_knowledge_status() -> Result<KnowledgeStatus, String> {
    knowledge::get_status()
}

/// 从一个知识包 ZIP 中校验并安装 MHWData、MOD 技术和 Acumod 帮助资料。
#[tauri::command]
pub async fn install_knowledge_bundle(
    app: tauri::AppHandle,
    source_path: String,
) -> Result<KnowledgeBundleInstallResult, String> {
    let unpack_app = app.clone();
    run_blocking_operation(
        app,
        "installKnowledgeBundle",
        "正在安装整套知识包",
        move |progress| knowledge::install_bundle(unpack_app, source_path, &progress),
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
        let root = knowledge::knowledge_root()?;
        mhwdata::lookup_game_entities(&root, &query, kinds.as_deref(), limit.unwrap_or(20))
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
        let root = knowledge::knowledge_root()?;
        mhwdata::get_game_entity_relations(
            &root,
            &entity_id,
            predicates.as_deref(),
            direction.as_deref().unwrap_or("both"),
            limit.unwrap_or(30),
        )
    })
    .await
    .map_err(|error| format!("游戏关系查询任务失败：{error}"))?
}
