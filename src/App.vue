<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getAppInfo, type AppInfo } from "./api/app";
import {
  detectGameDirectory,
  getGameDirectoryStatus,
  saveGameDirectory,
  type GameDirectoryStatus,
} from "./api/game";
import {
  applyConflictOrder,
  disableMod,
  enableMod,
  getModConflictReport,
  getModLibraryStatus,
  installModFromArchive,
  installModFromCandidate,
  installModFromFolder,
  listInstalledMods,
  moveConflictParticipant,
  openInstalledModFolder,
  previewApplyConflictOrder,
  previewDisableMod,
  previewEnableMod,
  previewModImport,
  previewRestoreAllMods,
  previewUninstallMod,
  restoreAllMods,
  uninstallMod,
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
  type ModelReplacement,
  type ModUninstallPlan,
  type ModUninstallResult,
  type RestoreAllPlan,
  type RestoreAllResult,
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
const isRestoringAll = ref(false);
const isApplyingConflict = ref(false);
const isConflictManagerOpen = ref(false);
const selectedConflictGroupId = ref("");
const isDragActive = ref(false);
const isHandlingDrop = ref(false);
const pendingDropPath = ref("");
const dragError = ref("");
const conflictActionError = ref("");
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
const deploymentPlanFiles = computed(() => deploymentPlan.value?.files.slice(0, 12) ?? []);
const deployedFiles = computed(() => deploymentResult.value?.files.slice(0, 12) ?? []);
const disablePlanFiles = computed(() => disablePlan.value?.files.slice(0, 12) ?? []);
const uninstallLibraryFiles = computed(() => uninstallPlan.value?.libraryFiles.slice(0, 12) ?? []);
const restorePlanMods = computed(() => restorePlan.value?.mods.slice(0, 12) ?? []);
const restoreResultMods = computed(() => restoreResult.value?.mods.slice(0, 12) ?? []);
const conflictGroups = computed(() => conflictReport.value?.groups ?? []);
const conflictingModIds = computed(
  () =>
    new Set(
      conflictGroups.value.flatMap((group) =>
        group.participants.map((participant) => participant.modId),
      ),
    ),
);
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

async function loadConflictReport() {
  try {
    conflictReport.value = await getModConflictReport();
    modLibraryError.value = "";
  } catch (error) {
    modLibraryError.value = error instanceof Error ? error.message : String(error);
  }
}

async function refreshModViews() {
  await loadInstalledMods();
  await loadConflictReport();
}

function openConflictManager() {
  if (!selectedConflictGroupId.value && conflictGroups.value.length) {
    selectedConflictGroupId.value = conflictGroups.value[0].groupId;
  }

  conflictActionError.value = "";
  conflictOrderPlan.value = null;
  conflictOrderResult.value = null;
  isConflictManagerOpen.value = true;
}

function closeConflictManager() {
  isConflictManagerOpen.value = false;
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
    await loadInstalledMods();
    await loadConflictReport();
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
    await loadInstalledMods();
    await loadConflictReport();
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
    await loadInstalledMods();
    await loadConflictReport();
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
    await loadInstalledMods();
    await loadConflictReport();
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
    await loadInstalledMods();
    await loadConflictReport();
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
    await loadInstalledMods();
    await loadConflictReport();
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
  void loadInstalledMods();
  void loadConflictReport();
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
  <main v-if="!isConflictManagerOpen" class="app-shell">
    <header class="topbar">
      <div>
        <p class="eyebrow">Acumod</p>
        <h1>MHW MOD Manager</h1>
      </div>
      <span class="status-pill" :class="statusClass">{{ statusLabel }}</span>
    </header>

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
        <h3>模型替换识别</h3>
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
        <p class="hint">{{ installedModList?.message ?? "正在读取本地 MOD 库..." }}</p>
        <ul v-if="installedMods.length" class="mod-list">
          <li v-for="(mod, index) in installedMods" :key="mod.id">
            <div class="mod-summary">
              <strong>#{{ index + 1 }} {{ mod.name }}</strong>
              <span>{{ mod.id }}</span>
              <ul v-if="mod.modelReplacements.length" class="model-replacement-list compact">
                <li
                  v-for="replacement in mod.modelReplacements"
                  :key="`${replacement.modelKind}-${replacement.subKind}-${replacement.modelPart}-${replacement.modelId}`"
                >
                  <strong>{{ modelReplacementTitle(replacement) }}</strong>
                  <span>{{ summarizeModelNames(replacement) }}</span>
                  <small>
                    {{ replacement.modelId }}
                    <template v-if="summarizeGameIds(replacement)">
                      · {{ summarizeGameIds(replacement) }}
                    </template>
                  </small>
                </li>
              </ul>
              <details class="installed-file-details">
                <summary>文件列表 ({{ mod.files.length }})</summary>
                <ul class="file-preview installed-file-preview">
                  <li v-for="file in mod.files" :key="file.libraryRelativePath">
                    <span>{{ file.deployRelativePath }}</span>
                    <strong>{{ file.libraryRelativePath }}</strong>
                  </li>
                </ul>
              </details>
            </div>
            <div class="mod-actions">
              <span>{{ mod.fileCount }} files</span>
              <span>{{ mod.enabled ? "已启用" : "未启用" }}</span>
              <span v-if="conflictingModIds.has(mod.id)" class="conflict-state">存在冲突</span>
              <span>{{ mod.deployRoot }}</span>
              <button
                type="button"
                class="secondary-button"
                :disabled="openingModFolderId === mod.id"
                title="在资源管理器中打开软件保存的 MOD 文件夹"
                @click="showInstalledModFolder(mod)"
              >
                {{ openingModFolderId === mod.id ? "打开中" : "打开文件夹" }}
              </button>
              <button
                v-if="!mod.enabled"
                type="button"
                class="secondary-button"
                :disabled="!!activeModAction"
                @click="enableInstalledMod(mod)"
              >
                {{ activeModAction === mod.id ? "启用中" : "启用" }}
              </button>
              <button
                v-else
                type="button"
                class="secondary-button danger-button"
                :disabled="!!activeModAction"
                @click="disableInstalledMod(mod)"
              >
                {{ activeModAction === mod.id ? "禁用中" : "禁用" }}
              </button>
              <button
                type="button"
                class="secondary-button danger-button"
                :disabled="!!activeModAction"
                @click="uninstallInstalledMod(mod)"
              >
                {{ activeModAction === mod.id ? "处理中" : "卸载" }}
              </button>
            </div>
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

      <div v-if="importPreview?.warnings.length" class="preview-block">
        <h3>警告</h3>
        <ul class="compact-list">
          <li v-for="warning in importPreview.warnings" :key="warning">
            <span>{{ warning }}</span>
          </li>
        </ul>
      </div>
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
  </main>

  <main v-else class="conflict-workspace">
    <header class="topbar">
      <div>
        <p class="eyebrow">Acumod</p>
        <h1>冲突管理</h1>
      </div>
      <button type="button" class="secondary-button" @click="closeConflictManager">返回 MOD 库</button>
    </header>

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
  </main>

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
  width: min(980px, calc(100vw - 40px));
  min-height: 100vh;
  margin: 0 auto;
  padding: 40px 0;
}

.topbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
  margin-bottom: 24px;
}

.eyebrow {
  margin: 0 0 8px;
  color: #24745b;
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: uppercase;
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
  box-shadow: 0 18px 48px rgba(34, 47, 62, 0.08);
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

input {
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

.mod-actions {
  justify-content: flex-end;
}

.mod-actions .conflict-state {
  color: #9a3412;
  font-weight: 700;
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
  width: min(1180px, calc(100vw - 40px));
  min-height: 100vh;
  margin: 0 auto;
  padding: 40px 0;
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

@media (max-width: 720px) {
  .app-shell {
    width: min(100% - 24px, 980px);
    padding: 24px 0;
  }

  .conflict-workspace {
    width: min(100% - 24px, 1180px);
    padding: 24px 0;
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

  .topbar,
  .panel-heading,
  .path-row,
  .notice,
  .file-preview li,
  .compact-list li,
  .mod-list > li {
    grid-template-columns: 1fr;
  }

  .topbar,
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
