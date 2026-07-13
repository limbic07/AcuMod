<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import AppSidebar, { type WorkspaceView } from "./components/AppSidebar.vue";
import AppTopbar from "./components/AppTopbar.vue";
import FloatingAgentPanel from "./components/FloatingAgentPanel.vue";
import ModCategoryManager from "./components/ModCategoryManager.vue";
import ModLibraryTable from "./components/ModLibraryTable.vue";
import ModLibraryToolbar from "./components/ModLibraryToolbar.vue";
import { getAppInfo, type AppInfo } from "./api/app";
import {
  detectGameDirectory,
  getGameDirectoryStatus,
  saveGameDirectory,
  type GameDirectoryStatus,
} from "./api/game";
import {
  applyConflictOrder,
  createUserModCategory,
  createModProfile,
  deleteUserModCategory,
  deleteModProfile,
  disableMod,
  enableMod,
  getModConflictReport,
  getModLibraryStatus,
  installModFromArchive,
  installModFromCandidate,
  installModFromFolder,
  listInstalledMods,
  listModProfiles,
  listUserModCategories,
  moveConflictParticipant,
  openInstalledModFolder,
  previewApplyConflictOrder,
  previewDisableMod,
  previewEnableMod,
  previewModImport,
  previewRestoreAllMods,
  previewSwitchModProfile,
  previewUninstallMod,
  renameModProfile,
  renameUserModCategory,
  restoreAllMods,
  switchModProfile,
  uninstallMod,
  updateModMetadata,
  type InstalledModList,
  type InstalledModSummary,
  type ApplyConflictOrderPlan,
  type ApplyConflictOrderResult,
  type ModConflictReport,
  type ModDeploymentPlan,
  type ModDeploymentResult,
  type ModDisablePlan,
  type ModImportPreview,
  type ModInstallResult,
  type ModLibraryStatus,
  type ModMetadataPatch,
  type ModelReplacement,
  type ModUninstallPlan,
  type ModUninstallResult,
  type ModProfileList,
  type ProfileSwitchPlan,
  type ProfileSwitchResult,
  type RestoreAllPlan,
  type RestoreAllResult,
  type UserModCategory,
} from "./api/modLibrary";

const appInfo = ref<AppInfo | null>(null);
const gameStatus = ref<GameDirectoryStatus | null>(null);
const modLibraryStatus = ref<ModLibraryStatus | null>(null);
const installedModList = ref<InstalledModList | null>(null);
const importPreview = ref<ModImportPreview | null>(null);
const installResult = ref<ModInstallResult | null>(null);
const deploymentPlan = ref<ModDeploymentPlan | null>(null);
const deploymentResult = ref<ModDeploymentResult | null>(null);
const disablePlan = ref<ModDisablePlan | null>(null);
const uninstallPlan = ref<ModUninstallPlan | null>(null);
const uninstallResult = ref<ModUninstallResult | null>(null);
const restorePlan = ref<RestoreAllPlan | null>(null);
const restoreResult = ref<RestoreAllResult | null>(null);
const conflictReport = ref<ModConflictReport | null>(null);
const modProfileList = ref<ModProfileList | null>(null);
const userModCategories = ref<UserModCategory[]>([]);
const profileSwitchPlan = ref<ProfileSwitchPlan | null>(null);
const profileSwitchResult = ref<ProfileSwitchResult | null>(null);
const conflictOrderPlan = ref<ApplyConflictOrderPlan | null>(null);
const conflictOrderResult = ref<ApplyConflictOrderResult | null>(null);
const manualPath = ref("");
const importPath = ref("");
const archivePath = ref("");
const candidateImportSourcePath = ref("");
const candidateOriginalArchivePath = ref<string | null>(null);
const selectedCandidateRootPath = ref("");
const appError = ref("");
const gameError = ref("");
const modLibraryError = ref("");
const importError = ref("");
const archiveError = ref("");
const deploymentError = ref("");
const isLoadingApp = ref(false);
const isLoadingGame = ref(false);
const isLoadingModLibrary = ref(false);
const isPreviewingImport = ref(false);
const isInstallingMod = ref(false);
const isInstallingArchive = ref(false);
const activeModAction = ref("");
const openingModFolderId = ref("");
const metadataSavingModId = ref("");
const metadataErrorModId = ref("");
const metadataError = ref("");
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
const profileError = ref("");
const isProfileAction = ref(false);
const isCategoryAction = ref(false);
const isCategoryManagerOpen = ref(false);
const pendingCategoryModId = ref("");
const categoryError = ref("");
const selectedProfileId = ref("");
const modSearchQuery = ref("");
const modCategoryFilter = ref("all");
const modStatusFilter = ref("all");
const modConflictFilter = ref("all");
const modSort = ref<"installation" | "name" | "category" | "replacement">("installation");
let stopDragListener: (() => void) | undefined;

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

