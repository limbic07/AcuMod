<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import type {
  BranchGroupSuggestion,
  BranchGroupSuggestionSelection,
} from "../domain/branchGroupSuggestions";

const props = defineProps<{
  isOpen: boolean;
  suggestions: BranchGroupSuggestion[];
  isBusy: boolean;
  error: string;
}>();

const emit = defineEmits<{
  close: [];
  confirm: [selections: BranchGroupSuggestionSelection[]];
}>();

const selectedSuggestionIds = ref(new Set<string>());
const groupNames = reactive<Record<string, string>>({});

const allSelected = computed(
  () =>
    props.suggestions.length > 0 &&
    selectedSuggestionIds.value.size === props.suggestions.length,
);
const someSelected = computed(
  () => selectedSuggestionIds.value.size > 0 && !allSelected.value,
);
const selectedCount = computed(() => selectedSuggestionIds.value.size);

watch(
  () => [props.isOpen, props.suggestions] as const,
  ([isOpen, suggestions]) => {
    if (!isOpen) {
      return;
    }
    selectedSuggestionIds.value = new Set();
    for (const key of Object.keys(groupNames)) {
      delete groupNames[key];
    }
    for (const suggestion of suggestions) {
      groupNames[suggestion.id] = suggestion.suggestedName;
    }
  },
  { immediate: true },
);

function toggleAll(event: Event) {
  selectedSuggestionIds.value = (event.target as HTMLInputElement).checked
    ? new Set(props.suggestions.map((suggestion) => suggestion.id))
    : new Set();
}

function toggleSuggestion(suggestionId: string, event: Event) {
  const next = new Set(selectedSuggestionIds.value);
  if ((event.target as HTMLInputElement).checked) {
    next.add(suggestionId);
  } else {
    next.delete(suggestionId);
  }
  selectedSuggestionIds.value = next;
}

function confirmSelections() {
  const selections = props.suggestions
    .filter((suggestion) => selectedSuggestionIds.value.has(suggestion.id))
    .map((suggestion) => ({
      suggestionId: suggestion.id,
      name: groupNames[suggestion.id]?.trim() || suggestion.suggestedName,
      modIds: suggestion.members.map((member) => member.modId),
    }));
  emit("confirm", selections);
}
</script>

<template>
  <div
    v-if="props.isOpen"
    class="suggestion-backdrop"
    role="presentation"
    @mousedown.self="!props.isBusy && $emit('close')"
  >
    <section
      class="suggestion-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="branch-suggestion-title"
      @keydown.esc.prevent="!props.isBusy && $emit('close')"
    >
      <header class="suggestion-heading">
        <div>
          <h2 id="branch-suggestion-title">自动创建分支组</h2>
          <p>{{ props.suggestions.length }} 组建议</p>
        </div>
        <button
          type="button"
          class="close-button"
          :disabled="props.isBusy"
          aria-label="关闭"
          title="关闭"
          @click="$emit('close')"
        >
          &times;
        </button>
      </header>

      <label v-if="props.suggestions.length" class="select-all-row">
        <input
          type="checkbox"
          :checked="allSelected"
          :indeterminate="someSelected"
          :disabled="props.isBusy"
          @change="toggleAll"
        />
        <span>全选建议</span>
      </label>

      <div v-if="props.suggestions.length" class="suggestion-table-wrap">
        <table class="suggestion-table">
          <colgroup>
            <col class="selection-column" />
            <col class="name-column" />
            <col class="member-column" />
            <col class="evidence-column" />
          </colgroup>
          <thead>
            <tr>
              <th scope="col">选择</th>
              <th scope="col">分支组名称</th>
              <th scope="col">包含 MOD</th>
              <th scope="col">识别依据</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="suggestion in props.suggestions" :key="suggestion.id">
              <td class="suggestion-selection">
                <input
                  type="checkbox"
                  :checked="selectedSuggestionIds.has(suggestion.id)"
                  :disabled="props.isBusy"
                  :aria-label="`选择 ${suggestion.suggestedName}`"
                  @change="toggleSuggestion(suggestion.id, $event)"
                />
              </td>
              <td>
                <input
                  v-model="groupNames[suggestion.id]"
                  class="group-name-input"
                  maxlength="120"
                  :disabled="props.isBusy"
                  aria-label="分支组名称"
                />
              </td>
              <td>
                <ul class="member-list">
                  <li v-for="member in suggestion.members" :key="member.modId">
                    <strong>{{ member.name }}</strong>
                    <span>{{ member.enabled ? "已启用" : "未启用" }} · {{ member.fileCount }} 个文件</span>
                  </li>
                </ul>
              </td>
              <td>
                <div class="evidence-list">
                  <span>全部共同文件 {{ suggestion.sharedFileCount }}</span>
                  <span>最低相似度 {{ suggestion.similarityPercent }}%</span>
                  <span v-if="suggestion.minimumNameSimilarityPercent">
                    名称相似 {{ suggestion.minimumNameSimilarityPercent }}%
                  </span>
                  <span v-if="suggestion.sameImportSource">相同导入来源</span>
                  <span v-if="suggestion.conflictPairCount">冲突关系 {{ suggestion.conflictPairCount }}</span>
                  <span v-for="label in suggestion.sharedTargetLabels" :key="label">{{ label }}</span>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <p v-else class="empty-state">没有识别到可建议的分支组。</p>
      <p v-if="props.error" class="error-message">{{ props.error }}</p>

      <footer class="dialog-actions">
        <span>已选择 {{ selectedCount }} 组</span>
        <button type="button" class="secondary-button" :disabled="props.isBusy" @click="$emit('close')">
          取消
        </button>
        <button
          type="button"
          class="primary-button"
          :disabled="!selectedCount || props.isBusy"
          @click="confirmSelections"
        >
          {{ props.isBusy ? "创建中" : `创建 ${selectedCount} 组` }}
        </button>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.suggestion-backdrop {
  position: fixed;
  z-index: 24;
  inset: 0;
  display: grid;
  padding: 24px;
  place-items: center;
  background: rgba(23, 33, 31, 0.4);
}

