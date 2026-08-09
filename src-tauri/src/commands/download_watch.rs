use crate::services::download_watch::{self, DownloadWatchStart};

/// 在用户选择的目录中短时等待浏览器完成下载；发现文件后只通知前端确认导入。
#[tauri::command]
pub fn start_download_watch(
    app: tauri::AppHandle,
    directory: String,
    source_url: String,
) -> Result<DownloadWatchStart, String> {
    download_watch::start(app, directory, source_url)
}
