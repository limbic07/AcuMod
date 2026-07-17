use std::{
    env,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::Duration,
};

use futures_util::StreamExt;
use keyring::Entry;
use reqwest::{Client, StatusCode, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::{operations::OperationReporter, services::mod_library};

const NEXUS_API_ROOT: &str = "https://api.nexusmods.com/v1";
const NEXUS_GAME_DOMAIN: &str = "monsterhunterworld";
const KEYRING_SERVICE: &str = "Acumen MOD Manager";
const KEYRING_USER: &str = "nexus-personal-api-key";
const MAX_API_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusCredentialStatus {
    pub configured: bool,
    pub hint: Option<String>,
    pub source: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusConnectionResult {
    pub user_name: String,
    pub is_premium: bool,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusModSummary {
    pub mod_id: u64,
    pub name: String,
    pub summary: String,
    pub author: String,
    pub version: String,
    pub updated_at_unix_seconds: u64,
    pub page_url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusModFile {
    pub mod_id: u64,
    pub file_id: u64,
    pub name: String,
    pub file_name: String,
    pub version: String,
    pub category_name: String,
    pub size_bytes: u64,
    pub uploaded_at_unix_seconds: u64,
    pub description: String,
    pub is_primary: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusModFiles {
    pub mod_info: NexusModSummary,
    pub files: Vec<NexusModFile>,
    pub total_file_count: usize,
    pub direct_download_available: bool,
    pub membership_message: String,
}

#[derive(Clone)]
pub struct NexusDownloadTarget {
    pub mod_info: NexusModSummary,
    pub file: NexusModFile,
}

struct StoredCredential {
    key: String,
    source: String,
}

#[derive(Deserialize)]
struct NexusUserResponse {
    #[serde(default)]
    name: String,
    #[serde(default)]
    is_premium: bool,
}

#[derive(Deserialize)]
struct NexusModResponse {
    mod_id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    updated_timestamp: u64,
}

#[derive(Deserialize)]
struct NexusFilesResponse {
    #[serde(default)]
    files: Vec<NexusFileResponse>,
}

#[derive(Deserialize)]
struct NexusFileResponse {
    file_id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    category_name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    size_kb: u64,
    #[serde(default)]
    size_in_bytes: u64,
    #[serde(default)]
    uploaded_timestamp: u64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    is_primary: bool,
}

#[derive(Deserialize)]
struct NexusDownloadLink {
    #[serde(rename = "URI")]
    uri: String,
}

pub fn credential_status() -> Result<NexusCredentialStatus, String> {
    let credential = load_api_key()?;
    Ok(NexusCredentialStatus {
        configured: credential.is_some(),
        hint: credential.as_ref().map(|value| api_key_hint(&value.key)),
        source: credential.map(|value| value.source),
    })
}

pub fn set_api_key(api_key: String) -> Result<NexusCredentialStatus, String> {
    let api_key = validate_api_key(&api_key)?;
    keyring_entry()?
        .set_password(&api_key)
        .map_err(|error| format!("无法把 Nexus API Key 保存到 Windows 凭据管理器：{error}"))?;
    credential_status()
}

pub fn delete_api_key() -> Result<NexusCredentialStatus, String> {
    match keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => {
            return Err(format!(
                "无法从 Windows 凭据管理器删除 Nexus API Key：{error}"
            ));
        }
    }
    credential_status()
}

pub async fn test_connection() -> Result<NexusConnectionResult, String> {
    let user = validate_current_user().await?;
    Ok(NexusConnectionResult {
        user_name: user.name,
        is_premium: user.is_premium,
        message: if user.is_premium {
            "Nexus Mods 连接正常，当前账户支持 API 直接下载。".to_string()
        } else {
            "Nexus Mods 连接正常；普通会员需在网页发起下载，不能由 API 直接下载。".to_string()
        },
    })
}

pub async fn get_mod_summary(mod_id: u64) -> Result<NexusModSummary, String> {
    let key = require_api_key()?;
    let response = api_get(
        &key,
        &format!("/games/{NEXUS_GAME_DOMAIN}/mods/{mod_id}.json"),
    )
    .await?;
    let body = read_json_limited::<NexusModResponse>(response).await?;
    Ok(to_mod_summary(body))
}

pub async fn get_mod_files(mod_id: u64) -> Result<NexusModFiles, String> {
    let key = require_api_key()?;
    let user = validate_user_with_key(&key).await?;
    let mod_response = api_get(
        &key,
        &format!("/games/{NEXUS_GAME_DOMAIN}/mods/{mod_id}.json"),
    )
    .await?;
    let mod_info = to_mod_summary(read_json_limited::<NexusModResponse>(mod_response).await?);
    let files_response = api_get(
        &key,
        &format!("/games/{NEXUS_GAME_DOMAIN}/mods/{mod_id}/files.json"),
    )
    .await?;
    let body = read_json_limited::<NexusFilesResponse>(files_response).await?;
    let total_file_count = body.files.len();
    let mut files = body
        .files
        .into_iter()
        .map(|file| to_mod_file(mod_id, file))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right.is_primary.cmp(&left.is_primary).then_with(|| {
            right
                .uploaded_at_unix_seconds
                .cmp(&left.uploaded_at_unix_seconds)
        })
    });
    files.truncate(100);

    Ok(NexusModFiles {
        mod_info,
        files,
        total_file_count,
        direct_download_available: user.is_premium,
        membership_message: if user.is_premium {
            "当前账户可以生成受控下载计划。".to_string()
        } else {
            "Nexus 普通会员不能通过 API 直接下载，请打开 MOD 页面手动下载后再导入。".to_string()
        },
    })
}

pub async fn get_download_target(mod_id: u64, file_id: u64) -> Result<NexusDownloadTarget, String> {
    let files = get_mod_files(mod_id).await?;
    if !files.direct_download_available {
        return Err(files.membership_message);
    }
    let file = files
        .files
        .into_iter()
        .find(|file| file.file_id == file_id)
        .ok_or_else(|| "Nexus 文件不存在，或不在当前可读取的文件列表中。".to_string())?;
    validate_archive_file_name(&file.file_name)?;
    Ok(NexusDownloadTarget {
        mod_info: files.mod_info,
        file,
    })
}

/// 下载计划执行时重新读取文件元数据和会员状态，避免使用过期链接或变化后的文件。
pub async fn download_archive(
    app: &AppHandle,
    expected: &NexusDownloadTarget,
    progress: &OperationReporter,
) -> Result<PathBuf, String> {
    let current = get_download_target(expected.mod_info.mod_id, expected.file.file_id).await?;
    if current.file.file_name != expected.file.file_name
        || current.file.size_bytes != expected.file.size_bytes
        || current.file.uploaded_at_unix_seconds != expected.file.uploaded_at_unix_seconds
    {
        return Err("Nexus 文件信息在确认后发生变化，请重新生成下载计划。".to_string());
    }

    let key = require_api_key()?;
    let response = api_get(
        &key,
        &format!(
            "/games/{NEXUS_GAME_DOMAIN}/mods/{}/files/{}/download_link.json",
            current.mod_info.mod_id, current.file.file_id
        ),
    )
    .await?;
    let links = read_json_limited::<Vec<NexusDownloadLink>>(response).await?;
    let link = links
        .first()
        .ok_or_else(|| "Nexus 没有返回可用的下载地址。".to_string())?;
    let download_url =
        Url::parse(&link.uri).map_err(|_| "Nexus 返回了无效的下载地址。".to_string())?;
    validate_download_url(&download_url)?;

    let extension = validate_archive_file_name(&current.file.file_name)?;
    let file_stem = format!(
        "nexus-{}-{}-{}",
        current.mod_info.mod_id, current.file.file_id, current.file.uploaded_at_unix_seconds
    );
    let (final_path, part_path) =
        mod_library::prepare_download_staging_archive(app, &file_stem, &extension)?;
    mod_library::remove_download_staging_file(app, &part_path)?;
    mod_library::remove_download_staging_file(app, &final_path)?;

    let client = download_client()?;
    progress.report(
        "正在连接 Nexus 下载服务",
        0,
        None,
        Some(current.file.file_name.clone()),
    );
    let response = client
        .get(download_url)
        .send()
        .await
        .map_err(map_request_error)?;
    let response = ensure_success(response).await?;
    validate_download_url(response.url())?;
    let response_length = response.content_length();
    let total =
        response_length.or((current.file.size_bytes > 0).then_some(current.file.size_bytes));
    if total.is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
        return Err("Nexus 文件超过 Acumod 当前允许的 8 GB 下载上限。".to_string());
    }

    let mut output = File::create(&part_path)
        .map_err(|error| format!("无法创建下载暂存文件 {}：{error}", part_path.display()))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_request_error)?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_DOWNLOAD_BYTES {
            let _ = fs::remove_file(&part_path);
            return Err("Nexus 文件超过 Acumod 当前允许的 8 GB 下载上限。".to_string());
        }
        output
            .write_all(&chunk)
            .map_err(|error| format!("写入下载暂存文件 {} 失败：{error}", part_path.display()))?;
        progress.report(
            "正在下载 Nexus 文件",
            downloaded as usize,
            total.map(|value| value as usize),
            Some(current.file.file_name.clone()),
        );
    }
    if response_length.is_some_and(|expected_size| downloaded != expected_size) {
        drop(output);
        let _ = fs::remove_file(&part_path);
        return Err("Nexus 下载未完整结束，已丢弃不完整文件。".to_string());
    }
    output
        .flush()
        .map_err(|error| format!("写入下载暂存文件失败：{error}"))?;
    drop(output);
    fs::rename(&part_path, &final_path)
        .map_err(|error| format!("无法完成下载暂存文件 {}：{error}", final_path.display()))?;
    progress.report(
        "Nexus 文件下载完成",
        downloaded as usize,
        Some(downloaded as usize),
        Some(current.file.file_name),
    );
    Ok(final_path)
}

