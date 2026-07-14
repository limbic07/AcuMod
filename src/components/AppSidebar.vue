<script setup lang="ts">
export type WorkspaceView = "library" | "import" | "conflicts" | "settings";

defineProps<{
  activeView: WorkspaceView;
  gameStatusLabel: string;
  gameStatusClass: string;
  conflictCount: number;
}>();

defineEmits<{
  select: [view: WorkspaceView];
}>();
</script>

<template>
  <aside class="app-sidebar">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true">A</span>
      <div>
        <strong>Acumod</strong>
        <span>Acumen MOD Manager</span>
      </div>
    </div>

    <nav class="workspace-nav" aria-label="主导航">
      <p>工作区</p>
      <button
        type="button"
        :class="{ active: activeView === 'library' }"
        @click="$emit('select', 'library')"
      >
        MOD 库
      </button>
      <button
        type="button"
        :class="{ active: activeView === 'import' }"
        @click="$emit('select', 'import')"
      >
        导入 MOD
      </button>
      <button
        type="button"
        class="conflict-link"
        :class="{ active: activeView === 'conflicts' }"
        @click="$emit('select', 'conflicts')"
      >
        <span>冲突管理</span>
        <strong>{{ conflictCount }}</strong>
      </button>
      <button
        type="button"
        :class="{ active: activeView === 'settings' }"
        @click="$emit('select', 'settings')"
      >
        设置
      </button>
    </nav>

    <div class="game-state">
      <span>游戏目录</span>
      <strong :class="gameStatusClass">{{ gameStatusLabel }}</strong>
    </div>
  </aside>
</template>

<style scoped>
.app-sidebar {
  display: flex;
  min-height: 100vh;
  flex-direction: column;
  padding: 24px 16px;
  border-right: 1px solid #d7e0dc;
  background: #f7faf8;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 8px;
}

.brand-mark {
  display: grid;
  width: 34px;
  height: 34px;
  place-items: center;
  border: 1px solid #1d6f55;
  border-radius: 7px;
  color: #ffffff;
  background: #24745b;
  font-size: 0.9rem;
  font-weight: 800;
}

.brand div {
  display: grid;
  gap: 1px;
}

.brand strong {
  color: #17211f;
  font-size: 1rem;
}

.brand span:last-child {
  color: #61756f;
  font-size: 0.72rem;
}

.workspace-nav {
  display: grid;
  gap: 4px;
  margin-top: 40px;
}

.workspace-nav > p {
  margin: 0 0 6px;
  padding: 0 8px;
  color: #72837e;
  font-size: 0.72rem;
  font-weight: 700;
}

.workspace-nav button {
  display: flex;
  min-height: 38px;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  color: #435650;
  background: transparent;
  font: inherit;
  font-weight: 650;
  text-align: left;
  cursor: pointer;
}

.workspace-nav button:hover {
  color: #17613f;
  background: #edf5f1;
}

.workspace-nav button.active {
  border-color: #b9d8ca;
  color: #17613f;
  background: #e6f3ec;
}

.workspace-nav .conflict-link strong {
  display: grid;
  min-width: 22px;
  height: 22px;
  place-items: center;
  border: 1px solid #d4ded9;
  border-radius: 50%;
  color: #17613f;
  background: #ffffff;
  font-size: 0.76rem;
}

.game-state {
  display: grid;
  gap: 4px;
  margin-top: auto;
  padding: 14px 8px 0;
  border-top: 1px solid #dfe7e3;
}

.game-state span {
  color: #72837e;
  font-size: 0.72rem;
  font-weight: 700;
}

.game-state strong {
  color: #435650;
  font-size: 0.85rem;
}

.game-state strong.success {
  color: #17613f;
}

.game-state strong.warning {
  color: #9a5b00;
}

@media (max-width: 760px) {
  .app-sidebar {
    min-height: 0;
    gap: 16px;
    padding: 16px;
    border-right: 0;
    border-bottom: 1px solid #d7e0dc;
  }

  .workspace-nav {
    display: flex;
    min-width: 0;
    gap: 6px;
    margin-top: 0;
    overflow-x: auto;
  }

  .workspace-nav > p,
  .game-state {
    display: none;
  }

  .workspace-nav button {
    flex: 0 0 auto;
  }
}
</style>
