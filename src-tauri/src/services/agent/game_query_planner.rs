use std::{
    collections::{BTreeMap, HashSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    services::{knowledge, mhwdata},
    storage::config::DeepSeekModel,
};

use super::{tools, AgentKnowledgeEvidence};

const DEEPSEEK_CHAT_URL: &str = "https://api.deepseek.com/chat/completions";
const MAX_ENTITY_PLANS: usize = 4;
const MAX_SEARCHES_PER_ENTITY: usize = 6;
const MAX_KINDS_PER_ENTITY: usize = 3;
const MAX_SECTIONS_PER_ENTITY: usize = 8;
const MAX_CANDIDATES_PER_ENTITY: usize = 3;
const MAX_RELATIONS_PER_ENTITY: usize = 24;
const MAX_MODEL_RECORD_BYTES: usize = 16 * 1024;
const MAX_MODEL_CONTEXT_BYTES: usize = 96 * 1024;
const EXACT_MATCH_SCORE: u16 = 95;
const AMBIGUITY_SCORE_GAP: u16 = 5;
const PLAN_CACHE_TTL: Duration = Duration::from_secs(120);
const MAX_PLAN_CACHE_ENTRIES: usize = 64;

const PLANNER_SYSTEM_PROMPT: &str = r#"你是 AcuAI 的 MHW 本地资料检索规划器，只做语义解析，绝不回答用户问题。
你的任务是判断用户这一句是否需要 MHW 游戏资料，并把其中的玩家简称、繁中、英文或社区俗称改写成可供本地 MHWData 验证的候选查询。你不知道也不能声称任何游戏事实、数值、稳定 ID、版本、网页链接或资料来源。
只返回一个 JSON 对象，不要 Markdown，不要解释。JSON 严格使用：
{
  \"route\": \"gameData\" | \"guide\" | \"mixed\" | \"other\",
  \"entities\": [{
    \"mention\": \"用户原始称呼\",
    \"kindHints\": [\"monster\" | \"weapon\" | \"armor\" | \"armorSet\" | \"skill\" | \"item\" | \"quest\" | \"decoration\" | \"charm\" | \"kinsect\" | \"tool\" | \"location\"],
    \"searches\": [\"最多六个候选名称\"],
    \"sections\": [\"需要读取的固定 MHWData section\"],
    \"includeArmorSetCrafting\": false
  }]
}
若不涉及 MHW 游戏资料，route 为 other 且 entities 为空。用户问肉质、掉落、技能、装备属性、制作材料、任务报酬等精确数据时，route 为 gameData 或 mixed，并提供实体与 section。用户问整套防具制作材料时，armorSet 使用 includeArmorSetCrafting=true。名称有歧义时保留多个搜索候选，不能擅自选定一个。"#;

const ALLOWED_ENTITY_KINDS: &[&str] = &[
    "armor",
    "armorSet",
    "armorSetBonus",
    "charm",
    "decoration",
    "dropRateTable",
    "item",
    "kinsect",
    "location",
    "monster",
    "quest",
    "skill",
    "tool",
    "weapon",
];

const ALLOWED_SECTIONS: &[&str] = &[
    "armor.base",
    "armor.crafting",
    "armor.skills",
    "armor.translation",
    "armorSet.base",
    "armorSet.translation",
    "armorSetBonus.base",
    "armorSetBonus.translation",
    "charm.base",
    "charm.crafting",
    "charm.translation",
    "decoration.base",
    "decoration.dropRates",
    "decoration.translation",
    "item.base",
    "item.combination",
    "item.translation",
    "kinsect.base",
    "kinsect.crafting",
    "kinsect.translation",
    "location.base",
    "location.camps",
    "location.gatheringStacks",
    "location.items",
    "monster.ailments",
    "monster.base",
    "monster.breaks",
    "monster.habitats",
    "monster.hitzones",
    "monster.rewardCondition",
    "monster.rewards",
    "monster.translation",
    "monster.weaknesses",
    "quest.base",
    "quest.monsters",
    "quest.rewards",
    "quest.translation",
    "skill.base",
    "skill.levels",
    "skill.translation",
    "tool.base",
    "tool.translation",
    "weapon.ammo",
    "weapon.base",
    "weapon.bow",
    "weapon.crafting",
    "weapon.melody",
    "weapon.melodyNotes",
    "weapon.sharpness",
    "weapon.translation",
];

#[derive(Clone)]
pub(crate) struct VerifiedGameContext {
    pub model_context: String,
    pub knowledge_evidence: Vec<AgentKnowledgeEvidence>,
}