pub fn page_url(mod_id: u64) -> String {
    format!("https://www.nexusmods.com/monsterhunterworld/mods/{mod_id}")
}

pub fn parse_mod_id_from_url(url: &Url) -> Option<u64> {
    let host = url.host_str()?.to_ascii_lowercase();
    if !matches!(host.as_str(), "nexusmods.com" | "www.nexusmods.com") {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if !segments
        .first()
        .is_some_and(|segment| segment.eq_ignore_ascii_case(NEXUS_GAME_DOMAIN))
    {
        return None;
    }
    let mods_index = segments.iter().position(|segment| *segment == "mods")?;
    segments.get(mods_index + 1)?.parse().ok()
}

fn to_mod_summary(value: NexusModResponse) -> NexusModSummary {
    NexusModSummary {
        mod_id: value.mod_id,
        name: trimmed_text(value.name, 200),
        summary: trimmed_text(value.summary, 600),
        author: trimmed_text(value.author, 120),
        version: trimmed_text(value.version, 80),
        updated_at_unix_seconds: value.updated_timestamp,
        page_url: page_url(value.mod_id),
    }
}

fn to_mod_file(mod_id: u64, value: NexusFileResponse) -> NexusModFile {
    let size_bytes = if value.size_in_bytes > 0 {
        value.size_in_bytes
    } else if value.size_kb > 0 {
        value.size_kb.saturating_mul(1024)
    } else {
        value.size
    };
    NexusModFile {
        mod_id,
        file_id: value.file_id,
        name: trimmed_text(value.name, 200),
        file_name: trimmed_text(value.file_name, 260),
        version: trimmed_text(value.version, 80),
        category_name: trimmed_text(value.category_name, 80),
        size_bytes,
        uploaded_at_unix_seconds: value.uploaded_timestamp,
        description: trimmed_text(value.description, 600),
        is_primary: value.is_primary,
    }
}

async fn validate_current_user() -> Result<NexusUserResponse, String> {
    let key = require_api_key()?;
    validate_user_with_key(&key).await
}

async fn validate_user_with_key(key: &str) -> Result<NexusUserResponse, String> {
    let response = api_get(key, "/users/validate.json").await?;
    read_json_limited(response).await
}

async fn api_get(key: &str, path: &str) -> Result<reqwest::Response, String> {
    let response = api_client()?
        .get(format!("{NEXUS_API_ROOT}{path}"))
        .header("apikey", key)
        .header("Application-Name", "Acumen-MOD-Manager")
        .header("Application-Version", env!("CARGO_PKG_VERSION"))
        .send()
        .await
        .map_err(map_request_error)?;
    ensure_success(response).await
}

async fn read_json_limited<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_API_RESPONSE_BYTES as u64)
    {
        return Err("Nexus API 返回内容过大，已停止处理。".to_string());
    }
    let bytes = response.bytes().await.map_err(map_request_error)?;
    if bytes.len() > MAX_API_RESPONSE_BYTES {
        return Err("Nexus API 返回内容过大，已停止处理。".to_string());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("无法解析 Nexus API 响应：{error}"))
}

