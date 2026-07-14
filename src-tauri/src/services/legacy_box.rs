use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
};

use quick_xml::de::from_str;
use serde::{Deserialize, Serialize};

use crate::operations::OperationReporter;

const LEGACY_GAME_SECTION: &str = "582010";
const LEGACY_MODS_DIRECTORY: &str = "Mods_582010";
const MHW_EXECUTABLE: &str = "MonsterHunterWorld.exe";
const FILE_COMPARE_BUFFER_SIZE: usize = 1024 * 1024;

/// 狩技 MOD 盒子中单个安装文件的只读信息。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxModFile {
    pub source_relative_path: String,
    pub file_size_bytes: u64,
}

/// 狩技盒子文件与当前游戏目录的实际比对结果。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxDeploymentStatus {
    pub status: String,
    pub total_file_count: usize,
    pub matching_file_count: usize,
    pub missing_file_count: usize,
    pub different_file_count: usize,
}

/// 狩技 MOD 盒子内可导入的一项 MOD。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxMod {
    pub module_id: String,
    pub name: String,
    pub box_enabled: bool,
    pub box_index: Option<u64>,
    pub mod_type: String,
    pub install_time: String,
    pub install_source: String,
    pub module_path: String,
    pub files_path: String,
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub files: Vec<LegacyBoxModFile>,
    pub deployment: LegacyBoxDeploymentStatus,
}

/// 狩技 MOD 盒子扫描结果；只读取外部目录，不写入任何盒子或游戏文件。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxScan {
    pub box_path: String,
    pub box_game_path: Option<String>,
    pub is_box_game_path_valid: bool,
    pub acumod_game_path: Option<String>,
    pub game_paths_match: Option<bool>,
    pub mods: Vec<LegacyBoxMod>,
    pub warnings: Vec<String>,
    pub message: String,
}

/// 单个狩技盒子 MOD 导入到 Acumod 本地库后的结果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxImportItem {
    pub module_id: String,
    pub name: String,
    pub status: String,
    pub mod_id: Option<String>,
    pub message: String,
}

/// 批量导入狩技盒子 MOD 的汇总结果。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBoxImportResult {
    pub items: Vec<LegacyBoxImportItem>,
    pub imported_count: usize,
    pub already_installed_count: usize,
    pub failed_count: usize,
    pub message: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyBoxInfoXml {
    module_id: String,
    module_name: String,
    name: String,
    enable: String,
    index: String,
    mod_type: String,
    install_time: String,
    install_source: String,
}

struct LegacyBoxPaths {
    box_path: PathBuf,
    mods_path: PathBuf,
    game_path: Option<PathBuf>,
}

pub(crate) struct LegacyBoxImportSource {
    module_id: String,
    name: String,
    module_path: PathBuf,
    files_path: PathBuf,
}

struct CollectedLegacyFile {
    source_path: PathBuf,
    relative_path: PathBuf,
    dto: LegacyBoxModFile,
}

struct PreparedLegacyBoxMod {
    dto: LegacyBoxMod,
    files: Vec<CollectedLegacyFile>,
}

/// 扫描狩技 MOD 盒子，并核验其文件是否仍部署在盒子记录的 MHW 目录中。
pub fn scan_legacy_box_with_progress(
    raw_box_path: String,
    acumod_game_path: Option<String>,
    progress: &OperationReporter,
) -> Result<LegacyBoxScan, String> {
    progress.report("正在读取狩技 MOD 盒子", 0, None, None);
    let paths = resolve_legacy_box_paths(&raw_box_path)?;
    let mut warnings = Vec::new();
    let mut prepared_mods = read_prepared_mods(&paths, &mut warnings, progress)?;
    let is_box_game_path_valid = paths
        .game_path
        .as_deref()
        .is_some_and(is_valid_mhw_game_path);

    if !is_box_game_path_valid {
        warnings.push("狩技 MOD 盒子未记录可用的 MHW 游戏目录，无法核验实际部署文件。".to_string());
    }

    let total_file_count = prepared_mods
        .iter()
        .map(|entry| entry.files.len())
        .sum::<usize>();
    let mut verified_file_count = 0;

    for prepared in &mut prepared_mods {
        prepared.dto.deployment = verify_deployment(
            &prepared.files,
            paths
                .game_path
                .as_deref()
                .filter(|_| is_box_game_path_valid),
            &mut warnings,
            progress,
            &mut verified_file_count,
            total_file_count,
            &prepared.dto.name,
        );
    }

    let box_game_path = paths.game_path.as_ref().map(|path| path_to_string(path));
    let game_paths_match = match (paths.game_path.as_deref(), acumod_game_path.as_deref()) {
        (Some(box_game_path), Some(acumod_game_path)) => {
            Some(paths_equal(box_game_path, Path::new(acumod_game_path)))
        }
        _ => None,
    };
    let mods = prepared_mods
        .into_iter()
        .map(|entry| entry.dto)
        .collect::<Vec<_>>();
    let message = format!("已读取狩技 MOD 盒子的 {} 个 MOD。", mods.len());

    Ok(LegacyBoxScan {
        box_path: path_to_string(&paths.box_path),
        box_game_path,
        is_box_game_path_valid,
        acumod_game_path,
        game_paths_match,
        mods,
        warnings,
        message,
    })
}