const importStatusLabel = computed(() => {
  if (!importPreview.value) {
    return "未预览";
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

const previewedFiles = computed(() => importPreview.value?.files.slice(0, 12) ?? []);
const installedFiles = computed(() => installResult.value?.files.slice(0, 12) ?? []);
const installedMods = computed(() => installedModList.value?.mods ?? []);
function visibleModCategories(mod: InstalledModSummary) {
  const categories = [...mod.categories];
  const userCategoryName = mod.userCategory?.name;

  if (userCategoryName && !categories.includes(userCategoryName)) {
    categories.push(userCategoryName);
  }

  return categories.length ? categories : ["未识别"];
}

const availableModCategories = computed(() =>
  [...new Set(installedMods.value.flatMap((installedMod) => visibleModCategories(installedMod)))].sort(
    (left, right) => left.localeCompare(right, "zh-Hans-CN"),
  ),
);
const filteredInstalledMods = computed(() => {
  const searchText = modSearchQuery.value.trim().toLocaleLowerCase();

  return installedMods.value.filter((installedMod) => {
    if (
      modCategoryFilter.value !== "all" &&
      !visibleModCategories(installedMod).includes(modCategoryFilter.value)
    ) {
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
      ...visibleModCategories(installedMod),
      ...installedMod.modelReplacements.flatMap((replacement) => [
        replacement.modelKind,
        replacement.subKind,
        replacement.modelId,
        ...replacement.gameIds,
        ...replacement.displayNames,
      ]),
    ]
      .join(" ")
      .toLocaleLowerCase();
    return searchableText.includes(searchText);
  });
});
const displayedInstalledMods = computed(() => {
  const mods = [...filteredInstalledMods.value];

  mods.sort((left, right) => {
    if (modSort.value === "name") {
      return left.name.localeCompare(right.name, "zh-Hans-CN");
    }

    if (modSort.value === "category") {
      return visibleModCategories(left).join("、").localeCompare(
        visibleModCategories(right).join("、"),
        "zh-Hans-CN",
      )
        || left.name.localeCompare(right.name, "zh-Hans-CN");
    }

    if (modSort.value === "replacement") {
      return summarizeModReplacements(left).localeCompare(summarizeModReplacements(right), "zh-Hans-CN")
        || left.name.localeCompare(right.name, "zh-Hans-CN");
    }

    return right.installedAtUnixSeconds - left.installedAtUnixSeconds
      || left.name.localeCompare(right.name, "zh-Hans-CN");
  });

  return mods;
});
const enabledModCount = computed(
  () => installedMods.value.filter((installedMod) => installedMod.enabled).length,
);
const deploymentPlanFiles = computed(() => deploymentPlan.value?.files.slice(0, 12) ?? []);
const deployedFiles = computed(() => deploymentResult.value?.files.slice(0, 12) ?? []);
const disablePlanFiles = computed(() => disablePlan.value?.files.slice(0, 12) ?? []);
const uninstallLibraryFiles = computed(() => uninstallPlan.value?.libraryFiles.slice(0, 12) ?? []);
const restorePlanMods = computed(() => restorePlan.value?.mods.slice(0, 12) ?? []);
const restoreResultMods = computed(() => restoreResult.value?.mods.slice(0, 12) ?? []);
const conflictGroups = computed(() => conflictReport.value?.groups ?? []);
const modProfiles = computed(() => modProfileList.value?.profiles ?? []);
const activeModProfile = computed(() =>
  modProfiles.value.find((profile) => profile.isActive) ?? null,
);
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
          !partners.includes(otherParticipant.name)
        ) {
          partners.push(otherParticipant.name);
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
  const name = target.displayNames[0] ?? target.modelId;
  return `${modelKindLabel(target.modelKind)} · ${target.subKind} · ${name}`;
}

function summarizeModelNames(replacement: ModelReplacement) {
  if (!replacement.displayNames.length) {
    return replacement.recognitionSource === "pathPattern"
      ? "当前 ID 表暂无名称，已按资源路径识别"
      : "已识别模型 ID，当前无可用游戏名称";
  }

  const visibleNames = replacement.displayNames.slice(0, 4);
  const remainingCount = replacement.displayNames.length - visibleNames.length;
  return remainingCount > 0
    ? `${visibleNames.join("、")}，另有 ${remainingCount} 个共用模型名称`
    : visibleNames.join("、");
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
    appError.value = error instanceof Error ? error.message : String(error);
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
    gameError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isLoadingGame.value = false;
  }
}

async function loadModLibraryStatus() {
  isLoadingModLibrary.value = true;

  try {
    modLibraryStatus.value = await getModLibraryStatus();
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isLoadingModLibrary.value = false;
  }
}

async function loadInstalledMods() {
  try {
    installedModList.value = await listInstalledMods();
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = error instanceof Error ? error.message : String(error);
  }
}

async function loadUserModCategories() {
  try {
    const categoryList = await listUserModCategories();
    userModCategories.value = categoryList.categories;
    categoryError.value = "";
  } catch (error) {
    categoryError.value = error instanceof Error ? error.message : String(error);
  }
}

async function loadConflictReport() {
  try {
    conflictReport.value = await getModConflictReport();
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = error instanceof Error ? error.message : String(error);
  }
}

async function loadModProfiles() {
  try {
    const profiles = await listModProfiles();
    modProfileList.value = profiles;
    if (!profiles.profiles.some((profile) => profile.id === selectedProfileId.value)) {
      selectedProfileId.value = profiles.activeProfileId;
    }
    profileError.value = "";
  } catch (error) {
    profileError.value = error instanceof Error ? error.message : String(error);
  }
}

async function refreshModViews() {
  await Promise.all([
    loadInstalledMods(),
    loadConflictReport(),
    loadModProfiles(),
    loadUserModCategories(),
  ]);
}

async function refreshCurrentWorkspace() {
  if (activeView.value === "settings") {
    await Promise.all([loadGameStatus(), loadAppInfo()]);
    return;
  }

  if (activeView.value === "import") {
    await loadModLibraryStatus();
    return;
  }

  if (activeView.value === "conflicts") {
    await loadConflictReport();
    return;
  }

  await refreshModViews();
}

function syncCategoryFilter() {
  if (
    modCategoryFilter.value !== "all" &&
    !availableModCategories.value.includes(modCategoryFilter.value)
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
    await updateModMetadata(mod.id, patch);
    await loadInstalledMods();
    syncCategoryFilter();
    return true;
  } catch (error) {
    metadataErrorModId.value = mod.id;
    metadataError.value = error instanceof Error ? error.message : String(error);
    return false;
  } finally {
    metadataSavingModId.value = "";
  }
}

function openCategoryManager(mod?: InstalledModSummary) {
  pendingCategoryModId.value = mod?.id ?? "";
  categoryError.value = "";
  isCategoryManagerOpen.value = true;
}

function closeCategoryManager() {
  if (isCategoryAction.value) {
    return;
  }

  isCategoryManagerOpen.value = false;
  pendingCategoryModId.value = "";
  categoryError.value = "";
}

async function createUserCategory(name: string) {
  isCategoryAction.value = true;
  categoryError.value = "";
  let shouldCloseDialog = false;

  try {
    const category = await createUserModCategory(name);
    await loadUserModCategories();
    const targetMod = installedMods.value.find((mod) => mod.id === pendingCategoryModId.value);

    if (targetMod) {
      shouldCloseDialog = await saveModMetadata(targetMod, {
        categoryOverride: category.id,
      });
      if (!shouldCloseDialog) {
        categoryError.value = "分类已创建，但未能应用到目标 MOD。";
      }
    }
  } catch (error) {
    categoryError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isCategoryAction.value = false;
  }

  if (shouldCloseDialog) {
    closeCategoryManager();
  }
}

async function renameUserCategory(categoryId: string, name: string) {
  isCategoryAction.value = true;
  categoryError.value = "";

  try {
    await renameUserModCategory(categoryId, name);
    await Promise.all([loadUserModCategories(), loadInstalledMods()]);
    syncCategoryFilter();
  } catch (error) {
    categoryError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isCategoryAction.value = false;
  }
}

async function deleteUserCategory(category: UserModCategory) {
  const shouldDelete = window.confirm(
    `删除分类 ${category.name} 会使使用它的 MOD 回到自动分类。是否继续？`,
  );
  if (!shouldDelete) {
    return;
  }

  isCategoryAction.value = true;
  categoryError.value = "";

  try {
    await deleteUserModCategory(category.id);
    await Promise.all([loadUserModCategories(), loadInstalledMods()]);
    syncCategoryFilter();
  } catch (error) {
    categoryError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isCategoryAction.value = false;
  }
}

async function createProfile() {
  const name = window.prompt("新 Profile 会从当前启用状态和冲突顺序复制。请输入名称：", "");
  if (name === null) {
    return;
  }

  isProfileAction.value = true;
  try {
    const profile = await createModProfile(name);
    selectedProfileId.value = profile.id;
    profileError.value = "";
    await loadModProfiles();
  } catch (error) {
    profileError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isProfileAction.value = false;
  }
}

async function renameSelectedProfile() {
  const profile = modProfiles.value.find((item) => item.id === selectedProfileId.value);
  if (!profile) {
    return;
  }

  const name = window.prompt("请输入新的 Profile 名称：", profile.name);
  if (name === null) {
    return;
  }

  isProfileAction.value = true;
  try {
    await renameModProfile(profile.id, name);
    profileError.value = "";
    await loadModProfiles();
  } catch (error) {
    profileError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isProfileAction.value = false;
  }
}

async function deleteSelectedProfile() {
  const profile = modProfiles.value.find((item) => item.id === selectedProfileId.value);
  if (!profile || profile.isActive) {
    return;
  }

  if (!window.confirm(`删除 Profile ${profile.name} 不会删除 MOD 文件。是否继续？`)) {
    return;
  }

  isProfileAction.value = true;
  try {
    await deleteModProfile(profile.id);
    selectedProfileId.value = modProfileList.value?.activeProfileId ?? "";
    profileError.value = "";
    await loadModProfiles();
  } catch (error) {
    profileError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isProfileAction.value = false;
  }
}

async function switchSelectedProfile(profileId = selectedProfileId.value) {
  const activeProfile = activeModProfile.value;

  selectedProfileId.value = profileId;

  if (!profileId || !activeProfile || profileId === activeProfile.id) {
    return;
  }

  isProfileAction.value = true;
  try {
    const plan = await previewSwitchModProfile(profileId);
    profileSwitchPlan.value = plan;
    profileSwitchResult.value = null;
    profileError.value = "";
    const confirmed = window.confirm(
      `${plan.message}\n\n将启用：${plan.enableMods.length} 个 MOD\n将禁用：${plan.disableMods.length} 个 MOD` +
        (plan.requiresOverwriteConfirmation ? "\n将覆盖已有文件。" : "") +
        "\n\n是否切换？",
    );

    if (!confirmed) {
      selectedProfileId.value = activeProfile.id;
      return;
    }

    profileSwitchResult.value = await switchModProfile(
      profileId,
      plan.requiresOverwriteConfirmation,
    );
    await refreshModViews();
  } catch (error) {
    profileError.value = error instanceof Error ? error.message : String(error);
    selectedProfileId.value = activeProfile.id;
  } finally {
    isProfileAction.value = false;
  }
}

function openConflictManager() {
  if (!selectedConflictGroupId.value && conflictGroups.value.length) {
    selectedConflictGroupId.value = conflictGroups.value[0].groupId;
  }

  conflictActionError.value = "";
  conflictOrderPlan.value = null;
  conflictOrderResult.value = null;
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
  conflictOrderPlan.value = null;
  conflictOrderResult.value = null;
}

async function runGameAction(action: () => Promise<GameDirectoryStatus>) {
  isLoadingGame.value = true;

  try {
    const status = await action();
    applyGameStatus(status);
    gameError.value = "";
  } catch (error) {
    gameError.value = error instanceof Error ? error.message : String(error);
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

async function previewImportPath(allowGameRoot = false) {
  isPreviewingImport.value = true;

  try {
    installResult.value = null;
    const preview = await previewModImport(importPath.value, allowGameRoot);
    importPreview.value = preview;
    candidateImportSourcePath.value = preview.status === "ambiguous" ? preview.sourcePath : "";
    candidateOriginalArchivePath.value = null;
    selectedCandidateRootPath.value = preview.candidates[0]?.rootPath ?? "";
    importError.value = "";

    if (preview.requiresGameRootConfirmation) {
      const shouldUseGameRoot = window.confirm(
        "未识别到 nativePC 或常见 nativePC 内部目录。这个 MOD 可能需要安装到游戏根目录。是否按游戏根目录预览？",
      );

      if (shouldUseGameRoot) {
        await previewImportPath(true);
      }
    }
  } catch (error) {
    importError.value = error instanceof Error ? error.message : String(error);
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
    await refreshModViews();
  } catch (error) {
    importError.value = error instanceof Error ? error.message : String(error);
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
    candidateOriginalArchivePath.value =
      outcome.status === "ambiguous" ? outcome.originalArchivePath : null;
    selectedCandidateRootPath.value = outcome.preview?.candidates[0]?.rootPath ?? "";
    await loadModLibraryStatus();
    await refreshModViews();
  } catch (error) {
    archiveError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isInstallingArchive.value = false;
  }
}

async function installSelectedCandidate() {
  if (!candidateImportSourcePath.value || !selectedCandidateRootPath.value) {
    return;
  }

  isInstallingMod.value = true;

  try {
    installResult.value = await installModFromCandidate(
      candidateImportSourcePath.value,
      selectedCandidateRootPath.value,
      candidateOriginalArchivePath.value,
    );
    importError.value = "";
    archiveError.value = "";
    importPreview.value = null;
    candidateImportSourcePath.value = "";
    candidateOriginalArchivePath.value = null;
    selectedCandidateRootPath.value = "";
    await refreshModViews();
    await loadModLibraryStatus();
  } catch (error) {
    importError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isInstallingMod.value = false;
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
    dragError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isHandlingDrop.value = false;
  }
}

async function enableInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;

  try {
    deploymentResult.value = null;
    disablePlan.value = null;
    const plan = await previewEnableMod(mod.id);
    deploymentPlan.value = plan;
    deploymentError.value = "";

    let confirmOverwrite = false;

    if (plan.requiresOverwriteConfirmation) {
      confirmOverwrite = window.confirm(
        `启用 ${mod.name} 会覆盖 ${plan.fileCount} 个目标文件中的已有文件。是否继续？`,
      );

      if (!confirmOverwrite) {
        return;
      }
    }

    deploymentResult.value = await enableMod(mod.id, confirmOverwrite);
    await refreshModViews();
  } catch (error) {
    deploymentError.value = error instanceof Error ? error.message : String(error);
  } finally {
    activeModAction.value = "";
  }
}

async function disableInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;

  try {
    deploymentPlan.value = null;
    deploymentResult.value = null;
    const plan = await previewDisableMod(mod.id);
    disablePlan.value = plan;
    deploymentError.value = "";

    const shouldDisable = window.confirm(
      `禁用 ${mod.name} 会删除游戏目录中已记录的 ${plan.fileCount} 个部署文件，MOD 库内副本会保留。是否继续？`,
    );

    if (!shouldDisable) {
      return;
    }

    deploymentResult.value = await disableMod(mod.id);
    await refreshModViews();
  } catch (error) {
    deploymentError.value = error instanceof Error ? error.message : String(error);
  } finally {
    activeModAction.value = "";
  }
}

async function showInstalledModFolder(mod: InstalledModSummary) {
  openingModFolderId.value = mod.id;

  try {
    await openInstalledModFolder(mod.id);
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = error instanceof Error ? error.message : String(error);
  } finally {
    openingModFolderId.value = "";
  }
}

async function uninstallInstalledMod(mod: InstalledModSummary) {
  activeModAction.value = mod.id;

  try {
    uninstallResult.value = null;
    const plan = await previewUninstallMod(mod.id);
    uninstallPlan.value = plan;
    deploymentError.value = "";

    const shouldUninstall = window.confirm(
      `卸载 ${mod.name} 会删除 Acumod 本地 MOD 库副本 ${plan.libraryFileCount} 个文件` +
        (plan.deployedFileCount > 0
          ? `，并先清理游戏目录中已记录的 ${plan.deployedFileCount} 个部署文件。`
          : "。") +
        "是否继续？",
    );

    if (!shouldUninstall) {
      return;
    }

    uninstallResult.value = await uninstallMod(mod.id);
    deploymentPlan.value = null;
    deploymentResult.value = null;
    disablePlan.value = null;
    await loadModLibraryStatus();
    await refreshModViews();
  } catch (error) {
    deploymentError.value = error instanceof Error ? error.message : String(error);
  } finally {
    activeModAction.value = "";
  }
}

async function restoreAllInstalledMods() {
  isRestoringAll.value = true;

  try {
    restoreResult.value = null;
    const plan = await previewRestoreAllMods();
    restorePlan.value = plan;
    deploymentError.value = "";

    if (plan.affectedModCount === 0) {
      return;
    }

    const shouldRestore = window.confirm(
      `一键还原会禁用 ${plan.affectedModCount} 个 MOD，并删除 ${plan.deployedFileCount} 个由 Acumod 记录的部署文件。是否继续？`,
    );

    if (!shouldRestore) {
      return;
    }

    restoreResult.value = await restoreAllMods();
    deploymentPlan.value = null;
    deploymentResult.value = null;
    disablePlan.value = null;
    uninstallPlan.value = null;
    uninstallResult.value = null;
    await refreshModViews();
  } catch (error) {
    deploymentError.value = error instanceof Error ? error.message : String(error);
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
    await moveConflictParticipant(selectedConflictGroupId.value, modId, direction);
    conflictActionError.value = "";
    await loadConflictReport();
  } catch (error) {
    conflictActionError.value = error instanceof Error ? error.message : String(error);
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
    conflictOrderResult.value = null;
    const plan = await previewApplyConflictOrder(selectedConflictGroupId.value);
    conflictOrderPlan.value = plan;
    conflictActionError.value = "";

    if (plan.applicableFileCount === 0) {
      return;
    }

    const shouldApply = window.confirm(
      plan.requiresOverwriteConfirmation
        ? "目标文件不是 Acumod 已记录的文件，将被覆盖。是否继续？"
        : `将当前覆盖顺序应用到 ${plan.applicableFileCount} 个冲突文件，是否继续？`,
    );

    if (!shouldApply) {
      return;
    }

    conflictOrderResult.value = await applyConflictOrder(
      selectedConflictGroupId.value,
      plan.requiresOverwriteConfirmation,
    );
    await refreshModViews();
  } catch (error) {
    conflictActionError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isApplyingConflict.value = false;
  }
}

onMounted(() => {
  void loadAppInfo();
  void loadGameStatus();
  void loadModLibraryStatus();
  void refreshModViews();
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
  stopDragListener?.();
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
        :is-refreshing="isLoadingGame || isLoadingApp || isLoadingModLibrary"
        :agent-open="isAgentPanelOpen"
        @refresh="refreshCurrentWorkspace"
        @toggle-agent="isAgentPanelOpen = !isAgentPanelOpen"
      />

      <div class="workspace-content">
        <div v-show="activeView === 'settings'" class="workspace-page">
          <section class="panel">
      <div class="panel-heading">
        <div>
          <h2>游戏目录</h2>
          <p>{{ gameStatus?.message ?? "Loading game directory status..." }}</p>
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
          <dd>{{ gameStatus.source }}</dd>
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
            <div class="panel-heading compact">
              <div>
                <h2>应用信息</h2>
                <p v-if="isLoadingApp">Loading app info...</p>
                <p v-else-if="appError" class="error">{{ appError }}</p>
                <p v-else>{{ appInfo?.backend ?? "No backend response yet." }}</p>
              </div>
            </div>

            <dl v-if="appInfo" class="facts compact-facts">
              <div>
                <dt>Name</dt>
                <dd>{{ appInfo.name }}</dd>
              </div>
              <div>
                <dt>Version</dt>
                <dd>{{ appInfo.version }}</dd>
              </div>
            </dl>
          </section>
        </div>

        <div v-show="activeView === 'import'" class="workspace-page">
          <section class="panel">
      <div class="panel-heading">
        <div>
          <h2>MOD 导入识别预览</h2>
          <p>
            {{
              importPreview?.message ??
              modLibraryStatus?.message ??
              "Loading MOD library status..."
            }}
          </p>
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
            placeholder="D:\Downloads\Cool Sword Mod"
          />
          <button type="submit" :disabled="isPreviewingImport || !importPath">
            {{ isPreviewingImport ? "识别中" : "预览" }}
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
            placeholder="D:\Downloads\Cool Sword Mod.zip"
          />
          <button type="submit" :disabled="isInstallingArchive || !archivePath">
            {{ isInstallingArchive ? "解包导入中" : "导入压缩包" }}
          </button>
        </div>
        <p class="hint">支持 .zip / .7z / .rar；通过 Acumod 内置解包组件处理。</p>
      </form>

      <div
        v-if="importPreview?.requiresGameRootConfirmation"
        class="notice warning-notice"
      >
        <p>{{ importPreview.message }}</p>
        <button type="button" :disabled="isPreviewingImport" @click="confirmGameRootPreview">
          确认按游戏根目录预览
        </button>
      </div>

      <div v-if="importPreview?.status === 'ready'" class="notice success-notice">
        <p>当前预览可以导入到 Acumod 本地 MOD 库；这一步不会写入 MHW 游戏目录。</p>
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
          <dd>{{ importPreview.detectionMethod }}</dd>
        </div>
        <div>
          <dt>部署根</dt>
          <dd>{{ importPreview.deployRoot }}</dd>
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
          <dd>{{ installResult.message }}</dd>
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
          <dt>Manifest</dt>
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
          </li>
        </ul>
      </div>

      <div v-if="importPreview?.candidates.length" class="preview-block">
        <div class="section-title-row">
          <h3>选择 MOD 版本</h3>
          <button
            type="button"
            class="secondary-button"
            :disabled="isInstallingMod || !selectedCandidateRootPath"
            @click="installSelectedCandidate"
          >
            {{ isInstallingMod ? "导入中" : "导入所选版本" }}
          </button>
        </div>
        <ul class="candidate-list">
          <li v-for="candidate in importPreview.candidates" :key="candidate.rootPath">
            <label>
              <input
                v-model="selectedCandidateRootPath"
                type="radio"
                name="mod-import-candidate"
                :value="candidate.rootPath"
              />
              <span>
                <strong>{{ candidate.relativePath || candidate.rootPath }}</strong>
                <small>{{ candidate.fileCount }} files / {{ candidate.deployRoot }}</small>
              </span>
            </label>
          </li>
        </ul>
      </div>

      <div v-if="previewedFiles.length" class="preview-block">
        <h3>部署路径预览</h3>
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

        <div v-show="activeView === 'library'" class="workspace-page">
          <section class="panel">
      <div class="preview-block">
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
            <button type="button" class="secondary-button" @click="refreshModViews">
              刷新
            </button>
          </div>
        </div>
        <ModLibraryToolbar
          :profiles="modProfiles"
          :active-profile="activeModProfile"
          :selected-profile-id="selectedProfileId"
          :is-profile-action="isProfileAction"
          :is-category-action="isCategoryAction"
          :search-query="modSearchQuery"
          :category-filter="modCategoryFilter"
          :status-filter="modStatusFilter"
          :conflict-filter="modConflictFilter"
          :sort="modSort"
          :categories="availableModCategories"
          @select-profile="switchSelectedProfile"
          @create-profile="createProfile"
          @rename-profile="renameSelectedProfile"
          @delete-profile="deleteSelectedProfile"
          @manage-categories="openCategoryManager()"
          @update-search-query="modSearchQuery = $event"
          @update-category-filter="modCategoryFilter = $event"
          @update-status-filter="modStatusFilter = $event"
          @update-conflict-filter="modConflictFilter = $event"
          @update-sort="modSort = $event"
        />
        <p v-if="profileError" class="error">{{ profileError }}</p>
        <p v-if="categoryError && !isCategoryManagerOpen" class="error">{{ categoryError }}</p>
        <p class="hint">{{ installedModList?.message ?? "正在读取本地 MOD 库..." }}</p>
        <p v-if="installedMods.length" class="hint">
          显示 {{ displayedInstalledMods.length }} / {{ installedMods.length }} 个 MOD；此处排序不影响冲突覆盖顺序。
        </p>
        <ModLibraryTable
          :mods="displayedInstalledMods"
          :installed-mod-count="installedMods.length"
          :user-categories="userModCategories"
          :conflicting-mod-ids="conflictingModIds"
          :conflict-partner-names="conflictPartnerNames"
          :active-mod-action="activeModAction"
          :opening-mod-folder-id="openingModFolderId"
          :metadata-saving-mod-id="metadataSavingModId"
          :metadata-error-mod-id="metadataErrorModId"
          :metadata-error="metadataError"
          @update-metadata="saveModMetadata"
          @create-category="openCategoryManager"
          @open-folder="showInstalledModFolder"
          @enable="enableInstalledMod"
          @disable="disableInstalledMod"
          @uninstall="uninstallInstalledMod"
        />
      </div>

      <div v-if="profileSwitchPlan" class="preview-block">
        <h3>Profile 切换预览</h3>
        <p class="hint">{{ profileSwitchPlan.message }}</p>
        <ul v-if="profileSwitchPlan.enableMods.length || profileSwitchPlan.disableMods.length" class="compact-list">
          <li v-for="mod in profileSwitchPlan.enableMods" :key="`enable-${mod.modId}`">
            <span>启用：{{ mod.name }}</span>
            <strong>{{ mod.fileCount }} 个文件</strong>
          </li>
          <li v-for="mod in profileSwitchPlan.disableMods" :key="`disable-${mod.modId}`">
            <span>禁用：{{ mod.name }}</span>
            <strong>{{ mod.fileCount }} 个文件</strong>
          </li>
        </ul>
        <ul v-if="profileSwitchPlan.warnings.length" class="compact-list">
          <li v-for="warning in profileSwitchPlan.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>

      <div v-if="profileSwitchResult" class="preview-block">
        <h3>Profile 切换结果</h3>
        <p class="hint">{{ profileSwitchResult.message }}</p>
        <p class="hint">
          已启用 {{ profileSwitchResult.enabledModCount }} 个，已禁用 {{ profileSwitchResult.disabledModCount }} 个，已应用 {{ profileSwitchResult.appliedConflictGroupCount }} 组冲突顺序。
        </p>
        <ul v-if="profileSwitchResult.warnings.length" class="compact-list">
          <li v-for="warning in profileSwitchResult.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>

      <p v-if="deploymentError" class="error">{{ deploymentError }}</p>

      <div v-if="deploymentPlan" class="preview-block">
        <h3>启用计划</h3>
        <p class="hint">{{ deploymentPlan.message }}</p>
        <ul v-if="deploymentPlan.warnings.length" class="compact-list">
          <li v-for="warning in deploymentPlan.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="deploymentPlanFiles.length" class="file-preview">
          <li v-for="file in deploymentPlanFiles" :key="file.targetPath">
            <span>{{ file.deployRelativePath }}</span>
            <strong>{{ file.targetExists ? "目标已存在" : file.targetPath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="disablePlan" class="preview-block">
        <h3>禁用预览</h3>
        <p class="hint">{{ disablePlan.message }}</p>
        <ul v-if="disablePlan.warnings.length" class="compact-list">
          <li v-for="warning in disablePlan.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="disablePlanFiles.length" class="file-preview">
          <li v-for="file in disablePlanFiles" :key="file.deployedPath">
            <span>{{ file.deployRelativePath }}</span>
            <strong>{{ file.deployedPath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="deploymentResult" class="preview-block">
        <h3>启停结果</h3>
        <p class="hint">{{ deploymentResult.message }}</p>
        <ul v-if="deploymentResult.warnings.length" class="compact-list">
          <li v-for="warning in deploymentResult.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="deployedFiles.length" class="file-preview">
          <li v-for="file in deployedFiles" :key="file.deployedPath">
            <span>{{ file.deployRelativePath }}</span>
            <strong>{{ file.deployedPath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="restorePlan" class="preview-block">
        <h3>一键还原预览</h3>
        <p class="hint">{{ restorePlan.message }}</p>
        <ul v-if="restorePlan.warnings.length" class="compact-list">
          <li v-for="warning in restorePlan.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="restorePlanMods.length" class="compact-list">
          <li v-for="mod in restorePlanMods" :key="mod.modId">
            <span>{{ mod.name }}</span>
            <strong>{{ mod.deployedFileCount }} files</strong>
          </li>
        </ul>
      </div>

      <div v-if="restoreResult" class="preview-block">
        <h3>一键还原结果</h3>
        <p class="hint">{{ restoreResult.message }}</p>
        <ul v-if="restoreResult.warnings.length" class="compact-list">
          <li v-for="warning in restoreResult.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="restoreResultMods.length" class="compact-list">
          <li v-for="mod in restoreResultMods" :key="mod.modId">
            <span>{{ mod.name }}</span>
            <strong>{{ mod.deployedFileCount }} files</strong>
          </li>
        </ul>
      </div>

      <div v-if="uninstallPlan" class="preview-block">
        <h3>卸载预览</h3>
        <p class="hint">{{ uninstallPlan.message }}</p>
        <ul v-if="uninstallPlan.warnings.length" class="compact-list">
          <li v-for="warning in uninstallPlan.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
        <ul v-if="uninstallLibraryFiles.length" class="file-preview">
          <li v-for="file in uninstallLibraryFiles" :key="file.libraryRelativePath">
            <span>{{ file.deployRelativePath }}</span>
            <strong>{{ file.libraryRelativePath }}</strong>
          </li>
        </ul>
      </div>

      <div v-if="uninstallResult" class="preview-block">
        <h3>卸载结果</h3>
        <p class="hint">{{ uninstallResult.message }}</p>
        <dl class="facts compact-facts">
          <div>
            <dt>部署文件</dt>
            <dd>{{ uninstallResult.removedDeployedFileCount }}</dd>
          </div>
          <div>
            <dt>库内文件</dt>
            <dd>{{ uninstallResult.removedLibraryFileCount }}</dd>
          </div>
        </dl>
        <ul v-if="uninstallResult.warnings.length" class="compact-list">
          <li v-for="warning in uninstallResult.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>

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
              <strong>{{ group.participants.map((participant) => participant.name).join(" / ") }}</strong>
              <span>{{ group.participantCount }} 个 MOD / {{ group.conflictFileCount }} 个冲突文件</span>
            </button>
          </li>
        </ul>
      </aside>

      <section class="conflict-detail">
        <template v-if="selectedConflictGroup">
          <div class="panel-heading">
            <div>
              <h2>{{ selectedConflictGroup.participants.map((participant) => participant.name).join(" / ") }}</h2>
              <p>从上到下为覆盖顺序；对每个冲突文件，排在最后且包含该文件的已启用 MOD 生效。</p>
            </div>
            <button
              type="button"
              :disabled="isApplyingConflict || selectedConflictGroup.enabledParticipantCount === 0"
              @click="applySelectedConflictOrder"
            >
              {{ isApplyingConflict ? "应用中" : "应用此组顺序" }}
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

          <ol class="conflict-order-list">
            <li v-for="(participant, index) in selectedConflictGroup.participants" :key="participant.modId">
              <span class="conflict-order-number">{{ participant.order }}</span>
              <div>
                <strong>{{ participant.name }}</strong>
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

          <div v-if="conflictOrderPlan" class="preview-block">
            <h3>应用预览</h3>
            <p class="hint">{{ conflictOrderPlan.message }}</p>
            <ul v-if="conflictOrderPlan.warnings.length" class="compact-list">
              <li v-for="warning in conflictOrderPlan.warnings" :key="warning">
                <span>{{ warning }}</span>
              </li>
            </ul>
          </div>

          <div v-if="conflictOrderResult" class="preview-block">
            <h3>应用结果</h3>
            <p class="hint">{{ conflictOrderResult.message }}</p>
          </div>
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
    :categories="userModCategories"
    :is-busy="isCategoryAction"
    :error="categoryError"
    @close="closeCategoryManager"
    @create="createUserCategory"
    @rename="renameUserCategory"
    @delete="deleteUserCategory"
  />

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
  grid-template-columns: 224px minmax(0, 1fr);
  min-height: 100vh;
  background: #f4f7f6;
}

.app-workspace {
  display: grid;
  min-width: 0;
  min-height: 100vh;
  grid-template-rows: auto minmax(0, 1fr);
}

.workspace-content {
  min-width: 0;
  padding: 24px 28px 48px;
  overflow: auto;
}

.workspace-page {
  width: min(1180px, 100%);
  margin: 0 auto;
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
  padding: 28px;
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

.path-form {
  display: grid;
  gap: 10px;
  margin-top: 24px;
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

.profile-toolbar {
  display: flex;
  align-items: end;
  gap: 10px;
  margin-top: 18px;
  padding: 12px;
  border: 1px solid #dfe7e3;
  border-radius: 6px;
  background: #f7faf8;
}

.profile-selector {
  display: grid;
  min-width: min(280px, 100%);
  gap: 5px;
}

.profile-selector span,
.mod-browser-controls label > span {
  color: #61756f;
  font-size: 0.74rem;
  font-weight: 700;
}

.profile-actions {
  display: flex;
  gap: 6px;
}

.profile-summary {
  min-width: 0;
  color: #52645f;
  font-size: 0.82rem;
  overflow-wrap: anywhere;
}

.mod-browser-controls {
  display: grid;
  grid-template-columns: minmax(220px, 1.6fr) repeat(4, minmax(110px, 0.55fr));
  gap: 10px;
  margin-top: 18px;
}

.mod-browser-controls label {
  display: grid;
  min-width: 0;
  gap: 5px;
}

.mod-browser-controls input,
.mod-browser-controls select,
.profile-selector select {
  min-width: 0;
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

.candidate-list label {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr);
  gap: 10px;
  align-items: center;
  padding: 12px;
  cursor: pointer;
}

.candidate-list input {
  width: 18px;
  min-height: 18px;
  margin: 0;
  padding: 0;
  accent-color: #24745b;
}

.candidate-list label > span {
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

.mod-table-scroll {
  margin-top: 12px;
  overflow: auto;
  border: 1px solid #dfe7e3;
  border-radius: 6px;
}

.mod-table {
  width: 100%;
  min-width: 1020px;
  border-collapse: collapse;
  background: #ffffff;
}

.mod-table th,
.mod-table td {
  padding: 12px;
  border-bottom: 1px solid #e7eeeb;
  color: #435650;
  font-size: 0.86rem;
  text-align: left;
  vertical-align: middle;
}

.mod-table th {
  color: #61756f;
  background: #f7faf8;
  font-size: 0.76rem;
  font-weight: 750;
  white-space: nowrap;
}

.mod-table tbody tr:last-child td {
  border-bottom: 0;
}

.mod-table tbody tr:hover {
  background: #f9fcfa;
}

.mod-table td:nth-child(1) {
  width: 56px;
}

.mod-table td:nth-child(2) {
  min-width: 180px;
}

.mod-table td:nth-child(3) {
  width: 130px;
}

.mod-table td:nth-child(4) {
  min-width: 300px;
}

.mod-table td:nth-child(5) {
  width: 150px;
}

.mod-table td:nth-child(6) {
  width: 164px;
}

.mod-index {
  color: #72837e !important;
  font-variant-numeric: tabular-nums;
}

.mod-name strong {
  display: block;
  color: #17211f;
  overflow-wrap: anywhere;
}

.mod-name small {
  display: block;
  margin-top: 3px;
  color: #72837e;
  font-size: 0.76rem;
  overflow-wrap: anywhere;
}

.replacement-summary {
  color: #334b44 !important;
  line-height: 1.45;
}

.mod-table .conflict-state {
  color: #9a3412;
  font-weight: 700;
}

.mod-actions {
  white-space: nowrap;
}

.mod-action-buttons {
  display: flex;
  gap: 6px;
  justify-content: flex-start;
}

.icon-button {
  position: relative;
  display: grid;
  width: 32px;
  min-height: 32px;
  padding: 0;
  place-items: center;
  border-color: #cbd8d4;
  border-radius: 5px;
  color: #24745b;
  background: #ffffff;
  font-size: 0.86rem;
}

.icon-button:hover:not(:disabled),
.icon-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.icon-button.warning-icon {
  color: #9a5b00;
}

.icon-button.danger-icon {
  color: #b42318;
  font-size: 1rem;
}

.icon-button.busy span {
  animation: subtle-pulse 1.1s ease-in-out infinite;
}

.icon-button[data-tooltip]::after {
  position: absolute;
  z-index: 4;
  right: 0;
  top: calc(100% + 7px);
  display: none;
  padding: 5px 7px;
  border: 1px solid #cbd8d4;
  border-radius: 4px;
  color: #ffffff;
  background: #17211f;
  content: attr(data-tooltip);
  font-size: 0.72rem;
  font-weight: 600;
  line-height: 1.2;
  white-space: nowrap;
}

.icon-button[data-tooltip]:hover::after,
.icon-button[data-tooltip]:focus-visible::after {
  display: block;
}

.empty-table-state {
  margin: 12px 0 0;
  padding: 20px 12px;
  border: 1px dashed #cbd8d4;
  border-radius: 6px;
  color: #61756f;
  text-align: center;
}

@keyframes subtle-pulse {
  50% {
    opacity: 0.4;
  }
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
    grid-template-columns: 1fr;
  }

  .workspace-content {
    padding: 16px 12px 40px;
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

  .profile-toolbar,
  .mod-browser-controls {
    grid-template-columns: 1fr;
  }

  .profile-toolbar {
    display: grid;
    align-items: stretch;
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
