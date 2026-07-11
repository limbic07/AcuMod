<script setup lang="ts">
import type { ModProfileSummary } from "../api/modLibrary";

const props = defineProps<{
  profiles: ModProfileSummary[];
  activeProfile: ModProfileSummary | null;
  selectedProfileId: string;
  isProfileAction: boolean;
  searchQuery: string;
  categoryFilter: string;
  statusFilter: string;
  conflictFilter: string;
  sort: "installation" | "name" | "category" | "replacement";
  categories: string[];
}>();

const emit = defineEmits<{
  selectProfile: [profileId: string];
  createProfile: [];
  renameProfile: [];
  deleteProfile: [];
  updateSearchQuery: [value: string];
  updateCategoryFilter: [value: string];
  updateStatusFilter: [value: string];
  updateConflictFilter: [value: string];
  updateSort: [value: "installation" | "name" | "category" | "replacement"];
}>();

function selectProfile(event: Event) {
  emit("selectProfile", (event.target as HTMLSelectElement).value);
}

function updateSearchQuery(event: Event) {
  emit("updateSearchQuery", (event.target as HTMLInputElement).value);
}

function updateCategoryFilter(event: Event) {
  emit("updateCategoryFilter", (event.target as HTMLSelectElement).value);
}

function updateStatusFilter(event: Event) {
  emit("updateStatusFilter", (event.target as HTMLSelectElement).value);
}

function updateConflictFilter(event: Event) {
  emit("updateConflictFilter", (event.target as HTMLSelectElement).value);
}

function updateSort(event: Event) {
  emit(
    "updateSort",
    (event.target as HTMLSelectElement).value as "installation" | "name" | "category" | "replacement",
  );
}
</script>

<template>
  <div class="profile-toolbar">
    <label class="profile-selector" for="active-mod-profile">
      <span>当前 Profile</span>
      <select
        id="active-mod-profile"
        :value="props.selectedProfileId"
        :disabled="props.isProfileAction || !props.profiles.length"
        @change="selectProfile"
      >
        <option v-for="profile in props.profiles" :key="profile.id" :value="profile.id">
          {{ profile.name }}{{ profile.isActive ? "（当前）" : "" }}
        </option>
      </select>
    </label>
    <div class="profile-actions">
      <button
        type="button"
        class="icon-button"
        :disabled="props.isProfileAction"
        aria-label="从当前配置创建 Profile"
        data-tooltip="新建 Profile"
        @click="$emit('createProfile')"
      >
        <span aria-hidden="true">+</span>
      </button>
      <button
        type="button"
        class="icon-button"
        :disabled="props.isProfileAction || !props.selectedProfileId"
        aria-label="重命名当前选择的 Profile"
        data-tooltip="重命名 Profile"
        @click="$emit('renameProfile')"
      >
        <span aria-hidden="true">&#9998;</span>
      </button>
      <button
        type="button"
        class="icon-button danger-icon"
        :disabled="
          props.isProfileAction ||
          !props.selectedProfileId ||
          props.activeProfile?.id === props.selectedProfileId
        "
        aria-label="删除当前选择的 Profile"
        data-tooltip="删除 Profile"
        @click="$emit('deleteProfile')"
      >
        <span aria-hidden="true">&#128465;</span>
      </button>
    </div>
    <span v-if="props.activeProfile" class="profile-summary">
      {{ props.activeProfile.enabledModCount }} 个已启用 MOD / {{ props.activeProfile.conflictOrderCount }} 组排序记录
    </span>
  </div>

  <div class="mod-browser-controls">
    <label class="mod-search-control">
      <span>搜索</span>
      <input
        :value="props.searchQuery"
        type="search"
        placeholder="名称、备注、替换目标"
        @input="updateSearchQuery"
      />
    </label>
    <label>
      <span>类别</span>
      <select :value="props.categoryFilter" @change="updateCategoryFilter">
        <option value="all">全部类别</option>
        <option v-for="category in props.categories" :key="category" :value="category">
          {{ category }}
        </option>
      </select>
    </label>
    <label>
      <span>状态</span>
      <select :value="props.statusFilter" @change="updateStatusFilter">
        <option value="all">全部状态</option>
        <option value="enabled">已启用</option>
        <option value="disabled">未启用</option>
      </select>
    </label>
    <label>
      <span>冲突</span>
      <select :value="props.conflictFilter" @change="updateConflictFilter">
        <option value="all">全部</option>
        <option value="conflict">存在冲突</option>
        <option value="normal">无冲突</option>
      </select>
    </label>
    <label>
      <span>排序</span>
      <select :value="props.sort" @change="updateSort">
        <option value="installation">导入顺序</option>
        <option value="name">名称</option>
        <option value="category">类别</option>
        <option value="replacement">替换目标</option>
      </select>
    </label>
  </div>
</template>

<style scoped>
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

.profile-selector,
.mod-browser-controls label {
  display: grid;
  min-width: 0;
  gap: 5px;
}

.profile-selector {
  min-width: min(280px, 100%);
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

.icon-button {
  position: relative;
  display: grid;
  width: 32px;
  min-height: 32px;
  padding: 0;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #24745b;
  background: #ffffff;
  font: inherit;
  font-size: 0.86rem;
  cursor: pointer;
}

.icon-button:disabled {
  color: #72837e;
  background: #f1f5f3;
  cursor: not-allowed;
}

.icon-button.danger-icon {
  color: #b42318;
  font-size: 1rem;
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

@media (max-width: 760px) {
  .profile-toolbar,
  .mod-browser-controls {
    grid-template-columns: 1fr;
  }

  .profile-toolbar {
    display: grid;
    align-items: stretch;
  }
}
</style>
