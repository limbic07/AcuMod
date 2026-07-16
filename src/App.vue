<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import AppSidebar, { type WorkspaceView } from "./components/AppSidebar.vue";
import AppTopbar from "./components/AppTopbar.vue";
import BranchGroupSuggestionDialog from "./components/BranchGroupSuggestionDialog.vue";
import FloatingAgentPanel from "./components/FloatingAgentPanel.vue";
import ModCategoryManager from "./components/ModCategoryManager.vue";
import ModLibraryTable from "./components/ModLibraryTable.vue";
import ModLibraryToolbar from "./components/ModLibraryToolbar.vue";
import OperationStatusBar from "./components/OperationStatusBar.vue";
import {
  buildBranchGroupSuggestions,
  type BranchGroupSuggestion,
  type BranchGroupSuggestionSelection,
} from "./domain/branchGroupSuggestions";
import { earliestArmorMenuOrder } from "./domain/armorMenuOrder";
import { armorTargetDisplayLabel } from "./domain/armorLabels";
import { compareNaturalText } from "./domain/textSort";
import { getAppInfo, type AppInfo } from "./api/app";
import { listenOperationProgress, type OperationProgress } from "./api/operations";
import {
  detectGameDirectory,
  getGameDirectoryStatus,
  getGameTextSettings,
  saveGameDirectory,
  saveGameTextLanguage,
  type GameDirectoryStatus,
} from "./api/game";
import {
  localizeGameText,
  localizeGameTexts,
  type GameTextLanguage,
} from "./domain/gameText";
import {
  applyModRemap,
  applyConflictOrder,
  batchUpdateMods,
  createModBranchGroup,
  createModCategory,
  deleteModCategory,
  disableMod,
  enableMod,
  getModLibraryStatus,
  getModRemapDetails,
  getModWorkspaceSnapshot,
  refreshModWorkspaceSnapshot,
  installModFromArchive,
  installModBranches,
  installModFromFolder,
  importLegacyBoxMods,
  moveConflictParticipant,
  moveModLibraryItems,
  openInstalledModFolder,
  previewApplyConflictOrder,
  previewEnableMod,
  previewModImport,
  previewModRemap,
  previewRestoreAllMods,
  previewUninstallMod,
  renameModCategory,
  renameModBranchGroup,
  removeModsFromBranchGroup,
  replaceModLibraryOrder,
  refreshGameModStates,
  restoreModLibraryImportOrder,
  restoreAllMods,
  scanLegacyBoxMods,
  uninstallMod,
  updateModCategories,
  updateModMetadata,
  type BatchModAction,
  type InstalledModList,
  type InstalledModSummary,
  type LegacyBoxImportResult,
  type LegacyBoxMod,
  type LegacyBoxScan,
  type ModConflictReport,
  type ModDeploymentConflict,
  type ModImportPreview,
  type ModInstallResult,
  type ModLibraryStatus,
  type ModStateSyncResult,
  type ModMetadataPatch,
  type ModRemapDetails,
  type ModelRemapGroup,
  type ModelReplacement,
  type ModCategory,
  type ModCategoryAssignment,
  type ModBranchGroup,
  type ModMetadataUpdateResult,
} from "./api/modLibrary";

const MANUAL_SLINGER_TARGET = "__manual_slinger_target__";

type ConfirmationTone = "default" | "danger";

interface ConfirmationRequest {
  title: string;
  message: string;
  details?: string[];
  conflicts?: ModDeploymentConflict[];
  confirmLabel: string;
  cancelLabel?: string;
  tone?: ConfirmationTone;
}

const appInfo = ref<AppInfo | null>(null);
const gameStatus = ref<GameDirectoryStatus | null>(null);
const gameTextLanguage = ref<GameTextLanguage>("simplifiedChinese");
const modLibraryStatus = ref<ModLibraryStatus | null>(null);
const installedModList = ref<InstalledModList | null>(null);
const importPreview = ref<ModImportPreview | null>(null);
const installResult = ref<ModInstallResult | null>(null);
const conflictReport = ref<ModConflictReport | null>(null);
const modCategories = ref<ModCategory[]>([]);
const modBranchGroups = ref<ModBranchGroup[]>([]);
const remapDetails = ref<ModRemapDetails | null>(null);
const selectedRemapGroupKey = ref("");
const selectedRemapTargetId = ref("");
const manualSlingerTargetId = ref("");
const remapSaveWarnings = ref<string[]>([]);
const remapError = ref("");
const isApplyingRemap = ref(false);
const manualPath = ref("");
const importPath = ref("");
const archivePath = ref("");
const legacyBoxPath = ref("");
const legacyBoxScan = ref<LegacyBoxScan | null>(null);
const legacyBoxImportResult = ref<LegacyBoxImportResult | null>(null);
const legacyBoxStateSyncResult = ref<ModStateSyncResult | null>(null);
const selectedLegacyBoxModuleIds = ref<string[]>([]);
const selectedLegacyBoxModuleId = ref("");
const candidateImportSourcePath = ref("");
const candidateOriginalSourcePath = ref<string | null>(null);
const selectedCandidateRootPaths = ref<string[]>([]);
const candidateBranchNames = ref<Record<string, string>>({});
const candidateImportMode = ref<"branchGroup" | "standalone">("branchGroup");
const candidateGroupName = ref("");
const candidateSelectionSection = ref<HTMLElement | null>(null);
const appError = ref("");
const gameError = ref("");
const gameTextError = ref("");
const modLibraryError = ref("");
const importError = ref("");
const archiveError = ref("");
const legacyBoxError = ref("");
const deploymentError = ref("");
const isLoadingApp = ref(false);
const isLoadingGame = ref(false);
const isSavingGameTextLanguage = ref(false);
const isLoadingModLibrary = ref(false);
const isRefreshingModViews = ref(false);
const isPreviewingImport = ref(false);
const isInstallingMod = ref(false);
const isInstallingArchive = ref(false);
const isScanningLegacyBox = ref(false);
const isImportingLegacyBox = ref(false);
const isRefreshingGameModStates = ref(false);
const activeModAction = ref("");
const activeBatchAction = ref<BatchModAction | "">("");
const batchOperationSummary = ref("");
const openingModFolderId = ref("");
const metadataSavingModId = ref("");
const metadataErrorModId = ref("");
const metadataError = ref("");
const isUpdatingBranchGroups = ref(false);
const isBranchSuggestionOpen = ref(false);
const branchGroupSuggestions = ref<BranchGroupSuggestion[]>([]);
const branchSuggestionError = ref("");
const isRestoringAll = ref(false);
const isApplyingConflict = ref(false);
const activeView = ref<WorkspaceView>("library");
const isAgentPanelOpen = ref(false);
const selectedConflictGroupId = ref("");
const isDragActive = ref(false);
const isHandlingDrop = ref(false);
const pendingDropPath = ref("");
const dragError = ref("");
const conflictActionError = ref("");
const isCategoryAction = ref(false);
const isCategoryManagerOpen = ref(false);
const pendingCategoryModIds = ref<string[]>([]);
const categoryError = ref("");
const modSearchQuery = ref("");
const modCategoryFilter = ref("all");
const modStatusFilter = ref("all");
const modConflictFilter = ref("all");
type ModSortMode = "manual" | "installation" | "name" | "category" | "replacement";

const modSort = ref<ModSortMode>("manual");
const modSortDirection = ref<"asc" | "desc">("asc");
const prioritizeBranchGroups = ref(true);
const reorderingModId = ref("");
const isUpdatingModOrder = ref(false);
const confirmationRequest = ref<ConfirmationRequest | null>(null);
const confirmationCancelButton = ref<HTMLButtonElement | null>(null);
let stopDragListener: (() => void) | undefined;
let stopOperationProgressListener: (() => void) | undefined;
let clearOperationStatusTimer: ReturnType<typeof setTimeout> | undefined;
let resolveConfirmation: ((confirmed: boolean) => void) | undefined;
const activeOperation = ref<OperationProgress | null>(null);

function userFacingError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  if (message && !/^[A-Za-z]/.test(message.trim())) {
    return message;
  }
  console.error(message);
  return "操作失败，请检查输入内容、文件权限和游戏目录设置。";
}

function requestConfirmation(request: ConfirmationRequest): Promise<boolean> {
  resolveConfirmation?.(false);
  confirmationRequest.value = request;
  void nextTick(() => confirmationCancelButton.value?.focus());

  return new Promise((resolve) => {
    resolveConfirmation = resolve;
  });
}

function finishConfirmation(confirmed: boolean) {
  const resolver = resolveConfirmation;
  resolveConfirmation = undefined;
  confirmationRequest.value = null;
  resolver?.(confirmed);
}

const statusLabel = computed(() => {
  if (!gameStatus.value) {
    return "未读取";
  }

  if (gameStatus.value.isValid) {
    return "已配置";
  }

  if (gameStatus.value.isConfigured) {
    return "配置失效";
  }

  return "未配置";
});

const statusClass = computed(() => {
  if (!gameStatus.value) {
    return "neutral";
  }

  return gameStatus.value.isValid ? "success" : "warning";
});

const gameStatusMessage = computed(() => {
  if (!gameStatus.value) {
    return "Loading...";
  }
  if (gameStatus.value.isValid) {
    return "游戏目录可用。";
  }
  return gameStatus.value.isConfigured
    ? "已保存的游戏目录当前不可用，请重新设置。"
    : "尚未设置游戏目录。";
});

function gameSourceLabel(source: string) {
  return source === "autoDetection" ? "自动检测" : source === "savedConfig" ? "已保存配置" : "未设置";
}

function localizedGameText(value: string) {
  return localizeGameText(value, gameTextLanguage.value);
}

const importStatusLabel = computed(() => {
  if (!importPreview.value) {
    return "未识别";
  }

  if (importPreview.value.status === "ready") {
    return "可导入";
  }

  if (importPreview.value.status === "ambiguous") {
    return "多候选";
  }

  if (importPreview.value.requiresGameRootConfirmation) {
    return "需确认";
  }

  return "无效";
});

const importStatusClass = computed(() => {
  if (!importPreview.value) {
    return "neutral";
  }

  return importPreview.value.status === "ready" ? "success" : "warning";
});

const importHeaderMessage = computed(() => {
  const preview = importPreview.value;
  if (!preview) {
    return "选择 MOD 文件夹或压缩包开始识别。";
  }
  if (preview.status === "ready") {
    return "已识别 MOD 内容，可以导入本地 MOD 库。";
  }
  if (preview.status === "ambiguous") {
    return "识别到多个 MOD 版本，请选择需要导入的内容。";
  }
  if (preview.requiresGameRootConfirmation) {
    return "未识别到 nativePC 结构，请确认此 MOD 是否安装到游戏根目录。";
  }
  return "未找到可导入的 MOD 内容。";
});

function detectionMethodLabel(method: string) {
  const labels: Record<string, string> = {
    multipleCandidates: "多个可选内容根",
    emptyDirectory: "空目录",
    userConfirmedGameRoot: "用户确认游戏根目录",
    unrecognizedRoot: "未识别目录结构",
    invalidSource: "无效来源",
    nativePcDirectory: "nativePC 目录",
    selectedNativePcChildDirectory: "nativePC 一级目录",
    nativePcChildDirectory: "常见 nativePC 目录",
  };
  return labels[method] ?? "未知方式";
}

function deployRootLabel(deployRoot: string) {
  return deployRoot === "nativePC"
    ? "nativePC"
    : deployRoot === "gameRoot"
      ? "游戏根目录"
      : "未确定";
}

function legacyBoxDeploymentLabel(mod: LegacyBoxMod) {
  const labels = {
    fullyMatched: "文件完全一致",
    partiallyMatched: "部分文件一致",
    notDeployed: "未检测到部署",
    different: "文件不一致",
    unavailable: "无法核验",
  } as const;
  return labels[mod.deployment.status] ?? "无法核验";
}

function legacyBoxDeploymentClass(mod: LegacyBoxMod) {
  if (mod.deployment.status === "fullyMatched") {
    return "success";
  }
  if (mod.deployment.status === "notDeployed" || mod.deployment.status === "unavailable") {
    return "neutral";
  }
  return "warning";
}

function legacyBoxRecordLabel(mod: LegacyBoxMod) {
  return mod.boxEnabled ? "盒子记录：已启用" : "盒子记录：未启用";
}

function selectAllLegacyBoxMods() {
  if (!legacyBoxScan.value) {
    return;
  }

  selectedLegacyBoxModuleIds.value = legacyBoxScan.value.mods.map((mod) => mod.moduleId);
}

function clearLegacyBoxModSelection() {
  selectedLegacyBoxModuleIds.value = [];
}

