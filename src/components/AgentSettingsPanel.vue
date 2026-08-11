<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  deleteDeepSeekApiKey,
  getAgentSettings,
  saveAgentModel,
  setDeepSeekApiKey,
  testAgentConnection,
  testAgentWebSearch,
  type AgentSettings,
  type DeepSeekModel,
} from "../api/agent";

const settings = ref<AgentSettings | null>(null);
const apiKey = ref("");
const error = ref("");
const resultMessage = ref("");
const isLoading = ref(false);
const isSavingKey = ref(false);
const isTestingConnection = ref(false);
const isTestingWebSearch = ref(false);

function visibleError(value: unknown) {
  return value instanceof Error ? value.message : String(value);
}

async function loadSettings() {
  isLoading.value = true;
  error.value = "";
  try {
    settings.value = await getAgentSettings();
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isLoading.value = false;
  }
}

async function updateModel(event: Event) {
  const model = (event.target as HTMLSelectElement).value as DeepSeekModel;
  error.value = "";
  resultMessage.value = "";
  try {
    settings.value = await saveAgentModel(model);
  } catch (value) {
    error.value = visibleError(value);
    await loadSettings();
  }
}

async function saveApiKey() {
  isSavingKey.value = true;
  error.value = "";
  resultMessage.value = "";
  try {
    settings.value = await setDeepSeekApiKey(apiKey.value);
    apiKey.value = "";
    resultMessage.value = "访问密钥已保存到 Windows 凭据管理器。";
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isSavingKey.value = false;
  }
}

async function removeApiKey() {
  isSavingKey.value = true;
  error.value = "";
  resultMessage.value = "";
  try {
    settings.value = await deleteDeepSeekApiKey();
    resultMessage.value = settings.value.apiKeyConfigured
      ? "已删除凭据管理器中的 Key，环境变量仍在生效。"
      : "DeepSeek 访问密钥已删除。";
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isSavingKey.value = false;
  }
}

async function testConnection() {
  isTestingConnection.value = true;
  error.value = "";
  resultMessage.value = "";
  try {
    const result = await testAgentConnection();
    resultMessage.value = `${result.message} ${result.elapsedMillis} 毫秒`;
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isTestingConnection.value = false;
  }
}

async function testWebSearch() {
  isTestingWebSearch.value = true;
  error.value = "";
  resultMessage.value = "";
  try {
    const result = await testAgentWebSearch();
    resultMessage.value = `${result.message} ${result.elapsedMillis} 毫秒`;
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isTestingWebSearch.value = false;
  }
}

onMounted(() => {
  void loadSettings();
});
</script>

<template>
  <section class="panel secondary agent-settings-panel">
    <div class="agent-settings-heading">
      <div>
        <h2>AcuAI</h2>
        <p>使用 DeepSeek V4 查询本地 MOD、冲突和游戏术语。</p>
      </div>
      <div class="agent-settings-actions">
        <button
          type="button"
          :disabled="isLoading || isTestingConnection || isTestingWebSearch || !settings?.apiKeyConfigured"
          @click="testConnection"
        >
          {{ isTestingConnection ? "测试中" : "测试连接" }}
        </button>
        <button
          type="button"
          :disabled="isLoading || isTestingConnection || isTestingWebSearch || !settings?.apiKeyConfigured"
          @click="testWebSearch"
        >
          {{ isTestingWebSearch ? "联网搜索测试中" : "测试联网搜索" }}
        </button>
      </div>
    </div>

    <div v-if="settings" class="agent-settings-grid">
      <label>
        <span>模型</span>
        <select :value="settings.model" :disabled="isSavingKey" @change="updateModel">
          <option value="v4Flash">DeepSeek V4 Flash</option>
          <option value="v4Pro">DeepSeek V4 Pro</option>
        </select>
      </label>

      <div class="credential-status">
        <span>访问密钥</span>
        <strong v-if="settings.apiKeyConfigured">
          {{ settings.apiKeyHint }} ·
          {{ settings.apiKeySource === "environment" ? "环境变量" : "Windows 凭据" }}
        </strong>
        <strong v-else>未配置</strong>
      </div>
    </div>

    <form class="api-key-form" @submit.prevent="saveApiKey">
      <label for="deepseek-api-key">DeepSeek 访问密钥</label>
      <div>
        <input
          id="deepseek-api-key"
          v-model="apiKey"
          type="password"
          autocomplete="off"
          placeholder="sk-..."
        />
        <button type="submit" :disabled="isSavingKey || !apiKey.trim()">
          {{ isSavingKey ? "保存中" : "保存" }}
        </button>
        <button
          v-if="settings?.apiKeySource === 'credentialManager'"
          type="button"
          class="secondary-command"
          :disabled="isSavingKey"
          @click="removeApiKey"
        >
          删除
        </button>
      </div>
    </form>

    <p v-if="isLoading" class="settings-message">正在读取 AcuAI 设置...</p>
    <p v-else-if="error" class="settings-message error">{{ error }}</p>
    <p v-else-if="resultMessage" class="settings-message success">{{ resultMessage }}</p>
  </section>
</template>

<style scoped>
.agent-settings-panel {
  display: grid;
  gap: 18px;
}

.agent-settings-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 18px;
}

.agent-settings-heading h2,
.agent-settings-heading p,
.settings-message {
  margin: 0;
}

.agent-settings-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.agent-settings-heading p {
  margin-top: 8px;
  color: #52645f;
}

.agent-settings-panel button {
  min-height: 40px;
  padding: 0 14px;
  border: 1px solid #b9cbc4;
  border-radius: 5px;
  color: #24745b;
  background: #ffffff;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.agent-settings-panel button:disabled {
  cursor: default;
  opacity: 0.55;
}

.agent-settings-grid {
  display: grid;
  grid-template-columns: minmax(220px, 0.45fr) minmax(220px, 1fr);
  gap: 16px;
}

.agent-settings-grid label,
.credential-status {
  display: grid;
  gap: 7px;
}

.agent-settings-grid select,
.api-key-form input {
  min-height: 40px;
  padding: 0 10px;
  border: 1px solid #bdccc7;
  border-radius: 4px;
  color: #203b34;
  background: #ffffff;
  font: inherit;
}

.credential-status strong {
  display: flex;
  min-height: 40px;
  align-items: center;
  color: #284c41;
  font-size: 0.9rem;
}

.api-key-form {
  display: grid;
  gap: 8px;
}

.api-key-form label,
.agent-settings-grid span,
.credential-status span {
  color: #61756f;
  font-size: 0.86rem;
  font-weight: 600;
}

.api-key-form > div {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  gap: 8px;
}

.secondary-command {
  color: #8a3d32;
  background: #fffafa;
}

.settings-message {
  color: #61756f;
  font-size: 0.86rem;
}

.settings-message.error {
  color: #a34133;
}

.settings-message.success {
  color: #17613f;
}

@media (max-width: 760px) {
  .agent-settings-heading {
    display: grid;
  }

  .agent-settings-actions {
    justify-content: flex-start;
  }

  .agent-settings-grid,
  .api-key-form > div {
    grid-template-columns: 1fr;
  }
}
</style>