fn api_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .user_agent(concat!("Acumen-MOD-Manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("无法初始化 Nexus API 客户端：{error}"))
}

fn download_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("Acumen-MOD-Manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("无法初始化 Nexus 下载客户端：{error}"))
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body).ok().and_then(|value| {
        value
            .get("message")
            .and_then(Value::as_str)
            .map(sanitize_error)
    });
    let summary = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "Nexus API Key 无效、权限不足，或当前会员无法使用这个下载接口。"
        }
        StatusCode::NOT_FOUND => "Nexus MOD 或文件不存在。",
        StatusCode::TOO_MANY_REQUESTS => "Nexus 请求过于频繁，请稍后重试。",
        status if status.is_server_error() => "Nexus 服务暂时不可用，请稍后重试。",
        _ => "Nexus 请求失败。",
    };
    Err(match detail {
        Some(detail) if !detail.is_empty() => format!("{summary} {detail}"),
        _ => summary.to_string(),
    })
}

fn map_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "连接 Nexus 超时，请检查网络后重试。".to_string()
    } else if error.is_connect() {
        "无法连接 Nexus，请检查网络设置。".to_string()
    } else {
        format!("Nexus 网络请求失败：{error}")
    }
}

fn validate_download_url(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("Nexus 下载地址不是 HTTPS，已拒绝连接。".to_string());
    }
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Nexus 下载地址没有有效域名。".to_string())?;
    if host == "nexusmods.com"
        || host.ends_with(".nexusmods.com")
        || host == "nexus-cdn.com"
        || host.ends_with(".nexus-cdn.com")
    {
        Ok(())
    } else {
        Err("Nexus 下载响应跳转到了非受信任域名，已停止下载。".to_string())
    }
}

