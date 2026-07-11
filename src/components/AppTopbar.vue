<script setup lang="ts">
import type { WorkspaceView } from "./AppSidebar.vue";

defineProps<{
  activeView: WorkspaceView;
  modCount: number;
  enabledModCount: number;
  isRefreshing: boolean;
  agentOpen: boolean;
}>();

defineEmits<{
  refresh: [];
  toggleAgent: [];
}>();

const pageCopy: Record<WorkspaceView, { title: string; description: string }> = {
  library: {
    title: "MOD 库",
    description: "管理本地 MOD、部署状态和识别结果。",
  },
  import: {
    title: "导入 MOD",
    description: "识别文件夹或压缩包后再写入本地 MOD 库。",
  },
  conflicts: {
    title: "冲突管理",
    description: "调整已启用 MOD 的覆盖顺序。",
  },
  settings: {
    title: "设置",
    description: "确认游戏目录和桌面应用状态。",
  },
};
</script>

<template>
  <header class="workspace-topbar">
    <div>
      <p>Acumod / {{ pageCopy[activeView].title }}</p>
      <h1>{{ pageCopy[activeView].title }}</h1>
      <span>{{ pageCopy[activeView].description }}</span>
    </div>

    <div class="topbar-actions">
      <dl class="mod-summary">
        <div>
          <dt>已安装</dt>
          <dd>{{ modCount }}</dd>
        </div>
        <div>
          <dt>已启用</dt>
          <dd>{{ enabledModCount }}</dd>
        </div>
      </dl>
      <button type="button" class="secondary-action" :disabled="isRefreshing" @click="$emit('refresh')">
        {{ isRefreshing ? "刷新中" : "刷新" }}
      </button>
      <button type="button" class="agent-action" @click="$emit('toggleAgent')">
        {{ agentOpen ? "收起 AI" : "AI 助手" }}
      </button>
    </div>
  </header>
</template>

<style scoped>
.workspace-topbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
  padding: 24px 28px;
  border-bottom: 1px solid #d7e0dc;
  background: #ffffff;
}

.workspace-topbar p,
.workspace-topbar h1,
.workspace-topbar span,
.mod-summary {
  margin: 0;
}

.workspace-topbar p {
  color: #24745b;
  font-size: 0.76rem;
  font-weight: 750;
}

.workspace-topbar h1 {
  margin-top: 5px;
  color: #17211f;
  font-size: 1.55rem;
  line-height: 1.2;
}

.workspace-topbar > div:first-child > span {
  display: block;
  margin-top: 6px;
  color: #61756f;
  font-size: 0.88rem;
}

.topbar-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.mod-summary {
  display: flex;
  overflow: hidden;
  border: 1px solid #d7e0dc;
  border-radius: 6px;
}

.mod-summary div {
  display: grid;
  gap: 1px;
  min-width: 60px;
  padding: 6px 10px;
}

.mod-summary div + div {
  border-left: 1px solid #d7e0dc;
}

.mod-summary dt {
  color: #72837e;
  font-size: 0.68rem;
  font-weight: 650;
}

.mod-summary dd {
  color: #17211f;
  font-size: 0.95rem;
  font-weight: 750;
}

button {
  min-height: 36px;
  padding: 0 12px;
  border: 1px solid #cbd8d4;
  border-radius: 6px;
  background: #ffffff;
  color: #24745b;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

button:hover {
  border-color: #8cbca8;
  background: #edf5f1;
}

button:disabled {
  color: #72837e;
  background: #f1f5f3;
  cursor: not-allowed;
}

.agent-action {
  border-color: #1d6f55;
  color: #ffffff;
  background: #24745b;
}

.agent-action:hover {
  border-color: #17613f;
  color: #ffffff;
  background: #17613f;
}

@media (max-width: 900px) {
  .workspace-topbar {
    align-items: stretch;
    flex-direction: column;
  }

  .topbar-actions {
    flex-wrap: wrap;
  }
}

@media (max-width: 520px) {
  .workspace-topbar {
    padding: 20px 16px;
  }

  .mod-summary {
    order: 3;
    width: 100%;
  }

  .mod-summary div {
    flex: 1;
  }
}
</style>
