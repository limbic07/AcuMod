use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

/// 一项有效部署文件与当前游戏目录的比对事实。
///
/// 这个 DTO 不携带文件内容；调用方已经用实际部署逻辑相同的转换规则完成了比较。
#[derive(Clone, Debug)]
pub(crate) struct ModStateSyncFile {
    pub path_key: String,
    pub matches_game_directory: bool,
    pub has_existing_record: bool,
}

/// 状态同步分析所需的一项 MOD 输入。
#[derive(Clone, Debug)]
pub(crate) struct ModStateSyncInput {
    pub mod_id: String,
    /// 外部盒子关联的 MOD 才会由本次同步推导状态。
    pub is_candidate: bool,
    /// 已由 Acumod 主动部署的启用 MOD 是可信提供者，但不会被本次扫描改写状态。
    pub is_trusted_enabled: bool,
    pub files: Vec<ModStateSyncFile>,
}

/// 一个可确定的“赢家覆盖当前 MOD”优先级边。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModStateSyncPriorityEdge {
    pub winner_mod_id: String,
    pub covered_mod_id: String,
}

/// 分析完成后，供本地库服务写入 manifest 的内部计划。
#[derive(Clone, Debug)]
pub(crate) struct ModStateSyncPlan {
    pub mod_states: Vec<ModStateSyncPlanMod>,
    pub priority_edges: Vec<ModStateSyncPriorityEdge>,
    pub mixed_conflict_groups: Vec<Vec<String>>,
}

/// 一项待写入的外部 MOD 状态。
#[derive(Clone, Debug)]
pub(crate) struct ModStateSyncPlanMod {
    pub mod_id: String,
    pub enabled: bool,
    pub partially_overridden: bool,
    /// 当前游戏目录实际由该 MOD 提供的路径。等价内容只交给一个规范记录者。
    pub observed_path_keys: Vec<String>,
    pub message: String,
}

/// 前端只需要看到的状态同步结果；逐文件匹配细节保留在 Rust 内部。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStateSyncResult {
    pub enabled_mod_count: usize,
    pub partially_overridden_mod_count: usize,
    pub disabled_mod_count: usize,
    pub mixed_conflict_group_count: usize,
    pub mods: Vec<ModStateSyncModResult>,
    pub warnings: Vec<String>,
    pub message: String,
}

/// 单个外部关联 MOD 的简化同步结果。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStateSyncModResult {
    pub mod_id: String,
    pub enabled: bool,
    pub partially_overridden: bool,
    pub message: String,
}

impl ModStateSyncResult {
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            enabled_mod_count: 0,
            partially_overridden_mod_count: 0,
            disabled_mod_count: 0,
            mixed_conflict_group_count: 0,
            mods: Vec::new(),
            warnings: vec![message.into()],
            message: "未能检测游戏目录中的 MOD 状态。".to_string(),
        }
    }
}

impl ModStateSyncPlan {
    pub(crate) fn to_result(&self, warnings: Vec<String>) -> ModStateSyncResult {
        let enabled_mod_count = self.mod_states.iter().filter(|state| state.enabled).count();
        let partially_overridden_mod_count = self
            .mod_states
            .iter()
            .filter(|state| state.enabled && state.partially_overridden)
            .count();
        let disabled_mod_count = self.mod_states.len() - enabled_mod_count;
        let message = if self.mod_states.is_empty() {
            "没有需要检测状态的盒子 MOD。".to_string()
        } else {
            format!(
                "游戏状态检测完成：已启用 {enabled_mod_count} 个，其中部分被覆盖 {partially_overridden_mod_count} 个，未启用 {disabled_mod_count} 个。"
            )
        };

        ModStateSyncResult {
            enabled_mod_count,
            partially_overridden_mod_count,
            disabled_mod_count,
            mixed_conflict_group_count: self.mixed_conflict_groups.len(),
            mods: self
                .mod_states
                .iter()
                .map(|state| ModStateSyncModResult {
                    mod_id: state.mod_id.clone(),
                    enabled: state.enabled,
                    partially_overridden: state.partially_overridden,
                    message: state.message.clone(),
                })
                .collect(),
            warnings,
            message,
        }
    }
}