/// 重新校验用户选择的盒子模块，返回可交给本地库导入链路的只读来源目录。
pub(crate) fn load_legacy_box_import_sources(
    raw_box_path: &str,
    module_ids: &[String],
) -> Result<Vec<LegacyBoxImportSource>, String> {
    if module_ids.is_empty() {
        return Err("请至少选择一个狩技 MOD 盒子中的 MOD。".to_string());
    }

    let paths = resolve_legacy_box_paths(raw_box_path)?;
    let mut seen_ids = HashSet::new();
    let mut sources = Vec::new();

    for module_id in module_ids {
        if !seen_ids.insert(module_id.clone()) {
            continue;
        }
        if !is_valid_module_id(module_id) {
            return Err(format!("无效的狩技 MOD 模块 ID：{module_id}"));
        }

        let module_path = canonical_child_directory(&paths.mods_path, module_id, "MOD 模块目录")?;
        let info = read_info_xml(&module_path.join("info.xml"))?;
        let files_path = canonical_child_directory(&module_path, "files", "MOD 文件目录")?;
        let name = display_name(&info, module_id);

        if !contains_regular_file(&files_path)? {
            return Err(format!("狩技 MOD “{name}” 不包含可导入文件。"));
        }

        sources.push(LegacyBoxImportSource {
            module_id: module_id.clone(),
            name,
            module_path,
            files_path,
        });
    }

    if sources.is_empty() {
        return Err("没有可导入的狩技 MOD 模块。".to_string());
    }

    Ok(sources)
}

pub(crate) fn import_source_module_id(source: &LegacyBoxImportSource) -> &str {
    &source.module_id
}

pub(crate) fn import_source_name(source: &LegacyBoxImportSource) -> &str {
    &source.name
}

pub(crate) fn import_source_module_path(source: &LegacyBoxImportSource) -> &Path {
    &source.module_path
}

pub(crate) fn import_source_files_path(source: &LegacyBoxImportSource) -> &Path {
    &source.files_path
}

fn resolve_legacy_box_paths(raw_box_path: &str) -> Result<LegacyBoxPaths, String> {
    let requested_path = normalize_user_path(raw_box_path);
    if requested_path.as_os_str().is_empty() {
        return Err("请选择狩技 MOD 盒子目录。".to_string());
    }
    let box_path = requested_path.canonicalize().map_err(|error| {
        format!(
            "无法读取狩技 MOD 盒子目录 {}：{error}",
            requested_path.display()
        )
    })?;
    if !box_path.is_dir() {
        return Err("狩技 MOD 盒子路径不是目录。".to_string());
    }

    let mods_path =
        canonical_child_directory(&box_path, LEGACY_MODS_DIRECTORY, "Mods_582010 目录")?;
    let game_path = read_legacy_game_path(&box_path.join("config.ini"))?;

    Ok(LegacyBoxPaths {
        box_path,
        mods_path,
        game_path,
    })
}

fn read_legacy_game_path(config_path: &Path) -> Result<Option<PathBuf>, String> {
    let contents = match fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "无法读取狩技 MOD 盒子配置 {}：{error}",
                config_path.display()
            ));
        }
    };

    let mut current_section = String::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        if !current_section.eq_ignore_ascii_case(LEGACY_GAME_SECTION) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("gamepath") && !value.trim().is_empty() {
            return Ok(Some(normalize_user_path(value)));
        }
    }

    Ok(None)
}