.suggestion-dialog {
  display: grid;
  width: min(1040px, 100%);
  max-height: calc(100vh - 48px);
  gap: 12px;
  padding: 20px;
  overflow-x: hidden;
  overflow-y: auto;
  border: 1px solid #d9e2df;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 20px 60px rgba(23, 33, 31, 0.2);
}

.suggestion-heading,
.dialog-actions {
  display: flex;
  gap: 12px;
  align-items: center;
}

.suggestion-heading {
  justify-content: space-between;
}

.suggestion-heading h2,
.suggestion-heading p {
  margin: 0;
}

.suggestion-heading h2 {
  color: #17211f;
  font-size: 1.2rem;
}

.suggestion-heading p,
.dialog-actions > span {
  margin-top: 3px;
  color: #61756f;
  font-size: 0.8rem;
  font-weight: 700;
}

.close-button {
  display: grid;
  width: 34px;
  height: 34px;
  padding: 0;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #435650;
  background: #ffffff;
  font: inherit;
  font-size: 1.25rem;
  cursor: pointer;
}

.select-all-row {
  display: flex;
  min-height: 34px;
  gap: 8px;
  align-items: center;
  color: #435650;
  font-size: 0.82rem;
  font-weight: 700;
}

input[type="checkbox"] {
  width: 16px;
  height: 16px;
  margin: 0;
  accent-color: #24745b;
}

.suggestion-table-wrap {
  min-height: 0;
  max-height: min(62vh, 620px);
  overflow: auto;
  border: 1px solid #dfe7e3;
  border-radius: 6px;
}

.suggestion-table {
  width: 100%;
  min-width: 820px;
  border-collapse: collapse;
  table-layout: fixed;
}

.selection-column {
  width: 58px;
}

.name-column {
  width: 24%;
}

.member-column {
  width: 42%;
}

.evidence-column {
  width: 25%;
}

.suggestion-table th,
.suggestion-table td {
  padding: 10px 12px;
  border-bottom: 1px solid #e7eeeb;
  color: #435650;
  font-size: 0.8rem;
  text-align: left;
  vertical-align: top;
}

.suggestion-table th {
  position: sticky;
  z-index: 1;
  top: 0;
  color: #61756f;
  background: #f7faf8;
  font-size: 0.74rem;
}

.suggestion-selection,
.suggestion-table th:first-child {
  text-align: center;
  vertical-align: middle;
}

.group-name-input {
  width: 100%;
  min-height: 34px;
  padding: 0 9px;
  border: 1px solid #b9cbc4;
  border-radius: 5px;
  color: #17211f;
  background: #ffffff;
  font: inherit;
}

.member-list {
  display: grid;
  gap: 7px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.member-list li {
  display: grid;
  gap: 2px;
}

.member-list strong {
  color: #24332f;
  overflow-wrap: anywhere;
}

.member-list span {
  color: #72837e;
  font-size: 0.72rem;
}

.evidence-list {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
}

.evidence-list span {
  padding: 3px 6px;
  border: 1px solid #c7dcd3;
  border-radius: 4px;
  color: #315e52;
  background: #f1f7f4;
  font-size: 0.7rem;
  font-weight: 700;
}

.empty-state {
  margin: 0;
  padding: 36px 12px;
  border: 1px dashed #cbd8d4;
  border-radius: 6px;
  color: #61756f;
  text-align: center;
}

.error-message {
  margin: 0;
  color: #b42318;
  font-size: 0.82rem;
}

.dialog-actions {
  justify-content: flex-end;
}

.dialog-actions > span {
  margin-right: auto;
}

.dialog-actions button {
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid #1d6f55;
  border-radius: 5px;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.dialog-actions .secondary-button {
  border-color: #cbd8d4;
  color: #24745b;
  background: #ffffff;
}

.dialog-actions .primary-button {
  color: #ffffff;
  background: #24745b;
}

button:disabled,
input:disabled {
  color: #72837e;
  background: #e8eeec;
  cursor: not-allowed;
}

@media (max-width: 760px) {
  .suggestion-backdrop {
    padding: 12px;
  }

  .suggestion-dialog {
    max-height: calc(100vh - 24px);
    padding: 14px;
  }
}
</style>
