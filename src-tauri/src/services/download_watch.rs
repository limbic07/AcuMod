use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const DOWNLOAD_WATCH_EVENT: &str = "acumod://download-watch";
const MAX_WATCH_SECONDS: u64 = 5 * 60;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const REQUIRED_STABLE_POLLS: u8 = 2;
static NEXT_WATCH_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadWatchStart {
    pub watch_id: String,
    pub directory: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadWatchEvent {
    watch_id: String,
    status: String,
    source_url: String,
    file_path: Option<String>,
    file_name: Option<String>,
    size_bytes: Option<u64>,
    message: String,
}

#[derive(Clone, Copy)]
struct FileObservation {
    size_bytes: u64,
    modified_at: SystemTime,
    stable_polls: u8,
}

/// 启动一次短时下载目录会话；只发现完成归档，不读取浏览器或自动导入文件。
pub fn start(
    app: AppHandle,
    raw_directory: String,
    source_url: String,
) -> Result<DownloadWatchStart, String> {
    let directory = canonical_directory(&raw_directory)?;
    validate_source_url(&source_url)?;
    let baseline = directory_snapshot(&directory)?;
    let watch_id = format!(
        "download-watch-{}",
        NEXT_WATCH_ID.fetch_add(1, Ordering::Relaxed)
    );
    let started_at = SystemTime::now();
    let worker_app = app.clone();
    let worker_watch_id = watch_id.clone();
    let worker_directory = directory.clone();

    thread::spawn(move || {
        let result = watch_for_archive(&worker_directory, baseline, started_at);
        let event = match result {
            Ok(Some((path, size_bytes))) => DownloadWatchEvent {
                watch_id: worker_watch_id,
                status: "found".to_string(),
                source_url,
                file_path: Some(path.to_string_lossy().into_owned()),
                file_name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                size_bytes: Some(size_bytes),
                message: "发现已完成的下载归档，请确认后导入。".to_string(),
            },
            Ok(None) => DownloadWatchEvent {
                watch_id: worker_watch_id,
                status: "expired".to_string(),
                source_url,
                file_path: None,
                file_name: None,
                size_bytes: None,
                message: "等待浏览器下载已结束，未发现可导入的完成归档。".to_string(),
            },
            Err(error) => DownloadWatchEvent {
                watch_id: worker_watch_id,
                status: "failed".to_string(),
                source_url,
                file_path: None,
                file_name: None,
                size_bytes: None,
                message: format!("下载目录监听失败：{error}"),
            },
        };
        let _ = worker_app.emit(DOWNLOAD_WATCH_EVENT, event);
    });

    Ok(DownloadWatchStart {
        watch_id,
        directory: directory.to_string_lossy().into_owned(),
        message: "已开始等待浏览器下载，最长等待 5 分钟。".to_string(),
    })
}

fn watch_for_archive(
    directory: &Path,
    baseline: HashMap<PathBuf, SystemTime>,
    started_at: SystemTime,
) -> Result<Option<(PathBuf, u64)>, String> {
    let mut observations = HashMap::<PathBuf, FileObservation>::new();
    for _ in 0..MAX_WATCH_SECONDS {
        for entry in fs::read_dir(directory)
            .map_err(|error| format!("无法读取下载目录 {}：{error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("无法读取下载目录项目：{error}"))?;
            let path = entry.path();
            if !is_supported_archive(&path) || !is_direct_regular_child(directory, &path)? {
                continue;
            }
            let metadata = fs::metadata(&path)
                .map_err(|error| format!("无法读取下载文件元数据 {}：{error}", path.display()))?;
            let modified_at = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let is_new_or_changed = baseline
                .get(&path)
                .is_none_or(|baseline_modified| modified_at > *baseline_modified)
                || modified_at >= started_at;
            if !is_new_or_changed {
                continue;
            }
            let observation = observations.entry(path.clone()).or_insert(FileObservation {
                size_bytes: metadata.len(),
                modified_at,
                stable_polls: 0,
            });
            if observation.size_bytes == metadata.len() && observation.modified_at == modified_at {
                observation.stable_polls = observation.stable_polls.saturating_add(1);
            } else {
                *observation = FileObservation {
                    size_bytes: metadata.len(),
                    modified_at,
                    stable_polls: 0,
                };
            }
            if observation.stable_polls >= REQUIRED_STABLE_POLLS {
                return Ok(Some((path, metadata.len())));
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(None)
}

fn canonical_directory(raw_directory: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw_directory.trim());
    if path.as_os_str().is_empty() {
        return Err("请选择要等待浏览器下载的目录。".to_string());
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("无法访问下载目录 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("下载目录必须是可访问的真实目录，不能是符号链接。".to_string());
    }
    path.canonicalize()
        .map_err(|error| format!("无法确认下载目录 {}：{error}", path.display()))
}

fn directory_snapshot(directory: &Path) -> Result<HashMap<PathBuf, SystemTime>, String> {
    let mut snapshot = HashMap::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("无法读取下载目录 {}：{error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("无法读取下载目录项目：{error}"))?;
        let path = entry.path();
        if is_supported_archive(&path) && is_direct_regular_child(directory, &path)? {
            let modified = fs::metadata(&path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            snapshot.insert(path, modified);
        }
    }
    Ok(snapshot)
}

fn is_direct_regular_child(directory: &Path, path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取下载文件 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("无法确认下载文件 {}：{error}", path.display()))?;
    Ok(canonical.parent() == Some(directory))
}

fn is_supported_archive(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("zip" | "7z" | "rar")
    )
}

fn validate_source_url(source_url: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(source_url.trim())
        .map_err(|_| "来源链接无效，无法开始下载会话。".to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("下载会话只接受 HTTPS 来源页面。".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_supported_archive, validate_source_url};
    use std::path::Path;

    #[test]
    fn only_accepts_supported_archives() {
        assert!(is_supported_archive(Path::new("example.ZIP")));
        assert!(is_supported_archive(Path::new("example.7z")));
        assert!(is_supported_archive(Path::new("example.rar")));
        assert!(!is_supported_archive(Path::new("example.zip.crdownload")));
        assert!(!is_supported_archive(Path::new("readme.txt")));
    }

    #[test]
    fn only_allows_https_source_urls() {
        assert!(validate_source_url("https://www.nexusmods.com/").is_ok());
        assert!(validate_source_url("http://www.nexusmods.com/").is_err());
        assert!(validate_source_url("not a url").is_err());
    }
}
