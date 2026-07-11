<script setup lang="ts">
import type { InstalledModSummary } from "../api/modLibrary";

const props = defineProps<{
  mods: InstalledModSummary[];
  installedModCount: number;
  conflictingModIds: Set<string>;
  activeModAction: string;
  openingModFolderId: string;
}>();

defineEmits<{
  edit: [mod: InstalledModSummary];
  openFolder: [mod: InstalledModSummary];
  enable: [mod: InstalledModSummary];
  disable: [mod: InstalledModSummary];
  uninstall: [mod: InstalledModSummary];
}>();

function modelKindLabel(modelKind: string) {
  const labels: Record<string, string> = {
    weapon: "武器",
    armor: "防具",
    hair: "发型",
    palicoWeapon: "随从武器",
    palicoArmor: "随从防具",
    kinsect: "猎虫",
    pendant: "挂件",
    npc: "NPC",
    slinger: "投射器",
    voice: "人物语音",
    face: "脸型",
    monster: "怪物",
    poogie: "噗吱猪服装",
    furniture: "家具",
    playerAccessory: "玩家附件",
    palicoAccessory: "随从附件",
  };

  return labels[modelKind] ?? modelKind;
}

function summarizeCategories(mod: InstalledModSummary) {
  return mod.categories.length ? mod.categories.join("、") : "未识别";
}

function summarizeReplacementNames(displayNames: string[], recognitionSource: string) {
  if (!displayNames.length) {
    return recognitionSource === "pathPattern"
      ? "当前 ID 表暂无名称，已按资源路径识别"
      : "已识别模型 ID，当前无可用游戏名称";
  }

  const visibleNames = displayNames.slice(0, 4);
  const remainingCount = displayNames.length - visibleNames.length;
  return remainingCount > 0
    ? `${visibleNames.join("、")}，另有 ${remainingCount} 个共用模型名称`
    : visibleNames.join("、");
}

function summarizeReplacements(mod: InstalledModSummary) {
  if (!mod.modelReplacements.length) {
    return "未识别到游戏内替换目标";
  }

  const summaries = mod.modelReplacements.map((replacement) =>
    `${modelKindLabel(replacement.modelKind)}：${summarizeReplacementNames(
      replacement.displayNames,
      replacement.recognitionSource,
    )}`,
  );
  const visibleSummaries = summaries.slice(0, 2);
  const remainingCount = summaries.length - visibleSummaries.length;
  return remainingCount > 0
    ? `${visibleSummaries.join("；")}；另有 ${remainingCount} 项`
    : visibleSummaries.join("；");
}

function summarizeNote(mod: InstalledModSummary) {
  const notes = [];
  if (mod.note) {
    notes.push(mod.note);
  }
  notes.push(mod.enabled ? "已启用" : "未启用");
  if (props.conflictingModIds.has(mod.id)) {
    notes.push("存在冲突");
  }
  return notes.join("；");
}
</script>