/// 根据已经完成的逐文件事实推导外部 MOD 的二元启用状态和可确定的优先级边。
///
/// 这里故意不读写文件。这样文件比较、模型改绑后的有效内容生成仍留在本地库服务，
/// 而“部分覆盖是否可以解释”的规则可以用小型内存测试完整覆盖。
pub(crate) fn analyze_mod_states(inputs: &[ModStateSyncInput]) -> ModStateSyncPlan {
    let mods = normalize_inputs(inputs);
    let candidate_ids = mods
        .values()
        .filter(|input| input.is_candidate)
        .map(|input| input.mod_id.clone())
        .collect::<BTreeSet<_>>();
    let mut confirmed_ids = mods
        .values()
        .filter(|input| input.is_trusted_enabled)
        .map(|input| input.mod_id.clone())
        .collect::<BTreeSet<_>>();
    let matches_by_path = matching_providers_by_path(&mods);

    let mut states = candidate_ids
        .iter()
        .map(|mod_id| {
            (
                mod_id.clone(),
                CandidateState {
                    enabled: false,
                    partially_overridden: false,
                    message: String::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut zero_match_candidates = BTreeSet::new();

    // 先确认所有完全匹配项，避免候选遍历顺序影响后续部分覆盖的解释。
    for mod_id in &candidate_ids {
        let input = &mods[mod_id];
        let matching_count = input
            .files
            .values()
            .filter(|file| file.matches_game_directory)
            .count();
        let is_fully_matching = !input.files.is_empty() && matching_count == input.files.len();

        if is_fully_matching {
            let state = states.get_mut(mod_id).expect("candidate state must exist");
            state.enabled = true;
            state.message = "所有有效文件都与游戏目录一致。".to_string();
            confirmed_ids.insert(mod_id.clone());
            continue;
        }

        if matching_count == 0 {
            zero_match_candidates.insert(mod_id.clone());
        }
    }

    let mut priority_edges = BTreeSet::new();
    loop {
        let mut accepted_any = false;

        for mod_id in &candidate_ids {
            if states.get(mod_id).is_some_and(|state| state.enabled)
                || zero_match_candidates.contains(mod_id)
            {
                continue;
            }

            let input = &mods[mod_id];
            let mut confirmed_winners = BTreeSet::new();
            let mut all_paths_explained = true;
            for file in input
                .files
                .values()
                .filter(|file| !file.matches_game_directory)
            {
                let Some(winner_mod_id) = canonical_matching_provider(
                    &matches_by_path,
                    &mods,
                    &file.path_key,
                    Some(&confirmed_ids),
                ) else {
                    all_paths_explained = false;
                    break;
                };
                confirmed_winners.insert(winner_mod_id);
            }

            if all_paths_explained && !confirmed_winners.is_empty() {
                let state = states.get_mut(mod_id).expect("candidate state must exist");
                state.enabled = true;
                state.partially_overridden = true;
                state.message = "已启用，部分文件被更高优先级 MOD 覆盖。".to_string();
                confirmed_ids.insert(mod_id.clone());
                for winner_mod_id in confirmed_winners {
                    priority_edges.insert((winner_mod_id, mod_id.clone()));
                }
                accepted_any = true;
            }
        }

        if !accepted_any {
            break;
        }
    }

    // 只有再也无法通过已确认提供者解释的候选才参与环检测。这样不会把存在明确赢家的
    // 多层覆盖链误判为混合覆盖，也不会让旧观察记录改变正确的推导结果。
    let unresolved_candidate_ids = candidate_ids
        .iter()
        .filter(|mod_id| {
            !states.get(*mod_id).is_some_and(|state| state.enabled)
                && !zero_match_candidates.contains(*mod_id)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut potential_winners = BTreeMap::<String, Vec<String>>::new();
    for mod_id in &unresolved_candidate_ids {
        let input = &mods[mod_id];
        let mut winners = Vec::new();
        let mut all_paths_explained = true;
        for file in input
            .files
            .values()
            .filter(|file| !file.matches_game_directory)
        {
            let Some(winner_mod_id) =
                canonical_matching_provider(&matches_by_path, &mods, &file.path_key, None)
            else {
                all_paths_explained = false;
                break;
            };
            winners.push(winner_mod_id);
        }

        if all_paths_explained {
            winners.sort();
            winners.dedup();
            potential_winners.insert(mod_id.clone(), winners);
        }
    }
    let mixed_conflict_groups =
        mixed_conflict_groups(&unresolved_candidate_ids, &potential_winners);
    let mixed_candidate_ids = mixed_conflict_groups
        .iter()
        .flatten()
        .cloned()
        .collect::<HashSet<_>>();

    for mod_id in &candidate_ids {
        let state = states.get_mut(mod_id).expect("candidate state must exist");
        if state.enabled {
            continue;
        }
        state.message = if mixed_candidate_ids.contains(mod_id) {
            "检测到无法表达为整体优先级的混合覆盖，保持未启用。".to_string()
        } else if zero_match_candidates.contains(mod_id) {
            "未检测到与本 MOD 一致的游戏文件，保持未启用。".to_string()
        } else {
            "部分文件无法由已启用 MOD 的覆盖关系完整解释，保持未启用。".to_string()
        };
    }

    let enabled_provider_ids = confirmed_ids;
    let observed_paths_by_mod = observed_paths_by_mod(
        &candidate_ids,
        &states,
        &mods,
        &matches_by_path,
        &enabled_provider_ids,
    );

    ModStateSyncPlan {
        mod_states: candidate_ids
            .iter()
            .map(|mod_id| {
                let state = &states[mod_id];
                ModStateSyncPlanMod {
                    mod_id: mod_id.clone(),
                    enabled: state.enabled,
                    partially_overridden: state.partially_overridden,
                    observed_path_keys: observed_paths_by_mod
                        .get(mod_id)
                        .cloned()
                        .unwrap_or_default(),
                    message: state.message.clone(),
                }
            })
            .collect(),
        priority_edges: priority_edges
            .into_iter()
            .map(|(winner_mod_id, covered_mod_id)| ModStateSyncPriorityEdge {
                winner_mod_id,
                covered_mod_id,
            })
            .collect(),
        mixed_conflict_groups,
    }
}

#[derive(Clone, Debug)]
struct IndexedSyncMod {
    mod_id: String,
    is_candidate: bool,
    is_trusted_enabled: bool,
    files: BTreeMap<String, ModStateSyncFile>,
}

#[derive(Default)]
struct CandidateState {
    enabled: bool,
    partially_overridden: bool,
    message: String,
}

fn normalize_inputs(inputs: &[ModStateSyncInput]) -> BTreeMap<String, IndexedSyncMod> {
    let mut mods = BTreeMap::new();

    for input in inputs {
        let mut files = BTreeMap::new();
        for file in &input.files {
            // 一个 MOD 的有效部署路径应唯一。若错误数据造成重复，保留第一个，
            // 让本地库层在构建输入时记录诊断，而不是让排序结果依赖遍历顺序。
            files
                .entry(file.path_key.clone())
                .or_insert_with(|| file.clone());
        }
        mods.insert(
            input.mod_id.clone(),
            IndexedSyncMod {
                mod_id: input.mod_id.clone(),
                is_candidate: input.is_candidate,
                is_trusted_enabled: input.is_trusted_enabled,
                files,
            },
        );
    }

    mods
}

fn matching_providers_by_path(
    mods: &BTreeMap<String, IndexedSyncMod>,
) -> HashMap<String, BTreeSet<String>> {
    let mut matches_by_path = HashMap::new();

    for input in mods.values() {
        for file in input
            .files
            .values()
            .filter(|file| file.matches_game_directory)
        {
            matches_by_path
                .entry(file.path_key.clone())
                .or_insert_with(BTreeSet::new)
                .insert(input.mod_id.clone());
        }
    }

    matches_by_path
}

fn canonical_matching_provider(
    matches_by_path: &HashMap<String, BTreeSet<String>>,
    mods: &BTreeMap<String, IndexedSyncMod>,
    path_key: &str,
    enabled_provider_ids: Option<&BTreeSet<String>>,
) -> Option<String> {
    let matching_mod_ids = matches_by_path.get(path_key)?;
    let candidates = matching_mod_ids
        .iter()
        .filter(|mod_id| {
            enabled_provider_ids
                .map(|enabled_ids| enabled_ids.contains(*mod_id))
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();

    // 已由 Acumod 主动部署的记录优先成为等价内容的规范记录者；它们已有更明确的
    // 文件归属。其余情况再退回任意已有记录和稳定 MOD ID。
    candidates
        .iter()
        .find(|mod_id| {
            mods.get(*mod_id).is_some_and(|input| {
                input.is_trusted_enabled
                    && input
                        .files
                        .get(path_key)
                        .is_some_and(|file| file.has_existing_record)
            })
        })
        .cloned()
        .or_else(|| {
            candidates
                .iter()
                .find(|mod_id| {
                    mods.get(*mod_id)
                        .and_then(|input| input.files.get(path_key))
                        .is_some_and(|file| file.has_existing_record)
                })
                .cloned()
        })
        .or_else(|| candidates.into_iter().next())
}

fn mixed_conflict_groups(
    candidate_ids: &BTreeSet<String>,
    potential_winners: &BTreeMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    let mut graph = candidate_ids
        .iter()
        .map(|mod_id| (mod_id.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();

    for (covered_mod_id, winners) in potential_winners {
        for winner_mod_id in winners {
            if candidate_ids.contains(winner_mod_id) {
                graph
                    .entry(winner_mod_id.clone())
                    .or_default()
                    .insert(covered_mod_id.clone());
            }
        }
    }

    let mut reversed_graph = candidate_ids
        .iter()
        .map(|mod_id| (mod_id.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (source, targets) in &graph {
        for target in targets {
            reversed_graph
                .entry(target.clone())
                .or_default()
                .insert(source.clone());
        }
    }

    let mut visited = HashSet::new();
    let mut finish_order = Vec::new();
    for mod_id in candidate_ids {
        dfs_finish(mod_id, &graph, &mut visited, &mut finish_order);
    }

    let mut components = Vec::new();
    let mut assigned = HashSet::new();
    for mod_id in finish_order.into_iter().rev() {
        if !assigned.insert(mod_id.clone()) {
            continue;
        }
        let mut component = Vec::new();
        dfs_collect(&mod_id, &reversed_graph, &mut assigned, &mut component);
        component.sort();
        let is_self_cycle = component.len() == 1
            && graph
                .get(&component[0])
                .is_some_and(|targets| targets.contains(&component[0]));
        if component.len() > 1 || is_self_cycle {
            components.push(component);
        }
    }

    components.sort();
    components
}

fn dfs_finish(
    mod_id: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut HashSet<String>,
    finish_order: &mut Vec<String>,
) {
    if !visited.insert(mod_id.to_string()) {
        return;
    }
    if let Some(targets) = graph.get(mod_id) {
        for target in targets {
            dfs_finish(target, graph, visited, finish_order);
        }
    }
    finish_order.push(mod_id.to_string());
}

fn dfs_collect(
    mod_id: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    assigned: &mut HashSet<String>,
    component: &mut Vec<String>,
) {
    component.push(mod_id.to_string());
    if let Some(targets) = graph.get(mod_id) {
        for target in targets {
            if assigned.insert(target.clone()) {
                dfs_collect(target, graph, assigned, component);
            }
        }
    }
}

fn observed_paths_by_mod(
    candidate_ids: &BTreeSet<String>,
    states: &BTreeMap<String, CandidateState>,
    mods: &BTreeMap<String, IndexedSyncMod>,
    matches_by_path: &HashMap<String, BTreeSet<String>>,
    enabled_provider_ids: &BTreeSet<String>,
) -> BTreeMap<String, Vec<String>> {
    let mut observed_paths = BTreeMap::new();

    for mod_id in candidate_ids {
        let Some(state) = states.get(mod_id) else {
            continue;
        };
        if !state.enabled {
            continue;
        }
        let Some(input) = mods.get(mod_id) else {
            continue;
        };

        let mut paths = input
            .files
            .values()
            .filter(|file| file.matches_game_directory)
            .filter_map(|file| {
                let recorder = canonical_matching_provider(
                    matches_by_path,
                    mods,
                    &file.path_key,
                    Some(enabled_provider_ids),
                )?;
                (recorder == *mod_id).then_some(file.path_key.clone())
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        observed_paths.insert(mod_id.clone(), paths);
    }

    observed_paths
}

#[cfg(test)]
mod tests {
    use super::{analyze_mod_states, ModStateSyncFile, ModStateSyncInput};

    fn input(
        mod_id: &str,
        is_candidate: bool,
        is_trusted_enabled: bool,
        files: &[(&str, bool)],
    ) -> ModStateSyncInput {
        ModStateSyncInput {
            mod_id: mod_id.to_string(),
            is_candidate,
            is_trusted_enabled,
            files: files
                .iter()
                .map(|(path, matches_game_directory)| ModStateSyncFile {
                    path_key: (*path).to_string(),
                    matches_game_directory: *matches_game_directory,
                    has_existing_record: false,
                })
                .collect(),
        }
    }

    fn state<'a>(
        plan: &'a super::ModStateSyncPlan,
        mod_id: &str,
    ) -> &'a super::ModStateSyncPlanMod {
        plan.mod_states
            .iter()
            .find(|state| state.mod_id == mod_id)
            .unwrap()
    }

    #[test]
    fn fully_matching_mod_is_enabled() {
        let plan = analyze_mod_states(&[input("a", true, false, &[("nativePC/a", true)])]);

        assert!(state(&plan, "a").enabled);
        assert!(!state(&plan, "a").partially_overridden);
    }

    #[test]
    fn partial_mod_with_missing_file_is_disabled() {
        let plan = analyze_mod_states(&[input(
            "a",
            true,
            false,
            &[("nativePC/a", true), ("nativePC/missing", false)],
        )]);

        assert!(!state(&plan, "a").enabled);
    }

    #[test]
    fn partial_mod_is_enabled_when_confirmed_winner_explains_every_difference() {
        let plan = analyze_mod_states(&[
            input("b", true, false, &[("nativePC/shared", true)]),
            input(
                "a",
                true,
                false,
                &[("nativePC/own", true), ("nativePC/shared", false)],
            ),
        ]);

        assert!(state(&plan, "b").enabled);
        assert!(state(&plan, "a").enabled);
        assert!(state(&plan, "a").partially_overridden);
        assert_eq!(plan.priority_edges.len(), 1);
        assert_eq!(plan.priority_edges[0].winner_mod_id, "b");
        assert_eq!(plan.priority_edges[0].covered_mod_id, "a");
    }

    #[test]
    fn multi_level_overrides_are_resolved_iteratively() {
        let plan = analyze_mod_states(&[
            input("c", true, false, &[("nativePC/c", true)]),
            input(
                "b",
                true,
                false,
                &[("nativePC/b", true), ("nativePC/c", false)],
            ),
            input(
                "a",
                true,
                false,
                &[("nativePC/a", true), ("nativePC/b", false)],
            ),
        ]);

        assert!(state(&plan, "a").enabled);
        assert!(state(&plan, "b").enabled);
        assert!(state(&plan, "c").enabled);
        assert_eq!(plan.priority_edges.len(), 2);
    }

    #[test]
    fn confirmed_provider_wins_over_an_unconfirmed_recorded_candidate() {
        let plan = analyze_mod_states(&[
            input(
                "a",
                true,
                false,
                &[("nativePC/a", true), ("nativePC/shared", false)],
            ),
            input("b", true, false, &[("nativePC/shared", true)]),
            ModStateSyncInput {
                mod_id: "c".to_string(),
                is_candidate: true,
                is_trusted_enabled: false,
                files: vec![
                    ModStateSyncFile {
                        path_key: "nativePC/shared".to_string(),
                        matches_game_directory: true,
                        has_existing_record: true,
                    },
                    ModStateSyncFile {
                        path_key: "nativePC/a".to_string(),
                        matches_game_directory: false,
                        has_existing_record: false,
                    },
                ],
            },
        ]);

        assert!(state(&plan, "a").enabled);
        assert!(state(&plan, "b").enabled);
        assert!(state(&plan, "c").enabled);
        assert!(plan.mixed_conflict_groups.is_empty());
    }

    #[test]
    fn identical_content_does_not_create_a_priority_edge() {
        let plan = analyze_mod_states(&[
            input("a", true, false, &[("nativePC/shared", true)]),
            input("b", true, false, &[("nativePC/shared", true)]),
        ]);

        assert!(state(&plan, "a").enabled);
        assert!(state(&plan, "b").enabled);
        assert!(plan.priority_edges.is_empty());
    }

    #[test]
    fn mixed_overrides_are_kept_disabled() {
        let plan = analyze_mod_states(&[
            input(
                "a",
                true,
                false,
                &[("nativePC/a", true), ("nativePC/b", false)],
            ),
            input(
                "b",
                true,
                false,
                &[("nativePC/a", false), ("nativePC/b", true)],
            ),
        ]);

        assert!(!state(&plan, "a").enabled);
        assert!(!state(&plan, "b").enabled);
        assert_eq!(
            plan.mixed_conflict_groups,
            vec![vec!["a".to_string(), "b".to_string()]]
        );
    }

    #[test]
    fn no_matching_file_never_becomes_enabled() {
        let plan = analyze_mod_states(&[
            input("b", true, false, &[("nativePC/shared", true)]),
            input("a", true, false, &[("nativePC/shared", false)]),
        ]);

        assert!(state(&plan, "b").enabled);
        assert!(!state(&plan, "a").enabled);
    }
}
