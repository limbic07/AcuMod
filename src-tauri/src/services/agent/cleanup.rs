use std::{
    collections::{BTreeMap, HashMap},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    operations::OperationReporter,
    services::mod_library::{self, ModCleanupCandidate, ModCleanupScan},
};

use super::AgentCoordinator;

static NEXT_AUDIT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_REVIEW_ID: AtomicU64 = AtomicU64::new(1);
const MAX_CLEANUP_REVIEW_ITEMS: usize = 2_000;
const MAX_AI_GROUP_FILES: usize = 40;

#[derive(Clone)]
pub struct AgentCleanupAudit {
    pub audit_id: String,
    pub scan: ModCleanupScan,
    pub ai_groups: Vec<AgentCleanupAiGroup>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCleanupAiGroup {
    pub group_id: String,
    pub mod_id: String,
    pub mod_name: String,
    pub directory: String,
    pub extension: String,
    pub risk_level: String,
    pub keep_signals: Vec<String>,
    pub exclude_signals: Vec<String>,
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub files: Vec<AgentCleanupAiFile>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCleanupAiFile {
    pub candidate_id: String,
    pub file_name: String,
    pub library_relative_path: String,
    pub size_bytes: u64,
    pub currently_deployed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCleanupReview {
    pub review_id: String,
    pub candidate_count: usize,
    pub scanned_file_count: usize,
    pub local_keep_count: usize,
    pub local_remove_count: usize,
    pub ai_review_count: usize,
    pub ai_group_count: usize,
    pub rule_version: u32,
    pub items: Vec<AgentCleanupReviewItem>,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCleanupReviewItem {
    pub candidate_id: String,
    pub mod_id: String,
    pub mod_name: String,
    pub library_relative_path: String,
    pub deploy_relative_path: String,
    pub size_bytes: u64,
    pub currently_deployed: bool,
    pub recommendation: String,
    pub reason: String,
    pub confidence: f64,
    pub selected_by_default: bool,
    pub decision_source: String,
    pub risk_level: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupClassification {
    pub group_id: String,
    pub recommendation: String,
    pub reason: String,
    pub confidence: f64,
}

/// 在统一后台任务中扫描，进度会直接驱动主界面的全局进度条。
pub fn scan_audit(
    app: &AppHandle,
    progress: &OperationReporter,
) -> Result<AgentCleanupAudit, String> {
    let scan = mod_library::scan_mod_cleanup_candidates_with_progress(app, progress)?;
    if scan.candidates.len() > MAX_CLEANUP_REVIEW_ITEMS {
        return Err(format!(
            "待确认文件超过 {MAX_CLEANUP_REVIEW_ITEMS} 个，请先缩小本地 MOD 库范围。"
        ));
    }
    let ai_groups = build_ai_groups(&scan.candidates);
    Ok(AgentCleanupAudit {
        audit_id: format!(
            "cleanup-audit-{}",
            NEXT_AUDIT_ID.fetch_add(1, Ordering::Relaxed)
        ),
        scan,
        ai_groups,
    })
}

pub fn create_review(
    coordinator: &AgentCoordinator,
    audit_id: &str,
    classifications: Vec<CleanupClassification>,
) -> Result<AgentCleanupReview, String> {
    let audit = coordinator.get_cleanup_audit(audit_id)?;
    if audit.scan.candidates.is_empty() {
        return Err("本地规则和 AcuAI 都没有发现需要确认的文件。".to_string());
    }
    if audit.scan.candidates.len() > MAX_CLEANUP_REVIEW_ITEMS {
        return Err(format!(
            "待确认文件超过 {MAX_CLEANUP_REVIEW_ITEMS} 个，请缩小本地 MOD 库范围。"
        ));
    }
    if classifications.len() != audit.ai_groups.len() {
        return Err(format!(
            "必须分类全部 {} 个模糊文件组，当前只提交了 {} 个。",
            audit.ai_groups.len(),
            classifications.len()
        ));
    }

    let classifications = classifications
        .into_iter()
        .map(|classification| (classification.group_id.clone(), classification))
        .collect::<HashMap<_, _>>();
    if classifications.len() != audit.ai_groups.len() {
        return Err("分类结果包含重复文件组。".to_string());
    }

    let candidates = audit
        .scan
        .candidates
        .iter()
        .map(|candidate| (candidate.candidate_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let mut items = audit
        .scan
        .candidates
        .iter()
        .filter(|candidate| candidate.review_source == "localRule")
        .map(local_rule_review_item)
        .collect::<Vec<_>>();

    for group in &audit.ai_groups {
        let classification = classifications
            .get(&group.group_id)
            .ok_or_else(|| format!("缺少文件组分类：{}", group.group_id))?;
        validate_classification(classification)?;
        for file in &group.files {
            let candidate = candidates
                .get(file.candidate_id.as_str())
                .ok_or_else(|| "清理审查快照中的文件组已经损坏。".to_string())?;
            items.push(ai_review_item(candidate, classification));
        }
    }

    items.sort_by(|left, right| {
        left.mod_name
            .to_lowercase()
            .cmp(&right.mod_name.to_lowercase())
            .then_with(|| {
                left.library_relative_path
                    .to_lowercase()
                    .cmp(&right.library_relative_path.to_lowercase())
            })
    });
    // “保留”项只作为扫描计数，不占用用户的清理决策列表。
    items.retain(|item| item.recommendation != "keep");
    let cleanup_item_count = items.len();
    let review = AgentCleanupReview {
        review_id: format!(
            "cleanup-review-{}",
            NEXT_REVIEW_ID.fetch_add(1, Ordering::Relaxed)
        ),
        candidate_count: items.len(),
        scanned_file_count: audit.scan.scanned_file_count,
        local_keep_count: audit.scan.local_keep_count,
        local_remove_count: audit.scan.local_remove_count,
        ai_review_count: audit.scan.ai_review_count,
        ai_group_count: audit.ai_groups.len(),
        rule_version: audit.scan.rule_version,
        items,
        message: format!(
            "已盘点 {} 个文件，发现 {} 个可确认清理项。",
            audit.scan.scanned_file_count, cleanup_item_count,
        ),
    };
    coordinator.take_cleanup_audit(audit_id)?;
    coordinator.store_cleanup_review(review.clone())?;
    Ok(review)
}

pub fn read_text_preview(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    audit_id: &str,
    candidate_id: &str,
) -> Result<mod_library::ModCleanupTextPreview, String> {
    let audit = coordinator.get_cleanup_audit(audit_id)?;
    let candidate = audit
        .scan
        .candidates
        .iter()
        .find(|candidate| {
            candidate.review_source == "acuAi" && candidate.candidate_id == candidate_id
        })
        .ok_or_else(|| "该文件不属于当前 AcuAI 审查范围。".to_string())?;
    mod_library::read_mod_cleanup_text_preview(
        app,
        candidate.mod_id.clone(),
        candidate.candidate_id.clone(),
    )
}

fn build_ai_groups(candidates: &[ModCleanupCandidate]) -> Vec<AgentCleanupAiGroup> {
    let mut grouped = BTreeMap::<String, Vec<&ModCleanupCandidate>>::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.review_source == "acuAi")
    {
        let directory = cleanup_parent_directory(&candidate.deploy_relative_path);
        let key = format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            candidate.mod_id,
            directory.to_lowercase(),
            candidate.extension,
            candidate.risk_level,
            candidate.keep_signals.join("\u{1f}"),
            candidate.exclude_signals.join("\u{1f}")
        );
        grouped.entry(key).or_default().push(candidate);
    }

    let mut groups = Vec::new();
    for (_, candidates) in grouped {
        for chunk in candidates.chunks(MAX_AI_GROUP_FILES) {
            let first = chunk[0];
            let files = chunk
                .iter()
                .map(|candidate| AgentCleanupAiFile {
                    candidate_id: candidate.candidate_id.clone(),
                    file_name: cleanup_file_name(&candidate.deploy_relative_path),
                    library_relative_path: candidate.library_relative_path.clone(),
                    size_bytes: candidate.size_bytes,
                    currently_deployed: candidate.currently_deployed,
                })
                .collect::<Vec<_>>();
            let group_id = cleanup_group_id(files.iter().map(|file| file.candidate_id.as_str()));
            groups.push(AgentCleanupAiGroup {
                group_id,
                mod_id: first.mod_id.clone(),
                mod_name: first.mod_name.clone(),
                directory: cleanup_parent_directory(&first.deploy_relative_path),
                extension: first.extension.clone(),
                risk_level: first.risk_level.clone(),
                keep_signals: first.keep_signals.clone(),
                exclude_signals: first.exclude_signals.clone(),
                file_count: files.len(),
                total_size_bytes: files.iter().map(|file| file.size_bytes).sum(),
                files,
            });
        }
    }
    groups.sort_by(|left, right| {
        left.mod_name
            .to_lowercase()
            .cmp(&right.mod_name.to_lowercase())
            .then_with(|| {
                left.directory
                    .to_lowercase()
                    .cmp(&right.directory.to_lowercase())
            })
            .then_with(|| left.extension.cmp(&right.extension))
    });
    groups
}

fn local_rule_review_item(candidate: &ModCleanupCandidate) -> AgentCleanupReviewItem {
    AgentCleanupReviewItem {
        candidate_id: candidate.candidate_id.clone(),
        mod_id: candidate.mod_id.clone(),
        mod_name: candidate.mod_name.clone(),
        library_relative_path: candidate.library_relative_path.clone(),
        deploy_relative_path: candidate.deploy_relative_path.clone(),
        size_bytes: candidate.size_bytes,
        currently_deployed: candidate.currently_deployed,
        recommendation: "remove".to_string(),
        reason: candidate.local_hint.clone(),
        confidence: 0.99,
        selected_by_default: candidate.risk_level == "low",
        decision_source: "localRule".to_string(),
        risk_level: candidate.risk_level.clone(),
    }
}

fn ai_review_item(
    candidate: &ModCleanupCandidate,
    classification: &CleanupClassification,
) -> AgentCleanupReviewItem {
    AgentCleanupReviewItem {
        candidate_id: candidate.candidate_id.clone(),
        mod_id: candidate.mod_id.clone(),
        mod_name: candidate.mod_name.clone(),
        library_relative_path: candidate.library_relative_path.clone(),
        deploy_relative_path: candidate.deploy_relative_path.clone(),
        size_bytes: candidate.size_bytes,
        currently_deployed: candidate.currently_deployed,
        recommendation: classification.recommendation.clone(),
        reason: classification.reason.trim().chars().take(300).collect(),
        confidence: classification.confidence,
        // 模糊项即使由模型建议排除也不自动勾选，避免把不确定性转化为静默操作。
        selected_by_default: false,
        decision_source: "acuAi".to_string(),
        risk_level: candidate.risk_level.clone(),
    }
}

fn validate_classification(classification: &CleanupClassification) -> Result<(), String> {
    if !matches!(
        classification.recommendation.as_str(),
        "remove" | "review" | "keep"
    ) {
        return Err("分类结果只能是“建议清理”“需要确认”或“建议保留”。".to_string());
    }
    if !classification.confidence.is_finite() || !(0.0..=1.0).contains(&classification.confidence) {
        return Err("分类可信度必须位于 0 到 1。".to_string());
    }
    if classification.reason.trim().is_empty() {
        return Err("每个模糊文件组都必须提供简短理由。".to_string());
    }
    Ok(())
}

fn cleanup_parent_directory(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit_once('/')
        .map(|(directory, _)| directory.to_string())
        .unwrap_or_else(|| ".".to_string())
}

fn cleanup_file_name(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn cleanup_group_id<'a>(candidate_ids: impl Iterator<Item = &'a str>) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for candidate_id in candidate_ids {
        for byte in candidate_id.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("cleanup-group-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::{build_ai_groups, ModCleanupCandidate};

    fn candidate(path: &str) -> ModCleanupCandidate {
        ModCleanupCandidate {
            candidate_id: format!("id-{path}"),
            mod_id: "mod-1".to_string(),
            mod_name: "测试 MOD".to_string(),
            library_relative_path: path.to_string(),
            deploy_relative_path: path.to_string(),
            extension: "dds".to_string(),
            size_bytes: 10,
            local_kind: "ambiguous".to_string(),
            local_hint: "用途不明".to_string(),
            currently_deployed: false,
            review_source: "acuAi".to_string(),
            risk_level: "medium".to_string(),
            keep_signals: Vec::new(),
            exclude_signals: Vec::new(),
        }
    }

    #[test]
    fn groups_ambiguous_files_by_directory_and_extension() {
        let groups = build_ai_groups(&[
            candidate("nativePC/wp/a/first.dds"),
            candidate("nativePC/wp/a/second.dds"),
            candidate("nativePC/wp/b/third.dds"),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].file_count, 2);
        assert_eq!(groups[1].file_count, 1);
    }
}