<template>
  <div v-if="props.mods.length" class="mod-table-scroll">
    <table class="mod-table">
      <thead>
        <tr>
          <th scope="col">序号</th>
          <th scope="col">名字</th>
          <th scope="col">类别</th>
          <th scope="col">替换信息</th>
          <th scope="col">备注</th>
          <th scope="col">操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(mod, index) in props.mods" :key="mod.id">
          <td class="mod-index">{{ index + 1 }}</td>
          <td class="mod-name" :title="mod.id">
            <strong>{{ mod.name }}</strong>
            <small v-if="mod.name !== mod.originalName">原始：{{ mod.originalName }}</small>
          </td>
          <td :title="summarizeCategories(mod)">{{ summarizeCategories(mod) }}</td>
          <td class="replacement-summary" :title="summarizeReplacements(mod)">
            {{ summarizeReplacements(mod) }}
          </td>
          <td :class="{ 'conflict-state': props.conflictingModIds.has(mod.id) }">
            {{ summarizeNote(mod) }}
          </td>
          <td class="mod-actions">
            <div class="mod-action-buttons">
              <button
                type="button"
                class="icon-button"
                :class="{ busy: props.activeModAction === mod.id }"
                :disabled="!!props.activeModAction"
                :aria-label="props.activeModAction === mod.id ? '正在编辑 MOD 信息' : '编辑名称和备注'"
                :data-tooltip="props.activeModAction === mod.id ? '正在编辑 MOD 信息' : '编辑名称和备注'"
                @click="$emit('edit', mod)"
              >
                <span aria-hidden="true">&#9998;</span>
              </button>
              <button
                type="button"
                class="icon-button"
                :class="{ busy: props.openingModFolderId === mod.id }"
                :disabled="props.openingModFolderId === mod.id"
                :aria-label="props.openingModFolderId === mod.id ? '正在打开文件夹' : '打开文件夹'"
                :data-tooltip="props.openingModFolderId === mod.id ? '正在打开文件夹' : '打开文件夹'"
                @click="$emit('openFolder', mod)"
              >
                <span v-if="props.openingModFolderId === mod.id" aria-hidden="true">&#8987;</span>
                <span v-else aria-hidden="true">&#128194;</span>
              </button>
              <button
                v-if="!mod.enabled"
                type="button"
                class="icon-button"
                :class="{ busy: props.activeModAction === mod.id }"
                :disabled="!!props.activeModAction"
                :aria-label="props.activeModAction === mod.id ? '正在启用 MOD' : '启用 MOD'"
                :data-tooltip="props.activeModAction === mod.id ? '正在启用 MOD' : '启用 MOD'"
                @click="$emit('enable', mod)"
              >
                <span aria-hidden="true">&#9654;</span>
              </button>
              <button
                v-else
                type="button"
                class="icon-button warning-icon"
                :class="{ busy: props.activeModAction === mod.id }"
                :disabled="!!props.activeModAction"
                :aria-label="props.activeModAction === mod.id ? '正在禁用 MOD' : '禁用 MOD'"
                :data-tooltip="props.activeModAction === mod.id ? '正在禁用 MOD' : '禁用 MOD'"
                @click="$emit('disable', mod)"
              >
                <span aria-hidden="true">&#9632;</span>
              </button>
              <button
                type="button"
                class="icon-button danger-icon"
                :class="{ busy: props.activeModAction === mod.id }"
                :disabled="!!props.activeModAction"
                :aria-label="props.activeModAction === mod.id ? '正在卸载 MOD' : '卸载 MOD'"
                :data-tooltip="props.activeModAction === mod.id ? '正在卸载 MOD' : '卸载 MOD'"
                @click="$emit('uninstall', mod)"
              >
                <span aria-hidden="true">&#128465;</span>
              </button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
  <p v-else class="empty-table-state">
    {{ props.installedModCount ? "没有符合当前筛选条件的 MOD。" : "当前没有已安装的 MOD。" }}
  </p>
</template>

<style scoped>
.mod-table-scroll {
  margin-top: 12px;
  overflow: auto;
  border: 1px solid #dfe7e3;
  border-radius: 6px;
}

.mod-table {
  width: 100%;
  min-width: 1020px;
  border-collapse: collapse;
  background: #ffffff;
}

.mod-table th,
.mod-table td {
  padding: 12px;
  border-bottom: 1px solid #e7eeeb;
  color: #435650;
  font-size: 0.86rem;
  text-align: left;
  vertical-align: middle;
}

.mod-table th {
  color: #61756f;
  background: #f7faf8;
  font-size: 0.76rem;
  font-weight: 750;
  white-space: nowrap;
}

.mod-table tbody tr:last-child td {
  border-bottom: 0;
}

.mod-table tbody tr:hover {
  background: #f9fcfa;
}

.mod-table td:nth-child(1) {
  width: 56px;
}

.mod-table td:nth-child(2) {
  min-width: 180px;
}

.mod-table td:nth-child(3) {
  width: 130px;
}

.mod-table td:nth-child(4) {
  min-width: 300px;
}

.mod-table td:nth-child(5) {
  width: 150px;
}

.mod-table td:nth-child(6) {
  width: 164px;
}

.mod-index {
  color: #72837e !important;
  font-variant-numeric: tabular-nums;
}

.mod-name strong,
.mod-name small {
  display: block;
  overflow-wrap: anywhere;
}

.mod-name strong {
  color: #17211f;
}

.mod-name small {
  margin-top: 3px;
  color: #72837e;
  font-size: 0.76rem;
}

.replacement-summary {
  color: #334b44 !important;
  line-height: 1.45;
}

.conflict-state {
  color: #9a3412 !important;
  font-weight: 700;
}

.mod-actions {
  white-space: nowrap;
}

.mod-action-buttons {
  display: flex;
  gap: 6px;
  justify-content: flex-start;
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

.icon-button:hover:not(:disabled),
.icon-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.icon-button:disabled {
  color: #72837e;
  background: #f1f5f3;
  cursor: not-allowed;
}

.icon-button.warning-icon {
  color: #9a5b00;
}

.icon-button.danger-icon {
  color: #b42318;
  font-size: 1rem;
}

.icon-button.busy span {
  animation: subtle-pulse 1.1s ease-in-out infinite;
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

.empty-table-state {
  margin: 12px 0 0;
  padding: 20px 12px;
  border: 1px dashed #cbd8d4;
  border-radius: 6px;
  color: #61756f;
  text-align: center;
}

@keyframes subtle-pulse {
  50% {
    opacity: 0.4;
  }
}
</style>