#[derive(Clone)]
struct CachedPlan {
    expires_at: Instant,
    plan: ValidatedPlan,
}

static PLAN_CACHE: OnceLock<Mutex<BTreeMap<String, CachedPlan>>> = OnceLock::new();

#[derive(Deserialize)]
struct PlannerCompletionResponse {
    #[serde(default)]
    choices: Vec<PlannerChoice>,
}

#[derive(Deserialize)]
struct PlannerChoice {
    message: PlannerMessage,
}

#[derive(Deserialize)]
struct PlannerMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPlannerResult {
    route: String,
    #[serde(default)]
    entities: Vec<RawEntityPlan>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEntityPlan {
    mention: String,
    #[serde(default)]
    kind_hints: Vec<String>,
    #[serde(default)]
    searches: Vec<String>,
    #[serde(default)]
    sections: Vec<String>,
    #[serde(default)]
    include_armor_set_crafting: bool,
}

#[derive(Clone)]
struct ValidatedPlan {
    route: String,
    entities: Vec<EntityPlan>,
}

#[derive(Clone)]
struct EntityPlan {
    mention: String,
    kind_hints: Vec<String>,
    searches: Vec<String>,
    sections: Vec<String>,
    include_armor_set_crafting: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelContext {
    route: String,
    entities: Vec<ModelEntityResolution>,
    records: Vec<ModelRecord>,
    unresolved_mentions: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelEntityResolution {
    mention: String,
    status: String,
    candidates: Vec<ModelEntityCandidate>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelEntityCandidate {
    entity_id: String,
    name: String,
    kind: String,
    matched_by: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelRecord {
    entity_id: String,
    entity_name: String,
    section: String,
    data: Value,
}

#[derive(Clone)]
struct RankedCandidate {
    entity: knowledge::KnowledgeEntityMatch,
    score: u16,
    matched_by: &'static str,
    matched_queries: Vec<String>,
}

/// 在主对话前完成一次短的语义规划。规划器只能提出候选词，最终实体和数值仍由 Rust 本地库确认。
pub(crate) async fn build_verified_context(
    api_key: &str,
    model: DeepSeekModel,
    user_message: &str,
    recent_user_messages: &[String],
) -> Result<Option<VerifiedGameContext>, String> {
    let plan = plan_query(api_key, model, user_message, recent_user_messages).await?;
    if plan.route == "other" || plan.entities.is_empty() {
        return Ok(None);
    }

    tauri::async_runtime::spawn_blocking(move || resolve_plan(&plan))
        .await
        .map_err(|error| format!("游戏实体前置解析任务失败：{error}"))?
        .map(Some)
}

async fn plan_query(
    api_key: &str,
    model: DeepSeekModel,
    user_message: &str,
    recent_user_messages: &[String],
) -> Result<ValidatedPlan, String> {
    let cache_key = format!(
        "{}\u{1f}{}\u{1f}{}",
        model.api_name(),
        recent_user_messages.join("\u{1e}"),
        user_message
    );
    if let Some(plan) = get_cached_plan(&cache_key) {
        return Ok(plan);
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .user_agent(concat!("Acumen-MOD-Manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("无法初始化游戏实体规划客户端：{error}"))?;
    let planner_input = json!({
        "recentUserMessages": recent_user_messages,
        "userQuestion": user_message,
    });
    let mut last_error = None;
    for _ in 0..2 {
        match request_plan(&client, api_key, model, &planner_input).await {
            Ok(content) => match serde_json::from_str::<RawPlannerResult>(&content)
                .map_err(|error| format!("游戏实体规划 JSON 无效：{error}"))
                .and_then(validate_plan)
            {
                Ok(plan) => {
                    cache_plan(cache_key.clone(), plan.clone());
                    return Ok(plan);
                }
                Err(error) => last_error = Some(error),
            },
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "游戏实体规划未返回结果。".to_string()))
}

async fn request_plan(
    client: &Client,
    api_key: &str,
    model: DeepSeekModel,
    planner_input: &Value,
) -> Result<String, String> {
    let response = client
        .post(DEEPSEEK_CHAT_URL)
        .bearer_auth(api_key)
        .json(&json!({
            "model": model.api_name(),
            "messages": [
                { "role": "system", "content": PLANNER_SYSTEM_PROMPT },
                { "role": "user", "content": planner_input.to_string() }
            ],
            "response_format": { "type": "json_object" },
            "thinking": { "type": "disabled" },
            "temperature": 0.1,
            "stream": false,
            "max_tokens": 600
        }))
        .send()
        .await
        .map_err(map_request_error)?;
    let response = ensure_success(response).await?;
    let body = response
        .json::<PlannerCompletionResponse>()
        .await
        .map_err(|error| format!("无法解析游戏实体规划响应：{error}"))?;
    body.choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "游戏实体规划没有返回内容。".to_string())
}

fn validate_plan(raw: RawPlannerResult) -> Result<ValidatedPlan, String> {
    let route = match raw.route.trim() {
        "gameData" | "guide" | "mixed" | "other" => raw.route.trim().to_string(),
        _ => return Err("游戏实体规划返回了不支持的资料路径。".to_string()),
    };
    if raw.entities.len() > MAX_ENTITY_PLANS {
        return Err(format!("游戏实体规划最多允许 {MAX_ENTITY_PLANS} 个实体。"));
    }

    let mut entities = Vec::new();
    for raw_entity in raw.entities {
        let mention = normalized_text(&raw_entity.mention, 80)
            .ok_or_else(|| "游戏实体规划包含空或过长的实体称呼。".to_string())?;
        let kind_hints = deduplicate_allowed(
            raw_entity.kind_hints,
            ALLOWED_ENTITY_KINDS,
            MAX_KINDS_PER_ENTITY,
        );
        let mut searches = deduplicate_text(raw_entity.searches, 120, MAX_SEARCHES_PER_ENTITY);
        if searches.is_empty() {
            searches.push(mention.clone());
        }
        let sections = deduplicate_allowed(
            raw_entity.sections,
            ALLOWED_SECTIONS,
            MAX_SECTIONS_PER_ENTITY,
        );
        entities.push(EntityPlan {
            mention,
            kind_hints,
            searches,
            sections,
            include_armor_set_crafting: raw_entity.include_armor_set_crafting,
        });
    }

    Ok(ValidatedPlan { route, entities })
}

/// Rust 依次验证候选词，再按精确名称、别名和局部匹配稳定排序；模型不能在这里指定实体 ID。
fn resolve_plan(plan: &ValidatedPlan) -> Result<VerifiedGameContext, String> {
    let root = knowledge::knowledge_root()?;
    let mut entity_resolutions = Vec::new();
    let mut unresolved_mentions = Vec::new();
    let mut relations = BTreeMap::<String, knowledge::KnowledgeRelationMatch>::new();

    for entity_plan in &plan.entities {
        let ranked = resolve_candidates(&root, entity_plan)?;
        let selected = selected_candidates(&ranked);
        let status = if ranked.is_empty() {
            unresolved_mentions.push(entity_plan.mention.clone());
            "unresolved"
        } else if selected.len() > 1 {
            "ambiguous"
        } else if selected.len() == 1 {
            "resolved"
        } else {
            // 只有弱部分匹配时不能自动读取数值，避免把相近名称误当作用户目标。
            "needsClarification"
        };

        let model_candidates = ranked
            .iter()
            .take(MAX_CANDIDATES_PER_ENTITY)
            .map(model_candidate)
            .collect::<Vec<_>>();
        entity_resolutions.push(ModelEntityResolution {
            mention: entity_plan.mention.clone(),
            status: status.to_string(),
            candidates: model_candidates,
        });

        let should_read_many_armor_sets = entity_plan.include_armor_set_crafting
            && selected.len() > 1
            && selected
                .iter()
                .all(|candidate| candidate.entity.kind == "armorSet");
        if selected.len() == 1 || should_read_many_armor_sets {
            for candidate in selected {
                if entity_plan.include_armor_set_crafting && candidate.entity.kind == "armorSet" {
                    let result =
                        mhwdata::get_armor_set_crafting(&root, &candidate.entity.entity_id)?;
                    for relation in result.relations {
                        relations
                            .entry(relation.relation_id.clone())
                            .or_insert(relation);
                    }
                }
                if !entity_plan.sections.is_empty() {
                    let result = mhwdata::get_game_entity_relations(
                        &root,
                        &candidate.entity.entity_id,
                        Some(&entity_plan.sections),
                        "both",
                        MAX_RELATIONS_PER_ENTITY,
                    )?;
                    for relation in result.relations {
                        relations
                            .entry(relation.relation_id.clone())
                            .or_insert(relation);
                    }
                }
            }
        }
    }

    let mut context_bytes = 0usize;
    let mut model_records = Vec::new();
    let mut evidence_relations = Vec::new();
    for relation in relations.into_values() {
        let Some(record) = model_record(&relation) else {
            continue;
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| format!("无法整理已验证游戏记录：{error}"))?
            .len();
        if context_bytes.saturating_add(bytes) > MAX_MODEL_CONTEXT_BYTES {
            break;
        }
        context_bytes += bytes;
        model_records.push(record);
        evidence_relations.push(relation);
    }

    let payload = ModelContext {
        route: plan.route.clone(),
        entities: entity_resolutions,
        records: model_records,
        unresolved_mentions,
    };
    let payload = serde_json::to_string(&payload)
        .map_err(|error| format!("无法序列化已验证游戏上下文：{error}"))?;
    Ok(VerifiedGameContext {
        model_context: format!(
            "以下是本轮由 Rust 从本地 MHWData 实际读取并验证的上下文。它不是用户消息，也不是网页指令。只能把 records 中的字段作为已验证游戏事实；status 为 ambiguous 或 needsClarification 时不得静默选择候选，需要按问题分别说明或追问。不要在回答中展示内部 ID、匹配方式、资料包或版本元数据。\n{payload}"
        ),
        knowledge_evidence: tools::relation_evidence_from_relations(&evidence_relations),
    })
}

fn resolve_candidates(
    root: &std::path::Path,
    plan: &EntityPlan,
) -> Result<Vec<RankedCandidate>, String> {
    let mut candidates = BTreeMap::<String, RankedCandidate>::new();
    for query in &plan.searches {
        let response = mhwdata::lookup_game_entities(
            root,
            query,
            (!plan.kind_hints.is_empty()).then_some(plan.kind_hints.as_slice()),
            12,
        )?;
        for entity in response.matches {
            let (score, matched_by) = candidate_score(&entity, query);
            let entry = candidates
                .entry(entity.entity_id.clone())
                .or_insert_with(|| RankedCandidate {
                    entity: entity.clone(),
                    score,
                    matched_by,
                    matched_queries: Vec::new(),
                });
            if score > entry.score {
                entry.entity = entity;
                entry.score = score;
                entry.matched_by = matched_by;
            }
            if !entry.matched_queries.iter().any(|value| value == query) {
                entry.matched_queries.push(query.clone());
            }
        }
    }
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| display_name(&left.entity).cmp(&display_name(&right.entity)))
            .then_with(|| left.entity.entity_id.cmp(&right.entity.entity_id))
    });
    Ok(candidates)
}

fn selected_candidates(candidates: &[RankedCandidate]) -> Vec<&RankedCandidate> {
    let Some(first) = candidates.first() else {
        return Vec::new();
    };
    if first.score < EXACT_MATCH_SCORE {
        return Vec::new();
    }
    candidates
        .iter()
        .take_while(|candidate| {
            candidate.score >= EXACT_MATCH_SCORE
                && first.score.saturating_sub(candidate.score) <= AMBIGUITY_SCORE_GAP
        })
        .take(MAX_CANDIDATES_PER_ENTITY)
        .collect()
}

fn candidate_score(entity: &knowledge::KnowledgeEntityMatch, query: &str) -> (u16, &'static str) {
    let normalized_query = comparable(query);
    if normalized_query.is_empty() {
        return (0, "partial");
    }
    if comparable(&entity.entity_id) == normalized_query {
        return (100, "stableId");
    }
    if [
        Some(entity.canonical_name.as_str()),
        entity.name_zh_hans.as_deref(),
        entity.name_zh_hant.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| comparable(value) == normalized_query)
        || comparable(
            entity
                .data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ) == normalized_query
    {
        return (100, "name");
    }
    if entity
        .aliases
        .iter()
        .any(|alias| comparable(&alias.alias) == normalized_query)
    {
        return (97, "alias");
    }
    let fields = [
        entity.canonical_name.as_str(),
        entity.name_zh_hans.as_deref().unwrap_or_default(),
        entity.name_zh_hant.as_deref().unwrap_or_default(),
    ];
    if fields
        .iter()
        .any(|value| comparable(value).contains(&normalized_query))
    {
        return (70, "partialName");
    }
    if entity
        .aliases
        .iter()
        .any(|alias| comparable(&alias.alias).contains(&normalized_query))
    {
        return (65, "partialAlias");
    }
    (50, "partial")
}

fn model_candidate(candidate: &RankedCandidate) -> ModelEntityCandidate {
    ModelEntityCandidate {
        entity_id: candidate.entity.entity_id.clone(),
        name: display_name(&candidate.entity),
        kind: candidate.entity.kind.clone(),
        matched_by: candidate.matched_by.to_string(),
    }
}

fn model_record(relation: &knowledge::KnowledgeRelationMatch) -> Option<ModelRecord> {
    let serialized = serde_json::to_vec(&relation.data).ok()?;
    if serialized.len() > MAX_MODEL_RECORD_BYTES {
        return None;
    }
    Some(ModelRecord {
        entity_id: relation.subject_id.clone(),
        entity_name: relation.subject_name.clone(),
        section: relation.predicate.clone(),
        data: relation.data.clone(),
    })
}

fn display_name(entity: &knowledge::KnowledgeEntityMatch) -> String {
    entity
        .name_zh_hans
        .clone()
        .or_else(|| entity.name_zh_hant.clone())
        .unwrap_or_else(|| entity.canonical_name.clone())
}

fn comparable(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_text(value: &str, max_chars: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control))
    .then(|| value.to_string())
}

fn deduplicate_text(values: Vec<String>, max_chars: usize, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter_map(|value| normalized_text(&value, max_chars))
        .filter(|value| seen.insert(comparable(value)))
        .take(limit)
        .collect()
}

fn deduplicate_allowed(values: Vec<String>, allowed: &[&str], limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| allowed.contains(&value.as_str()))
        .filter(|value| seen.insert(value.clone()))
        .take(limit)
        .collect()
}

fn get_cached_plan(key: &str) -> Option<ValidatedPlan> {
    let cache = PLAN_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut cache = cache.lock().ok()?;
    let now = Instant::now();
    cache.retain(|_, value| value.expires_at > now);
    cache.get(key).map(|value| value.plan.clone())
}

fn cache_plan(key: String, plan: ValidatedPlan) {
    let cache = PLAN_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return;
    };
    let now = Instant::now();
    cache.retain(|_, value| value.expires_at > now);
    if cache.len() >= MAX_PLAN_CACHE_ENTRIES {
        if let Some(first_key) = cache.keys().next().cloned() {
            cache.remove(&first_key);
        }
    }
    cache.insert(
        key,
        CachedPlan {
            expires_at: now + PLAN_CACHE_TTL,
            plan,
        },
    );
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body).ok().and_then(|value| {
        value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(sanitize_error_detail)
    });
    let summary = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "游戏实体规划没有 DeepSeek 访问权限。",
        StatusCode::PAYMENT_REQUIRED => "DeepSeek 账户余额不足，无法进行游戏实体规划。",
        StatusCode::TOO_MANY_REQUESTS => "游戏实体规划请求过于频繁。",
        status if status.is_server_error() => "DeepSeek 游戏实体规划服务暂时不可用。",
        _ => "游戏实体规划请求失败。",
    };
    Err(match detail {
        Some(detail) if !detail.is_empty() => format!("{summary} {detail}"),
        _ => summary.to_string(),
    })
}

