<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { ModCategory } from "../api/modLibrary";

const props = defineProps<{
  isOpen: boolean;
  categories: ModCategory[];
  isBusy: boolean;
  error: string;
}>();

const emit = defineEmits<{
  close: [];
  create: [name: string, parentId: string | null];
  rename: [categoryId: string, name: string];
  delete: [category: ModCategory];
}>();

const newCategoryName = ref("");
const newCategoryParentId = ref("");
const newCategoryInput = ref<HTMLInputElement | null>(null);
const editingCategoryId = ref("");
const editingCategoryName = ref("");

const rootCategories = computed(() =>
  props.categories.filter((category) => !category.parentId),
);

const visibleCategories = computed(() => {
  const childrenByParentId = new Map<string, ModCategory[]>();
  for (const category of props.categories) {
    if (!category.parentId) {
      continue;
    }
    const children = childrenByParentId.get(category.parentId) ?? [];
    children.push(category);
    childrenByParentId.set(category.parentId, children);
  }

  return rootCategories.value.flatMap((category) => [
    category,
    ...(childrenByParentId.get(category.id) ?? []),
  ]);
});

function parentCategory(category: ModCategory) {
  return category.parentId
    ? props.categories.find((candidate) => candidate.id === category.parentId) ?? null
    : null;
}

function categoryLabel(category: ModCategory) {
  const parent = parentCategory(category);
  return parent ? `${parent.name}·${category.name}` : category.name;
}

function createCategory() {
  emit("create", newCategoryName.value, newCategoryParentId.value || null);
}

async function startCreatingChild(category: ModCategory) {
  newCategoryParentId.value = category.id;
  await nextTick();
  newCategoryInput.value?.focus();
}

function startRenaming(category: ModCategory) {
  editingCategoryId.value = category.id;
  editingCategoryName.value = category.name;
}

function cancelRenaming() {
  editingCategoryId.value = "";
  editingCategoryName.value = "";
}

function renameCategory() {
  if (!editingCategoryId.value) {
    return;
  }

  emit("rename", editingCategoryId.value, editingCategoryName.value);
}

watch(
  () => props.isOpen,
  (isOpen) => {
    if (!isOpen) {
      newCategoryName.value = "";
      newCategoryParentId.value = "";
      cancelRenaming();
    }
  },
);

watch(
  () => props.categories,
  () => {
    if (!props.error) {
      newCategoryName.value = "";
      newCategoryParentId.value = "";
      cancelRenaming();
    }
  },
);
</script>

<template>
  <div v-if="props.isOpen" class="category-dialog-backdrop" role="presentation">
    <section
      class="category-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="category-manager-title"
      @keydown.esc="$emit('close')"
    >
      <header class="category-dialog-header">
        <h2 id="category-manager-title">分类管理</h2>
        <button
          type="button"
          class="icon-button"
          :disabled="props.isBusy"
          aria-label="关闭分类管理"
          data-tooltip="关闭"
          @click="$emit('close')"
        >
          <span aria-hidden="true">&#215;</span>
        </button>
      </header>

      <form class="create-category-form" @submit.prevent="createCategory">
        <label for="new-mod-category">新建分类</label>
        <div>
          <input
            id="new-mod-category"
            ref="newCategoryInput"
            v-model="newCategoryName"
            :disabled="props.isBusy"
            maxlength="40"
            placeholder="分类名称"
          />
          <select v-model="newCategoryParentId" :disabled="props.isBusy">
            <option value="">顶级分类</option>
            <option v-for="category in rootCategories" :key="category.id" :value="category.id">
              {{ category.name }} 的子分类
            </option>
          </select>
          <button type="submit" :disabled="props.isBusy || !newCategoryName.trim()">新建</button>
        </div>
      </form>

      <p v-if="props.error" class="category-error">{{ props.error }}</p>

      <ul v-if="props.categories.length" class="category-list">
        <li
          v-for="category in visibleCategories"
          :key="category.id"
          :class="{ 'child-category-row': !!category.parentId }"
        >
          <template v-if="editingCategoryId === category.id">
            <input
              v-model="editingCategoryName"
              :disabled="props.isBusy"
              maxlength="40"
              :aria-label="`重命名分类 ${category.name}`"
              @keydown.enter.prevent="renameCategory"
              @keydown.esc.prevent="cancelRenaming"
            />
            <div class="category-row-actions">
              <button type="button" :disabled="props.isBusy" @click="renameCategory">保存</button>
              <button type="button" class="secondary-button" :disabled="props.isBusy" @click="cancelRenaming">
                取消
              </button>
            </div>
          </template>
          <template v-else>
            <span>{{ categoryLabel(category) }}</span>
            <div class="category-row-actions">
              <button
                v-if="!category.parentId"
                type="button"
                class="icon-button"
                :disabled="props.isBusy"
                :aria-label="`为分类 ${category.name} 新建子分类`"
                data-tooltip="新建子分类"
                @click="startCreatingChild(category)"
              >
                <span aria-hidden="true">+</span>
              </button>
              <button
                type="button"
                class="icon-button"
                :disabled="props.isBusy"
                :aria-label="`重命名分类 ${category.name}`"
                data-tooltip="重命名"
                @click="startRenaming(category)"
              >
                <span aria-hidden="true">&#9998;</span>
              </button>
              <button
                type="button"
                class="icon-button danger-icon"
                :disabled="props.isBusy"
                :aria-label="`删除分类 ${category.name}`"
                data-tooltip="删除"
                @click="$emit('delete', category)"
              >
                <span aria-hidden="true">&#128465;</span>
              </button>
            </div>
          </template>
        </li>
      </ul>
      <p v-else class="empty-category-state">还没有分类。</p>
    </section>
  </div>
