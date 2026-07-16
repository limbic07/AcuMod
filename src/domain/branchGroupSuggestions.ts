import type {
  InstalledModSummary,
  ModBranchGroup,
  ModConflictGroup,
} from "../api/modLibrary";
import { compareNaturalText } from "./textSort";

const MIN_PATH_COVERAGE = 0.9;
const MIN_PATH_JACCARD = 0.75;
const MIN_SMALL_MOD_NAME_SIMILARITY = 0.55;
const MAX_SUGGESTED_GROUP_SIZE = 16;

export interface BranchGroupSuggestionMember {
  modId: string;
  name: string;
  enabled: boolean;
  fileCount: number;
}

export interface BranchGroupSuggestion {
  id: string;
  suggestedName: string;
  members: BranchGroupSuggestionMember[];
  sharedFileCount: number;
  similarityPercent: number;
  minimumNameSimilarityPercent: number;
  sameImportSource: boolean;
  conflictPairCount: number;
  sharedTargetLabels: string[];
}

export interface BranchGroupSuggestionSelection {
  suggestionId: string;
  name: string;
  modIds: string[];
}

interface ModSimilarityIndex {
  mod: InstalledModSummary;
  deployPaths: Set<string>;
  modelTargets: Map<string, string>;
  normalizedName: string;
  normalizedSourcePath: string;
}

interface PairSimilarity {
  leftId: string;
  rightId: string;
  score: number;
  nameSimilarity: number;
}

function normalizeDeployPath(path: string) {
  return path.replace(/\\/g, "/").toLocaleLowerCase();
}

function normalizeSourcePath(path: string) {
  return path.trim().replace(/\\/g, "/").replace(/\/+$/g, "").toLocaleLowerCase();
}

function normalizeModName(name: string) {
  return name
    .toLocaleLowerCase()
    .replace(/\.(zip|7z|rar)$/gi, "")
    .replace(/(?:v(?:er)?\.?\s*\d+(?:\.\d+)*|版本?\s*\d+(?:\.\d+)*|第?\s*\d+\s*版)/gi, "")
    .replace(/[\s_\-—·:：()\[\]【】]+/g, "");
}

function bigrams(value: string) {
  const characters = Array.from(value);
  if (characters.length < 2) {
    return characters;
  }
  return characters.slice(0, -1).map((character, index) => `${character}${characters[index + 1]}`);
}

function nameSimilarity(left: string, right: string) {
  if (!left || !right) {
    return 0;
  }
  if (left === right) {
    return 1;
  }
  const leftBigrams = bigrams(left);
  const rightCounts = new Map<string, number>();
  for (const value of bigrams(right)) {
    rightCounts.set(value, (rightCounts.get(value) ?? 0) + 1);
  }
  let sharedCount = 0;
  for (const value of leftBigrams) {
    const count = rightCounts.get(value) ?? 0;
    if (count > 0) {
      sharedCount += 1;
      rightCounts.set(value, count - 1);
    }
  }
  return (2 * sharedCount) / Math.max(1, leftBigrams.length + bigrams(right).length);
}

function modelTargetKey(modelKind: string, subKind: string, modelId: string) {
  return `${modelKind}\u0000${subKind}\u0000${modelId}`.toLocaleLowerCase();
}

function buildModIndex(mod: InstalledModSummary): ModSimilarityIndex {
  const modelTargets = new Map<string, string>();
  for (const replacement of mod.modelReplacements) {
    const key = modelTargetKey(replacement.modelKind, replacement.subKind, replacement.modelId);
    modelTargets.set(key, replacement.displayNames[0] || replacement.modelId);
  }
  return {
    mod,
    deployPaths: new Set(mod.files.map((file) => normalizeDeployPath(file.deployRelativePath))),
    modelTargets,
    normalizedName: normalizeModName(mod.originalName || mod.name),
    normalizedSourcePath: normalizeSourcePath(mod.sourcePath),
  };
}

function intersectionCount<T>(left: Set<T>, right: Set<T>) {
  const [smaller, larger] = left.size <= right.size ? [left, right] : [right, left];
  let count = 0;
  for (const value of smaller) {
    if (larger.has(value)) {
      count += 1;
    }
  }
  return count;
}

function pairKey(leftId: string, rightId: string) {
  return [leftId, rightId].sort().join("\u0000");
}

function collectConflictPairs(conflictGroups: ModConflictGroup[]) {
  const pairs = new Set<string>();
  for (const group of conflictGroups) {
    for (let leftIndex = 0; leftIndex < group.participants.length; leftIndex += 1) {
      for (let rightIndex = leftIndex + 1; rightIndex < group.participants.length; rightIndex += 1) {
        pairs.add(
          pairKey(
            group.participants[leftIndex].modId,
            group.participants[rightIndex].modId,
          ),
        );
      }
    }
  }
  return pairs;
}

