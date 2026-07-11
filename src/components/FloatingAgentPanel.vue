<script setup lang="ts">
defineProps<{
  open: boolean;
}>();

defineEmits<{
  openPanel: [];
  close: [];
}>();
</script>

<template>
  <button
    v-if="!open"
    type="button"
    class="agent-launcher"
    title="打开 AI 助手"
    @click="$emit('openPanel')"
  >
    AI
  </button>

  <aside v-else class="agent-panel" aria-label="AI 助手">
    <header>
      <div>
        <p>Acumod</p>
        <h2>AI 助手</h2>
      </div>
      <button type="button" title="收起 AI 助手" @click="$emit('close')">收起</button>
    </header>
    <div class="agent-state">
      <strong>暂未接入</strong>
      <span>受控操作入口将随 AI 功能一起开放。</span>
    </div>
  </aside>
</template>

<style scoped>
.agent-launcher,
.agent-panel {
  position: fixed;
  z-index: 15;
  right: 24px;
  bottom: 24px;
}

.agent-launcher {
  display: grid;
  width: 48px;
  height: 48px;
  place-items: center;
  border: 1px solid #1d6f55;
  border-radius: 50%;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-size: 0.82rem;
  font-weight: 800;
  cursor: pointer;
  box-shadow: 0 12px 26px rgba(23, 97, 63, 0.2);
}

.agent-launcher:hover {
  background: #17613f;
}

.agent-panel {
  width: min(340px, calc(100vw - 32px));
  border: 1px solid #c9d9d2;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 18px 44px rgba(23, 33, 31, 0.18);
}

.agent-panel header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px;
  border-bottom: 1px solid #e3ebe7;
}

.agent-panel p,
.agent-panel h2,
.agent-state {
  margin: 0;
}

.agent-panel p {
  color: #24745b;
  font-size: 0.7rem;
  font-weight: 750;
}

.agent-panel h2 {
  margin-top: 2px;
  color: #17211f;
  font-size: 1rem;
}

.agent-panel button {
  min-height: 30px;
  padding: 0 8px;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #24745b;
  background: #ffffff;
  font: inherit;
  font-size: 0.78rem;
  font-weight: 700;
  cursor: pointer;
}

.agent-state {
  display: grid;
  gap: 5px;
  padding: 20px 16px;
}

.agent-state strong {
  color: #17211f;
}

.agent-state span {
  color: #61756f;
  font-size: 0.84rem;
}

@media (max-width: 520px) {
  .agent-launcher,
  .agent-panel {
    right: 16px;
    bottom: 16px;
  }
}
</style>
