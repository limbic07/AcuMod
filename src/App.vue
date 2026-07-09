<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getAppInfo, type AppInfo } from "./api/app";
import {
  detectGameDirectory,
  getGameDirectoryStatus,
  saveGameDirectory,
  type GameDirectoryStatus,
} from "./api/game";

const appInfo = ref<AppInfo | null>(null);
const gameStatus = ref<GameDirectoryStatus | null>(null);
const manualPath = ref("");
const appError = ref("");
const gameError = ref("");
const isLoadingApp = ref(false);
const isLoadingGame = ref(false);

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

onMounted(() => {
  void loadAppInfo();
  void loadGameStatus();
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

@media (max-width: 720px) {
  .app-shell {
    width: min(100% - 24px, 980px);
    padding: 24px 0;
  }

  .topbar,
  .panel-heading,
  .path-row {
    grid-template-columns: 1fr;
  }

  .topbar,
  .panel-heading {
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