function pairSimilarity(
  left: ModSimilarityIndex,
  right: ModSimilarityIndex,
): PairSimilarity | null {
  const sharedFileCount = intersectionCount(left.deployPaths, right.deployPaths);
  if (!sharedFileCount) {
    return null;
  }
  const smallerFileCount = Math.min(left.deployPaths.size, right.deployPaths.size);
  const unionFileCount = left.deployPaths.size + right.deployPaths.size - sharedFileCount;
  const pathCoverage = sharedFileCount / Math.max(1, smallerFileCount);
  const pathJaccard = sharedFileCount / Math.max(1, unionFileCount);
  if (pathCoverage < MIN_PATH_COVERAGE || pathJaccard < MIN_PATH_JACCARD) {
    return null;
  }

  const comparedNameSimilarity = nameSimilarity(left.normalizedName, right.normalizedName);
  const sameSource =
    Boolean(left.normalizedSourcePath) &&
    left.normalizedSourcePath === right.normalizedSourcePath;
  const sharedTargetCount = intersectionCount(
    new Set(left.modelTargets.keys()),
    new Set(right.modelTargets.keys()),
  );

  // 单文件 MOD 部署到同一路径时就是互斥版本；双文件 MOD 仍需附加证据，避免普通冲突误组。
  if (
    smallerFileCount === 2 &&
    !(
      comparedNameSimilarity >= MIN_SMALL_MOD_NAME_SIMILARITY &&
      (sameSource || sharedTargetCount > 0)
    )
  ) {
    return null;
  }

  const score = Math.min(
    1,
    pathCoverage * 0.55 +
      pathJaccard * 0.3 +
      comparedNameSimilarity * 0.15 +
      (sameSource ? 0.05 : 0),
  );
  return {
    leftId: left.mod.id,
    rightId: right.mod.id,
    score,
    nameSimilarity: comparedNameSimilarity,
  };
}

function canMergeClusters(
  leftCluster: Set<string>,
  rightCluster: Set<string>,
  pairs: Map<string, PairSimilarity>,
) {
  for (const leftId of leftCluster) {
    for (const rightId of rightCluster) {
      if (!pairs.has(pairKey(leftId, rightId))) {
        return false;
      }
    }
  }
  return true;
}

function buildCompleteLinkClusters(
  pairs: Map<string, PairSimilarity>,
): Set<string>[] {
  const clusters: Set<string>[] = [];
  const sortedPairs = [...pairs.values()].sort(
    (left, right) =>
      right.score - left.score ||
      pairKey(left.leftId, left.rightId).localeCompare(pairKey(right.leftId, right.rightId)),
  );

  for (const pair of sortedPairs) {
    const leftCluster = clusters.find((cluster) => cluster.has(pair.leftId));
    const rightCluster = clusters.find((cluster) => cluster.has(pair.rightId));
    if (!leftCluster && !rightCluster) {
      clusters.push(new Set([pair.leftId, pair.rightId]));
      continue;
    }
    if (leftCluster && rightCluster) {
      if (leftCluster === rightCluster || !canMergeClusters(leftCluster, rightCluster, pairs)) {
        continue;
      }
      for (const modId of rightCluster) {
        leftCluster.add(modId);
      }
      clusters.splice(clusters.indexOf(rightCluster), 1);
      continue;
    }

    const existingCluster = leftCluster ?? rightCluster;
    const ungroupedId = leftCluster ? pair.rightId : pair.leftId;
    if (
      existingCluster &&
      canMergeClusters(existingCluster, new Set([ungroupedId]), pairs)
    ) {
      existingCluster.add(ungroupedId);
    }
  }
  return clusters;
}

function commonSet<T>(sets: Set<T>[]) {
  const common = new Set(sets[0] ?? []);
  for (const values of sets.slice(1)) {
    for (const value of common) {
      if (!values.has(value)) {
        common.delete(value);
      }
    }
  }
  return common;
}

function unionSet<T>(sets: Set<T>[]) {
  return new Set(sets.flatMap((values) => [...values]));
}

function commonTargetLabels(indexes: ModSimilarityIndex[]) {
  const commonKeys = commonSet(indexes.map((index) => new Set(index.modelTargets.keys())));
  return [...commonKeys]
    .map((key) => indexes[0].modelTargets.get(key) ?? key)
    .sort(compareNaturalText);
}

function commonNamePrefix(names: string[]) {
  if (!names.length) {
    return "";
  }
  let prefix = names[0];
  for (const name of names.slice(1)) {
    while (prefix && !name.toLocaleLowerCase().startsWith(prefix.toLocaleLowerCase())) {
      prefix = prefix.slice(0, -1);
    }
  }
  return prefix.replace(/[\s_\-—·:：()\[\]【】]+$/g, "").trim();
}

function suggestedGroupName(indexes: ModSimilarityIndex[], sharedTargetLabels: string[]) {
  const prefix = commonNamePrefix(indexes.map((index) => index.mod.originalName || index.mod.name));
  if (prefix.length >= 4) {
    return `${prefix} 分支组`;
  }
  if (sharedTargetLabels.length) {
    return `${sharedTargetLabels[0]} 分支组`;
  }
  return `${indexes[0].mod.name} 分支组`;
}