function formatFileSize(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

const previewedFiles = computed(() => importPreview.value?.files.slice(0, 12) ?? []);
const installedFiles = computed(() => installResult.value?.files.slice(0, 12) ?? []);
const selectedLegacyBoxMod = computed<LegacyBoxMod | null>(() => {
  const selectedModuleId = selectedLegacyBoxModuleId.value;
  return legacyBoxScan.value?.mods.find((mod) => mod.moduleId === selectedModuleId) ?? null;
});
const previewedLegacyBoxFiles = computed(() => selectedLegacyBoxMod.value?.files.slice(0, 80) ?? []);
const selectedLegacyBoxModCount = computed(() => selectedLegacyBoxModuleIds.value.length);
const installedMods = computed(() => installedModList.value?.mods ?? []);

function installedModName(modId: string): string {
  const mod = installedMods.value.find((candidate) => candidate.id === modId);
  if (!mod) {
    return modId;
  }
  const group = modBranchGroups.value.find((candidate) => candidate.modIds.includes(modId));
  return group ? `${group.name}（${mod.name}）` : mod.name;
}
const isOperationInProgress = computed(
  () => activeOperation.value !== null && !activeOperation.value.terminal,
);

function categoryLabel(category: ModCategory) {
  const parent = category.parentId
    ? modCategories.value.find((candidate) => candidate.id === category.parentId)
    : null;
  return parent ? `${parent.name}·${category.name}` : category.name;
}

function visibleModCategories(mod: InstalledModSummary) {
  return mod.categories.length
    ? mod.categories.map((category) => categoryLabel(category))
    : ["未分类"];
}

const availableModCategories = computed(() =>
  modCategories.value.map((category) => ({
    id: category.id,
    label: categoryLabel(category),
  })),
);

function matchesCategoryFilter(mod: InstalledModSummary) {
  if (modCategoryFilter.value === "all") {
    return true;
  }

  const selectedCategoryId = modCategoryFilter.value;
  const selectedCategoryAndChildren = new Set([
    selectedCategoryId,
    ...modCategories.value
      .filter((category) => category.parentId === selectedCategoryId)
      .map((category) => category.id),
  ]);
  return mod.categoryIds.some((categoryId) => selectedCategoryAndChildren.has(categoryId));
}

const filteredInstalledMods = computed(() => {
  const searchText = modSearchQuery.value.trim().toLocaleLowerCase();

  return installedMods.value.filter((installedMod) => {
    if (!matchesCategoryFilter(installedMod)) {
      return false;
    }

    if (modStatusFilter.value === "enabled" && !installedMod.enabled) {
      return false;
    }

    if (modStatusFilter.value === "disabled" && installedMod.enabled) {
      return false;
    }

    const hasConflict = conflictingModIds.value.has(installedMod.id);
    if (modConflictFilter.value === "conflict" && !hasConflict) {
      return false;
    }

    if (modConflictFilter.value === "normal" && hasConflict) {
      return false;
    }

    if (!searchText) {
      return true;
    }

    const searchableText = [
      installedMod.name,
      installedMod.originalName,
      installedMod.note,
      modBranchGroups.value.find((group) => group.modIds.includes(installedMod.id))?.name ?? "",
      ...visibleModCategories(installedMod),
      ...installedMod.modelReplacements.flatMap((replacement) => [
        replacement.modelKind,
        replacement.subKind,
        replacement.modelId,
        ...replacement.gameIds,
        ...replacement.displayNames,
        ...localizeGameTexts(replacement.displayNames, gameTextLanguage.value),
        ...replacement.associations.flatMap((association) => [
          association.modelId,
          ...association.displayNames,
          ...localizeGameTexts(association.displayNames, gameTextLanguage.value),
        ]),
      ]),
    ]
      .join(" ")
      .toLocaleLowerCase();
    return searchableText.includes(searchText);
  });
});
function groupModsIntoContiguousBlocks(mods: InstalledModSummary[]) {
  const groupByModId = new Map<string, string>();
  for (const group of modBranchGroups.value) {
    for (const modId of group.modIds) {
      groupByModId.set(modId, group.id);
    }
  }
  const groupedMods = new Map<string, InstalledModSummary[]>();
  for (const mod of mods) {
    const groupId = groupByModId.get(mod.id);
    if (groupId) {
      groupedMods.set(groupId, [...(groupedMods.get(groupId) ?? []), mod]);
    }
  }
  const seenGroups = new Set<string>();
  return mods.flatMap((mod) => {
    const groupId = groupByModId.get(mod.id);
    if (!groupId) {
      return [mod];
    }
    if (seenGroups.has(groupId)) {
      return [];
    }
    seenGroups.add(groupId);
    return groupedMods.get(groupId) ?? [];
  });
}

interface SortableModLibraryItem {
  groupId: string | null;
  name: string;
  categories: string;
  categoryRank: number;
  replacements: string;
  armorMenuOrder: number;
  installedAtUnixSeconds: number;
  mods: InstalledModSummary[];
}

const MOD_CATEGORY_SORT_ORDER = [
  "防具",
  "武器",
  "发型",
  "投射器",
  "随从防具",
  "随从武器",
  "猎虫",
  "挂件",
  "人物语音",
  "武器语音",
  "NPC",
  "脸型",
  "怪物",
  "噗吱猪服装",
  "插件",
  "数据修改",
];

function categorySortRank(categories: string[]) {
  return Math.min(
    ...categories.map((category) => {
      const rootCategory = category.split("·", 1)[0]?.trim() || "未分类";
      if (rootCategory === "未分类") {
        return MOD_CATEGORY_SORT_ORDER.length + 1;
      }
      const rank = MOD_CATEGORY_SORT_ORDER.indexOf(rootCategory);
      // 用户自建分类排在内置分类之后、未分类之前，并继续使用自然文字排序。
      return rank >= 0 ? rank : MOD_CATEGORY_SORT_ORDER.length;
    }),
  );
}

function sortInstalledModsForLibrary(
  sourceMods: InstalledModSummary[],
  sort: ModSortMode,
  direction: "asc" | "desc",
  branchGroupsFirst: boolean,
) {
  if (sort === "manual") {
    return groupModsIntoContiguousBlocks([...sourceMods]);
  }

  const sourceIds = new Set(sourceMods.map((mod) => mod.id));
  const groupedModIds = new Set(modBranchGroups.value.flatMap((group) => group.modIds));
  const items: SortableModLibraryItem[] = [];

  // 分支组是一级列表项。自动排序时先整理为组，避免按成员排序后把组的位置拆散。
  for (const group of modBranchGroups.value) {
    const groupIds = new Set(group.modIds);
    const visibleMembers = sourceMods.filter(
      (mod) => sourceIds.has(mod.id) && groupIds.has(mod.id),
    );
    if (!visibleMembers.length) {
      continue;
    }
    const allMembers = installedMods.value.filter((mod) => groupIds.has(mod.id));
    const categoryNames = [...new Set(allMembers.flatMap(visibleModCategories))].sort(
      compareNaturalText,
    );
    const replacements = [...new Set(allMembers.map(summarizeModReplacements).filter(Boolean))]
      .sort(compareNaturalText)
      .join("、");
    items.push({
      groupId: group.id,
      name: group.name,
      categories: categoryNames.join("、"),
      categoryRank: categorySortRank(categoryNames),
      replacements,
      armorMenuOrder: earliestArmorMenuOrder(
        allMembers.flatMap((mod) => mod.modelReplacements),
      ),
      installedAtUnixSeconds: Math.min(...allMembers.map((mod) => mod.installedAtUnixSeconds)),
      mods: visibleMembers,
    });
  }

  for (const mod of sourceMods) {
    if (groupedModIds.has(mod.id)) {
      continue;
    }
    const categoryNames = visibleModCategories(mod);
    items.push({
      groupId: null,
      name: mod.name,
      categories: categoryNames.join("、"),
      categoryRank: categorySortRank(categoryNames),
      replacements: summarizeModReplacements(mod),
      armorMenuOrder: earliestArmorMenuOrder(mod.modelReplacements),
      installedAtUnixSeconds: mod.installedAtUnixSeconds,
      mods: [mod],
    });
  }

  items.sort((left, right) => {
    // 用户可选择让分支组形成独立的顶部区域；关闭后按普通一级列表项参与排序。
    if (branchGroupsFirst && Boolean(left.groupId) !== Boolean(right.groupId)) {
      return left.groupId ? -1 : 1;
    }

    let comparison = 0;
    if (sort === "name") {
      comparison = compareNaturalText(left.name, right.name);
    } else if (sort === "category") {
      comparison =
        left.categoryRank - right.categoryRank ||
        left.armorMenuOrder - right.armorMenuOrder ||
        compareNaturalText(left.categories, right.categories) ||
        compareNaturalText(left.name, right.name);
    } else if (sort === "replacement") {
      comparison =
        compareNaturalText(left.replacements, right.replacements) ||
        compareNaturalText(left.name, right.name);
    } else {
      comparison =
        left.installedAtUnixSeconds - right.installedAtUnixSeconds ||
        compareNaturalText(left.name, right.name);
    }
    return direction === "asc" ? comparison : -comparison;
  });

  return items.flatMap((item) => item.mods);
}

const displayedInstalledMods = computed(() => {
  return sortInstalledModsForLibrary(
    filteredInstalledMods.value,
    modSort.value,
    modSortDirection.value,
    prioritizeBranchGroups.value,
  );
});
const enabledModCount = computed(
  () => installedMods.value.filter((installedMod) => installedMod.enabled).length,
);
const conflictGroups = computed(() => conflictReport.value?.groups ?? []);
const conflictingModIds = computed(
  () =>
    new Set(
      conflictGroups.value.flatMap((group) =>
        group.participants.map((participant) => participant.modId),
      ),
    ),
);
const conflictPartnerNames = computed<Record<string, string[]>>(() => {
  const partnersByModId: Record<string, string[]> = {};

  for (const group of conflictGroups.value) {
    const enabledParticipants = group.participants.filter((participant) => participant.enabled);
    for (const participant of enabledParticipants) {
      const partners = (partnersByModId[participant.modId] ??= []);
      for (const otherParticipant of enabledParticipants) {
        if (
          otherParticipant.modId !== participant.modId &&
          !partners.includes(installedModName(otherParticipant.modId))
        ) {
          partners.push(installedModName(otherParticipant.modId));
        }
      }
    }
  }

  return partnersByModId;
});
const selectedConflictGroup = computed(() =>
  conflictGroups.value.find(
    (group) => group.groupId === selectedConflictGroupId.value,
  ),
);
const selectedRemapGroup = computed(() =>
  remapDetails.value?.groups.find((group) => group.groupKey === selectedRemapGroupKey.value),
);
const visibleRemapWarnings = computed(() =>
  [...new Set([...(remapDetails.value?.warnings ?? []), ...remapSaveWarnings.value])],
);

function modelReplacementTitle(replacement: ModelReplacement) {
  if (replacement.modelKind === "weapon") {
    const modelPart = replacement.modelPart === "accessory" ? "附件模型" : "主模型";
    return `武器 · ${replacement.subKind} · ${modelPart}`;
  }

  if (replacement.modelKind === "armor") {
    if (replacement.modelPart === "set") {
      return "防具套装";
    }

    return `防具 · ${replacement.subKind}`;
  }

  if (replacement.modelKind === "palicoWeapon") {
    return "随从武器";
  }

  if (replacement.modelKind === "palicoArmor") {
    return `随从防具 · ${replacement.subKind}`;
  }

  return replacement.subKind;
}

function modelKindLabel(modelKind: string) {
  const labels: Record<string, string> = {
    weapon: "武器",
    armor: "防具",
    hair: "发型",
    palicoWeapon: "随从武器",
    palicoArmor: "随从防具",
    kinsect: "猎虫",
    pendant: "挂件",
    npc: "NPC",
    slinger: "投射器",
    voice: "人物语音",
    weaponVoice: "武器语音",
    plugin: "插件",
    face: "脸型",
    monster: "怪物",
    poogie: "噗吱猪服装",
    furniture: "家具",
    playerAccessory: "玩家附件",
    palicoAccessory: "随从附件",
  };

  return labels[modelKind] ?? modelKind;
}

function summarizeModReplacements(mod: InstalledModSummary) {
  if (!mod.modelReplacements.length) {
    return "未识别到游戏内替换目标";
  }

  const summaries = mod.modelReplacements.map((replacement) => {
    const target = summarizeModelNames(replacement);
    return `${modelKindLabel(replacement.modelKind)}：${target}`;
  });
  const visibleSummaries = summaries.slice(0, 2);
  const remainingCount = summaries.length - visibleSummaries.length;

  return remainingCount > 0
    ? `${visibleSummaries.join("；")}；另有 ${remainingCount} 项`
    : visibleSummaries.join("；");
}

function summarizeSharedModelTarget(target: {
  modelKind: string;
  subKind: string;
  modelId: string;
  displayNames: string[];
}) {
  if (target.modelKind === "armor" && target.subKind.includes("防具套装")) {
    return `防具 · ${armorSetTargetLabel(target)}`;
  }

  const name = target.displayNames[0]
    ? localizedGameText(target.displayNames[0])
    : target.modelId;
  return `${modelKindLabel(target.modelKind)} · ${target.subKind} · ${name}`;
}

function summarizeModelNames(replacement: ModelReplacement) {
  if (replacement.modelKind === "armor" && replacement.modelPart === "set") {
    return armorSetTargetLabel(replacement);
  }

  if (!replacement.displayNames.length) {
    return replacement.recognitionSource === "pathPattern"
      ? "当前 ID 表暂无名称，已按资源路径识别"
      : "已识别模型 ID，当前无可用游戏名称";
  }

  const visibleNames = localizeGameTexts(
    replacement.displayNames.slice(0, 4),
    gameTextLanguage.value,
  );
  const remainingCount = replacement.displayNames.length - visibleNames.length;
  return remainingCount > 0
    ? `${visibleNames.join("、")}，另有 ${remainingCount} 个共用模型名称`
    : visibleNames.join("、");
}

function armorSetTargetLabel(target: { displayNames: string[]; modelId: string }) {
  return armorTargetDisplayLabel(target.displayNames, target.modelId, localizedGameText);
}

function summarizeModelAssociations(replacement: ModelReplacement) {
  const armorNames = replacement.associations
    .filter((association) => association.modelKind === "armor")
    .map((association) =>
      association.displayNames[0]
        ? localizedGameText(association.displayNames[0])
        : association.modelId,
    );
  return armorNames.length ? `关联防具：${armorNames.join("、")}` : "";
}

function summarizeGameIds(replacement: ModelReplacement) {
  if (!replacement.gameIds.length) {
    return "";
  }

  const visibleIds = replacement.gameIds.slice(0, 5);
  const remainingCount = replacement.gameIds.length - visibleIds.length;
  return remainingCount > 0
    ? `游戏 ID ${visibleIds.join(", ")} 等 ${replacement.gameIds.length} 个`
    : `游戏 ID ${visibleIds.join(", ")}`;
}

function summarizeAffectedParts(replacement: ModelReplacement) {
  if (replacement.modelKind === "armor" && replacement.modelPart === "set") {
    return "";
  }

  return replacement.affectedParts.length
    ? `替换部位：${replacement.affectedParts.join("、")}`
    : "";
}

async function loadAppInfo() {
  isLoadingApp.value = true;

  try {
    appInfo.value = await getAppInfo();
    appError.value = "";
  } catch (error) {
    appError.value = userFacingError(error);
  } finally {
    isLoadingApp.value = false;
  }
}

async function loadGameStatus() {
  isLoadingGame.value = true;

  try {
    const status = await getGameDirectoryStatus();
    applyGameStatus(status);
    gameError.value = "";
  } catch (error) {
    gameError.value = userFacingError(error);
  } finally {
    isLoadingGame.value = false;
  }
}

async function loadGameTextSettings() {
  try {
    gameTextLanguage.value = (await getGameTextSettings()).language;
    gameTextError.value = "";
  } catch (error) {
    gameTextError.value = userFacingError(error);
  }
}

async function updateGameTextLanguage(event: Event) {
  const language = (event.target as HTMLSelectElement).value as GameTextLanguage;
  const previousLanguage = gameTextLanguage.value;
  gameTextLanguage.value = language;
  isSavingGameTextLanguage.value = true;
  gameTextError.value = "";

  try {
    gameTextLanguage.value = (await saveGameTextLanguage(language)).language;
  } catch (error) {
    gameTextLanguage.value = previousLanguage;
    gameTextError.value = userFacingError(error);
  } finally {
    isSavingGameTextLanguage.value = false;
  }
}

async function loadModLibraryStatus() {
  isLoadingModLibrary.value = true;

  try {
    modLibraryStatus.value = await getModLibraryStatus();
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    isLoadingModLibrary.value = false;
  }
}

async function refreshModViews() {
  isRefreshingModViews.value = true;

  try {
    const snapshot = await refreshModWorkspaceSnapshot();
    installedModList.value = snapshot.installedMods;
    modCategories.value = snapshot.categories.categories;
    conflictReport.value = snapshot.conflictReport;
    modBranchGroups.value = snapshot.branchGroups;
    modLibraryError.value = "";
    categoryError.value = "";
    syncCategoryFilter();
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    isRefreshingModViews.value = false;
  }
}

function preserveCurrentModOrder(nextMods: InstalledModSummary[]) {
  const nextById = new Map(nextMods.map((mod) => [mod.id, mod]));
  const ordered = installedMods.value
    .map((mod) => nextById.get(mod.id))
    .filter((mod): mod is InstalledModSummary => Boolean(mod));
  const knownIds = new Set(ordered.map((mod) => mod.id));

  // 新导入项仍按后端给出的顺序补入；状态更新只替换原位置上的数据。
  return [...ordered, ...nextMods.filter((mod) => !knownIds.has(mod.id))];
}

async function loadModViewsFromSnapshot(options?: { preserveModOrder?: boolean }) {
  isRefreshingModViews.value = true;

  try {
    const snapshot = await getModWorkspaceSnapshot();
    if (options?.preserveModOrder && installedModList.value) {
      snapshot.installedMods.mods = preserveCurrentModOrder(snapshot.installedMods.mods);
    }
    installedModList.value = snapshot.installedMods;
    modCategories.value = snapshot.categories.categories;
    conflictReport.value = snapshot.conflictReport;
    modBranchGroups.value = snapshot.branchGroups;
    modLibraryError.value = "";
    categoryError.value = "";
    syncCategoryFilter();
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    isRefreshingModViews.value = false;
  }
}

async function refreshCurrentWorkspace() {
  if (activeView.value === "settings") {
    await Promise.all([loadGameStatus(), loadGameTextSettings(), loadAppInfo()]);
    return;
  }

  if (activeView.value === "import") {
    await loadModLibraryStatus();
    return;
  }

  await refreshModViews();
}

function updateOperationStatus(progress: OperationProgress) {
  activeOperation.value = progress;

  if (!progress.terminal) {
    if (clearOperationStatusTimer) {
      clearTimeout(clearOperationStatusTimer);
      clearOperationStatusTimer = undefined;
    }
    return;
  }

  clearOperationStatusTimer = setTimeout(() => {
    if (activeOperation.value?.operationId === progress.operationId) {
      activeOperation.value = null;
    }
  }, 900);
}

function syncCategoryFilter() {
  if (
    modCategoryFilter.value !== "all" &&
    !availableModCategories.value.some((category) => category.id === modCategoryFilter.value)
  ) {
    modCategoryFilter.value = "all";
  }
}

async function saveModMetadata(
  mod: InstalledModSummary,
  patch: ModMetadataPatch,
): Promise<boolean> {
  metadataSavingModId.value = mod.id;
  metadataErrorModId.value = "";
  metadataError.value = "";

  try {
    const result = await updateModMetadata(mod.id, patch);
    applyMetadataUpdateResults([result]);
    syncCategoryFilter();
    return true;
  } catch (error) {
    metadataErrorModId.value = mod.id;
    metadataError.value = userFacingError(error);
    return false;
  } finally {
    metadataSavingModId.value = "";
  }
}

function applyMetadataUpdateResults(results: ModMetadataUpdateResult[]) {
  if (!installedModList.value) {
    return;
  }
  const resultByModId = new Map(results.map((result) => [result.modId, result]));
  installedModList.value = {
    ...installedModList.value,
    mods: installedModList.value.mods.map((installed) => {
      const result = resultByModId.get(installed.id);
      return result
        ? {
            ...installed,
            name: result.name,
            originalName: result.originalName,
            note: result.note,
            categoryIds: result.categoryIds,
            categories: result.categories,
          }
        : installed;
    }),
  };
}

async function saveModCategoryAssignments(
  assignments: ModCategoryAssignment[],
  busyId: string,
): Promise<boolean> {
  metadataSavingModId.value = busyId;
  metadataErrorModId.value = "";
  metadataError.value = "";
  categoryError.value = "";
  try {
    const result = await updateModCategories(assignments);
    applyMetadataUpdateResults(result.mods);
    syncCategoryFilter();
    return true;
  } catch (error) {
    metadataErrorModId.value = busyId;
    metadataError.value = userFacingError(error);
    categoryError.value = metadataError.value;
    return false;
  } finally {
    metadataSavingModId.value = "";
  }
}

async function updateBranchGroupCategory(
  group: ModBranchGroup,
  categoryId: string | null,
  selected: boolean,
) {
  const memberIds = new Set(group.modIds);
  const assignments = installedMods.value
    .filter((mod) => memberIds.has(mod.id))
    .map((mod) => {
      const categoryIds = new Set(categoryId === null ? [] : mod.categoryIds);
      if (categoryId !== null) {
        if (selected) {
          categoryIds.add(categoryId);
        } else {
          categoryIds.delete(categoryId);
        }
      }
      return { modId: mod.id, categoryIds: [...categoryIds] };
    });
  await saveModCategoryAssignments(assignments, group.id);
}

function openCategoryManager(mod?: InstalledModSummary) {
  pendingCategoryModIds.value = mod ? [mod.id] : [];
  categoryError.value = "";
  isCategoryManagerOpen.value = true;
}

function openCategoryManagerForBranchGroup(group: ModBranchGroup) {
  pendingCategoryModIds.value = [...group.modIds];
  categoryError.value = "";
  isCategoryManagerOpen.value = true;
}

function closeCategoryManager() {
  if (isCategoryAction.value) {
    return;
  }

  isCategoryManagerOpen.value = false;
  pendingCategoryModIds.value = [];
  categoryError.value = "";
}

async function createCategory(name: string, parentId: string | null) {
  isCategoryAction.value = true;
  categoryError.value = "";
  let shouldCloseDialog = false;

  try {
    const category = await createModCategory(name, parentId);
    modCategories.value = [...modCategories.value, category].sort((left, right) =>
      compareNaturalText(left.name, right.name),
    );
    const targetModIds = new Set(pendingCategoryModIds.value);
    const targets = installedMods.value.filter((mod) => targetModIds.has(mod.id));

    if (targets.length === 1) {
      shouldCloseDialog = await saveModMetadata(targets[0], {
        categoryIds: [...new Set([...targets[0].categoryIds, category.id])],
      });
      if (!shouldCloseDialog) {
        categoryError.value = "分类已创建，但未能应用到目标 MOD。";
      }
    } else if (targets.length > 1) {
      shouldCloseDialog = await saveModCategoryAssignments(
        targets.map((mod) => ({
          modId: mod.id,
          categoryIds: [...new Set([...mod.categoryIds, category.id])],
        })),
        "branch-group-category",
      );
      if (!shouldCloseDialog) {
        categoryError.value = "分类已创建，但未能应用到目标分支组。";
      }
    }
  } catch (error) {
    categoryError.value = userFacingError(error);
  } finally {
    isCategoryAction.value = false;
  }

  if (shouldCloseDialog) {
    closeCategoryManager();
  }
}

async function renameCategory(categoryId: string, name: string) {
  isCategoryAction.value = true;
  categoryError.value = "";

  try {
    const updated = await renameModCategory(categoryId, name);
    modCategories.value = modCategories.value.map((category) =>
      category.id === updated.id ? updated : category,
    );
    if (installedModList.value) {
      installedModList.value = {
        ...installedModList.value,
        mods: installedModList.value.mods.map((installed) => ({
          ...installed,
          categories: installed.categories.map((category) =>
            category.id === updated.id ? updated : category,
          ),
        })),
      };
    }
    syncCategoryFilter();
  } catch (error) {
    categoryError.value = userFacingError(error);
  } finally {
    isCategoryAction.value = false;
  }
}

async function deleteCategory(category: ModCategory) {
  const hasChildren = modCategories.value.some((candidate) => candidate.parentId === category.id);
  const shouldDelete = await requestConfirmation({
    title: "删除分类",
    message: hasChildren
      ? `删除“${categoryLabel(category)}”后，使用它的 MOD 将移除此分类；其子分类会保留并成为顶级分类。`
      : `删除“${categoryLabel(category)}”后，使用它的 MOD 将移除此分类。`,
    confirmLabel: "删除分类",
    tone: "danger",
  });
  if (!shouldDelete) {
    return;
  }

  isCategoryAction.value = true;
  categoryError.value = "";

  try {
    await deleteModCategory(category.id);
    modCategories.value = modCategories.value
      .filter((candidate) => candidate.id !== category.id)
      .map((candidate) =>
        candidate.parentId === category.id ? { ...candidate, parentId: null } : candidate,
      );
    if (installedModList.value) {
      installedModList.value = {
        ...installedModList.value,
        mods: installedModList.value.mods.map((installed) => ({
          ...installed,
          categoryIds: installed.categoryIds.filter((id) => id !== category.id),
          categories: installed.categories
            .filter((candidate) => candidate.id !== category.id)
            .map((candidate) =>
              candidate.parentId === category.id
                ? { ...candidate, parentId: null }
                : candidate,
            ),
        })),
      };
    }
    syncCategoryFilter();
  } catch (error) {
    categoryError.value = userFacingError(error);
  } finally {
    isCategoryAction.value = false;
  }
}

async function reorderModLibraryItem(
  modIds: string[],
  targetModIds: string[],
  placeAfter: boolean,
) {
  const sourceIds = new Set(modIds);
  if (
    reorderingModId.value ||
    !modIds.length ||
    !targetModIds.length ||
    targetModIds.some((modId) => sourceIds.has(modId))
  ) {
    return;
  }

  reorderingModId.value = modIds[0];
  try {
    await moveModLibraryItems(modIds, targetModIds, placeAfter);
    reorderInstalledModsInMemory(modIds, targetModIds, placeAfter);
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    reorderingModId.value = "";
  }
}

function reorderInstalledModsInMemory(
  modIds: string[],
  targetModIds: string[],
  placeAfter: boolean,
) {
  const modList = installedModList.value;
  if (!modList) {
    return;
  }

  const sourceIds = new Set(modIds);
  const targetIds = new Set(targetModIds);
  const movedMods = modList.mods.filter((mod) => sourceIds.has(mod.id));
  const remainingMods = modList.mods.filter((mod) => !sourceIds.has(mod.id));
  const targetIndexes = remainingMods
    .map((mod, index) => (targetIds.has(mod.id) ? index : -1))
    .filter((index) => index >= 0);
  if (movedMods.length !== sourceIds.size || targetIndexes.length !== targetIds.size) {
    return;
  }
  const insertIndex = placeAfter
    ? Math.max(...targetIndexes) + 1
    : Math.min(...targetIndexes);
  remainingMods.splice(insertIndex, 0, ...movedMods);
  installedModList.value = { ...modList, mods: remainingMods };
}

function applyInstalledModOrderInMemory(modIds: string[]) {
  const modList = installedModList.value;
  if (!modList) {
    return;
  }
  const modsById = new Map(modList.mods.map((mod) => [mod.id, mod]));
  const orderedMods = modIds
    .map((modId) => modsById.get(modId))
    .filter((mod): mod is InstalledModSummary => Boolean(mod));
  if (orderedMods.length === modList.mods.length) {
    installedModList.value = { ...modList, mods: orderedMods };
  }
}

async function applyCurrentSortAsManualOrder() {
  if (modSort.value === "manual" || isUpdatingModOrder.value || !installedMods.value.length) {
    return;
  }
  isUpdatingModOrder.value = true;
  batchOperationSummary.value = "";
  try {
    // 搜索和筛选只影响可见结果；持久化时始终提交完整 MOD 库，避免遗漏隐藏项。
    const completeOrder = sortInstalledModsForLibrary(
      installedMods.value,
      modSort.value,
      modSortDirection.value,
      prioritizeBranchGroups.value,
    ).map((mod) => mod.id);
    const result = await replaceModLibraryOrder(completeOrder);
    applyInstalledModOrderInMemory(result.manualModIds);
    modSort.value = "manual";
    batchOperationSummary.value = result.message;
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    isUpdatingModOrder.value = false;
  }
}

async function restoreOriginalImportOrder() {
  if (isUpdatingModOrder.value || !installedMods.value.length) {
    return;
  }
  isUpdatingModOrder.value = true;
  batchOperationSummary.value = "";
  try {
    const result = await restoreModLibraryImportOrder();
    applyInstalledModOrderInMemory(result.manualModIds);
    modSort.value = "manual";
    batchOperationSummary.value = result.message;
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    isUpdatingModOrder.value = false;
  }
}

function openConflictManager() {
  if (!selectedConflictGroupId.value && conflictGroups.value.length) {
    selectedConflictGroupId.value = conflictGroups.value[0].groupId;
  }

  conflictActionError.value = "";
  activeView.value = "conflicts";
}

function selectWorkspace(view: WorkspaceView) {
  if (view === "conflicts") {
    openConflictManager();
    return;
  }

  activeView.value = view;
}

function selectConflict(groupId: string) {
  selectedConflictGroupId.value = groupId;
  conflictActionError.value = "";
}

async function runGameAction(action: () => Promise<GameDirectoryStatus>) {
  isLoadingGame.value = true;

  try {
    const status = await action();
    applyGameStatus(status);
    gameError.value = "";
  } catch (error) {
    gameError.value = userFacingError(error);
  } finally {
    isLoadingGame.value = false;
  }
}

function applyGameStatus(status: GameDirectoryStatus) {
  gameStatus.value = status;

  if (status.path) {
    manualPath.value = status.path;
  }
}

function autoDetectGameDirectory() {
  void runGameAction(detectGameDirectory);
}

function saveManualPath() {
  void runGameAction(() => saveGameDirectory(manualPath.value));
}

function importSourceDisplayName(path: string) {
  const pathParts = path.split(/[\\/]/).filter(Boolean);
  const fileName = pathParts[pathParts.length - 1] ?? "MOD 分支组";
  return fileName.replace(/\.(zip|7z|rar)$/i, "") || "MOD 分支组";
}

function prepareCandidateSelection(preview: ModImportPreview | null, originalSourcePath: string | null) {
  const candidates = preview?.candidates ?? [];
  selectedCandidateRootPaths.value = candidates.map((candidate) => candidate.rootPath);
  candidateBranchNames.value = Object.fromEntries(
    candidates.map((candidate, index) => [
      candidate.rootPath,
      candidate.suggestedName || `分支 ${index + 1}`,
    ]),
  );
  candidateImportMode.value = candidates.length > 1 ? "branchGroup" : "standalone";
  candidateGroupName.value = importSourceDisplayName(originalSourcePath || preview?.sourcePath || "");
}

async function revealCandidateSelection(preview: ModImportPreview | null) {
  if (!preview?.candidates.length) {
    return;
  }

  // 全局拖拽可能发生在任意工作区；候选生成后必须把用户带到实际可操作的位置。
  activeView.value = "import";
  await nextTick();
  candidateSelectionSection.value?.scrollIntoView({ behavior: "smooth", block: "start" });
  candidateSelectionSection.value?.focus({ preventScroll: true });
}

async function previewImportPath(allowGameRoot = false) {
  isPreviewingImport.value = true;

  try {
    installResult.value = null;
    const preview = await previewModImport(importPath.value, allowGameRoot);
    importPreview.value = preview;
    candidateImportSourcePath.value = preview.status === "ambiguous" ? preview.sourcePath : "";
    candidateOriginalSourcePath.value =
      preview.status === "ambiguous" ? preview.originalSourcePath : null;
    prepareCandidateSelection(preview, preview.originalSourcePath);
    await revealCandidateSelection(preview);
    importError.value = "";

    if (preview.requiresGameRootConfirmation) {
      const shouldUseGameRoot = await requestConfirmation({
        title: "确认游戏根目录导入",
        message: "未识别到 nativePC 或常见 nativePC 内部目录。这个 MOD 可能需要安装到游戏根目录。",
        confirmLabel: "继续识别",
      });

      if (shouldUseGameRoot) {
        await previewImportPath(true);
      }
    }
  } catch (error) {
    importError.value = userFacingError(error);
  } finally {
    isPreviewingImport.value = false;
  }
}

function runImportPreview() {
  void previewImportPath(false);
}

function confirmGameRootPreview() {
  void previewImportPath(true);
}

async function installPreviewedMod() {
  if (!importPreview.value || importPreview.value.status !== "ready") {
    return;
  }

  isInstallingMod.value = true;

  try {
    const allowGameRoot = importPreview.value.deployRoot === "gameRoot";
    installResult.value = await installModFromFolder(importPath.value, allowGameRoot);
    importError.value = "";
    await loadModLibraryStatus();
    await loadModViewsFromSnapshot();
  } catch (error) {
    importError.value = userFacingError(error);
  } finally {
    isInstallingMod.value = false;
  }
}

async function installArchive() {
  isInstallingArchive.value = true;

  try {
    const outcome = await installModFromArchive(archivePath.value, false);
    installResult.value = outcome.installResult;
    archiveError.value = "";
    importPreview.value = outcome.preview;
    candidateImportSourcePath.value = outcome.status === "ambiguous" ? outcome.sourcePath : "";
    candidateOriginalSourcePath.value =
      outcome.status === "ambiguous" ? outcome.originalArchivePath : null;
    prepareCandidateSelection(outcome.preview, outcome.originalArchivePath);
    await revealCandidateSelection(outcome.preview);
    await loadModLibraryStatus();
    await loadModViewsFromSnapshot();
  } catch (error) {
    archiveError.value = userFacingError(error);
  } finally {
    isInstallingArchive.value = false;
  }
}

async function installSelectedCandidate() {
  if (!candidateImportSourcePath.value || !selectedCandidateRootPaths.value.length) {
    return;
  }

  const selectedRootPaths = new Set(selectedCandidateRootPaths.value);
  const selectedCandidates = (importPreview.value?.candidates ?? []).filter((candidate) =>
    selectedRootPaths.has(candidate.rootPath),
  );
  const gameRootCandidates = selectedCandidates.filter(
    (candidate) => candidate.requiresGameRootConfirmation,
  );
  if (gameRootCandidates.length) {
    const shouldContinue = await requestConfirmation({
      title: "确认游戏根目录分支",
      message: `所选分支中有 ${gameRootCandidates.length} 个未识别到 nativePC 或常见内部目录，将按游戏根目录相对路径导入。`,
      details: gameRootCandidates.map(
        (candidate) => candidateBranchNames.value[candidate.rootPath] || candidate.relativePath,
      ),
      confirmLabel: "确认导入",
    });
    if (!shouldContinue) {
      return;
    }
  }

  isInstallingMod.value = true;

  try {
    const result = await installModBranches(
      candidateImportSourcePath.value,
      selectedCandidateRootPaths.value.map((candidateRootPath) => ({
        candidateRootPath,
        branchName: candidateBranchNames.value[candidateRootPath]?.trim() || "未命名分支",
        allowGameRoot: gameRootCandidates.some(
          (candidate) => candidate.rootPath === candidateRootPath,
        ),
      })),
      candidateOriginalSourcePath.value,
      candidateImportMode.value === "branchGroup" ? candidateGroupName.value : null,
      candidateImportMode.value === "branchGroup",
    );
    installResult.value = result.installResults[result.installResults.length - 1] ?? null;
    importError.value = "";
    archiveError.value = "";
    importPreview.value = null;
    candidateImportSourcePath.value = "";
    candidateOriginalSourcePath.value = null;
    selectedCandidateRootPaths.value = [];
    candidateBranchNames.value = {};
    candidateGroupName.value = "";
    await loadModViewsFromSnapshot();
    await loadModLibraryStatus();
  } catch (error) {
    importError.value = userFacingError(error);
  } finally {
    isInstallingMod.value = false;
  }
}

async function createBranchGroupFromMods(mods: InstalledModSummary[]) {
  if (mods.length < 2) {
    return;
  }
  isUpdatingBranchGroups.value = true;
  try {
    const modIds = mods.map((mod) => mod.id);
    const group = await createModBranchGroup(`${mods[0].name} 分支组`, modIds);
    mergeCreatedBranchGroup(group);
    deploymentError.value = "";
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    isUpdatingBranchGroups.value = false;
  }
}

function mergeCreatedBranchGroup(group: ModBranchGroup) {
  const selectedIds = new Set(group.modIds);
  modBranchGroups.value = [
    ...modBranchGroups.value
      .map((existing) => ({
        ...existing,
        modIds: existing.modIds.filter((modId) => !selectedIds.has(modId)),
      }))
      .filter((existing) => existing.modIds.length >= 2),
    group,
  ];
}

function findBranchGroupSuggestions() {
  branchGroupSuggestions.value = buildBranchGroupSuggestions(
    installedMods.value,
    modBranchGroups.value,
    conflictGroups.value,
  );
  branchSuggestionError.value = "";
  isBranchSuggestionOpen.value = true;
}

function closeBranchSuggestionDialog() {
  if (isUpdatingBranchGroups.value) {
    return;
  }
  isBranchSuggestionOpen.value = false;
  branchSuggestionError.value = "";
}

async function createSuggestedBranchGroups(
  selections: BranchGroupSuggestionSelection[],
) {
  if (!selections.length || isUpdatingBranchGroups.value) {
    return;
  }

  isUpdatingBranchGroups.value = true;
  branchSuggestionError.value = "";
  let createdCount = 0;
  try {
    // 候选组互不重叠，逐组复用现有命令，确保每次写入都经过 Rust 成员校验和快照更新。
    for (const selection of selections) {
      const group = await createModBranchGroup(selection.name, selection.modIds);
      mergeCreatedBranchGroup(group);
      createdCount += 1;
    }
    batchOperationSummary.value = `已创建 ${createdCount} 个 MOD 分支组。`;
    isBranchSuggestionOpen.value = false;
  } catch (error) {
    branchSuggestionError.value = userFacingError(error);
    branchGroupSuggestions.value = buildBranchGroupSuggestions(
      installedMods.value,
      modBranchGroups.value,
      conflictGroups.value,
    );
  } finally {
    isUpdatingBranchGroups.value = false;
  }
}

async function renameBranchGroup(group: ModBranchGroup, name: string) {
  isUpdatingBranchGroups.value = true;
  try {
    const updated = await renameModBranchGroup(group.id, name);
    modBranchGroups.value = modBranchGroups.value.map((candidate) =>
      candidate.id === updated.id ? updated : candidate,
    );
    deploymentError.value = "";
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    isUpdatingBranchGroups.value = false;
  }
}

async function ungroupInstalledMods(mods: InstalledModSummary[]) {
  if (!mods.length) {
    return;
  }
  isUpdatingBranchGroups.value = true;
  try {
    modBranchGroups.value = await removeModsFromBranchGroup(mods.map((mod) => mod.id));
    deploymentError.value = "";
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    isUpdatingBranchGroups.value = false;
  }
}

async function scanLegacyBox() {
  isScanningLegacyBox.value = true;

  try {
    // 盒子扫描只读取其配置、info.xml 和文件内容；实际部署状态不能直接改变 Acumod 的启用状态。
    const scan = await scanLegacyBoxMods(legacyBoxPath.value);
    legacyBoxScan.value = scan;
    legacyBoxImportResult.value = null;
    legacyBoxStateSyncResult.value = null;
    // 盒子目录中的每个模块都已有完整 files 内容，因此默认全部导入；启用状态由导入后的实际文件检测决定。
    selectedLegacyBoxModuleIds.value = scan.mods.map((mod) => mod.moduleId);
    selectedLegacyBoxModuleId.value = scan.mods[0]?.moduleId ?? "";
    legacyBoxError.value = "";
  } catch (error) {
    legacyBoxError.value = userFacingError(error);
  } finally {
    isScanningLegacyBox.value = false;
  }
}

async function importSelectedLegacyBoxMods() {
  if (!legacyBoxScan.value || !selectedLegacyBoxModuleIds.value.length) {
    return;
  }

  isImportingLegacyBox.value = true;
  try {
    // Rust 会在复制和内容关联后自动完成只读状态检测；前端不推断启用状态。
    const result = await importLegacyBoxMods(
      legacyBoxScan.value.boxPath,
      selectedLegacyBoxModuleIds.value,
    );
    legacyBoxImportResult.value = result;
    legacyBoxStateSyncResult.value = result.stateSync;
    legacyBoxError.value = "";
    await loadModLibraryStatus();
    await loadModViewsFromSnapshot();
  } catch (error) {
    legacyBoxError.value = userFacingError(error);
  } finally {
    isImportingLegacyBox.value = false;
  }
}

async function refreshLegacyBoxGameModStates() {
  isRefreshingGameModStates.value = true;
  try {
    // 手动检测复用同一条 Rust 状态同步链路，不会复制、覆盖或删除游戏目录文件。
    legacyBoxStateSyncResult.value = await refreshGameModStates();
    legacyBoxError.value = "";
    await loadModViewsFromSnapshot();
  } catch (error) {
    legacyBoxError.value = userFacingError(error);
  } finally {
    isRefreshingGameModStates.value = false;
  }
}

function handleDroppedPaths(paths: string[]) {
  isDragActive.value = false;
  dragError.value = "";

  if (paths.length !== 1) {
    dragError.value = "一次只能拖入一个 MOD 文件夹或压缩包。";
    return;
  }

  pendingDropPath.value = paths[0];
}

function cancelDroppedImport() {
  pendingDropPath.value = "";
}

async function confirmDroppedImport() {
  const droppedPath = pendingDropPath.value;
  pendingDropPath.value = "";

  if (!droppedPath) {
    return;
  }

  const extension = droppedPath.split(".").pop()?.toLowerCase();
  isHandlingDrop.value = true;

  try {
    if (extension === "zip" || extension === "7z" || extension === "rar") {
      archivePath.value = droppedPath;
      await installArchive();
    } else {
      importPath.value = droppedPath;
      await previewImportPath(false);

      if (importPreview.value?.status === "ready") {
        await installPreviewedMod();
      }
    }
  } catch (error) {
    dragError.value = userFacingError(error);
  } finally {
    isHandlingDrop.value = false;
  }
}

async function enableInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;
  batchOperationSummary.value = "";

  try {
    deploymentError.value = "";
    const plan = await previewEnableMod(mod.id);
    if (plan.conflicts.length) {
      const conflictFileCount = new Set(
        plan.conflicts.flatMap((conflict) => conflict.conflictFiles),
      ).size;
      const shouldEnable = await requestConfirmation({
        title: "确认启用冲突 MOD",
        message: `“${mod.name}”与 ${plan.conflicts.length} 个已启用 MOD 存在 ${conflictFileCount} 个冲突文件。启用后，该 MOD 将获得更高优先级并覆盖这些文件。`,
        conflicts: plan.conflicts,
        confirmLabel: "确认启用",
        cancelLabel: "暂不启用",
      });
      if (!shouldEnable) {
        return;
      }
    }
    await enableMod(mod.id, true);
    await loadModViewsFromSnapshot({ preserveModOrder: true });
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    activeModAction.value = "";
  }
}

