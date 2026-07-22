<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  deleteKnowledgePack,
  getKnowledgeStatus,
  installKnowledgePack,
  type KnowledgePackKind,
  type KnowledgeStatus,
} from "../api/knowledge";

const status = ref<KnowledgeStatus | null>(null);
const packagePath = ref("");
const error = ref("");
const message = ref("");
const isLoading = ref(false);
const isInstalling = ref(false);
const deletingPackId = ref("");
const confirmingDeletePackId = ref("");

const kindLabels: Record<KnowledgePackKind, string> = {
  "mhw-modding": "MOD 技术",
  "mhw-game-facts": "游戏事实",
  "mhw-game-guides": "攻略资料",
};

function visibleError(value: unknown) {
  return value instanceof Error ? value.message : String(value);
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

async function loadStatus() {
  isLoading.value = true;
  error.value = "";
  try {
    status.value = await getKnowledgeStatus();
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isLoading.value = false;
  }
}

async function installPackage() {
  const sourcePath = packagePath.value.trim();
  if (!sourcePath) return;
  isInstalling.value = true;
  error.value = "";
  message.value = "";
  try {
    const result = await installKnowledgePack(sourcePath);
    status.value = result.status;
    packagePath.value = "";
    message.value = result.message;
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isInstalling.value = false;
  }
}

async function removePackage(packId: string) {
  if (confirmingDeletePackId.value !== packId) {
    confirmingDeletePackId.value = packId;
    message.value = "再次点击“确认删除”将删除该知识包的全部本地版本。";
    return;
  }
  deletingPackId.value = packId;
  error.value = "";
  message.value = "";
  try {
    status.value = await deleteKnowledgePack(packId);
    confirmingDeletePackId.value = "";
    message.value = "知识包已删除。";
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    deletingPackId.value = "";
  }
}

onMounted(() => {
  void loadStatus();
});
</script>

<template>
  <section class="panel secondary knowledge-settings-panel">
    <div class="knowledge-heading">
      <div>
        <h2>知识库</h2>
        <p>为 AcuAI 提供带版本和来源的 MHW 游戏知识与 MOD 技术资料。</p>
      </div>
      <button type="button" :disabled="isLoading || isInstalling" @click="loadStatus">
        {{ isLoading ? "读取中" : "刷新" }}
      </button>
    </div>

    <form class="knowledge-import" @submit.prevent="installPackage">
      <label for="knowledge-package-path">本地知识包</label>
      <div>
        <input
          id="knowledge-package-path"
          v-model="packagePath"
          type="text"
          autocomplete="off"
          placeholder="输入或粘贴 .acukb 文件路径"
        />
        <button type="submit" :disabled="isInstalling || !packagePath.trim()">
          {{ isInstalling ? "安装中" : "安装" }}
        </button>
      </div>
      <small>知识包独立存放在软件目录旁，不增加主程序安装包体积。</small>
    </form>

    <div v-if="status?.packs.length" class="pack-list">
      <article v-for="pack in status.packs" :key="`${pack.packId}:${pack.sha256}`" class="pack-row">
        <div class="pack-state" :class="{ unhealthy: !pack.healthy }" aria-hidden="true"></div>
        <div class="pack-summary">
          <div class="pack-title">
            <strong>{{ pack.displayName }}</strong>
            <span>{{ kindLabels[pack.kind] }}</span>
            <span v-if="pack.active">当前</span>
          </div>
          <p>{{ pack.description || "暂无说明" }}</p>
          <div class="pack-facts">
            <span>包 {{ pack.version }}</span>
            <span>游戏 {{ pack.gameVersion }}</span>
            <span>{{ formatSize(pack.sizeBytes) }}</span>
            <span>{{ pack.entityCount }} 实体</span>
            <span>{{ pack.documentCount }} 文档</span>
            <span>{{ pack.sourceCount }} 来源</span>
          </div>
          <p v-if="pack.error" class="pack-error">{{ pack.error }}</p>
        </div>
        <button
          type="button"
          class="delete-button"
          :class="{ confirming: confirmingDeletePackId === pack.packId }"
          :disabled="deletingPackId === pack.packId"
          @click="removePackage(pack.packId)"
        >
          {{
            deletingPackId === pack.packId
              ? "删除中"
              : confirmingDeletePackId === pack.packId
                ? "确认删除"
                : "删除"
          }}
        </button>
      </article>
    </div>
    <p v-else-if="status && !isLoading" class="knowledge-empty">尚未安装知识包。</p>

    <p v-if="error" class="knowledge-message error">{{ error }}</p>
    <p v-else-if="message" class="knowledge-message success">{{ message }}</p>
    <p v-else-if="status" class="knowledge-message">{{ status.message }}</p>
  </section>
</template>

<style scoped>
.knowledge-settings-panel,
.knowledge-import,
.pack-list {
  display: grid;
  gap: 16px;
}

.knowledge-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 18px;
}

.knowledge-heading h2,
.knowledge-heading p,
.pack-summary p,
.knowledge-message,
.knowledge-empty {
  margin: 0;
}

.knowledge-heading p,
.pack-summary p,
.knowledge-import small,
.knowledge-message,
.knowledge-empty {
  color: #60736d;
}

.knowledge-heading p {
  margin-top: 8px;
}

.knowledge-settings-panel button {
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

.knowledge-settings-panel button:disabled {
  cursor: default;
  opacity: 0.55;
}

.knowledge-import label {
  color: #60736d;
  font-size: 0.86rem;
  font-weight: 600;
}

.knowledge-import > div {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
}

.knowledge-import input {
  min-height: 40px;
  padding: 0 10px;
  border: 1px solid #bdccc7;
  border-radius: 4px;
  color: #203b34;
  background: #ffffff;
  font: inherit;
}

.pack-list {
  border-top: 1px solid #dbe5e1;
  padding-top: 16px;
}

.pack-row {
  display: grid;
  grid-template-columns: 8px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  padding: 12px;
  border: 1px solid #d5e1dd;
  border-radius: 5px;
  background: #fbfdfc;
}

.pack-state {
  width: 8px;
  min-height: 54px;
  border-radius: 3px;
  background: #398064;
}

.pack-state.unhealthy {
  background: #b95845;
}

.pack-summary {
  min-width: 0;
}

.pack-title,
.pack-facts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px 12px;
}

.pack-title span {
  padding: 2px 6px;
  border: 1px solid #c5d9d1;
  border-radius: 4px;
  color: #286b55;
  background: #f2f8f5;
  font-size: 0.78rem;
}

.pack-summary > p {
  margin-top: 5px;
  font-size: 0.86rem;
}

.pack-facts {
  margin-top: 8px;
  color: #52645f;
  font-size: 0.8rem;
}

.knowledge-settings-panel .delete-button {
  color: #8a3d32;
}

.knowledge-settings-panel .delete-button.confirming {
  border-color: #d39a8e;
  color: #9d3424;
  background: #fff8f6;
}

.pack-summary .pack-error,
.knowledge-message.error {
  color: #a34133;
}

.knowledge-message.success {
  color: #17613f;
}

@media (max-width: 760px) {
  .knowledge-import > div,
  .pack-row {
    grid-template-columns: 1fr;
  }

  .pack-state {
    width: 100%;
    min-height: 4px;
  }
}
</style>