export function buildBranchGroupSuggestions(
  mods: InstalledModSummary[],
  existingGroups: ModBranchGroup[],
  conflictGroups: ModConflictGroup[],
): BranchGroupSuggestion[] {
  const groupedModIds = new Set(existingGroups.flatMap((group) => group.modIds));
  const indexes = mods
    .filter((mod) => !groupedModIds.has(mod.id) && mod.files.length > 0)
    .map(buildModIndex);
  const indexByModId = new Map(indexes.map((index) => [index.mod.id, index]));
  const pathOwners = new Map<string, number[]>();
  for (const [indexPosition, index] of indexes.entries()) {
    for (const path of index.deployPaths) {
      pathOwners.set(path, [...(pathOwners.get(path) ?? []), indexPosition]);
    }
  }

  const candidatePairs = new Set<string>();
  for (const owners of pathOwners.values()) {
    for (let leftIndex = 0; leftIndex < owners.length; leftIndex += 1) {
      for (let rightIndex = leftIndex + 1; rightIndex < owners.length; rightIndex += 1) {
        candidatePairs.add(`${owners[leftIndex]}:${owners[rightIndex]}`);
      }
    }
  }
  const similarities = new Map<string, PairSimilarity>();
  for (const candidatePair of candidatePairs) {
    const [leftIndex, rightIndex] = candidatePair.split(":").map(Number);
    const similarity = pairSimilarity(indexes[leftIndex], indexes[rightIndex]);
    if (similarity) {
      similarities.set(pairKey(similarity.leftId, similarity.rightId), similarity);
    }
  }

  const conflictPairs = collectConflictPairs(conflictGroups);
  return buildCompleteLinkClusters(similarities)
    .map((cluster) => [...cluster].map((modId) => indexByModId.get(modId)))
    .filter((cluster): cluster is ModSimilarityIndex[] => cluster.every(Boolean))
    .filter((cluster) => cluster.length <= MAX_SUGGESTED_GROUP_SIZE)
    .map((cluster) => {
      const commonPaths = commonSet(cluster.map((index) => index.deployPaths));
      const allPaths = unionSet(cluster.map((index) => index.deployPaths));
      const smallestFileCount = Math.min(...cluster.map((index) => index.deployPaths.size));
      const groupCoverage = commonPaths.size / Math.max(1, smallestFileCount);
      const groupJaccard = commonPaths.size / Math.max(1, allPaths.size);
      if (groupCoverage < MIN_PATH_COVERAGE || groupJaccard < MIN_PATH_JACCARD) {
        return null;
      }

      const groupPairs: PairSimilarity[] = [];
      let conflictPairCount = 0;
      for (let leftIndex = 0; leftIndex < cluster.length; leftIndex += 1) {
        for (let rightIndex = leftIndex + 1; rightIndex < cluster.length; rightIndex += 1) {
          const key = pairKey(cluster[leftIndex].mod.id, cluster[rightIndex].mod.id);
          const similarity = similarities.get(key);
          if (similarity) {
            groupPairs.push(similarity);
          }
          if (conflictPairs.has(key)) {
            conflictPairCount += 1;
          }
        }
      }

      const sharedTargetLabels = commonTargetLabels(cluster);
      const sortedMembers = cluster
        .map((index) => index.mod)
        .sort((left, right) => left.installedAtUnixSeconds - right.installedAtUnixSeconds);
      const sourcePaths = new Set(
        cluster.map((index) => index.normalizedSourcePath).filter(Boolean),
      );
      return {
        id: sortedMembers.map((mod) => mod.id).join("\u0000"),
        suggestedName: suggestedGroupName(cluster, sharedTargetLabels),
        members: sortedMembers.map((mod) => ({
          modId: mod.id,
          name: mod.name,
          enabled: mod.enabled,
          fileCount: mod.fileCount,
        })),
        sharedFileCount: commonPaths.size,
        similarityPercent: Math.round(
          Math.min(...groupPairs.map((pair) => pair.score)) * 100,
        ),
        minimumNameSimilarityPercent: Math.round(
          Math.min(...groupPairs.map((pair) => pair.nameSimilarity)) * 100,
        ),
        sameImportSource: sourcePaths.size === 1 && cluster.every((index) => index.normalizedSourcePath),
        conflictPairCount,
        sharedTargetLabels: sharedTargetLabels.slice(0, 3),
      } satisfies BranchGroupSuggestion;
    })
    .filter((suggestion): suggestion is BranchGroupSuggestion => suggestion !== null)
    .sort(
      (left, right) =>
        right.similarityPercent - left.similarityPercent ||
        right.sharedFileCount - left.sharedFileCount ||
        compareNaturalText(left.suggestedName, right.suggestedName),
    );
}