async function disableInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;
  batchOperationSummary.value = "";

  try {
    deploymentError.value = "";
    await disableMod(mod.id);
    await loadModViewsFromSnapshot({ preserveModOrder: true });
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    activeModAction.value = "";
  }
}

function batchActionLabel(action: BatchModAction) {
  const labels: Record<BatchModAction, string> = {
    enable: "启用",
    disable: "禁用",
    uninstall: "卸载",
  };
  return labels[action];
}

async function updateInstalledModsInBatch(
  action: BatchModAction,
  mods: InstalledModSummary[],
) {
  if (!mods.length) {
    return;
  }

  if (action === "uninstall") {
    const enabledCount = mods.filter((mod) => mod.enabled).length;
    const localFileCount = mods.reduce((total, mod) => total + mod.fileCount, 0);
    const detailNames = mods.slice(0, 8).map((mod) => mod.name);
    if (mods.length > detailNames.length) {
      detailNames.push(`另有 ${mods.length - detailNames.length} 个 MOD`);
    }
    const shouldUninstall = await requestConfirmation({
      title: "确认批量卸载 MOD",
      message:
        `将从 Acumod 本地库卸载 ${mods.length} 个 MOD，共 ${localFileCount} 个本地文件。` +
        (enabledCount
          ? `其中 ${enabledCount} 个当前已启用，会先按部署记录清理游戏目录。`
          : ""),
      details: detailNames,
      confirmLabel: `卸载 ${mods.length} 个 MOD`,
      tone: "danger",
    });
    if (!shouldUninstall) {
      return;
    }
  }

  activeBatchAction.value = action;
  batchOperationSummary.value = "";
  deploymentError.value = "";
  try {
    const result = await batchUpdateMods(
      action,
      mods.map((mod) => mod.id),
    );
    const warningItems = result.items.filter((item) => item.warnings.length);
    batchOperationSummary.value =
      result.message +
      (warningItems.length
        ? ` ${warningItems.map((item) => item.name).join("、")} 产生了操作提示。`
        : "");

    const failedItems = result.items.filter((item) => item.status === "failed");
    if (failedItems.length) {
      deploymentError.value =
        `以下 MOD 未能完成批量${batchActionLabel(action)}：` +
        failedItems.map((item) => item.name).join("、");
    }

    if (action === "uninstall") {
      await loadModLibraryStatus();
    }
    await loadModViewsFromSnapshot({ preserveModOrder: true });
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    activeBatchAction.value = "";
  }
}