</template>

<style scoped>
.category-dialog-backdrop {
  position: fixed;
  z-index: 24;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 20px;
  background: rgba(23, 33, 31, 0.38);
}

.category-dialog {
  display: grid;
  width: min(560px, 100%);
  max-height: min(680px, calc(100vh - 40px));
  gap: 18px;
  overflow: auto;
  padding: 20px;
  border: 1px solid #dfe7e3;
  border-radius: 6px;
  background: #ffffff;
  box-shadow: 0 20px 60px rgba(23, 33, 31, 0.18);
}

.category-dialog-header,
.category-list li,
.category-row-actions,
.create-category-form > div {
  display: flex;
  align-items: center;
  gap: 8px;
}

.category-dialog-header {
  justify-content: space-between;
}

.category-dialog-header h2 {
  margin: 0;
  color: #17211f;
  font-size: 1rem;
}

.create-category-form {
  display: grid;
  gap: 6px;
}

.create-category-form label {
  color: #61756f;
  font-size: 0.76rem;
  font-weight: 700;
}

.create-category-form input,
.create-category-form select,
.category-list input {
  min-width: 0;
  min-height: 36px;
  flex: 1;
  padding: 0 10px;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #17211f;
  background: #fbfdfc;
  font: inherit;
}

.create-category-form select {
  flex: 0 1 170px;
}

button {
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid #24745b;
  border-radius: 5px;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-size: 0.82rem;
  font-weight: 700;
  cursor: pointer;
}

button:hover:not(:disabled),
button:focus-visible {
  border-color: #17613f;
  background: #17613f;
}

button:disabled {
  border-color: #d7e1dd;
  color: #72837e;
  background: #f1f5f3;
  cursor: not-allowed;
}

.secondary-button {
  border-color: #cbd8d4;
  color: #435650;
  background: #ffffff;
}

.secondary-button:hover:not(:disabled),
.secondary-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.icon-button {
  position: relative;
  display: grid;
  width: 32px;
  min-height: 32px;
  padding: 0;
  place-items: center;
  border: 1px solid #cbd8d4;
  color: #24745b;
  background: #ffffff;
  font-size: 0.9rem;
}

.icon-button:hover:not(:disabled),
.icon-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.icon-button.danger-icon {
  color: #b42318;
  font-size: 1rem;
}

.icon-button[data-tooltip]::after {
  position: absolute;
  z-index: 2;
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

.category-list {
  display: grid;
  margin: 0;
  padding: 0;
  border-top: 1px solid #e7eeeb;
  list-style: none;
}

.category-list li {
  min-height: 48px;
  justify-content: space-between;
  padding: 8px 0;
  border-bottom: 1px solid #e7eeeb;
  color: #24332f;
}

.category-list li.child-category-row {
  margin-left: 18px;
  padding-left: 12px;
  border-left: 2px solid #c9dbd4;
}

.category-list li > span {
  min-width: 0;
  overflow-wrap: anywhere;
}

.category-row-actions {
  flex: none;
}

.category-error {
  margin: 0;
  color: #b42318;
  font-size: 0.82rem;
}

.empty-category-state {
  margin: 0;
  padding: 12px 0;
  border-top: 1px solid #e7eeeb;
  color: #61756f;
  font-size: 0.86rem;
}

@media (max-width: 540px) {
  .create-category-form > div {
    align-items: stretch;
    flex-direction: column;
  }

  .create-category-form button {
    width: 100%;
  }
}
</style>
