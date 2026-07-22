use crate::{
    operations::run_blocking_operation,
    services::mod_analysis::{self, ModAnalysisReport},
};

/// 分析一个已安装 MOD 的文件作用和资源依赖；调用方只能传稳定 MOD ID。
#[tauri::command]
pub async fn analyze_installed_mod(
    app: tauri::AppHandle,
    mod_id: String,
) -> Result<ModAnalysisReport, String> {
    let worker_app = app.clone();
    run_blocking_operation(
        app,
        "analyzeMod",
        "正在分析 MOD 文件",
        move |progress| mod_analysis::analyze_mod(&worker_app, &mod_id, &progress),
    )
    .await
}
