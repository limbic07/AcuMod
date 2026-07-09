<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getAppInfo, type AppInfo } from "./api/app";
import {
  detectGameDirectory,
  getGameDirectoryStatus,
  saveGameDirectory,
  type GameDirectoryStatus,
} from "./api/game";
import {
  getModLibraryStatus,
  installModFromFolder,
  previewModImport,
  type ModImportPreview,
  type ModInstallResult,
  type ModLibraryStatus,
} from "./api/modLibrary";

const appInfo = ref<AppInfo | null>(null);
const gameStatus = ref<GameDirectoryStatus | null>(null);
const modLibraryStatus = ref<ModLibraryStatus | null>(null);
const importPreview = ref<ModImportPreview | null>(null);
const installResult = ref<ModInstallResult | null>(null);
const manualPath = ref("");
const importPath = ref("");
const appError = ref("");
const gameError = ref("");
const modLibraryError = ref("");
const importError = ref("");
const isLoadingApp = ref(false);
const isLoadingGame = ref(false);
const isLoadingModLibrary = ref(false);
const isPreviewingImport = ref(false);
const isInstallingMod = ref(false);

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
  } catch (error) {
    importError.value = error instanceof Error ? error.message : String(error);
  } finally {
    isInstallingMod.value = false;
  }
}

onMounted(() => {
  void loadAppInfo();
  void loadGameStatus();
  void loadModLibraryStatus();
});
</script>

<template>
  <main class="app-shell">
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

      <div v-if="importPreview?.candidates.length" class="preview-block">
        <h3>候选内容根</h3>
        <ul class="compact-list">
          <li v-for="candidate in importPreview.candidates" :key="candidate.rootPath">
            <span>{{ candidate.rootPath }}</span>
            <strong>{{ candidate.fileCount }} files</strong>
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

.file-preview,
.compact-list {
  display: grid;
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.file-preview li,
.compact-list li {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(160px, 0.55fr);
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid #edf1f0;
  border-radius: 6px;
  background: #fbfdfc;
}

.compact-list li {
  grid-template-columns: minmax(0, 1fr) auto;
}

.file-preview span,
.file-preview strong,
.compact-list span,
.compact-list strong {
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

@media (max-width: 720px) {
  .app-shell {
    width: min(100% - 24px, 980px);
    padding: 24px 0;
  }

  .topbar,
  .panel-heading,
  .path-row,
  .notice,
  .file-preview li,
  .compact-list li {
    grid-template-columns: 1fr;
  }

  .topbar,
  .panel-heading,
  .notice {
    display: grid;
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