async function enableInstalledModsInBatch(mods: InstalledModSummary[]) {
  await updateInstalledModsInBatch("enable", mods);
}

async function disableInstalledModsInBatch(mods: InstalledModSummary[]) {
  await updateInstalledModsInBatch("disable", mods);
}

async function uninstallInstalledModsInBatch(mods: InstalledModSummary[]) {
  await updateInstalledModsInBatch("uninstall", mods);
}

async function showInstalledModFolder(mod: InstalledModSummary) {
  openingModFolderId.value = mod.id;

  try {
    await openInstalledModFolder(mod.id);
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = userFacingError(error);
  } finally {
    openingModFolderId.value = "";
  }
}

function remapTargetOptionLabel(group: ModelRemapGroup, targetId: string) {
  const target = group.targets.find((candidate) => candidate.targetId === targetId);
  if (!target) {
    return targetId;
  }

  if (group.modelKind === "armor") {
    const setLabel = armorSetTargetLabel(target);
    return setLabel.includes(target.modelId) ? setLabel : `${setLabel} · ${target.modelId}`;
  }

  const displayName = target.displayNames[0]
    ? localizedGameText(target.displayNames[0])
    : target.modelId;
  const sharedCount = Math.max(target.displayNames.length - 1, 0);
  return sharedCount ? `${displayName} 等 ${target.displayNames.length} 个名称 · ${target.modelId}` : `${displayName} · ${target.modelId}`;
}

function remapGroupSourceLabel(group: ModelRemapGroup) {
  if (group.modelKind === "armor") {
    return armorSetTargetLabel({
      displayNames: group.sourceDisplayNames,
      modelId: group.sourceModelIds[0] ?? "未知 ID",
    });
  }

  return group.sourceDisplayNames[0]
    ? localizedGameText(group.sourceDisplayNames[0])
    : group.sourceModelIds.join(" + ");
}

function selectRemapGroup(groupKey: string) {
  selectedRemapGroupKey.value = groupKey;
  const group = remapDetails.value?.groups.find((candidate) => candidate.groupKey === groupKey);
  const savedTargetId = group?.selectedTargetId ?? "";
  const isKnownTarget = group?.targets.some((target) => target.targetId === savedTargetId) ?? false;
  const usesManualSlingerTarget = Boolean(
    group?.allowsManualTarget && savedTargetId && !isKnownTarget,
  );
  selectedRemapTargetId.value = usesManualSlingerTarget
    ? MANUAL_SLINGER_TARGET
    : savedTargetId;
  manualSlingerTargetId.value = usesManualSlingerTarget
    ? savedTargetId.replace(/^slinger:/, "")
    : "";
  remapSaveWarnings.value = [];
  remapError.value = "";
}

function updateRemapTarget(event: Event) {
  selectedRemapTargetId.value = (event.target as HTMLSelectElement).value;
  if (selectedRemapTargetId.value !== MANUAL_SLINGER_TARGET) {
    manualSlingerTargetId.value = "";
  }
  remapSaveWarnings.value = [];
  remapError.value = "";
}

function updateManualSlingerTarget(event: Event) {
  manualSlingerTargetId.value = (event.target as HTMLInputElement).value;
  remapSaveWarnings.value = [];
  remapError.value = "";
}