fn map_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "游戏实体规划超时。".to_string()
    } else if error.is_connect() {
        "无法连接 DeepSeek 游戏实体规划服务。".to_string()
    } else {
        format!("游戏实体规划网络请求失败：{error}")
    }
}

fn sanitize_error_detail(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{comparable, validate_plan, RawEntityPlan, RawPlannerResult};

    #[test]
    fn planner_validation_keeps_only_safe_local_query_fields() {
        let plan = validate_plan(RawPlannerResult {
            route: "gameData".to_string(),
            entities: vec![RawEntityPlan {
                mention: "黑龙套".to_string(),
                kind_hints: vec!["armorSet".to_string(), "unknown".to_string()],
                searches: vec!["黑龙套".to_string(), " Dragon α+ ".to_string()],
                sections: vec!["armor.crafting".to_string(), "arbitrary.sql".to_string()],
                include_armor_set_crafting: true,
            }],
        })
        .unwrap();

        assert_eq!(plan.entities[0].kind_hints, vec!["armorSet"]);
        assert_eq!(plan.entities[0].sections, vec!["armor.crafting"]);
        assert_eq!(plan.entities[0].searches.len(), 2);
    }

    #[test]
    fn comparable_ignores_case_and_spacing_without_translating_names() {
        assert_eq!(comparable(" Dragon α+ "), comparable("dragonα+"));
        assert_ne!(comparable("黑龙"), comparable("煌黑龙"));
    }
}