fn read_prepared_mods(
    paths: &LegacyBoxPaths,
    warnings: &mut Vec<String>,
    progress: &OperationReporter,
) -> Result<Vec<PreparedLegacyBoxMod>, String> {
    let mut module_directories = fs::read_dir(&paths.mods_path)
        .map_err(|error| format!("无法读取 Mods_582010 目录：{error}"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            (!metadata.file_type().is_symlink() && metadata.is_dir()).then_some(path)
        })
        .collect::<Vec<_>>();
    module_directories.sort_by_key(|path| path_to_string(path).to_lowercase());

    let module_count = module_directories.len();
    let mut prepared = Vec::new();
    for (index, path) in module_directories.into_iter().enumerate() {
        let Some(directory_name) = path.file_name().and_then(|name| name.to_str()) else {
            warnings.push(format!("已跳过无法读取名称的盒子目录：{}", path.display()));
            continue;
        };
        if !is_valid_module_id(directory_name) {
            warnings.push(format!("已跳过非模块目录：{}", path.display()));
            continue;
        }

        progress.report(
            "正在读取狩技 MOD 记录",
            index + 1,
            Some(module_count),
            Some(directory_name.to_string()),
        );
        let module_path = match path.canonicalize() {
            Ok(path) if path.starts_with(&paths.mods_path) => path,
            Ok(_) => {
                warnings.push(format!("已跳过目录范围异常的模块：{}", path.display()));
                continue;
            }
            Err(error) => {
                warnings.push(format!("无法读取模块 {}：{error}", path.display()));
                continue;
            }
        };
        let info = match read_info_xml(&module_path.join("info.xml")) {
            Ok(info) => info,
            Err(error) => {
                warnings.push(format!("已跳过 {}：{error}", directory_name));
                continue;
            }
        };
        if !info.module_id.trim().is_empty() && info.module_id.trim() != directory_name {
            warnings.push(format!(
                "模块 {} 的目录 ID 与 info.xml 中的 ID 不一致，已按目录 ID 读取。",
                directory_name
            ));
        }
        let files_path = match canonical_child_directory(&module_path, "files", "MOD 文件目录")
        {
            Ok(path) => path,
            Err(error) => {
                warnings.push(format!("已跳过 {}：{error}", directory_name));
                continue;
            }
        };
        let files = match collect_files(&files_path, progress) {
            Ok(files) => files,
            Err(error) => {
                warnings.push(format!("无法读取 {} 的文件清单：{error}", directory_name));
                continue;
            }
        };
        let file_count = files.len();
        let total_size_bytes = files.iter().map(|file| file.dto.file_size_bytes).sum();
        let dto_files = files.iter().map(|file| file.dto.clone()).collect();
        prepared.push(PreparedLegacyBoxMod {
            dto: LegacyBoxMod {
                module_id: directory_name.to_string(),
                name: display_name(&info, directory_name),
                box_enabled: info.enable.trim().eq_ignore_ascii_case("true"),
                box_index: info.index.trim().parse().ok(),
                mod_type: info.mod_type.trim().to_string(),
                install_time: info.install_time.trim().to_string(),
                install_source: info.install_source.trim().to_string(),
                module_path: path_to_string(&module_path),
                files_path: path_to_string(&files_path),
                file_count,
                total_size_bytes,
                files: dto_files,
                deployment: unavailable_status(file_count),
            },
            files,
        });
    }

    prepared.sort_by(|left, right| {
        left.dto
            .box_index
            .unwrap_or(u64::MAX)
            .cmp(&right.dto.box_index.unwrap_or(u64::MAX))
            .then_with(|| {
                left.dto
                    .name
                    .to_lowercase()
                    .cmp(&right.dto.name.to_lowercase())
            })
    });
    Ok(prepared)
}

fn read_info_xml(info_path: &Path) -> Result<LegacyBoxInfoXml, String> {
    let contents =
        fs::read_to_string(info_path).map_err(|error| format!("无法读取 info.xml：{error}"))?;
    from_str(&contents).map_err(|error| format!("无法解析 info.xml：{error}"))
}