function requestedRemapTargetId() {
  const group = selectedRemapGroup.value;
  if (!group) {
    return null;
  }
  const manualTarget = manualSlingerTargetId.value.trim().toLowerCase();
  if (selectedRemapTargetId.value === MANUAL_SLINGER_TARGET && manualTarget) {
    return manualTarget;
  }
  return selectedRemapTargetId.value === MANUAL_SLINGER_TARGET
    ? null
    : selectedRemapTargetId.value || null;
}

async function openRemapManager(mod: InstalledModSummary) {
  remapDetails.value = null;
  remapError.value = "";
  remapSaveWarnings.value = [];

  try {
    remapDetails.value = await getModRemapDetails(mod.id);
    modLibraryError.value = "";
    const firstGroup = remapDetails.value.groups[0];
    selectedRemapGroupKey.value = "";
    if (firstGroup) {
      selectRemapGroup(firstGroup.groupKey);
    }
  } catch (error) {
    remapError.value = userFacingError(error);
    modLibraryError.value = remapError.value;
  }
}

function closeRemapManager(force = false) {
  if (isApplyingRemap.value && !force) {
    return;
  }
  remapDetails.value = null;
  selectedRemapGroupKey.value = "";
  selectedRemapTargetId.value = "";
  manualSlingerTargetId.value = "";
  remapSaveWarnings.value = [];
  remapError.value = "";
}

async function applySelectedRemap() {
  const details = remapDetails.value;
  const group = selectedRemapGroup.value;
  if (!details || !group) {
    return;
  }
  if (
    selectedRemapTargetId.value === MANUAL_SLINGER_TARGET &&
    !manualSlingerTargetId.value.trim()
  ) {
    remapError.value = "请输入飞翔爪编号。";
    return;
  }

  isApplyingRemap.value = true;
  remapError.value = "";
  try {
    const plan = await previewModRemap(
      details.modId,
      group.groupKey,
      requestedRemapTargetId(),
    );
    remapSaveWarnings.value = plan.warnings;
    if (plan.warnings.length) {
      const shouldSave = await requestConfirmation({
        title: "确认保存模型修改",
        message: "此修改需要在游戏中确认相关效果是否正常。",
        details: plan.warnings,
        confirmLabel: "仍要保存",
      });
      if (!shouldSave) {
        return;
      }
    }
    await applyModRemap(details.modId, group.groupKey, plan.targetId);
    await loadModViewsFromSnapshot();
    closeRemapManager(true);
  } catch (error) {
    remapError.value = userFacingError(error);
  } finally {
    isApplyingRemap.value = false;
  }
}

async function uninstallInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;
  batchOperationSummary.value = "";

  try {
    const plan = await previewUninstallMod(mod.id);
    deploymentError.value = "";

    const shouldUninstall = await requestConfirmation({
      title: "确认卸载 MOD",
      message:
        `卸载“${mod.name}”会删除 Acumod 本地 MOD 库副本 ${plan.libraryFileCount} 个文件` +
        (plan.deployedFileCount > 0
          ? `，并先清理游戏目录中已记录的 ${plan.deployedFileCount} 个部署文件。`
          : "。"),
      confirmLabel: "卸载 MOD",
      tone: "danger",
    });

    if (!shouldUninstall) {
      return;
    }

    await uninstallMod(mod.id);
    await loadModLibraryStatus();
    await loadModViewsFromSnapshot();
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    activeModAction.value = "";
  }
}

async function restoreAllInstalledMods() {
  isRestoringAll.value = true;
  batchOperationSummary.value = "";

  try {
    const plan = await previewRestoreAllMods();
    deploymentError.value = "";

    if (plan.affectedModCount === 0) {
      return;
    }

    const shouldRestore = await requestConfirmation({
      title: "确认一键还原",
      message: `一键还原会禁用 ${plan.affectedModCount} 个 MOD，并删除 ${plan.deployedFileCount} 个由 Acumod 记录的部署文件。`,
      confirmLabel: "一键还原",
      tone: "danger",
    });

    if (!shouldRestore) {
      return;
    }

    await restoreAllMods();
    await loadModViewsFromSnapshot();
  } catch (error) {
    deploymentError.value = userFacingError(error);
  } finally {
    isRestoringAll.value = false;
  }
}

async function moveSelectedConflictParticipant(
  modId: string,
  direction: "up" | "down",
) {
  if (!selectedConflictGroupId.value) {
    return;
  }

  activeModAction.value = modId;

  try {
    const currentGroup = selectedConflictGroup.value;
    if (!currentGroup) {
      return;
    }
    const result = await moveConflictParticipant(
      selectedConflictGroupId.value,
      modId,
      direction,
      currentGroup.participants.map((participant) => participant.modId),
    );
    const participantsById = new Map(
      currentGroup.participants.map((participant) => [participant.modId, participant]),
    );
    const reorderedParticipants = result.participantOrder.flatMap((participantId, index) => {
      const participant = participantsById.get(participantId);
      return participant ? [{ ...participant, order: index + 1 }] : [];
    });
    if (conflictReport.value && reorderedParticipants.length === currentGroup.participants.length) {
      conflictReport.value = {
        ...conflictReport.value,
        groups: conflictReport.value.groups.map((group) =>
          group.groupId === currentGroup.groupId
            ? { ...group, participants: reorderedParticipants }
            : group,
        ),
      };
    }
    conflictActionError.value = "";
  } catch (error) {
    conflictActionError.value = userFacingError(error);
  } finally {
    activeModAction.value = "";
  }
}

async function applySelectedConflictOrder() {
  if (!selectedConflictGroupId.value) {
    return;
  }

  isApplyingConflict.value = true;

  try {
    const plan = await previewApplyConflictOrder(selectedConflictGroupId.value);
    conflictActionError.value = "";

    if (plan.applicableFileCount === 0) {
      return;
    }

    const shouldApply = await requestConfirmation({
      title: "确认应用冲突优先级",
      message: plan.requiresOverwriteConfirmation
        ? "目标文件不是 Acumod 已记录的文件，将被覆盖。"
        : `应用当前优先级，并更新 ${plan.applicableFileCount} 个冲突文件。`,
      confirmLabel: "应用优先级",
    });

    if (!shouldApply) {
      return;
    }

    await applyConflictOrder(
      selectedConflictGroupId.value,
      plan.requiresOverwriteConfirmation,
    );
    await loadModViewsFromSnapshot();
  } catch (error) {
    conflictActionError.value = userFacingError(error);
  } finally {
    isApplyingConflict.value = false;
  }
}

onMounted(() => {
  void loadAppInfo();
  void loadGameStatus();
  void loadGameTextSettings();
  void loadModLibraryStatus();
  void listenOperationProgress(updateOperationStatus)
    .then((unlisten) => {
      stopOperationProgressListener = unlisten;
    })
    .catch(() => {
      // Browser-only Vite development has no Tauri event API.
    })
    .finally(() => {
      void loadModViewsFromSnapshot();
    });
  void getCurrentWebview()
    .onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        isDragActive.value = true;
        return;
      }

      if (event.payload.type === "leave") {
        isDragActive.value = false;
        return;
      }

      void handleDroppedPaths(event.payload.paths);
    })
    .then((unlisten) => {
      stopDragListener = unlisten;
    })
    .catch(() => {
      // Browser-only Vite development has no Tauri webview drag-drop API.
    });
});

onBeforeUnmount(() => {
  finishConfirmation(false);
  stopDragListener?.();
  stopOperationProgressListener?.();
  if (clearOperationStatusTimer) {
    clearTimeout(clearOperationStatusTimer);
  }
});
</script>

