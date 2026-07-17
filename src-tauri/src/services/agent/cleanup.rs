use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    operations::OperationReporter,
    services::mod_library::{self, ModCleanupCandidate},
};

use super::AgentCoordinator;

static NEXT_REVIEW_ID: AtomicU64 = AtomicU64::new(1);
const MAX_CLEANUP_REVIEW_ITEMS: usize = 2_000;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCleanupReview {
    pub review_id: String,
    pub candidate_count: usize,
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
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupClassification {
    pub candidate_id: String,
    pub recommendation: String,
    pub reason: String,
    pub confidence: f64,
}

pub fn scan_candidates(app: &AppHandle) -> Result<Vec<ModCleanupCandidate>, String> {
    mod_library::scan_mod_cleanup_candidates_with_progress(app, &OperationReporter::default())
        .map(|scan| scan.candidates)
}

pub fn create_review(
    app: &AppHandle,
    coordinator: &AgentCoordinator,
    classifications: Vec<CleanupClassification>,
) -> Result<AgentCleanupReview, String> {
    let candidates = scan_candidates(app)?;
    if candidates.is_empty() {
        return Err("当前没有可供分类的清理候选。".to_string());
    }
    if candidates.len() > MAX_CLEANUP_REVIEW_ITEMS {
        return Err(format!(
            "候选数量超过 {MAX_CLEANUP_REVIEW_ITEMS}，请先缩小本地 MOD 库范围。"
        ));
    }
    if classifications.len() != candidates.len() {
        return Err(format!(
            "必须分类全部 {} 个候选，当前只提交了 {} 个。",
            candidates.len(),
            classifications.len()
        ));
    }

    let classifications = classifications
        .into_iter()
        .map(|classification| (classification.candidate_id.clone(), classification))
        .collect::<HashMap<_, _>>();
    if classifications.len() != candidates.len() {
        return Err("分类结果包含重复候选。".to_string());
    }

    let mut items = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let classification = classifications
            .get(&candidate.candidate_id)
            .ok_or_else(|| format!("缺少候选分类：{}", candidate.candidate_id))?;
        if !matches!(
            classification.recommendation.as_str(),
            "remove" | "review" | "keep"
        ) {
            return Err("分类结果只能是“建议清理”“需要确认”或“建议保留”。".to_string());
        }
        if !classification.confidence.is_finite()
            || !(0.0..=1.0).contains(&classification.confidence)
        {
            return Err("分类可信度必须位于 0 到 1。".to_string());
        }
        let reason = classification
            .reason
            .trim()
            .chars()
            .take(300)
            .collect::<String>();
        if reason.is_empty() {
            return Err("每个清理候选都必须提供简短理由。".to_string());
        }
        items.push(AgentCleanupReviewItem {
            candidate_id: candidate.candidate_id,
            mod_id: candidate.mod_id,
            mod_name: candidate.mod_name,
            library_relative_path: candidate.library_relative_path,
            deploy_relative_path: candidate.deploy_relative_path,
            size_bytes: candidate.size_bytes,
            currently_deployed: candidate.currently_deployed,
            recommendation: classification.recommendation.clone(),
            reason,
            confidence: classification.confidence,
            selected_by_default: classification.recommendation == "remove"
                && classification.confidence >= 0.85,
        });
    }

    let review = AgentCleanupReview {
        review_id: format!(
            "cleanup-review-{}",
            NEXT_REVIEW_ID.fetch_add(1, Ordering::Relaxed)
        ),
        candidate_count: items.len(),
        items,
        message: "AI 分类已完成，请逐项确认要排除的文件。".to_string(),
    };
    coordinator.store_cleanup_review(review.clone())?;
    Ok(review)
}