fn validate_archive_file_name(file_name: &str) -> Result<String, String> {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .ok_or_else(|| "Nexus 文件没有可识别的压缩包扩展名。".to_string())?;
    if matches!(extension.as_str(), "zip" | "7z" | "rar") {
        Ok(extension)
    } else {
        Err("Nexus 文件不是 Acumod 支持的 ZIP、7Z 或 RAR 压缩包。".to_string())
    }
}

fn load_api_key() -> Result<Option<StoredCredential>, String> {
    match keyring_entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => {
            return Ok(Some(StoredCredential {
                key,
                source: "credentialManager".to_string(),
            }));
        }
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(format!("无法读取 Windows 凭据管理器：{error}")),
    }
    Ok(env::var("NEXUS_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .map(|key| StoredCredential {
            key,
            source: "environment".to_string(),
        }))
}

fn require_api_key() -> Result<String, String> {
    load_api_key()?
        .map(|credential| credential.key)
        .ok_or_else(|| "尚未配置 Nexus Personal API Key，请先在设置中保存。".to_string())
}

fn keyring_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| format!("无法访问 Windows 凭据管理器：{error}"))
}

fn validate_api_key(api_key: &str) -> Result<String, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() || api_key.len() > 512 || api_key.chars().any(char::is_whitespace) {
        return Err("Nexus API Key 格式无效。".to_string());
    }
    Ok(api_key.to_string())
}

fn api_key_hint(api_key: &str) -> String {
    let suffix = api_key.chars().rev().take(4).collect::<Vec<_>>();
    let suffix = suffix.into_iter().rev().collect::<String>();
    format!("****{suffix}")
}

fn trimmed_text(value: String, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

fn sanitize_error(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use reqwest::Url;

    use super::{parse_mod_id_from_url, validate_archive_file_name, validate_download_url};

    #[test]
    fn parses_monster_hunter_world_nexus_mod_id() {
        let url = Url::parse("https://www.nexusmods.com/monsterhunterworld/mods/1234").unwrap();
        assert_eq!(parse_mod_id_from_url(&url), Some(1234));
    }

    #[test]
    fn rejects_untrusted_download_host() {
        let url = Url::parse("https://example.com/mod.zip").unwrap();
        assert!(validate_download_url(&url).is_err());
    }

    #[test]
    fn only_supported_archives_can_enter_the_import_bridge() {
        assert_eq!(validate_archive_file_name("example.7z").unwrap(), "7z");
        assert!(validate_archive_file_name("installer.exe").is_err());
    }
}