<template>
  <main class="app-shell">
    <AppSidebar
      :active-view="activeView"
      :game-status-label="statusLabel"
      :game-status-class="statusClass"
      :conflict-count="conflictGroups.length"
      @select="selectWorkspace"
    />

    <div class="app-workspace">
      <AppTopbar
        :active-view="activeView"
        :mod-count="installedMods.length"
        :enabled-mod-count="enabledModCount"
        :is-refreshing="
          isLoadingGame ||
          isLoadingApp ||
          isLoadingModLibrary ||
          isRefreshingModViews ||
          isOperationInProgress
        "
        :agent-open="isAgentPanelOpen"
        @refresh="refreshCurrentWorkspace"
        @toggle-agent="isAgentPanelOpen = !isAgentPanelOpen"
      />
      <OperationStatusBar :operation="activeOperation" />

      <div class="workspace-content" :class="{ 'library-content': activeView === 'library' }">
        <div v-show="activeView === 'settings'" class="workspace-page">
          <section class="panel">
      <div class="panel-heading">
        <div>
          <h2>游戏目录</h2>
          <p>{{ gameStatusMessage }}</p>
        </div>
        <button type="button" :disabled="isLoadingGame" @click="autoDetectGameDirectory">
          {{ isLoadingGame ? "处理中" : "自动检测" }}
        </button>
      </div>

      <form class="path-form" @submit.prevent="saveManualPath">
        <label for="game-path">MHW 根目录</label>
        <div class="path-row">
          <input
            id="game-path"
            v-model.trim="manualPath"
            type="text"
            autocomplete="off"
            placeholder="C:\Program Files (x86)\Steam\steamapps\common\Monster Hunter World"
          />
          <button type="submit" :disabled="isLoadingGame || !manualPath">保存</button>
        </div>
      </form>

      <p v-if="gameError" class="error">{{ gameError }}</p>

      <dl v-if="gameStatus" class="facts">
        <div>
          <dt>状态</dt>
          <dd>{{ gameStatus.isValid ? "有效" : "无效" }}</dd>
        </div>
        <div>
          <dt>来源</dt>
          <dd>{{ gameSourceLabel(gameStatus.source) }}</dd>
        </div>
        <div>
          <dt>执行文件</dt>
          <dd>{{ gameStatus.executablePath ?? "未设置" }}</dd>
        </div>
        <div>
          <dt>nativePC</dt>
          <dd>{{ gameStatus.nativePcPath ?? "未设置" }}</dd>
        </div>
        <div>
          <dt>配置文件</dt>
          <dd>{{ gameStatus.configPath }}</dd>
        </div>
      </dl>
          </section>

          <section class="panel secondary">
            <div class="panel-heading">
              <div>
                <h2>游戏文本语言</h2>
                <p>切换武器、防具等游戏内容名称，不改变 Acumod 界面语言。</p>
              </div>
              <label class="game-text-language-control">
                <span>名称语言</span>
                <select
                  :value="gameTextLanguage"
                  :disabled="isSavingGameTextLanguage"
                  @change="updateGameTextLanguage"
                >
                  <option value="simplifiedChinese">简体中文</option>
                  <option value="traditionalChinese">繁體中文</option>
                </select>
              </label>
            </div>
            <p v-if="gameTextError" class="error settings-inline-error">{{ gameTextError }}</p>
          </section>

          <section class="panel secondary">
            <div class="panel-heading compact">
              <div>
                <h2>应用信息</h2>
                <p v-if="isLoadingApp">Loading...</p>
                <p v-else-if="appError" class="error">{{ appError }}</p>
                <p v-else>{{ appInfo ? "应用运行正常。" : "尚未读取应用信息。" }}</p>
              </div>
            </div>

            <dl v-if="appInfo" class="facts compact-facts">
              <div>
                <dt>名称</dt>
                <dd>{{ appInfo.name }}</dd>
              </div>
              <div>
                <dt>版本</dt>
                <dd>{{ appInfo.version }}</dd>
              </div>
            </dl>
          </section>
        </div>

        <div v-show="activeView === 'import'" class="workspace-page">
          <section class="panel">
      <div class="panel-heading">
        <div>
          <h2>识别 MOD</h2>
          <p>{{ importHeaderMessage }}</p>
        </div>
        <span class="status-pill" :class="importStatusClass">{{ importStatusLabel }}</span>
      </div>

      <form class="path-form" @submit.prevent="runImportPreview">
        <label for="import-path">本地 MOD 文件夹</label>
        <div class="path-row">
          <input
            id="import-path"
            v-model.trim="importPath"
            type="text"
            autocomplete="off"
            placeholder="D:\下载\太刀外观 MOD"
          />
          <button type="submit" :disabled="isPreviewingImport || !importPath">
            {{ isPreviewingImport ? "识别中" : "识别 MOD" }}
          </button>
        </div>
      </form>

      <form class="path-form" @submit.prevent="installArchive">
        <label for="archive-path">本地 MOD 压缩包</label>
        <div class="path-row">
          <input
            id="archive-path"
            v-model.trim="archivePath"
            type="text"
            autocomplete="off"
            placeholder="D:\下载\太刀外观 MOD.zip"
          />
          <button type="submit" :disabled="isInstallingArchive || !archivePath">
            {{ isInstallingArchive ? "解包导入中" : "导入压缩包" }}
          </button>
        </div>
        <p class="hint">支持 .zip / .7z / .rar；通过 Acumod 内置解包组件处理。</p>
      </form>

      <section class="legacy-box-import">
        <div class="section-title-row">
          <div>
            <h3>从狩技 MOD 盒子导入</h3>
            <p class="hint">扫描和状态检测只读取游戏文件；导入保存本地副本后会自动判断实际启用状态，不会改写游戏目录。</p>
          </div>
        </div>

        <form class="path-form compact-path-form" @submit.prevent="scanLegacyBox">
          <label for="legacy-box-path">狩技 MOD 盒子目录</label>
          <div class="path-row">
            <input
              id="legacy-box-path"
              v-model.trim="legacyBoxPath"
              type="text"
              autocomplete="off"
              placeholder="E:\Games\MHWI\狩技MOD盒子"
            />
            <button type="submit" :disabled="isScanningLegacyBox || !legacyBoxPath">
              {{ isScanningLegacyBox ? "扫描中" : "扫描盒子" }}
            </button>
          </div>
        </form>

        <p v-if="legacyBoxError" class="error">{{ legacyBoxError }}</p>

        <dl v-if="legacyBoxScan" class="facts compact-facts">
          <div>
            <dt>盒子游戏目录</dt>
            <dd>{{ legacyBoxScan.boxGamePath ?? "未记录" }}</dd>
          </div>
          <div>
            <dt>目录核验</dt>
            <dd>{{ legacyBoxScan.isBoxGamePathValid ? "可用" : "不可用，未核验游戏文件" }}</dd>
          </div>
          <div>
            <dt>与 Acumod 设置</dt>
            <dd>
              {{ legacyBoxScan.gamePathsMatch === true ? "游戏目录一致" : legacyBoxScan.gamePathsMatch === false ? "游戏目录不同" : "未设置 Acumod 游戏目录" }}
            </dd>
          </div>
          <div>
            <dt>盒子 MOD</dt>
            <dd>{{ legacyBoxScan.mods.length }} 个</dd>
          </div>
        </dl>

        <div v-if="legacyBoxScan" class="preview-block">
          <div class="section-title-row">
            <h3>选择要导入的 MOD</h3>
            <div class="section-actions">
              <span class="selection-count">已选择 {{ selectedLegacyBoxModCount }} 个</span>
              <button
                type="button"
                class="secondary-button"
                :disabled="isImportingLegacyBox || selectedLegacyBoxModCount === legacyBoxScan.mods.length"
                @click="selectAllLegacyBoxMods"
              >
                全选
              </button>
              <button
                type="button"
                class="secondary-button"
                :disabled="isImportingLegacyBox || !selectedLegacyBoxModCount"
                @click="clearLegacyBoxModSelection"
              >
                清空
              </button>
              <button
                type="button"
                class="secondary-button"
                :disabled="isImportingLegacyBox || !selectedLegacyBoxModCount"
                @click="importSelectedLegacyBoxMods"
              >
                {{ isImportingLegacyBox ? "导入中" : "导入到 MOD 库" }}
              </button>
              <button
                type="button"
                class="secondary-button"
                :disabled="isImportingLegacyBox || isRefreshingGameModStates"
                @click="refreshLegacyBoxGameModStates"
              >
                {{ isRefreshingGameModStates ? "检测中" : "检测游戏实际状态" }}
              </button>
            </div>
          </div>

          <div class="legacy-box-table-wrap">
            <table class="legacy-box-table">
              <thead>
                <tr>
                  <th scope="col">导入</th>
                  <th scope="col">名称</th>
                  <th scope="col">盒子记录</th>
                  <th scope="col">初步文件比对</th>
                  <th scope="col">文件</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="mod in legacyBoxScan.mods"
                  :key="mod.moduleId"
                  :class="{ selected: selectedLegacyBoxModuleId === mod.moduleId }"
                  @click="selectedLegacyBoxModuleId = mod.moduleId"
                >
                  <td @click.stop>
                    <input
                      v-model="selectedLegacyBoxModuleIds"
                      class="legacy-box-checkbox"
                      type="checkbox"
                      :value="mod.moduleId"
                      :aria-label="'选择 ' + mod.name"
                    />
                  </td>
                  <td>
                    <strong>{{ mod.name }}</strong>
                    <small v-if="mod.modType">{{ mod.modType }}</small>
                  </td>
                  <td>{{ legacyBoxRecordLabel(mod) }}</td>
                  <td>
                    <span class="legacy-box-status" :class="legacyBoxDeploymentClass(mod)">
                      {{ legacyBoxDeploymentLabel(mod) }}
                    </span>
                    <small>
                      一致 {{ mod.deployment.matchingFileCount }} / {{ mod.deployment.totalFileCount }}
                    </small>
                  </td>
                  <td>{{ mod.fileCount }} 个 · {{ formatFileSize(mod.totalSizeBytes) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div v-if="selectedLegacyBoxMod" class="preview-block">
          <div class="section-title-row">
            <h3>安装文件：{{ selectedLegacyBoxMod.name }}</h3>
            <span class="selection-count">共 {{ selectedLegacyBoxMod.fileCount }} 个</span>
          </div>
          <ul class="file-preview legacy-box-file-preview">
            <li v-for="file in previewedLegacyBoxFiles" :key="file.sourceRelativePath">
              <span>{{ file.sourceRelativePath }}</span>
              <strong>{{ formatFileSize(file.fileSizeBytes) }}</strong>
            </li>
          </ul>
          <p v-if="selectedLegacyBoxMod.fileCount > previewedLegacyBoxFiles.length" class="hint">
            当前显示前 {{ previewedLegacyBoxFiles.length }} 个安装文件。
          </p>
        </div>

        <div v-if="legacyBoxImportResult" class="preview-block">
          <h3>盒子导入结果</h3>
          <p class="hint">{{ legacyBoxImportResult.message }}</p>
          <ul class="compact-list">
            <li v-for="item in legacyBoxImportResult.items" :key="item.moduleId">
              <strong>{{ item.name }}</strong>
              <span>{{ item.message }}</span>
            </li>
          </ul>
        </div>

        <div v-if="legacyBoxStateSyncResult" class="preview-block">
          <h3>游戏实际状态</h3>
          <p class="hint">{{ legacyBoxStateSyncResult.message }}</p>
          <dl class="facts compact-facts">
            <div>
              <dt>已启用</dt>
              <dd>{{ legacyBoxStateSyncResult.enabledModCount }} 个</dd>
            </div>
            <div>
              <dt>部分被覆盖</dt>
              <dd>{{ legacyBoxStateSyncResult.partiallyOverriddenModCount }} 个</dd>
            </div>
            <div>
              <dt>未启用</dt>
              <dd>{{ legacyBoxStateSyncResult.disabledModCount }} 个</dd>
            </div>
            <div>
              <dt>混合覆盖</dt>
              <dd>{{ legacyBoxStateSyncResult.mixedConflictGroupCount }} 组</dd>
            </div>
          </dl>
          <ul v-if="legacyBoxStateSyncResult.mods.length" class="compact-list">
            <li v-for="mod in legacyBoxStateSyncResult.mods" :key="mod.modId">
              <strong>
                {{ installedModName(mod.modId) }} · {{ mod.enabled ? "已启用" : "未启用" }}
              </strong>
              <span>{{ mod.message }}</span>
            </li>
          </ul>
          <ul v-if="legacyBoxStateSyncResult.warnings.length" class="compact-list">
            <li v-for="warning in legacyBoxStateSyncResult.warnings" :key="warning">
              <span>{{ warning }}</span>
            </li>
          </ul>
        </div>

        <div v-if="legacyBoxScan?.warnings.length" class="preview-block">
          <h3>盒子扫描提示</h3>
          <ul class="compact-list">
            <li v-for="warning in legacyBoxScan.warnings" :key="warning">
              <span>{{ warning }}</span>
            </li>
          </ul>
        </div>
      </section>

      <div
        v-if="importPreview?.requiresGameRootConfirmation"
        class="notice warning-notice"
      >
        <p>此 MOD 没有可识别的 nativePC 结构，请确认它是否应安装到游戏根目录。</p>
        <button type="button" :disabled="isPreviewingImport" @click="confirmGameRootPreview">
          按游戏根目录识别
        </button>
      </div>

      <div v-if="importPreview?.status === 'ready'" class="notice success-notice">
        <p>识别完成，可以导入 Acumod 本地 MOD 库；此时不会写入 MHW 游戏目录。</p>
        <button type="button" :disabled="isInstallingMod" @click="installPreviewedMod">
          {{ isInstallingMod ? "导入中" : "导入到 MOD 库" }}
        </button>
      </div>

      <p v-if="modLibraryError" class="error">{{ modLibraryError }}</p>
      <p v-if="importError" class="error">{{ importError }}</p>
      <p v-if="archiveError" class="error">{{ archiveError }}</p>

      <dl v-if="modLibraryStatus" class="facts">
        <div>
          <dt>软件数据</dt>
          <dd>{{ modLibraryStatus.softwareDataPath }}</dd>
        </div>
        <div>
          <dt>MOD 库</dt>
          <dd>{{ modLibraryStatus.modsPath }}</dd>
        </div>
        <div>
          <dt>已安装</dt>
          <dd>{{ modLibraryStatus.installedPath }}</dd>
        </div>
        <div>
          <dt>导入暂存</dt>
          <dd>{{ modLibraryStatus.importStagingPath }}</dd>
        </div>
      </dl>

      <dl v-if="importPreview" class="facts">
        <div>
          <dt>识别方式</dt>
          <dd>{{ detectionMethodLabel(importPreview.detectionMethod) }}</dd>
        </div>
        <div>
          <dt>部署根</dt>
          <dd>{{ deployRootLabel(importPreview.deployRoot) }}</dd>
        </div>
        <div>
          <dt>内容根</dt>
          <dd>{{ importPreview.contentRootPath ?? "未识别" }}</dd>
        </div>
        <div>
          <dt>文件数</dt>
          <dd>{{ importPreview.fileCount }}</dd>
        </div>
      </dl>

      <dl v-if="installResult" class="facts">
        <div>
          <dt>导入结果</dt>
          <dd>{{ installResult.alreadyInstalled ? "MOD 已存在，未重复导入。" : "MOD 已导入本地库。" }}</dd>
        </div>
        <div>
          <dt>状态</dt>
          <dd>{{ installResult.alreadyInstalled ? "已导入，未重复安装" : "安装完成" }}</dd>
        </div>
        <div>
          <dt>MOD ID</dt>
          <dd>{{ installResult.modId }}</dd>
        </div>
        <div>
          <dt>名称</dt>
          <dd>{{ installResult.name }}</dd>
        </div>
        <div>
          <dt>库内目录</dt>
          <dd>{{ installResult.modPath }}</dd>
        </div>
        <div>
          <dt>内容目录</dt>
          <dd>{{ installResult.contentPath }}</dd>
        </div>
        <div>
          <dt>清单文件</dt>
          <dd>{{ installResult.manifestPath }}</dd>
        </div>
        <div>
          <dt>文件数</dt>
          <dd>{{ installResult.fileCount }}</dd>
        </div>
      </dl>

      <div v-if="installResult?.modelReplacements.length" class="preview-block">
        <h3>游戏内替换目标</h3>
        <ul class="model-replacement-list">
          <li
            v-for="replacement in installResult.modelReplacements"
            :key="`${replacement.modelKind}-${replacement.subKind}-${replacement.modelPart}-${replacement.modelId}`"
          >
            <strong>{{ modelReplacementTitle(replacement) }}</strong>
            <span>{{ summarizeModelNames(replacement) }}</span>
            <small>
              {{ replacement.modelId }}
              <template v-if="summarizeGameIds(replacement)">
                · {{ summarizeGameIds(replacement) }}
              </template>
              <template v-if="summarizeAffectedParts(replacement)">
                · {{ summarizeAffectedParts(replacement) }}
              </template>
            </small>
            <small v-if="summarizeModelAssociations(replacement)">
              {{ summarizeModelAssociations(replacement) }}
            </small>
          </li>
        </ul>
      </div>

      <div
        v-if="importPreview?.candidates.length"
        ref="candidateSelectionSection"
        class="preview-block candidate-selection-section"
        tabindex="-1"
      >
        <div class="section-title-row">
          <h3>选择 MOD 分支</h3>
          <button
            type="button"
            class="secondary-button"
            :disabled="isInstallingMod || !selectedCandidateRootPaths.length"
            @click="installSelectedCandidate"
          >
            {{ isInstallingMod ? "导入中" : `导入所选 ${selectedCandidateRootPaths.length} 个分支` }}
          </button>
        </div>
        <div class="candidate-import-options">
          <div class="candidate-mode-control" role="radiogroup" aria-label="分支导入方式">
            <label>
              <input v-model="candidateImportMode" type="radio" value="branchGroup" />
              <span>作为分支组导入</span>
            </label>
            <label>
              <input v-model="candidateImportMode" type="radio" value="standalone" />
              <span>作为独立 MOD 导入</span>
            </label>
          </div>
          <label v-if="candidateImportMode === 'branchGroup' && selectedCandidateRootPaths.length > 1" class="group-name-field">
            <span>分支组名称</span>
            <input v-model="candidateGroupName" maxlength="120" />
          </label>
          <div class="candidate-selection-actions">
            <button
              type="button"
              class="text-button"
              @click="selectedCandidateRootPaths = importPreview.candidates.map((candidate) => candidate.rootPath)"
            >
              全选
            </button>
            <button type="button" class="text-button" @click="selectedCandidateRootPaths = []">清空</button>
          </div>
        </div>
        <ul class="candidate-list">
          <li v-for="candidate in importPreview.candidates" :key="candidate.rootPath">
            <label class="candidate-choice">
              <input
                v-model="selectedCandidateRootPaths"
                type="checkbox"
                :value="candidate.rootPath"
              />
              <span>
                <strong>{{ candidate.relativePath || candidate.rootPath }}</strong>
                <small>
                  {{ candidate.fileCount }} 个文件 / {{ deployRootLabel(candidate.deployRoot) }}
                  <template v-if="candidate.requiresGameRootConfirmation"> / 需确认游戏根目录</template>
                </small>
              </span>
            </label>
            <label class="branch-name-field">
              <span>分支名称</span>
              <input v-model="candidateBranchNames[candidate.rootPath]" maxlength="120" />
            </label>
          </li>
        </ul>
      </div>

      <div v-if="previewedFiles.length" class="preview-block">
        <h3>将导入的文件</h3>
        <ul class="file-preview">
          <li v-for="file in previewedFiles" :key="file.sourcePath">
            <span>{{ file.sourceRelativePath }}</span>
            <strong>{{ file.deployRelativePath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="installedFiles.length" class="preview-block">
        <h3>库内文件</h3>
        <ul class="file-preview">
          <li v-for="file in installedFiles" :key="file.libraryRelativePath">
            <span>{{ file.deployRelativePath }}</span>
            <strong>{{ file.libraryRelativePath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="importPreview?.warnings.length" class="preview-block">
        <h3>警告</h3>
        <ul class="compact-list">
          <li v-for="warning in importPreview.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>
          </section>
        </div>

        <div v-show="activeView === 'library'" class="workspace-page library-workspace">
          <section class="panel library-panel">
      <div class="preview-block library-preview">
        <div class="library-controls">
        <div class="section-title-row">
          <h3>已安装 MOD</h3>
          <div class="section-actions">
            <button
              type="button"
              class="secondary-button danger-button"
              :disabled="isRestoringAll || !installedMods.length"
              @click="restoreAllInstalledMods"
            >
              {{ isRestoringAll ? "还原中" : "一键还原" }}
            </button>
            <button
              type="button"
              class="secondary-button"
              :disabled="!conflictGroups.length"
              @click="openConflictManager"
            >
              冲突管理 ({{ conflictGroups.length }})
            </button>
            <button
              type="button"
              class="secondary-button"
              :disabled="installedMods.length < 2 || isUpdatingBranchGroups"
              @click="findBranchGroupSuggestions"
            >
              自动创建分支组
            </button>
          </div>
        </div>
        <ModLibraryToolbar
          :is-category-action="isCategoryAction"
          :is-order-action="isUpdatingModOrder"
          :search-query="modSearchQuery"
          :category-filter="modCategoryFilter"
          :status-filter="modStatusFilter"
          :conflict-filter="modConflictFilter"
          :sort="modSort"
          :sort-direction="modSortDirection"
          :prioritize-branch-groups="prioritizeBranchGroups"
          :categories="availableModCategories"
          @manage-categories="openCategoryManager()"
          @update-search-query="modSearchQuery = $event"
          @update-category-filter="modCategoryFilter = $event"
          @update-status-filter="modStatusFilter = $event"
          @update-conflict-filter="modConflictFilter = $event"
          @update-sort="modSort = $event"
          @update-sort-direction="modSortDirection = $event"
          @update-prioritize-branch-groups="prioritizeBranchGroups = $event"
          @apply-sort="applyCurrentSortAsManualOrder"
          @restore-import-order="restoreOriginalImportOrder"
        />
        <p v-if="categoryError && !isCategoryManagerOpen" class="error">{{ categoryError }}</p>
        <p v-if="installedMods.length" class="hint">
          显示 {{ displayedInstalledMods.length }} / {{ installedMods.length }} 个 MOD；手动排序只影响 MOD 库浏览顺序，不影响冲突优先级。
        </p>
        </div>
        <ModLibraryTable
          :mods="displayedInstalledMods"
          :all-mods="installedMods"
          :branch-groups="modBranchGroups"
          :game-text-language="gameTextLanguage"
          :installed-mod-count="installedMods.length"
          :categories="modCategories"
          :conflicting-mod-ids="conflictingModIds"
          :conflict-partner-names="conflictPartnerNames"
          :active-mod-action="activeModAction"
          :active-batch-action="activeBatchAction"
          :opening-mod-folder-id="openingModFolderId"
          :metadata-saving-mod-id="metadataSavingModId"
          :metadata-error-mod-id="metadataErrorModId"
          :metadata-error="metadataError"
          :can-reorder="modSort === 'manual' && !reorderingModId && !isUpdatingModOrder"
          :reordering-mod-id="reorderingModId"
          :updating-branch-groups="isUpdatingBranchGroups"
          @update-metadata="saveModMetadata"
          @create-category="openCategoryManager"
          @update-branch-group-category="updateBranchGroupCategory"
          @create-category-for-branch-group="openCategoryManagerForBranchGroup"
          @open-folder="showInstalledModFolder"
          @enable="enableInstalledMod"
          @disable="disableInstalledMod"
          @batch-enable="enableInstalledModsInBatch"
          @batch-disable="disableInstalledModsInBatch"
          @batch-uninstall="uninstallInstalledModsInBatch"
          @create-branch-group="createBranchGroupFromMods"
          @rename-branch-group="renameBranchGroup"
          @ungroup-mods="ungroupInstalledMods"
          @manage-remap="openRemapManager"
          @reorder="reorderModLibraryItem"
          @uninstall="uninstallInstalledMod"
        />
      </div>

      <p v-if="batchOperationSummary" class="hint">{{ batchOperationSummary }}</p>
      <p v-if="deploymentError" class="error">{{ deploymentError }}</p>

      <div v-if="installedModList?.warnings.length" class="preview-block">
        <h3>MOD 库警告</h3>
        <ul class="compact-list">
          <li v-for="warning in installedModList.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>

          </section>
        </div>

        <div v-show="activeView === 'conflicts'" class="workspace-page conflict-workspace">
    <section class="conflict-layout">
      <aside class="conflict-sidebar">
        <div class="conflict-sidebar-heading">
          <h2>冲突组</h2>
          <span>{{ conflictGroups.length }}</span>
        </div>
        <p class="hint">互相冲突的 MOD 组成一组；彼此无关的冲突会分开显示。</p>
        <ul class="conflict-group-list">
          <li v-for="group in conflictGroups" :key="group.groupId">
            <button
              type="button"
              class="conflict-group-button"
              :class="{ selected: selectedConflictGroupId === group.groupId }"
              @click="selectConflict(group.groupId)"
            >
              <strong>{{ group.participants.map((participant) => installedModName(participant.modId)).join(" / ") }}</strong>
              <span>{{ group.participantCount }} 个 MOD / {{ group.conflictFileCount }} 个冲突文件</span>
            </button>
          </li>
        </ul>
      </aside>

      <section class="conflict-detail">
        <template v-if="selectedConflictGroup">
          <div class="panel-heading">
            <div>
              <h2>{{ selectedConflictGroup.participants.map((participant) => installedModName(participant.modId)).join(" / ") }}</h2>
              <p>从上到下为优先级；最上方且包含该文件的已启用 MOD 最后覆盖并生效。</p>
            </div>
            <button
              type="button"
              :disabled="isApplyingConflict || selectedConflictGroup.enabledParticipantCount === 0"
              @click="applySelectedConflictOrder"
            >
              {{ isApplyingConflict ? "应用中" : "应用优先级" }}
            </button>
          </div>

          <div v-if="selectedConflictGroup.sharedModelTargets.length" class="shared-model-targets">
            <strong>共同替换目标</strong>
            <span
              v-for="target in selectedConflictGroup.sharedModelTargets"
              :key="`${target.modelKind}-${target.modelId}`"
            >
              {{ summarizeSharedModelTarget(target) }}
            </span>
          </div>

          <details class="conflict-file-details">
            <summary>冲突文件（{{ selectedConflictGroup.conflictFiles.length }}）</summary>
            <ul>
              <li v-for="file in selectedConflictGroup.conflictFiles" :key="file">{{ file }}</li>
            </ul>
          </details>

          <ol class="conflict-order-list">
            <li v-for="(participant, index) in selectedConflictGroup.participants" :key="participant.modId">
              <span class="conflict-order-number">{{ participant.order }}</span>
              <div>
                <strong>{{ installedModName(participant.modId) }}</strong>
                <span>{{ participant.enabled ? "已启用" : "未启用" }}</span>
              </div>
              <div class="section-actions">
                <button
                  type="button"
                  class="secondary-button"
                  :disabled="!!activeModAction || index === 0"
                  @click="moveSelectedConflictParticipant(participant.modId, 'up')"
                >
                  上移
                </button>
                <button
                  type="button"
                  class="secondary-button"
                  :disabled="!!activeModAction || index + 1 === selectedConflictGroup.participants.length"
                  @click="moveSelectedConflictParticipant(participant.modId, 'down')"
                >
                  下移
                </button>
              </div>
            </li>
          </ol>

          <p v-if="selectedConflictGroup.enabledParticipantCount === 0" class="error">
            当前没有已启用的参与 MOD，无法应用这个冲突。
          </p>
          <p v-if="conflictActionError" class="error">{{ conflictActionError }}</p>
        </template>
        <p v-else class="hint">当前没有需要处理的 MOD 冲突组。</p>
      </section>
    </section>
        </div>
      </div>
    </div>

    <FloatingAgentPanel
      :open="isAgentPanelOpen"
      @open-panel="isAgentPanelOpen = true"
      @close="isAgentPanelOpen = false"
    />
  </main>

  <ModCategoryManager
    :is-open="isCategoryManagerOpen"
    :categories="modCategories"
    :is-busy="isCategoryAction"
    :error="categoryError"
    @close="closeCategoryManager"
    @create="createCategory"
    @rename="renameCategory"
    @delete="deleteCategory"
  />

  <BranchGroupSuggestionDialog
    :is-open="isBranchSuggestionOpen"
    :suggestions="branchGroupSuggestions"
    :is-busy="isUpdatingBranchGroups"
    :error="branchSuggestionError"
    @close="closeBranchSuggestionDialog"
    @confirm="createSuggestedBranchGroups"
  />

  <div v-if="remapDetails" class="dialog-backdrop" role="presentation">
    <section class="confirm-dialog remap-dialog" role="dialog" aria-modal="true" aria-labelledby="remap-dialog-title">
      <div class="remap-dialog-heading">
        <div>
          <h2 id="remap-dialog-title">修改替换模型</h2>
          <p>{{ remapDetails.name }}</p>
        </div>
        <button
          type="button"
          class="dialog-close-button"
          :disabled="isApplyingRemap"
          aria-label="关闭"
          data-tooltip="关闭"
          @click="closeRemapManager()"
        >
          <span aria-hidden="true">&times;</span>
        </button>
      </div>

      <ul v-if="visibleRemapWarnings.length" class="compact-list remap-warnings">
        <li v-for="warning in visibleRemapWarnings" :key="warning"><span>{{ warning }}</span></li>
      </ul>

      <p v-if="remapDetails.enabled" class="hint remap-disabled-hint">
        请先禁用此 MOD，再修改替换模型。
      </p>

      <template v-if="selectedRemapGroup">
        <div v-if="remapDetails.groups.length > 1" class="remap-group-tabs" role="tablist" aria-label="选择替换分组">
          <button
            v-for="group in remapDetails.groups"
            :key="group.groupKey"
            type="button"
            role="tab"
            :aria-selected="group.groupKey === selectedRemapGroupKey"
            :class="{ active: group.groupKey === selectedRemapGroupKey }"
            :disabled="isApplyingRemap || remapDetails.enabled"
            @click="selectRemapGroup(group.groupKey)"
          >
            {{ group.subKind }} · {{ remapGroupSourceLabel(group) }}
          </button>
        </div>

        <div class="remap-fields simplified-remap-fields">
          <label>
            <span>替换模型</span>
            <select :value="selectedRemapTargetId" :disabled="isApplyingRemap || remapDetails.enabled" @change="updateRemapTarget">
              <option value="">恢复默认</option>
              <option v-for="target in selectedRemapGroup.targets" :key="target.targetId" :value="target.targetId">
                {{ remapTargetOptionLabel(selectedRemapGroup, target.targetId) }}
              </option>
              <option v-if="selectedRemapGroup.allowsManualTarget" :value="MANUAL_SLINGER_TARGET">
                其他飞翔爪编号
              </option>
            </select>
          </label>

          <label v-if="selectedRemapTargetId === MANUAL_SLINGER_TARGET">
            <span>飞翔爪编号</span>
            <input
              :value="manualSlingerTargetId"
              :disabled="isApplyingRemap || remapDetails.enabled"
              placeholder="例如 slg106_0000"
              @input="updateManualSlingerTarget"
            />
          </label>
        </div>

        <p v-if="remapError" class="error">{{ remapError }}</p>

        <div class="section-actions">
          <button type="button" class="secondary-button" :disabled="isApplyingRemap" @click="closeRemapManager()">取消</button>
          <button type="button" :disabled="isApplyingRemap || remapDetails.enabled" @click="applySelectedRemap">
            {{ isApplyingRemap ? "保存中" : "保存修改" }}
          </button>
        </div>
      </template>
      <p v-else class="hint">没有可改绑的模型目标。人物语音仅保留识别。</p>
    </section>
  </div>

  <div
    v-if="confirmationRequest"
    class="dialog-backdrop confirmation-backdrop"
    role="presentation"
    @mousedown.self="finishConfirmation(false)"
  >
    <section
      class="confirm-dialog action-confirm-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="action-confirm-title"
      tabindex="-1"
      @keydown.esc.prevent="finishConfirmation(false)"
    >
      <h2 id="action-confirm-title">{{ confirmationRequest.title }}</h2>
      <p class="confirmation-message">{{ confirmationRequest.message }}</p>
      <ul v-if="confirmationRequest.details?.length" class="compact-list confirmation-details">
        <li v-for="detail in confirmationRequest.details" :key="detail">{{ detail }}</li>
      </ul>
      <div v-if="confirmationRequest.conflicts?.length" class="enable-conflict-list">
        <details v-for="conflict in confirmationRequest.conflicts" :key="conflict.modId">
          <summary>
            <strong>{{ conflict.name }}</strong>
            <span>{{ conflict.conflictFiles.length }} 个冲突文件</span>
          </summary>
          <ul>
            <li v-for="file in conflict.conflictFiles" :key="file">{{ file }}</li>
          </ul>
        </details>
      </div>
      <div class="section-actions">
        <button ref="confirmationCancelButton" type="button" class="secondary-button" @click="finishConfirmation(false)">
          {{ confirmationRequest.cancelLabel ?? "暂不操作" }}
        </button>
        <button
          type="button"
          :class="{ danger: confirmationRequest.tone === 'danger' }"
          @click="finishConfirmation(true)"
        >
          {{ confirmationRequest.confirmLabel }}
        </button>
      </div>
    </section>
  </div>

  <div v-if="isDragActive" class="drag-overlay">
    <div>
      <strong>{{ isHandlingDrop ? "正在处理导入" : "释放以导入 MOD" }}</strong>
      <span>支持 MOD 文件夹和 .zip / .7z / .rar 压缩包</span>
    </div>
  </div>
  <div v-if="pendingDropPath" class="dialog-backdrop" role="presentation">
    <section class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="drop-confirm-title">
      <h2 id="drop-confirm-title">确认导入 MOD</h2>
      <p>是否导入以下文件夹或压缩包？</p>
      <strong>{{ pendingDropPath }}</strong>
      <div class="section-actions">
        <button type="button" class="secondary-button" @click="cancelDroppedImport">取消</button>
        <button type="button" @click="confirmDroppedImport">确认导入</button>
      </div>
    </section>
  </div>
  <p v-if="dragError" class="drag-error">{{ dragError }}</p>
</template>

<style scoped>
.app-shell {
  display: grid;
  height: 100vh;
  grid-template-columns: 188px minmax(0, 1fr);
  overflow: hidden;
  background: #f4f7f6;
}

.app-workspace {
  display: block;
  height: 100vh;
  width: 100%;
  max-width: calc(100vw - 188px);
  min-width: 0;
  min-height: 0;
  overflow: auto;
}

.workspace-content {
  width: 100%;
  max-width: 100%;
  min-width: 0;
  min-height: 0;
  padding: 20px 24px 44px;
  overflow: visible;
}

.workspace-content.library-content {
  padding: 0 0 44px;
}

.workspace-page {
  width: 100%;
  max-width: 1180px;
  min-width: 0;
  margin: 0 auto;
}

.workspace-page.library-workspace {
  max-width: none;
  margin: 0;
}

.library-panel {
  padding: 0;
  border: 0;
  border-radius: 0;
  box-shadow: none;
}

.library-preview {
  gap: 0;
  margin-top: 0;
}

.library-controls {
  display: grid;
  gap: 10px;
  padding: 20px 24px 12px;
}

.library-preview :deep(.mod-table-region) {
  margin-top: 0;
}

.library-preview :deep(.batch-toolbar),
.library-preview :deep(.mod-table-scroll) {
  border-right: 0;
  border-left: 0;
  border-radius: 0;
}

h1,
h2,
p {
  margin: 0;
}

h1 {
  color: #17211f;
  font-size: 2.25rem;
  line-height: 1.1;
}

h2 {
  color: #17211f;
  font-size: 1.05rem;
  line-height: 1.3;
}

.status-pill {
  min-width: 88px;
  padding: 6px 12px;
  border: 1px solid #ccd8d4;
  border-radius: 999px;
  color: #435650;
  background: #ffffff;
  font-size: 0.85rem;
  font-weight: 700;
  text-align: center;
}

.status-pill.success {
  border-color: #a6d8bd;
  color: #17613f;
  background: #e8f6ee;
}

.status-pill.warning {
  border-color: #f1cf8a;
  color: #7a4d00;
  background: #fff7e6;
}

.panel {
  min-width: 0;
  padding: 22px;
  border: 1px solid #d9e2df;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 8px 24px rgba(34, 47, 62, 0.05);
}

.panel + .panel {
  margin-top: 18px;
}

.panel.secondary {
  box-shadow: none;
}

.panel-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 18px;
}

.panel-heading p {
  margin-top: 8px;
  color: #52645f;
}

.panel-heading.compact {
  align-items: center;
}

.game-text-language-control {
  display: grid;
  min-width: 180px;
  gap: 6px;
  color: #52645f;
  font-size: 0.8rem;
}

.game-text-language-control select {
  min-height: 40px;
  padding: 0 34px 0 10px;
  border: 1px solid #bdccc7;
  border-radius: 4px;
  color: #203b34;
  background: #ffffff;
  font: inherit;
}

.settings-inline-error {
  margin-top: 12px;
}

.path-form {
  display: grid;
  gap: 10px;
  margin-top: 24px;
}

.compact-path-form {
  margin-top: 14px;
}

.legacy-box-import {
  margin-top: 28px;
  padding-top: 22px;
  border-top: 1px solid #dfe8e4;
}

label,
dt {
  color: #61756f;
  font-size: 0.86rem;
  font-weight: 600;
}

h3 {
  margin: 0;
  color: #17211f;
  font-size: 0.95rem;
  line-height: 1.3;
}

.path-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
}

input,
select {
  width: 100%;
  min-height: 42px;
  padding: 0 12px;
  border: 1px solid #cbd8d4;
  border-radius: 6px;
  color: #17211f;
  background: #fbfdfc;
  font: inherit;
}

button {
  min-height: 42px;
  padding: 0 16px;
  border: 1px solid #1d6f55;
  border-radius: 6px;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

button:disabled {
  border-color: #aebbb7;
  color: #61756f;
  background: #e8eeec;
  cursor: not-allowed;
}

.secondary-button {
  min-height: 34px;
  padding: 0 12px;
  border-color: #cbd8d4;
  color: #24745b;
  background: #ffffff;
}

.danger-button {
  border-color: #e4b5ae;
  color: #b42318;
}

.facts {
  display: grid;
  gap: 0;
  margin: 24px 0 0;
}

.facts div {
  display: grid;
  grid-template-columns: 112px minmax(0, 1fr);
  gap: 18px;
  padding: 12px 0;
  border-top: 1px solid #edf1f0;
}

.facts.compact-facts {
  margin-top: 18px;
}

dd {
  min-width: 0;
  margin: 0;
  color: #17211f;
  font-weight: 700;
  overflow-wrap: anywhere;
}

.error {
  margin-top: 14px;
  color: #b42318;
}

.hint {
  margin-top: 8px;
  color: #61756f;
  font-size: 0.88rem;
}

.notice {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
  margin-top: 18px;
  padding: 14px;
  border-radius: 6px;
}

.warning-notice {
  border: 1px solid #f1cf8a;
  color: #7a4d00;
  background: #fff7e6;
}

.success-notice {
  border: 1px solid #a6d8bd;
  color: #17613f;
  background: #e8f6ee;
}

.preview-block {
  display: grid;
  gap: 10px;
  margin-top: 20px;
}

.section-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.section-actions {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.selection-count {
  color: #52645f;
  font-size: 0.84rem;
  font-weight: 700;
}

.file-preview,
.compact-list,
.mod-list,
.conflict-list,
.candidate-list,
.model-replacement-list {
  display: grid;
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.file-preview li,
.compact-list li,
.mod-list > li,
.conflict-list > li {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(160px, 0.55fr);
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid #edf1f0;
  border-radius: 6px;
  background: #fbfdfc;
}

.candidate-list li {
  padding: 0;
  border: 1px solid #edf1f0;
  border-radius: 6px;
  background: #fbfdfc;
}

.candidate-list .candidate-choice {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr);
  gap: 10px;
  align-items: center;
  padding: 12px;
  cursor: pointer;
}

.candidate-list .candidate-choice > input {
  width: 18px;
  min-height: 18px;
  margin: 0;
  padding: 0;
  accent-color: #24745b;
}

.candidate-list .candidate-choice > span {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.candidate-list strong,
.candidate-list small {
  overflow-wrap: anywhere;
}

.candidate-list small {
  color: #61756f;
}

.candidate-import-options {
  display: flex;
  flex-wrap: wrap;
  align-items: end;
  gap: 12px 18px;
  margin-bottom: 12px;
}

.candidate-selection-section {
  scroll-margin-top: 84px;
  outline: none;
}

.candidate-selection-section:focus-visible {
  box-shadow: 0 0 0 2px #8ebbaa;
}

.candidate-mode-control,
.candidate-selection-actions {
  display: flex;
  align-items: center;
  gap: 12px;
}

.candidate-mode-control label {
  display: flex;
  align-items: center;
  gap: 6px;
}

.group-name-field,
.branch-name-field {
  display: grid;
  gap: 5px;
  color: #536861;
  font-size: 0.82rem;
}

.group-name-field {
  min-width: min(320px, 100%);
}

.branch-name-field {
  grid-template-columns: auto minmax(180px, 320px);
  align-items: center;
  padding: 0 12px 12px 42px;
}

.branch-name-field input {
  min-width: 0;
}

.legacy-box-table-wrap {
  max-height: 520px;
  overflow: auto;
  border: 1px solid #dfe8e4;
  border-radius: 6px;
}

.legacy-box-table {
  width: 100%;
  min-width: 820px;
  border-collapse: collapse;
  table-layout: fixed;
}

.legacy-box-table th,
.legacy-box-table td {
  padding: 10px 12px;
  border-bottom: 1px solid #e7eeeb;
  color: #334b44;
  font-size: 0.87rem;
  text-align: left;
  vertical-align: middle;
}

.legacy-box-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  color: #52645f;
  background: #f6faf8;
  font-weight: 700;
}

.legacy-box-table th:nth-child(1),
.legacy-box-table td:nth-child(1) {
  width: 56px;
  text-align: center;
}

.legacy-box-table th:nth-child(2) {
  width: 35%;
}

.legacy-box-table th:nth-child(3) {
  width: 15%;
}

.legacy-box-table th:nth-child(4) {
  width: 25%;
}

.legacy-box-table tr:last-child td {
  border-bottom: 0;
}

.legacy-box-table tbody tr {
  cursor: pointer;
}

.legacy-box-table tbody tr:hover,
.legacy-box-table tbody tr.selected {
  background: #f2f8f5;
}

.legacy-box-table td strong,
.legacy-box-table td small {
  display: block;
  min-width: 0;
  overflow-wrap: anywhere;
}

.legacy-box-table td small {
  margin-top: 3px;
  color: #61756f;
}

.legacy-box-checkbox {
  width: 18px;
  min-height: 18px;
  margin: 0;
  padding: 0;
  accent-color: #24745b;
}

.legacy-box-status {
  display: inline-block;
  color: #52645f;
  font-weight: 700;
}

.legacy-box-status.success {
  color: #17613f;
}

.legacy-box-status.warning {
  color: #a15c00;
}

.legacy-box-status.neutral {
  color: #52645f;
}

.legacy-box-file-preview {
  max-height: 320px;
  overflow: auto;
}

.icon-button {
  display: grid;
  width: 32px;
  min-height: 32px;
  padding: 0;
  place-items: center;
  border-color: #cbd8d4;
  color: #24745b;
  background: #ffffff;
  line-height: 1;
}

.model-replacement-list li {
  display: grid;
  gap: 2px;
  padding: 10px 12px;
  border-left: 3px solid #72a995;
  background: #f2f8f5;
}

.model-replacement-list strong,
.model-replacement-list span,
.model-replacement-list small {
  min-width: 0;
  overflow-wrap: anywhere;
}

.model-replacement-list span {
  color: #334b44;
}

.model-replacement-list small {
  color: #61756f;
}

.model-replacement-list.compact {
  width: 100%;
  margin-top: 8px;
}

.model-replacement-list.compact li {
  padding: 8px 10px;
}

.compact-list li {
  grid-template-columns: minmax(0, 1fr) auto;
}

.mod-list > li {
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
}

.conflict-list > li {
  grid-template-columns: 1fr;
}

.mod-list > li > div {
  display: flex;
  min-width: 0;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}

.mod-list > li > .mod-summary {
  display: grid;
  gap: 2px;
}

.installed-file-details {
  width: 100%;
  margin-top: 6px;
}

.installed-file-details summary {
  width: fit-content;
  color: #345b50;
  cursor: pointer;
}

.installed-file-preview {
  max-height: 240px;
  margin-top: 8px;
  overflow: auto;
}

.mod-list li span {
  min-width: 0;
  color: #52645f;
  overflow-wrap: anywhere;
}

.conflict-list ul {
  display: grid;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.conflict-list ul li {
  display: flex;
  gap: 10px;
  align-items: center;
  justify-content: space-between;
  min-width: 0;
}

.file-preview span,
.file-preview strong,
.compact-list span,
.compact-list strong,
.mod-list strong,
.conflict-list span,
.conflict-list strong {
  min-width: 0;
  overflow-wrap: anywhere;
}

.file-preview span,
.compact-list span {
  color: #52645f;
}

.file-preview strong,
.compact-list strong {
  color: #17211f;
}

.conflict-workspace {
  width: min(1180px, 100%);
  min-height: 0;
  margin: 0 auto;
  padding: 0;
}

.conflict-layout {
  display: grid;
  grid-template-columns: minmax(260px, 0.38fr) minmax(0, 1fr);
  min-height: 560px;
  border: 1px solid #d9e2df;
  border-radius: 8px;
  background: #ffffff;
}

.conflict-sidebar {
  padding: 20px;
  border-right: 1px solid #d9e2df;
  background: #f7faf8;
}

.conflict-sidebar-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.conflict-sidebar-heading span {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 50%;
  color: #17613f;
  background: #ffffff;
  font-size: 0.82rem;
  font-weight: 700;
}

.conflict-group-list,
.conflict-order-list {
  display: grid;
  gap: 8px;
  margin: 18px 0 0;
  padding: 0;
  list-style: none;
}

.conflict-group-button {
  display: grid;
  width: 100%;
  min-height: 0;
  gap: 4px;
  padding: 12px;
  border-color: #d9e2df;
  color: #17211f;
  background: #ffffff;
  text-align: left;
}

.conflict-group-button span {
  color: #61756f;
  font-size: 0.82rem;
  font-weight: 400;
  overflow-wrap: anywhere;
}

.conflict-group-button.selected {
  border-color: #24745b;
  background: #e8f6ee;
}

.conflict-detail {
  min-width: 0;
  padding: 28px;
}

.conflict-order-list li {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
  padding: 12px;
  border: 1px solid #edf1f0;
  border-radius: 6px;
  background: #fbfdfc;
}

.shared-model-targets {
  display: grid;
  gap: 6px;
  margin-top: 18px;
  padding: 12px;
  border-left: 3px solid #b98929;
  background: #fff9ed;
}

.shared-model-targets strong {
  color: #7a4d00;
  font-size: 0.82rem;
}

.shared-model-targets span {
  color: #5f4a24;
  font-size: 0.86rem;
  overflow-wrap: anywhere;
}

.conflict-file-details {
  margin-top: 14px;
  border: 1px solid #d9e2df;
  border-radius: 6px;
  background: #fbfdfc;
}

.conflict-file-details summary {
  padding: 11px 12px;
  color: #315e52;
  cursor: pointer;
  font-size: 0.86rem;
  font-weight: 700;
}

.conflict-file-details ul {
  display: grid;
  gap: 5px;
  max-height: 240px;
  margin: 0;
  padding: 0 12px 12px 30px;
  overflow: auto;
  color: #52645f;
  font-family: Consolas, "Courier New", monospace;
  font-size: 0.78rem;
}

.conflict-file-details li {
  overflow-wrap: anywhere;
}

.conflict-order-list li > div:nth-child(2) {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.conflict-order-list li span {
  color: #52645f;
  overflow-wrap: anywhere;
}

.conflict-order-number {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 50%;
  color: #17613f;
  background: #ffffff;
  font-size: 0.82rem;
  font-weight: 700;
}

.drag-overlay {
  position: fixed;
  z-index: 10;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgba(18, 69, 52, 0.25);
  pointer-events: none;
}

.drag-overlay > div {
  display: grid;
  min-width: min(460px, 100%);
  gap: 8px;
  padding: 28px;
  border: 2px dashed #24745b;
  border-radius: 8px;
  color: #17613f;
  background: #ffffff;
  text-align: center;
}

.drag-overlay span {
  color: #52645f;
}

.dialog-backdrop {
  position: fixed;
  z-index: 20;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgba(23, 33, 31, 0.38);
}

.confirm-dialog {
  display: grid;
  width: min(520px, 100%);
  gap: 14px;
  padding: 24px;
  border: 1px solid #d9e2df;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 20px 60px rgba(23, 33, 31, 0.18);
}

.confirm-dialog p {
  color: #52645f;
}

.confirm-dialog > strong {
  padding: 10px 12px;
  border: 1px solid #edf1f0;
  border-radius: 6px;
  background: #fbfdfc;
  overflow-wrap: anywhere;
}

.confirmation-backdrop {
  z-index: 30;
}

.action-confirm-dialog {
  max-height: calc(100vh - 48px);
  overflow: auto;
  outline: none;
}

.confirmation-message {
  white-space: pre-line;
}

.confirmation-details {
  display: grid;
  gap: 6px;
  margin: 0;
  padding-left: 20px;
  color: #52645f;
}

.confirmation-details li::marker {
  color: #24745b;
}

.enable-conflict-list {
  display: grid;
  max-height: min(360px, 48vh);
  overflow: auto;
  border-top: 1px solid #e1e9e6;
}

.enable-conflict-list details {
  border-bottom: 1px solid #e1e9e6;
}

.enable-conflict-list summary {
  display: flex;
  min-height: 44px;
  gap: 12px;
  align-items: center;
  justify-content: space-between;
  padding: 8px 4px;
  color: #334b44;
  cursor: pointer;
}

.enable-conflict-list summary strong {
  overflow-wrap: anywhere;
}

.enable-conflict-list summary span {
  flex: none;
  color: #8a5a13;
  font-size: 0.76rem;
  font-weight: 700;
}

.enable-conflict-list ul {
  display: grid;
  gap: 5px;
  margin: 0;
  padding: 4px 8px 12px 24px;
  color: #52645f;
  font-size: 0.76rem;
}

.enable-conflict-list li {
  overflow-wrap: anywhere;
}

button.danger {
  border-color: #b9493b;
  color: #ffffff;
  background: #b9493b;
}

button.danger:hover {
  border-color: #943a2f;
  background: #943a2f;
}

.remap-dialog {
  width: min(620px, 100%);
  max-height: calc(100vh - 48px);
  overflow: auto;
}

.remap-dialog-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.remap-dialog-heading > div {
  display: grid;
  gap: 5px;
}

.remap-dialog-heading p {
  color: #52645f;
}

.dialog-close-button {
  position: relative;
  display: inline-grid;
  width: 34px;
  height: 34px;
  min-height: 34px;
  padding: 0;
  flex: 0 0 34px;
  place-items: center;
  border-color: #cbd8d4;
  color: #435650;
  background: #ffffff;
  font-size: 1.25rem;
}

.remap-fields {
  display: grid;
  grid-template-columns: 1fr;
  gap: 12px;
}

.remap-fields label {
  display: grid;
  min-width: 0;
  gap: 5px;
}

.remap-fields label > span {
  color: #61756f;
  font-size: 0.76rem;
  font-weight: 700;
}

.remap-group-tabs {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 2px;
}

.remap-group-tabs button {
  min-height: 34px;
  padding: 0 10px;
  flex: 0 0 auto;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #52645f;
  background: #ffffff;
  font: inherit;
  font-size: 0.78rem;
}

.remap-group-tabs button.active {
  border-color: #24745b;
  color: #17613f;
  background: #edf5f1;
}

.remap-warnings li {
  grid-template-columns: minmax(0, 1fr);
  border-color: #f1cf8a;
  color: #7a4d00;
  background: #fff7e6;
}

.remap-disabled-hint {
  padding: 9px 11px;
  border-left: 3px solid #c48a2c;
  color: #694800;
  background: #fff8e8;
}

.drag-error {
  position: fixed;
  z-index: 11;
  right: 20px;
  bottom: 20px;
  max-width: min(420px, calc(100vw - 40px));
  margin: 0;
  padding: 12px;
  border: 1px solid #e4b5ae;
  border-radius: 6px;
  color: #b42318;
  background: #ffffff;
}

@media (max-width: 760px) {
  .app-shell {
    height: auto;
    min-height: 100vh;
    grid-template-columns: 1fr;
    overflow: visible;
  }

  .app-workspace {
    height: auto;
    max-width: 100vw;
    min-height: 100vh;
    overflow: visible;
  }

  .workspace-content {
    padding: 16px 12px 40px;
    overflow: visible;
  }

  .remap-fields {
    grid-template-columns: 1fr;
  }

  .conflict-workspace {
    width: 100%;
    padding: 0;
  }

  .conflict-layout {
    grid-template-columns: 1fr;
  }

  .conflict-sidebar {
    border-right: 0;
    border-bottom: 1px solid #d9e2df;
  }

  .conflict-order-list li {
    grid-template-columns: 34px minmax(0, 1fr);
  }

  .conflict-order-list .section-actions {
    grid-column: 1 / -1;
  }

  .panel-heading,
  .path-row,
  .notice,
  .file-preview li,
  .compact-list li,
  .mod-list > li {
    grid-template-columns: 1fr;
  }

  .panel-heading,
  .notice {
    display: grid;
  }

  .section-title-row {
    display: grid;
  }

  .section-actions {
    justify-content: flex-start;
  }

  .status-pill {
    width: fit-content;
  }

  .facts div {
    grid-template-columns: 1fr;
    gap: 4px;
  }

}
</style>

<style>
:root {
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
    sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #17211f;
  background: #f4f7f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

* {
  box-sizing: border-box;
}

body {
  min-width: 320px;
  min-height: 100vh;
  margin: 0;
}
</style>