fn display_name(info: &LegacyBoxInfoXml, fallback: &str) -> String {
    [info.module_name.trim(), info.name.trim(), fallback]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn collect_files(
    root: &Path,
    progress: &OperationReporter,
) -> Result<Vec<CollectedLegacyFile>, String> {
    let mut files = Vec::new();
    collect_files_from_directory(root, root, &mut files, progress)?;
    files.sort_by_key(|file| file.dto.source_relative_path.to_lowercase());
    Ok(files)
}

fn collect_files_from_directory(
    root: &Path,
    directory: &Path,
    files: &mut Vec<CollectedLegacyFile>,
    progress: &OperationReporter,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("无法读取目录 {}：{error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("无法读取目录项：{error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("无法读取文件属性 {}：{error}", path.display()))?;

        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files_from_directory(root, &path, files, progress)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(root)
            .map_err(|error| format!("无法计算 {} 的相对路径：{error}", path.display()))?;
        let source_relative_path = safe_relative_path(relative_path)?;
        files.push(CollectedLegacyFile {
            source_path: path.clone(),
            relative_path: relative_path.to_path_buf(),
            dto: LegacyBoxModFile {
                source_relative_path: source_relative_path.clone(),
                file_size_bytes: metadata.len(),
            },
        });
        progress.report(
            "正在读取狩技 MOD 文件",
            files.len(),
            None,
            Some(source_relative_path),
        );
    }
    Ok(())
}

fn verify_deployment(
    files: &[CollectedLegacyFile],
    game_path: Option<&Path>,
    warnings: &mut Vec<String>,
    progress: &OperationReporter,
    verified_file_count: &mut usize,
    total_file_count: usize,
    mod_name: &str,
) -> LegacyBoxDeploymentStatus {
    let Some(game_path) = game_path else {
        return unavailable_status(files.len());
    };

    let mut matching_file_count = 0;
    let mut missing_file_count = 0;
    let mut different_file_count = 0;
    for file in files {
        *verified_file_count += 1;
        progress.report(
            "正在核验游戏目录文件",
            *verified_file_count,
            Some(total_file_count),
            Some(format!("{mod_name} · {}", file.dto.source_relative_path)),
        );
        let target_path = game_path.join(&file.relative_path);
        if !target_path.is_file() {
            missing_file_count += 1;
            continue;
        }
        match files_are_equal(&file.source_path, &target_path) {
            Ok(true) => matching_file_count += 1,
            Ok(false) => different_file_count += 1,
            Err(error) => {
                different_file_count += 1;
                warnings.push(format!(
                    "无法核验 {} 的游戏文件 {}：{error}",
                    mod_name,
                    target_path.display()
                ));
            }
        }
    }

    let status = if files.is_empty() {
        "notDeployed"
    } else if matching_file_count == files.len() {
        "fullyMatched"
    } else if matching_file_count > 0 {
        "partiallyMatched"
    } else if missing_file_count == files.len() {
        "notDeployed"
    } else {
        "different"
    };

    LegacyBoxDeploymentStatus {
        status: status.to_string(),
        total_file_count: files.len(),
        matching_file_count,
        missing_file_count,
        different_file_count,
    }
}

fn unavailable_status(file_count: usize) -> LegacyBoxDeploymentStatus {
    LegacyBoxDeploymentStatus {
        status: "unavailable".to_string(),
        total_file_count: file_count,
        matching_file_count: 0,
        missing_file_count: 0,
        different_file_count: 0,
    }
}

fn files_are_equal(source_path: &Path, target_path: &Path) -> Result<bool, String> {
    let source_size = fs::metadata(source_path)
        .map_err(|error| format!("无法读取源文件：{error}"))?
        .len();
    let target_size = fs::metadata(target_path)
        .map_err(|error| format!("无法读取目标文件：{error}"))?
        .len();
    if source_size != target_size {
        return Ok(false);
    }

    let mut source = BufReader::new(
        File::open(source_path).map_err(|error| format!("无法打开源文件：{error}"))?,
    );
    let mut target = BufReader::new(
        File::open(target_path).map_err(|error| format!("无法打开目标文件：{error}"))?,
    );
    let mut source_buffer = vec![0; FILE_COMPARE_BUFFER_SIZE];
    let mut target_buffer = vec![0; FILE_COMPARE_BUFFER_SIZE];

    loop {
        let source_read = source
            .read(&mut source_buffer)
            .map_err(|error| format!("无法读取源文件：{error}"))?;
        let target_read = target
            .read(&mut target_buffer)
            .map_err(|error| format!("无法读取目标文件：{error}"))?;
        if source_read != target_read {
            return Ok(false);
        }
        if source_read == 0 {
            return Ok(true);
        }
        if source_buffer[..source_read] != target_buffer[..target_read] {
            return Ok(false);
        }
    }
}

fn canonical_child_directory(root: &Path, child: &str, label: &str) -> Result<PathBuf, String> {
    let path = root.join(child);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("无法读取{label} {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label}不能是符号链接。"));
    }
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("无法读取{label} {}：{error}", path.display()))?;
    if !canonical_path.is_dir() {
        return Err(format!("{label}不是目录：{}", canonical_path.display()));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("无法读取目录 {}：{error}", root.display()))?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(format!("{label}不在允许的目录范围内。"));
    }
    Ok(canonical_path)
}

