<script setup lang="ts">
interface CategoryFilterOption {
  id: string;
  label: string;
}

const props = defineProps<{
  isCategoryAction: boolean;
  isOrderAction: boolean;
  searchQuery: string;
  categoryFilter: string;
  statusFilter: string;
  conflictFilter: string;
  sort: "manual" | "installation" | "name" | "category" | "replacement";
  sortDirection: "asc" | "desc";
  categories: CategoryFilterOption[];
}>();

const emit = defineEmits<{
  manageCategories: [];
  updateSearchQuery: [value: string];
  updateCategoryFilter: [value: string];
  updateStatusFilter: [value: string];
  updateConflictFilter: [value: string];
  updateSort: [value: "manual" | "installation" | "name" | "category" | "replacement"];
  updateSortDirection: [value: "asc" | "desc"];
  applySort: [];
  restoreImportOrder: [];
}>();

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
    (event.target as HTMLSelectElement).value as "manual" | "installation" | "name" | "category" | "replacement",
  );
}

function toggleSortDirection() {
  emit("updateSortDirection", props.sortDirection === "asc" ? "desc" : "asc");
}
</script>

<template>
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
    <label class="category-filter-control">
      <span>分类</span>
      <div class="category-filter-row">
        <select :value="props.categoryFilter" @change="updateCategoryFilter">
          <option value="all">全部分类</option>
          <option v-for="category in props.categories" :key="category.id" :value="category.id">
            {{ category.label }}
          </option>
        </select>
        <button
          type="button"
          class="icon-button"
          :disabled="props.isCategoryAction"
          aria-label="管理分类"
          data-tooltip="分类管理"
          @click="$emit('manageCategories')"
        >
          <span aria-hidden="true">&#9881;</span>
        </button>
      </div>
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
      <div class="sort-control-row">
        <select :value="props.sort" @change="updateSort">
          <option value="manual">手动排序</option>
          <option value="installation">导入顺序</option>
          <option value="name">名称</option>
          <option value="category">分类</option>
          <option value="replacement">替换目标</option>
        </select>
        <button
          type="button"
          class="icon-button sort-direction-button"
          :disabled="props.sort === 'manual'"
          :aria-label="props.sortDirection === 'asc' ? '切换为倒序' : '切换为升序'"
          :data-tooltip="props.sortDirection === 'asc' ? '当前升序' : '当前倒序'"
          @click="toggleSortDirection"
        >
          <span aria-hidden="true">{{ props.sortDirection === "asc" ? "↑" : "↓" }}</span>
        </button>
      </div>
      <div class="sort-action-row">
        <button
          type="button"
          class="sort-action-button"
          :disabled="props.sort === 'manual' || props.isOrderAction"
          @click="$emit('applySort')"
        >
          应用排序
        </button>
        <button
          type="button"
          class="sort-action-button"
          :disabled="props.isOrderAction"
          @click="$emit('restoreImportOrder')"
        >
          恢复导入顺序
        </button>
      </div>
    </label>
  </div>
</template>

<style scoped>
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

.mod-browser-controls label > span {
  color: #61756f;
  font-size: 0.74rem;
  font-weight: 700;
}

.category-filter-row {
  display: flex;
  gap: 6px;
}

.category-filter-row select {
  min-width: 0;
  flex: 1;
}

.sort-control-row {
  display: flex;
  gap: 6px;
}

.sort-control-row select {
  min-width: 0;
  flex: 1;
}

.sort-direction-button {
  min-height: 42px;
  flex: 0 0 32px;
  font-size: 1rem;
}

.sort-action-row {
  display: flex;
  gap: 6px;
}

.sort-action-button {
  min-height: 30px;
  padding: 0 8px;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #315e52;
  background: #ffffff;
  font: inherit;
  font-size: 0.72rem;
  font-weight: 700;
  cursor: pointer;
}

.sort-action-button:hover:not(:disabled),
.sort-action-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.sort-action-button:disabled {
  color: #8a9894;
  background: #f3f6f5;
  cursor: not-allowed;
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
  .mod-browser-controls {
    grid-template-columns: 1fr;
  }
}
</style>