fn contains_regular_file(root: &Path) -> Result<bool, String> {
    for entry in
        fs::read_dir(root).map_err(|error| format!("无法读取目录 {}：{error}", root.display()))?
    {
        let entry = entry.map_err(|error| format!("无法读取目录项：{error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("无法读取文件属性 {}：{error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() || (metadata.is_dir() && contains_regular_file(&path)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn safe_relative_path(path: &Path) -> Result<String, String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => segments.push(value.to_string_lossy().to_string()),
            _ => return Err(format!("文件路径不安全：{}", path.display())),
        }
    }
    if segments.is_empty() {
        return Err("文件路径为空。".to_string());
    }
    Ok(segments.join("/"))
}

fn is_valid_mhw_game_path(path: &Path) -> bool {
    path.is_dir() && path.join(MHW_EXECUTABLE).is_file()
}

fn is_valid_module_id(module_id: &str) -> bool {
    !module_id.is_empty() && module_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn normalize_user_path(path: &str) -> PathBuf {
    PathBuf::from(path.trim().trim_matches('"'))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{load_legacy_box_import_sources, scan_legacy_box_with_progress};
    use crate::operations::OperationReporter;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("acumod_legacy_box_{name}_{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn scans_box_metadata_and_verifies_matching_game_files() {
        let root = temp_root("scan");
        let game_root = root.join("game");
        fs::create_dir_all(game_root.join("nativePC/weapon")).unwrap();
        fs::write(game_root.join("MonsterHunterWorld.exe"), b"exe").unwrap();
        fs::write(game_root.join("nativePC/weapon/test.bin"), b"same").unwrap();
        let module_root = root.join("Mods_582010/1001/files/nativePC/weapon");
        fs::create_dir_all(&module_root).unwrap();
        fs::write(module_root.join("test.bin"), b"same").unwrap();
        fs::write(
            root.join("Mods_582010/1001/info.xml"),
            "<ModInfo><moduleId>1001</moduleId><moduleName>测试 MOD</moduleName><enable>true</enable><index>2</index></ModInfo>",
        )
        .unwrap();
        fs::write(
            root.join("config.ini"),
            format!("[582010]\ngamepath={}\n", game_root.display()),
        )
        .unwrap();

        let scan = scan_legacy_box_with_progress(
            root.to_string_lossy().to_string(),
            Some(game_root.to_string_lossy().to_string()),
            &OperationReporter::default(),
        )
        .unwrap();

        assert_eq!(scan.mods.len(), 1);
        assert!(scan.mods[0].box_enabled);
        assert_eq!(scan.mods[0].deployment.status, "fullyMatched");
        assert_eq!(
            scan.mods[0].files[0].source_relative_path,
            "nativePC/weapon/test.bin"
        );
        assert_eq!(scan.game_paths_match, Some(true));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_sources_only_accept_numeric_module_directories() {
        let root = temp_root("sources");
        let module_root = root.join("Mods_582010/88/files/nativePC");
        fs::create_dir_all(&module_root).unwrap();
        fs::write(module_root.join("test.bin"), b"mod").unwrap();
        fs::write(
            root.join("Mods_582010/88/info.xml"),
            "<ModInfo><moduleId>88</moduleId><name>来源测试</name></ModInfo>",
        )
        .unwrap();

        let sources =
            load_legacy_box_import_sources(&root.to_string_lossy(), &["88".to_string()]).unwrap();
        assert_eq!(sources.len(), 1);
        assert!(
            load_legacy_box_import_sources(&root.to_string_lossy(), &["../88".to_string()],)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
