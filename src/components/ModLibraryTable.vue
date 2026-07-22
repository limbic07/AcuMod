<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from "vue";
import type {
  InstalledModSummary,
  ModBranchGroup,
  ModCategory,
  ModMetadataPatch,
  ModelReplacement,
} from "../api/modLibrary";
import { localizeGameText, type GameTextLanguage } from "../domain/gameText";
import { armorTargetDisplayLabel } from "../domain/armorLabels";
import { compareNaturalText } from "../domain/textSort";

type EditableField = "name" | "note";

interface ModDraft {
  displayName?: string;
  note?: string;
}

interface PointerReorderState {
  sourceItemKey: string;
  sourceModIds: string[];
  pointerId: number;
  targetItemKey: string;
  targetModIds: string[];
  placeAfter: boolean;
}

const props = defineProps<{
  mods: InstalledModSummary[];
  allMods: InstalledModSummary[];
  branchGroups: ModBranchGroup[];
  gameTextLanguage: GameTextLanguage;
  installedModCount: number;
  categories: ModCategory[];
  conflictingModIds: Set<string>;
  conflictPartnerNames: Record<string, string[]>;
  activeModAction: string;
  openingModFolderId: string;
  metadataSavingModId: string;
  metadataErrorModId: string;
  metadataError: string;
  canReorder: boolean;
  reorderingModId: string;
  activeBatchAction: string;
  updatingBranchGroups: boolean;
}>();

const emit = defineEmits<{
  updateMetadata: [mod: InstalledModSummary, patch: ModMetadataPatch];
  createCategory: [mod: InstalledModSummary];
  updateBranchGroupCategory: [group: ModBranchGroup, categoryId: string | null, selected: boolean];
  createCategoryForBranchGroup: [group: ModBranchGroup];
  openFolder: [mod: InstalledModSummary];
  enable: [mod: InstalledModSummary];
  disable: [mod: InstalledModSummary];
  uninstall: [mod: InstalledModSummary];
  manageRemap: [mod: InstalledModSummary];
  analyze: [mod: InstalledModSummary];
  reorder: [modIds: string[], targetModIds: string[], placeAfter: boolean];
  batchEnable: [mods: InstalledModSummary[]];
  batchDisable: [mods: InstalledModSummary[]];
  batchUninstall: [mods: InstalledModSummary[]];
  createBranchGroup: [mods: InstalledModSummary[]];
  renameBranchGroup: [group: ModBranchGroup, name: string];
  ungroupMods: [mods: InstalledModSummary[]];
}>();

const editingCell = ref<{ modId: string; field: EditableField } | null>(null);
const editingCategoryModId = ref("");
const editingCategoryGroupId = ref("");
const editingInputs = ref<HTMLInputElement[]>([]);
const drafts = reactive<Record<string, ModDraft>>({});
const expandedModIds = ref(new Set<string>());
const draggedItemKey = ref("");
const dropTargetItemKey = ref("");
const pointerReorderState = ref<PointerReorderState | null>(null);
const modTableScroll = ref<HTMLElement | null>(null);
const reorderIndicatorTop = ref<number | null>(null);
const selectedModIds = ref(new Set<string>());
const expandedBranchGroupIds = ref(new Set<string>());
const editingBranchGroupId = ref("");
const branchGroupNameDraft = ref("");

function localizedGameText(value: string) {
  return localizeGameText(value, props.gameTextLanguage);
}

const branchGroupByModId = computed(() => {
  const byModId = new Map<string, ModBranchGroup>();
  for (const group of props.branchGroups) {
    for (const modId of group.modIds) {
      byModId.set(modId, group);
    }
  }
  return byModId;
});

const selectedMods = computed(() =>
  props.mods.filter((mod) => selectedModIds.value.has(mod.id)),
);
const selectedDisabledMods = computed(() => selectedMods.value.filter((mod) => !mod.enabled));
const selectedEnabledMods = computed(() => selectedMods.value.filter((mod) => mod.enabled));
const allVisibleModsSelected = computed(
  () => props.mods.length > 0 && selectedMods.value.length === props.mods.length,
);
const someVisibleModsSelected = computed(
  () => selectedMods.value.length > 0 && !allVisibleModsSelected.value,
);
const selectedGroupedMods = computed(() =>
  selectedMods.value.filter((mod) => branchGroupByModId.value.has(mod.id)),
);

function branchGroupForMod(mod: InstalledModSummary) {
  return branchGroupByModId.value.get(mod.id) ?? null;
}

function branchGroupForRow(mod: InstalledModSummary) {
  const group = branchGroupByModId.value.get(mod.id);
  if (!group) {
    throw new Error(`MOD ${mod.id} 不属于分支组。`);
  }
  return group;
}

function isFirstVisibleBranch(mod: InstalledModSummary, index: number) {
  const group = branchGroupForMod(mod);
  if (!group) {
    return false;
  }
  return !props.mods.slice(0, index).some((candidate) => group.modIds.includes(candidate.id));
}

function isBranchVisible(mod: InstalledModSummary) {
  const group = branchGroupForMod(mod);
  return !group || expandedBranchGroupIds.value.has(group.id);
}

function branchGroupMembers(group: ModBranchGroup) {
  const membersById = new Map(props.allMods.map((mod) => [mod.id, mod]));
  return group.modIds
    .map((modId) => membersById.get(modId))
    .filter((mod): mod is InstalledModSummary => Boolean(mod));
}

function visibleBranchGroupMembers(group: ModBranchGroup) {
  return props.mods.filter((mod) => group.modIds.includes(mod.id));
}

function libraryItemKeyForMod(mod: InstalledModSummary) {
  const group = branchGroupForMod(mod);
  return group ? `group:${group.id}` : `mod:${mod.id}`;
}

function libraryItemModIds(itemKey: string) {
  if (itemKey.startsWith("group:")) {
    const groupId = itemKey.slice("group:".length);
    return props.branchGroups.find((group) => group.id === groupId)?.modIds ?? [];
  }
  return itemKey.startsWith("mod:") ? [itemKey.slice("mod:".length)] : [];
}

function libraryItemIndex(index: number) {
  let itemCount = 0;
  const seenGroupIds = new Set<string>();
  for (const candidate of props.mods.slice(0, index + 1)) {
    const group = branchGroupForMod(candidate);
    if (!group) {
      itemCount += 1;
    } else if (!seenGroupIds.has(group.id)) {
      seenGroupIds.add(group.id);
      itemCount += 1;
    }
  }
  return itemCount;
}

function branchDisplayIndex(mod: InstalledModSummary, index: number) {
  const group = branchGroupForRow(mod);
  const branchIndex = visibleBranchGroupMembers(group).findIndex((candidate) => candidate.id === mod.id);
  return `${libraryItemIndex(index)}.${branchIndex + 1}`;
}

function isLastVisibleBranch(mod: InstalledModSummary, index: number) {
  const group = branchGroupForMod(mod);
  if (!group) {
    return false;
  }
  return !props.mods.slice(index + 1).some((candidate) => group.modIds.includes(candidate.id));
}

function branchGroupCategoryTags(group: ModBranchGroup) {
  const categories = new Map<string, ModCategory>();
  for (const mod of branchGroupMembers(group)) {
    for (const category of mod.categories) {
      categories.set(category.id, category);
    }
  }
  return [...categories.values()]
    .map((category) => ({ ...category, name: categoryDisplayName(category) }))
    .sort((left, right) => compareNaturalText(left.name, right.name));
}

function branchGroupCategorySelectionState(group: ModBranchGroup, categoryId: string) {
  const members = branchGroupMembers(group);
  const selectedCount = members.filter((mod) => mod.categoryIds.includes(categoryId)).length;
  return {
    checked: members.length > 0 && selectedCount === members.length,
    indeterminate: selectedCount > 0 && selectedCount < members.length,
  };
}

function beginBranchGroupCategoryEditing(group: ModBranchGroup) {
  if (isRowActionDisabled()) {
    return;
  }
  editingCategoryModId.value = "";
  editingCategoryGroupId.value = editingCategoryGroupId.value === group.id ? "" : group.id;
}

function closeBranchGroupCategoryMenuWhenFocusLeaves(group: ModBranchGroup, event: FocusEvent) {
  const editor = event.currentTarget as HTMLElement;
  // 点击 label 文字时，复选框取得焦点晚于原按钮的 focusout；等焦点稳定后再判断是否离开菜单。
  requestAnimationFrame(() => {
    if (!editor.contains(document.activeElement) && editingCategoryGroupId.value === group.id) {
      editingCategoryGroupId.value = "";
    }
  });
}

function toggleBranchGroupCategory(group: ModBranchGroup, categoryId: string) {
  const state = branchGroupCategorySelectionState(group, categoryId);
  emit("updateBranchGroupCategory", group, categoryId, !state.checked);
}

function clearBranchGroupCategories(group: ModBranchGroup) {
  emit("updateBranchGroupCategory", group, null, false);
}

function createCategoryForBranchGroup(group: ModBranchGroup) {
  editingCategoryGroupId.value = "";
  emit("createCategoryForBranchGroup", group);
}

function branchGroupReplacementLabels(group: ModBranchGroup) {
  const labels = new Set<string>();
  for (const mod of branchGroupMembers(group)) {
    for (const replacement of mod.modelReplacements) {
      labels.add(`${modelKindLabel(replacement.modelKind)} · ${replacementTargetLabel(replacement)}`);
    }
  }
  return [...labels];
}

function branchGroupSelectionState(group: ModBranchGroup) {
  const members = visibleBranchGroupMembers(group);
  const selectedCount = members.filter((mod) => selectedModIds.value.has(mod.id)).length;
  return {
    checked: members.length > 0 && selectedCount === members.length,
    indeterminate: selectedCount > 0 && selectedCount < members.length,
  };
}

function toggleBranchGroupSelection(group: ModBranchGroup, event: Event) {
  const nextSelectedIds = new Set(selectedModIds.value);
  for (const mod of visibleBranchGroupMembers(group)) {
    if ((event.target as HTMLInputElement).checked) {
      nextSelectedIds.add(mod.id);
    } else {
      nextSelectedIds.delete(mod.id);
    }
  }
  selectedModIds.value = nextSelectedIds;
}

function toggleBranchGroup(group: ModBranchGroup) {
  const next = new Set(expandedBranchGroupIds.value);
  if (next.has(group.id)) {
    next.delete(group.id);
  } else {
    next.add(group.id);
  }
  expandedBranchGroupIds.value = next;
}

function beginBranchGroupRename(group: ModBranchGroup) {
  editingBranchGroupId.value = group.id;
  branchGroupNameDraft.value = group.name;
  void nextTick(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-branch-group-input="${group.id}"]`);
    input?.focus();
    input?.select();
  });
}

function commitBranchGroupRename(group: ModBranchGroup) {
  const name = branchGroupNameDraft.value.trim();
  editingBranchGroupId.value = "";
  if (name && name !== group.name) {
    emit("renameBranchGroup", group, name);
  }
}

function setSelectedModIds(modIds: Iterable<string>) {
  selectedModIds.value = new Set(modIds);
}

function toggleAllVisibleMods(event: Event) {
  const checked = (event.target as HTMLInputElement).checked;
  setSelectedModIds(checked ? props.mods.map((mod) => mod.id) : []);
}

function toggleModSelection(modId: string, event: Event) {
  const nextSelectedIds = new Set(selectedModIds.value);
  if ((event.target as HTMLInputElement).checked) {
    nextSelectedIds.add(modId);
  } else {
    nextSelectedIds.delete(modId);
  }
  selectedModIds.value = nextSelectedIds;
}

function clearSelection() {
  setSelectedModIds([]);
}

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
    weaponVoice: "武器语音",
    plugin: "插件",
    face: "脸型",
    monster: "怪物",
    poogie: "噗吱猪服装",
    furniture: "家具",
    playerAccessory: "玩家附件",
    palicoAccessory: "随从附件",
  };

  return labels[modelKind] ?? modelKind;
}

function modelReplacementTitle(replacement: ModelReplacement) {
  if (replacement.modelKind === "weapon") {
    const modelPart = replacement.modelPart === "accessory" ? "附件模型" : "主模型";
    return `武器 · ${replacement.subKind} · ${modelPart}`;
  }

  if (replacement.modelKind === "armor") {
    return replacement.modelPart === "set" ? "防具套装" : `防具 · ${replacement.subKind}`;
  }

  return replacement.subKind || modelKindLabel(replacement.modelKind);
}

function categorySummary(mod: InstalledModSummary) {
  return mod.categories.length
    ? categoryTags(mod).map((category) => category.name).join("、")
    : "未分类";
}

function categoryTags(mod: InstalledModSummary) {
  if (!mod.categories.length) {
    return [{ id: "", name: "未分类", parentId: null, createdAtUnixSeconds: 0 }];
  }

  return mod.categories.map((category) => {
    return {
      ...category,
      name: categoryDisplayName(category),
    };
  });
}

function categoryDisplayName(category: ModCategory) {
  const parent = category.parentId
    ? props.categories.find((candidate) => candidate.id === category.parentId)
    : null;
  return parent ? `${parent.name}·${category.name}` : category.name;
}

function replacementTargetLabel(replacement: ModelReplacement) {
  if (replacement.modelKind === "armor" && replacement.modelPart === "set") {
    return armorSetTargetLabel(replacement);
  }

  const displayName = replacement.displayNames[0]
    ? localizedGameText(replacement.displayNames[0])
    : "";
  if (displayName) {
    return displayName;
  }

  const gameId = replacement.gameIds[0];
  return gameId ? `游戏 ID ${gameId}` : replacement.modelId;
}

function armorSetTargetLabel(replacement: ModelReplacement) {
  return armorTargetDisplayLabel(
    replacement.displayNames,
    replacement.modelId,
    localizedGameText,
  );
}

function associationLabel(replacement: ModelReplacement) {
  const armorNames = replacement.associations
    .filter((association) => association.modelKind === "armor")
    .map((association) =>
      association.displayNames[0]
        ? localizedGameText(association.displayNames[0])
        : association.modelId,
    );
  return armorNames.length ? `关联防具：${armorNames.join("、")}` : "";
}

function replacementSummary(mod: InstalledModSummary) {
  return mod.modelReplacements.slice(0, 2).map((replacement) => {
    return `${modelKindLabel(replacement.modelKind)} · ${replacementTargetLabel(replacement)}`;
  });
}

function remainingReplacementCount(mod: InstalledModSummary) {
  return Math.max(mod.modelReplacements.length - 2, 0);
}

function hasRemappableTarget(mod: InstalledModSummary) {
  const supportedKinds = new Set(["weapon", "armor", "palicoArmor", "slinger", "hair"]);
  return mod.originalModelReplacements.some((replacement) => supportedKinds.has(replacement.modelKind));
}

function isEditing(mod: InstalledModSummary, field: EditableField) {
  return editingCell.value?.modId === mod.id && editingCell.value.field === field;
}

function persistedDraftValue(mod: InstalledModSummary, field: EditableField) {
  if (field === "name") {
    return mod.name;
  }

  return mod.note;
}

function draftValue(mod: InstalledModSummary, field: EditableField) {
  const draft = drafts[mod.id];
  if (field === "name") {
    return draft?.displayName ?? persistedDraftValue(mod, field);
  }

  return draft?.note ?? persistedDraftValue(mod, field);
}

function setDraftValue(mod: InstalledModSummary, field: EditableField, event: Event) {
  const value = (event.target as HTMLInputElement).value;
  const draft = (drafts[mod.id] ??= {});

  if (field === "name") {
    draft.displayName = value;
  } else {
    draft.note = value;
  }
}

async function beginEditing(mod: InstalledModSummary, field: EditableField) {
  if (isRowActionDisabled()) {
    return;
  }

  const draft = (drafts[mod.id] ??= {});
  if (field === "name" && draft.displayName === undefined) {
    draft.displayName = persistedDraftValue(mod, field);
  }
  if (field === "note" && draft.note === undefined) {
    draft.note = persistedDraftValue(mod, field);
  }

  editingCell.value = { modId: mod.id, field };
  await nextTick();
  const input = editingInputs.value[0];
  input?.focus();
  const valueLength = input?.value.length ?? 0;
  input?.setSelectionRange(valueLength, valueLength);
}

function clearDraftField(mod: InstalledModSummary, field: EditableField) {
  const draft = drafts[mod.id];
  if (!draft) {
    return;
  }

  if (field === "name") {
    delete draft.displayName;
  } else {
    delete draft.note;
  }

  if (draft.displayName === undefined && draft.note === undefined) {
    delete drafts[mod.id];
  }
}

function cancelEditing(mod: InstalledModSummary, field: EditableField) {
  clearDraftField(mod, field);
  if (isEditing(mod, field)) {
    editingCell.value = null;
  }
}

function commitEditing(mod: InstalledModSummary, field: EditableField) {
  if (!isEditing(mod, field)) {
    return;
  }

  const value = draftValue(mod, field);
  const persistedValue = persistedDraftValue(mod, field);
  editingCell.value = null;

  if (field === "name" && !value.trim()) {
    clearDraftField(mod, field);
    return;
  }

  if (value.trim() === persistedValue.trim()) {
    clearDraftField(mod, field);
    return;
  }

  const patch =
    field === "name"
      ? { displayName: value.trim() === mod.originalName.trim() ? "" : value }
      : { note: value };
  emit("updateMetadata", mod, patch);
}

function toggleCategory(mod: InstalledModSummary, categoryId: string) {
  const isSelected = !mod.categoryIds.includes(categoryId);
  const categoryIds = new Set(mod.categoryIds);
  if (isSelected) {
    categoryIds.add(categoryId);
  } else {
    categoryIds.delete(categoryId);
  }
  emit("updateMetadata", mod, { categoryIds: [...categoryIds] });
}

function clearCategories(mod: InstalledModSummary) {
  if (!mod.categoryIds.length) {
    return;
  }
  emit("updateMetadata", mod, { categoryIds: [] });
}

function createCategory(mod: InstalledModSummary) {
  editingCategoryModId.value = "";
  emit("createCategory", mod);
}

function updatePointerReorderTarget(event: PointerEvent) {
  const reorderState = pointerReorderState.value;
  if (!reorderState || reorderState.pointerId !== event.pointerId) {
    return;
  }

  const row = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLTableRowElement>(
    "tr[data-library-item-key]",
  );
  const targetItemKey = row?.dataset.libraryItemKey ?? "";
  if (!row || !targetItemKey || targetItemKey === reorderState.sourceItemKey) {
    reorderState.targetItemKey = "";
    reorderState.targetModIds = [];
    dropTargetItemKey.value = "";
    reorderIndicatorTop.value = null;
    return;
  }

  const itemRows = [...(modTableScroll.value?.querySelectorAll<HTMLTableRowElement>(
    "tr[data-library-item-key]",
  ) ?? [])].filter((candidate) => candidate.dataset.libraryItemKey === targetItemKey);
  const firstBounds = itemRows[0]?.getBoundingClientRect() ?? row.getBoundingClientRect();
  const lastBounds = itemRows[itemRows.length - 1]?.getBoundingClientRect() ?? firstBounds;
  const scrollBounds = modTableScroll.value?.getBoundingClientRect();
  reorderState.targetItemKey = targetItemKey;
  reorderState.targetModIds = libraryItemModIds(targetItemKey);
  reorderState.placeAfter = event.clientY >= (firstBounds.top + lastBounds.bottom) / 2;
  dropTargetItemKey.value = targetItemKey;
  reorderIndicatorTop.value = scrollBounds
    ? Math.round(
        (reorderState.placeAfter ? lastBounds.bottom : firstBounds.top) -
          scrollBounds.top +
          (modTableScroll.value?.scrollTop ?? 0),
      )
    : null;
}

function startPointerReordering(itemKey: string, modIds: string[], event: PointerEvent) {
  if (!props.canReorder || isRowActionDisabled() || event.button !== 0) {
    return;
  }

  const dragHandle = event.currentTarget as HTMLElement;
  dragHandle.setPointerCapture(event.pointerId);
  draggedItemKey.value = itemKey;
  dropTargetItemKey.value = "";
  pointerReorderState.value = {
    sourceItemKey: itemKey,
    sourceModIds: modIds,
    pointerId: event.pointerId,
    targetItemKey: "",
    targetModIds: [],
    placeAfter: false,
  };
}

function trackPointerReordering(event: PointerEvent) {
  if (!pointerReorderState.value || pointerReorderState.value.pointerId !== event.pointerId) {
    return;
  }

  event.preventDefault();
  updatePointerReorderTarget(event);
}

function clearPointerReordering() {
  draggedItemKey.value = "";
  dropTargetItemKey.value = "";
  pointerReorderState.value = null;
  reorderIndicatorTop.value = null;
}

function finishPointerReordering(event: PointerEvent) {
  if (!pointerReorderState.value || pointerReorderState.value.pointerId !== event.pointerId) {
    return;
  }

  updatePointerReorderTarget(event);
  const reorderState = pointerReorderState.value;
  if (!reorderState) {
    return;
  }

  const dragHandle = event.currentTarget as HTMLElement;
  if (dragHandle.hasPointerCapture(event.pointerId)) {
    dragHandle.releasePointerCapture(event.pointerId);
  }

  if (reorderState.sourceModIds.length && reorderState.targetModIds.length) {
    emit(
      "reorder",
      reorderState.sourceModIds,
      reorderState.targetModIds,
      reorderState.placeAfter,
    );
  }
  clearPointerReordering();
}

function cancelPointerReordering(event: PointerEvent) {
  if (pointerReorderState.value?.pointerId !== event.pointerId) {
    return;
  }

  clearPointerReordering();
}

function isEditingCategory(mod: InstalledModSummary) {
  return editingCategoryModId.value === mod.id;
}

function beginCategoryEditing(mod: InstalledModSummary) {
  if (isRowActionDisabled()) {
    return;
  }

  editingCategoryGroupId.value = "";
  editingCategoryModId.value = editingCategoryModId.value === mod.id ? "" : mod.id;
}

function closeCategoryMenuWhenFocusLeaves(mod: InstalledModSummary, event: FocusEvent) {
  const categoryEditor = event.currentTarget as HTMLElement;
  requestAnimationFrame(() => {
    if (!categoryEditor.contains(document.activeElement) && isEditingCategory(mod)) {
      editingCategoryModId.value = "";
    }
  });
}

function toggleDetails(modId: string) {
  const nextExpandedIds = new Set(expandedModIds.value);
  if (nextExpandedIds.has(modId)) {
    nextExpandedIds.delete(modId);
  } else {
    nextExpandedIds.add(modId);
  }
  expandedModIds.value = nextExpandedIds;
}

function isExpanded(modId: string) {
  return expandedModIds.value.has(modId);
}

function isRowActionDisabled() {
  return Boolean(
    props.activeBatchAction ||
      props.activeModAction ||
      props.metadataSavingModId ||
      props.reorderingModId ||
      props.updatingBranchGroups,
  );
}

function conflictPartnerSummary(mod: InstalledModSummary) {
  const partners = props.conflictPartnerNames[mod.id] ?? [];
  if (!partners.length) {
    return "存在启用中的文件冲突";
  }

  const visiblePartners = partners.slice(0, 2);
  const remainingCount = partners.length - visiblePartners.length;
  return remainingCount > 0
    ? `与 ${visiblePartners.join("、")} 等 ${partners.length} 个 MOD 存在启用冲突`
    : `与 ${visiblePartners.join("、")} 存在启用冲突`;
}

watch(
  () => props.metadataSavingModId,
  (currentModId, previousModId) => {
    if (previousModId && !currentModId && props.metadataErrorModId !== previousModId) {
      delete drafts[previousModId];
    }
  },
);

watch(
  () => props.mods.map((mod) => mod.id).join("\u0000"),
  () => {
    // 批量选择只作用于当前筛选结果；隐藏项不会在用户看不到时继续被选中。
    const visibleIds = new Set(props.mods.map((mod) => mod.id));
    setSelectedModIds([...selectedModIds.value].filter((modId) => visibleIds.has(modId)));
  },
);
</script>

<template>
  <div v-if="props.mods.length" class="mod-table-region">
    <div class="batch-toolbar">
      <label class="batch-selection-summary">
        <input
          type="checkbox"
          :checked="allVisibleModsSelected"
          :indeterminate="someVisibleModsSelected"
          :disabled="isRowActionDisabled()"
          @change="toggleAllVisibleMods"
        />
        <span>{{ selectedMods.length ? `已选择 ${selectedMods.length} 项` : "选择当前结果" }}</span>
      </label>
      <div class="batch-actions">
        <button
          type="button"
          class="batch-action-button"
          :disabled="!selectedDisabledMods.length || isRowActionDisabled()"
          title="按当前表格顺序启用；后处理的冲突 MOD 优先级更高"
          @click="$emit('batchEnable', selectedDisabledMods)"
        >
          启用 {{ selectedDisabledMods.length }} 项
        </button>
        <button
          type="button"
          class="batch-action-button"
          :disabled="!selectedEnabledMods.length || isRowActionDisabled()"
          @click="$emit('batchDisable', selectedEnabledMods)"
        >
          禁用 {{ selectedEnabledMods.length }} 项
        </button>
        <button
          type="button"
          class="batch-action-button"
          :disabled="selectedMods.length < 2 || isRowActionDisabled()"
          @click="$emit('createBranchGroup', selectedMods)"
        >
          创建分支组
        </button>
        <button
          type="button"
          class="batch-action-button"
          :disabled="!selectedGroupedMods.length || isRowActionDisabled()"
          @click="$emit('ungroupMods', selectedGroupedMods)"
        >
          移出分支组
        </button>
        <button
          type="button"
          class="batch-action-button danger"
          :disabled="!selectedMods.length || isRowActionDisabled()"
          @click="$emit('batchUninstall', selectedMods)"
        >
          卸载 {{ selectedMods.length }} 项
        </button>
        <button
          v-if="selectedMods.length"
          type="button"
          class="batch-clear-button"
          :disabled="isRowActionDisabled()"
          @click="clearSelection"
        >
          清空选择
        </button>
      </div>
    </div>

    <div ref="modTableScroll" class="mod-table-scroll">
      <div
        v-if="reorderIndicatorTop !== null"
        class="reorder-indicator"
        :style="{ top: `${reorderIndicatorTop}px` }"
        aria-hidden="true"
      ></div>
      <table class="mod-table">
        <colgroup>
          <col class="status-column" />
          <col class="selection-column" />
          <col class="index-column" />
          <col class="name-column" />
          <col class="category-column" />
          <col class="replacement-column" />
          <col class="note-column" />
          <col class="actions-column" />
        </colgroup>
        <thead>
          <tr>
            <th scope="col">启用</th>
            <th scope="col" class="selection-heading">
              <input
                type="checkbox"
                :checked="allVisibleModsSelected"
                :indeterminate="someVisibleModsSelected"
                :disabled="isRowActionDisabled()"
                aria-label="选择当前显示的全部 MOD"
                title="选择当前显示的全部 MOD"
                @change="toggleAllVisibleMods"
              />
            </th>
            <th scope="col">序号</th>
            <th scope="col">名称</th>
            <th scope="col">分类</th>
            <th scope="col">替换信息</th>
            <th scope="col">备注</th>
            <th scope="col">操作</th>
          </tr>
        </thead>
      <tbody>
        <template v-for="(mod, index) in props.mods" :key="mod.id">
          <tr
            v-if="branchGroupForMod(mod) && isFirstVisibleBranch(mod, index)"
            class="branch-group-row"
            :class="{
              'is-selected': branchGroupSelectionState(branchGroupForRow(mod)).checked,
              'has-conflict': branchGroupMembers(branchGroupForRow(mod)).some((branch) => props.conflictingModIds.has(branch.id)),
              'is-dragging': draggedItemKey === libraryItemKeyForMod(mod),
              'is-drop-target': dropTargetItemKey === libraryItemKeyForMod(mod),
            }"
            :data-library-item-key="libraryItemKeyForMod(mod)"
          >
            <td class="mod-status branch-group-status">
              <button
                type="button"
                class="branch-group-toggle"
                :class="{ expanded: expandedBranchGroupIds.has(branchGroupForRow(mod).id) }"
                :aria-expanded="expandedBranchGroupIds.has(branchGroupForRow(mod).id)"
                :aria-label="expandedBranchGroupIds.has(branchGroupForRow(mod).id) ? '收起分支组' : '展开分支组'"
                :data-tooltip="expandedBranchGroupIds.has(branchGroupForRow(mod).id) ? '收起分支' : '展开分支'"
                @click="toggleBranchGroup(branchGroupForRow(mod))"
              >
                <span
                  class="branch-group-chevron"
                  :class="{ expanded: expandedBranchGroupIds.has(branchGroupForRow(mod).id) }"
                  aria-hidden="true"
                ></span>
              </button>
            </td>
            <td class="mod-selection">
              <input
                type="checkbox"
                :checked="branchGroupSelectionState(branchGroupForRow(mod)).checked"
                :indeterminate="branchGroupSelectionState(branchGroupForRow(mod)).indeterminate"
                :disabled="isRowActionDisabled()"
                :aria-label="`选择分支组 ${branchGroupForRow(mod).name}`"
                @change="toggleBranchGroupSelection(branchGroupForRow(mod), $event)"
              />
            </td>
            <td class="mod-index">
              <span class="mod-index-content">
                <span class="mod-index-number">{{ libraryItemIndex(index) }}</span>
                <button
                  v-if="props.canReorder"
                  type="button"
                  class="drag-handle"
                  aria-label="拖拽调整分支组顺序"
                  title="拖拽调整分支组顺序"
                  @pointerdown.prevent="startPointerReordering(libraryItemKeyForMod(mod), branchGroupForRow(mod).modIds, $event)"
                  @pointermove="trackPointerReordering"
                  @pointerup="finishPointerReordering"
                  @pointercancel="cancelPointerReordering"
                >
                  <span class="drag-grip" aria-hidden="true">
                    <span v-for="dot in 6" :key="dot"></span>
                  </span>
                </button>
              </span>
            </td>
            <td class="mod-name branch-group-name-cell">
              <div class="branch-group-title">
                <input
                  v-if="editingBranchGroupId === branchGroupForRow(mod).id"
                  v-model="branchGroupNameDraft"
                  :data-branch-group-input="branchGroupForRow(mod).id"
                  class="branch-group-name-input"
                  maxlength="120"
                  @blur="commitBranchGroupRename(branchGroupForRow(mod))"
                  @keydown.enter.prevent="commitBranchGroupRename(branchGroupForRow(mod))"
                  @keydown.esc.prevent="editingBranchGroupId = ''"
                />
                <button
                  v-else
                  type="button"
                  class="branch-group-name"
                  :disabled="isRowActionDisabled()"
                  title="点击修改分支组名称"
                  @click="beginBranchGroupRename(branchGroupForRow(mod))"
                >
                  {{ branchGroupForRow(mod).name }}
                </button>
                <span class="branch-group-label">分支组</span>
              </div>
            </td>
            <td class="mod-category branch-group-category">
              <div
                class="category-editor"
                @focusout="closeBranchGroupCategoryMenuWhenFocusLeaves(branchGroupForRow(mod), $event)"
              >
                <button
                  type="button"
                  class="category-tag-button"
                  :class="{ active: editingCategoryGroupId === branchGroupForRow(mod).id }"
                  :disabled="isRowActionDisabled()"
                  :title="branchGroupCategoryTags(branchGroupForRow(mod)).map((tag) => tag.name).join('、') || '未分类'"
                  :aria-label="`编辑分支组分类：${branchGroupForRow(mod).name}`"
                  :aria-expanded="editingCategoryGroupId === branchGroupForRow(mod).id"
                  @click="beginBranchGroupCategoryEditing(branchGroupForRow(mod))"
                >
                  <template v-if="branchGroupCategoryTags(branchGroupForRow(mod)).length">
                    <span
                      v-for="category in branchGroupCategoryTags(branchGroupForRow(mod))"
                      :key="category.id"
                      class="category-tag"
                    >
                      {{ category.name }}
                    </span>
                  </template>
                  <span v-else class="category-tag unknown-category-tag">未分类</span>
                </button>

                <div
                  v-if="editingCategoryGroupId === branchGroupForRow(mod).id"
                  class="category-menu"
                  @keydown.esc="editingCategoryGroupId = ''"
                >
                  <button
                    v-for="category in props.categories"
                    :key="category.id"
                    type="button"
                    class="category-option"
                    role="checkbox"
                    :aria-checked="
                      branchGroupCategorySelectionState(branchGroupForRow(mod), category.id).indeterminate
                        ? 'mixed'
                        : branchGroupCategorySelectionState(branchGroupForRow(mod), category.id).checked
                    "
                    :disabled="isRowActionDisabled()"
                    @click="toggleBranchGroupCategory(branchGroupForRow(mod), category.id)"
                  >
                    <span
                      class="category-checkbox"
                      :class="{
                        checked: branchGroupCategorySelectionState(branchGroupForRow(mod), category.id).checked,
                        mixed: branchGroupCategorySelectionState(branchGroupForRow(mod), category.id).indeterminate,
                      }"
                      aria-hidden="true"
                    ></span>
                    <span>{{ categoryDisplayName(category) }}</span>
                  </button>
                  <p v-if="!props.categories.length" class="empty-category-options">暂无分类</p>
                  <div class="category-menu-actions">
                    <button
                      type="button"
                      :disabled="!branchGroupCategoryTags(branchGroupForRow(mod)).length || isRowActionDisabled()"
                      @click="clearBranchGroupCategories(branchGroupForRow(mod))"
                    >
                      清空分类
                    </button>
                    <button
                      type="button"
                      :disabled="isRowActionDisabled()"
                      @click="createCategoryForBranchGroup(branchGroupForRow(mod))"
                    >
                      新建分类
                    </button>
                  </div>
                </div>
              </div>
            </td>
            <td class="replacement-summary branch-group-replacements">
              <template v-if="branchGroupReplacementLabels(branchGroupForRow(mod)).length">
                <span
                  v-for="label in branchGroupReplacementLabels(branchGroupForRow(mod))"
                  :key="label"
                >
                  {{ label }}
                </span>
              </template>
              <span v-else>未识别到游戏内替换目标</span>
            </td>
            <td class="mod-note branch-group-counts">
              <strong>{{ branchGroupMembers(branchGroupForRow(mod)).length }} 个分支</strong>
              <span>已启用 {{ branchGroupMembers(branchGroupForRow(mod)).filter((branch) => branch.enabled).length }} 个</span>
            </td>
            <td class="mod-actions">
              <div class="mod-action-buttons branch-group-actions">
                <button
                  type="button"
                  class="icon-button"
                  :disabled="!branchGroupMembers(branchGroupForRow(mod)).some((branch) => branch.enabled) || isRowActionDisabled()"
                  aria-label="禁用全部分支"
                  data-tooltip="禁用全部分支"
                  @click="$emit('batchDisable', branchGroupMembers(branchGroupForRow(mod)).filter((branch) => branch.enabled))"
                >
                  <span aria-hidden="true">&#9632;</span>
                </button>
                <button
                  type="button"
                  class="icon-button danger-icon"
                  :disabled="isRowActionDisabled()"
                  aria-label="卸载整个分支组"
                  data-tooltip="卸载整个分支组"
                  @click="$emit('batchUninstall', branchGroupMembers(branchGroupForRow(mod)))"
                >
                  <span aria-hidden="true">&#128465;</span>
                </button>
              </div>
            </td>
          </tr>
          <tr
            v-if="isBranchVisible(mod)"
            :class="{
              'is-enabled': mod.enabled,
              'is-branch': Boolean(branchGroupForMod(mod)),
              'is-branch-end': isLastVisibleBranch(mod, index),
              'is-selected': selectedModIds.has(mod.id),
              'has-conflict': props.conflictingModIds.has(mod.id),
              'is-dragging': draggedItemKey === libraryItemKeyForMod(mod),
              'is-drop-target': dropTargetItemKey === libraryItemKeyForMod(mod),
            }"
            :data-mod-id="mod.id"
            :data-library-item-key="libraryItemKeyForMod(mod)"
          >
            <td class="mod-status">
              <button
                type="button"
                class="status-button"
                :class="{
                  enabled: mod.enabled,
                  busy: props.activeModAction === mod.id,
                }"
                :disabled="isRowActionDisabled()"
                :aria-pressed="mod.enabled"
                :aria-label="
                  props.activeModAction === mod.id
                    ? '正在更新 MOD 启用状态'
                    : mod.enabled
                      ? '已启用，点击禁用 MOD'
                      : '未启用，点击启用 MOD'
                "
                :title="
                  props.activeModAction === mod.id
                    ? '正在更新'
                    : mod.enabled
                      ? '已启用，点击禁用'
                      : '未启用，点击启用'
                "
                @click="mod.enabled ? $emit('disable', mod) : $emit('enable', mod)"
              >
                <span v-if="props.activeModAction === mod.id" aria-hidden="true">&#8987;</span>
                <span v-else class="power-symbol" aria-hidden="true"></span>
              </button>
            </td>

            <td class="mod-selection">
              <input
                type="checkbox"
                :checked="selectedModIds.has(mod.id)"
                :disabled="isRowActionDisabled()"
                :aria-label="`选择 ${mod.name}`"
                @change="toggleModSelection(mod.id, $event)"
              />
            </td>

            <td class="mod-index">
              <span class="mod-index-content">
                <span class="mod-index-number">
                  {{ branchGroupForMod(mod) ? branchDisplayIndex(mod, index) : libraryItemIndex(index) }}
                </span>
                <button
                  v-if="props.canReorder && !branchGroupForMod(mod)"
                  type="button"
                  class="drag-handle"
                  aria-label="拖拽调整 MOD 顺序"
                  title="拖拽调整顺序"
                  @pointerdown.prevent="startPointerReordering(libraryItemKeyForMod(mod), [mod.id], $event)"
                  @pointermove="trackPointerReordering"
                  @pointerup="finishPointerReordering"
                  @pointercancel="cancelPointerReordering"
                >
                  <span class="drag-grip" aria-hidden="true">
                    <span v-for="dot in 6" :key="dot"></span>
                  </span>
                </button>
              </span>
            </td>

            <td class="mod-name">
              <div class="name-editor">
                <input
                  v-if="isEditing(mod, 'name')"
                  ref="editingInputs"
                  :value="draftValue(mod, 'name')"
                  class="inline-editor"
                  maxlength="120"
                  aria-label="MOD 显示名称"
                  @input="setDraftValue(mod, 'name', $event)"
                  @blur="commitEditing(mod, 'name')"
                  @keydown.enter.prevent="commitEditing(mod, 'name')"
                  @keydown.esc.prevent="cancelEditing(mod, 'name')"
                />
                <button
                  v-else
                  type="button"
                  class="inline-value-button mod-name-value"
                  :disabled="isRowActionDisabled()"
                  :title="mod.name"
                  @click="beginEditing(mod, 'name')"
                >
                  {{ mod.name }}
                </button>
                <span
                  v-if="mod.partiallyOverridden"
                  class="conflict-indicator partial-override-indicator"
                  title="已启用，部分文件被更高优先级 MOD 覆盖"
                  aria-label="已启用，部分文件被更高优先级 MOD 覆盖"
                >
                  !
                </span>
                <span
                  v-if="props.conflictingModIds.has(mod.id)"
                  class="conflict-indicator"
                  :title="conflictPartnerSummary(mod)"
                  aria-label="存在启用冲突"
                >
                  !
                </span>
              </div>
              <p v-if="props.metadataErrorModId === mod.id" class="metadata-error">
                {{ props.metadataError }}
              </p>
            </td>

            <td class="mod-category">
              <div class="category-editor" @focusout="closeCategoryMenuWhenFocusLeaves(mod, $event)">
                <button
                  type="button"
                  class="category-tag-button"
                  :class="{ active: isEditingCategory(mod) }"
                  :disabled="isRowActionDisabled()"
                  :title="categoryTags(mod).map((tag) => tag.name).join('、')"
                  :aria-label="`编辑 MOD 分类：${categoryTags(mod).map((tag) => tag.name).join('、')}`"
                  :aria-expanded="isEditingCategory(mod)"
                  @click="beginCategoryEditing(mod)"
                >
                  <span
                    v-for="tag in categoryTags(mod)"
                    :key="tag.id || tag.name"
                    class="category-tag"
                    :class="{ 'unknown-category-tag': !tag.id }"
                  >
                    {{ tag.name }}
                  </span>
                </button>

                <div v-if="isEditingCategory(mod)" class="category-menu" @keydown.esc="editingCategoryModId = ''">
                  <button
                    v-for="category in props.categories"
                    :key="category.id"
                    type="button"
                    class="category-option"
                    role="checkbox"
                    :aria-checked="mod.categoryIds.includes(category.id)"
                    :disabled="isRowActionDisabled()"
                    @click="toggleCategory(mod, category.id)"
                  >
                    <span
                      class="category-checkbox"
                      :class="{ checked: mod.categoryIds.includes(category.id) }"
                      aria-hidden="true"
                    ></span>
                    <span>{{ categoryDisplayName(category) }}</span>
                  </button>
                  <p v-if="!props.categories.length" class="empty-category-options">暂无分类</p>
                  <div class="category-menu-actions">
                    <button
                      type="button"
                      :disabled="!mod.categoryIds.length || isRowActionDisabled()"
                      @click="clearCategories(mod)"
                    >
                      清空分类
                    </button>
                    <button type="button" :disabled="isRowActionDisabled()" @click="createCategory(mod)">新建分类</button>
                  </div>
                </div>
              </div>
            </td>

            <td class="replacement-summary" :class="{ 'has-remap-entry': hasRemappableTarget(mod) }">
              <button
                v-if="hasRemappableTarget(mod)"
                type="button"
                class="replacement-entry-button"
                :disabled="isRowActionDisabled()"
                :aria-label="mod.enabled ? '查看模型替换目标，修改前需先禁用 MOD' : '修改模型替换目标'"
                :title="mod.enabled ? '查看替换目标，修改前需先禁用' : '修改替换目标'"
                @click="$emit('manageRemap', mod)"
              >
                <span class="replacement-entry-content">
                  <span v-for="summary in replacementSummary(mod)" :key="summary">{{ summary }}</span>
                  <span v-if="remainingReplacementCount(mod)" class="replacement-more">
                    +{{ remainingReplacementCount(mod) }}
                  </span>
                </span>
                <span class="replacement-entry-icon" aria-hidden="true">&#8644;</span>
              </button>
              <template v-else-if="mod.modelReplacements.length">
                <span v-for="summary in replacementSummary(mod)" :key="summary">{{ summary }}</span>
                <span v-if="remainingReplacementCount(mod)" class="replacement-more">
                  +{{ remainingReplacementCount(mod) }}
                </span>
              </template>
              <span v-else>未识别到游戏内替换目标</span>
            </td>

            <td class="mod-note">
              <input
                v-if="isEditing(mod, 'note')"
                ref="editingInputs"
                :value="draftValue(mod, 'note')"
                class="inline-editor"
                maxlength="800"
                aria-label="MOD 备注"
                @input="setDraftValue(mod, 'note', $event)"
                @blur="commitEditing(mod, 'note')"
                @keydown.enter.prevent="commitEditing(mod, 'note')"
                @keydown.esc.prevent="cancelEditing(mod, 'note')"
              />
              <button
                v-else
                type="button"
                class="inline-value-button note-value"
                :disabled="isRowActionDisabled()"
                :title="mod.note || '添加备注'"
                @click="beginEditing(mod, 'note')"
              >
                {{ mod.note || "添加备注" }}
              </button>
            </td>

            <td class="mod-actions">
              <div class="mod-action-buttons">
                <button
                  type="button"
                  class="icon-button"
                  :class="{ busy: props.openingModFolderId === mod.id }"
                  :disabled="isRowActionDisabled() || props.openingModFolderId === mod.id"
                  :aria-label="props.openingModFolderId === mod.id ? '正在打开文件夹' : '打开本地 MOD 文件夹'"
                  :data-tooltip="props.openingModFolderId === mod.id ? '正在打开文件夹' : '打开本地文件夹'"
                  @click="$emit('openFolder', mod)"
                >
                  <span v-if="props.openingModFolderId === mod.id" aria-hidden="true">&#8987;</span>
                  <span v-else aria-hidden="true">&#128194;</span>
                </button>
                <button
                  type="button"
                  class="icon-button"
                  :class="{ active: isExpanded(mod.id) }"
                  :disabled="isRowActionDisabled()"
                  :aria-label="isExpanded(mod.id) ? '收起 MOD 详情' : '展开 MOD 详情'"
                  :data-tooltip="isExpanded(mod.id) ? '收起详情' : '展开详情'"
                  @click="toggleDetails(mod.id)"
                >
                  <span aria-hidden="true">{{ isExpanded(mod.id) ? "⌃" : "⌄" }}</span>
                </button>
                <button
                  type="button"
                  class="icon-button danger-icon"
                  :class="{ busy: props.activeModAction === mod.id }"
                  :disabled="isRowActionDisabled()"
                  :aria-label="props.activeModAction === mod.id ? '正在卸载 MOD' : '卸载 MOD'"
                  :data-tooltip="props.activeModAction === mod.id ? '正在卸载 MOD' : '卸载 MOD'"
                  @click="$emit('uninstall', mod)"
                >
                  <span aria-hidden="true">&#128465;</span>
                </button>
              </div>
            </td>
          </tr>

          <tr
            v-if="isBranchVisible(mod) && isExpanded(mod.id)"
            class="mod-details-row"
            :class="{ 'is-branch-details': Boolean(branchGroupForMod(mod)) }"
            :data-library-item-key="libraryItemKeyForMod(mod)"
          >
            <td colspan="8" class="mod-details-cell">
              <div class="mod-details-grid">
                <section>
                  <h4>分类</h4>
                  <p>{{ categorySummary(mod) }}</p>
                </section>
                <section>
                  <h4>原始名称</h4>
                  <p>{{ mod.originalName }}</p>
                </section>
                <section>
                  <h4>本地文件</h4>
                  <p>{{ mod.fileCount }} 个文件</p>
                  <p class="detail-path">{{ mod.contentPath }}</p>
                  <button type="button" class="detail-command" @click="$emit('analyze', mod)">
                    分析文件作用
                  </button>
                </section>
                <section>
                  <h4>部署根目录</h4>
                  <p>{{ mod.deployRoot }}</p>
                </section>
                <section>
                  <h4>冲突状态</h4>
                  <p>
                    {{
                      mod.partiallyOverridden
                        ? "已启用，部分文件被更高优先级 MOD 覆盖"
                        : props.conflictingModIds.has(mod.id)
                          ? conflictPartnerSummary(mod)
                          : "当前没有启用冲突"
                    }}
                  </p>
                </section>
              </div>

              <section class="replacement-details">
                <div class="replacement-heading">
                  <h4>替换目标</h4>
                  <span v-if="mod.modelRemapCount" class="remap-count">已改绑 {{ mod.modelRemapCount }}</span>
                </div>
                <ul v-if="mod.modelReplacements.length">
                  <li v-for="replacement in mod.modelReplacements" :key="`${replacement.modelKind}-${replacement.modelId}`">
                    <strong>{{ modelReplacementTitle(replacement) }}</strong>
                    <span>{{ replacementTargetLabel(replacement) }}</span>
                    <small>
                      {{ replacement.gameIds.length ? `游戏 ID：${replacement.gameIds.join("、")}` : `资源 ID：${replacement.modelId}` }}
                    </small>
                    <small v-if="associationLabel(replacement)" class="model-association">
                      {{ associationLabel(replacement) }}
                    </small>
                  </li>
                </ul>
                <p v-else>未识别到游戏内替换目标。</p>
              </section>

              <details class="file-details">
                <summary>文件清单（{{ mod.fileCount }}）</summary>
                <ul>
                  <li v-for="file in mod.files" :key="file.deployRelativePath">
                    {{ file.deployRelativePath }}
                  </li>
                </ul>
              </details>
            </td>
          </tr>

          <tr
            v-if="branchGroupForMod(mod) && isBranchVisible(mod) && isLastVisibleBranch(mod, index)"
            class="branch-group-separator-row"
            :data-library-item-key="libraryItemKeyForMod(mod)"
            aria-hidden="true"
          >
            <td colspan="8"></td>
          </tr>
        </template>
      </tbody>
    </table>
    </div>
  </div>
  <p v-else class="empty-table-state">
    {{ props.installedModCount ? "没有符合当前筛选条件的 MOD。" : "当前没有已安装的 MOD。" }}
  </p>
</template>

<style scoped>
.mod-table-region {
  width: 100%;
  max-width: 100%;
  margin-top: 12px;
}

.batch-toolbar {
  position: sticky;
  top: 0;
  z-index: 7;
  display: flex;
  min-height: 42px;
  gap: 12px;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border: 1px solid #dfe7e3;
  border-bottom-color: #cbd8d4;
  border-radius: 6px 6px 0 0;
  background: rgba(255, 255, 255, 0.98);
  box-shadow: 0 5px 12px rgba(28, 55, 46, 0.07);
}

.batch-selection-summary,
.batch-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.batch-selection-summary {
  color: #52645f;
  font-size: 0.82rem;
  font-weight: 700;
  white-space: nowrap;
}

.branch-group-row {
  border-top: 2px solid #86ad9d;
  border-bottom: 1px solid #bcd3c9 !important;
}

.branch-group-row td {
  background: #edf5f1;
}

.branch-group-row td:first-child,
.mod-table tbody tr.is-branch td:first-child,
.mod-details-row.is-branch-details td:first-child {
  box-shadow: inset 3px 0 0 #5f947f;
}

.branch-group-toggle,
.branch-group-name {
  border: 0;
  background: transparent;
  color: #214b3d;
}

.branch-group-toggle {
  display: grid;
  width: 30px;
  height: 30px;
  margin: 0 auto;
  padding: 0;
  place-items: center;
  border-radius: 4px;
  font-size: 1.1rem;
  cursor: pointer;
}

.branch-group-status {
  padding-right: 0 !important;
  padding-left: 0 !important;
}

.branch-group-chevron {
  width: 8px;
  height: 8px;
  border-right: 1.5px solid currentColor;
  border-bottom: 1.5px solid currentColor;
  transform: translateY(-2px) rotate(45deg);
}

.branch-group-chevron.expanded {
  transform: translateY(2px) rotate(225deg);
}

.branch-group-toggle:hover,
.branch-group-toggle:focus-visible {
  color: #17613f;
  background: #dbece4;
}

.branch-group-title {
  display: flex;
  gap: 7px;
  align-items: flex-start;
  min-width: 0;
}

.branch-group-name {
  min-width: 0;
  width: auto;
  max-width: 100%;
  flex: 1 1 auto;
  padding: 0;
  font-size: 0.95rem;
  font-weight: 800;
  line-height: 1.45;
  text-align: left;
  overflow-wrap: anywhere;
  white-space: normal;
}

.branch-group-label {
  flex: none;
  color: #61756f;
  font-size: 0.7rem;
  font-weight: 700;
}

.branch-group-name-input {
  width: min(420px, 100%);
}

.branch-group-actions {
  gap: 3px;
}

.mod-action-buttons.branch-group-actions {
  display: grid;
  grid-template-columns: repeat(3, 30px);
}

.branch-group-actions .icon-button:not(.danger-icon) {
  grid-column: 2;
}

.branch-group-actions .danger-icon {
  grid-column: 3;
}

.branch-group-category {
  line-height: 1.8;
  overflow-wrap: anywhere;
}

.branch-group-category .category-tag {
  display: inline-block;
  margin: 2px 3px 2px 0;
  overflow: visible;
  overflow-wrap: anywhere;
  text-overflow: clip;
  white-space: normal;
}

.category-more,
.branch-group-counts span {
  color: #61756f;
  font-size: 0.72rem;
  font-weight: 700;
}

.branch-group-replacements span {
  display: block;
  overflow-wrap: anywhere;
  white-space: normal;
}

.branch-group-counts {
  line-height: 1.35;
}

.branch-group-counts strong,
.branch-group-counts span {
  display: block;
}

.branch-group-counts strong {
  color: #334b44;
  font-size: 0.8rem;
}

.mod-table tbody tr.is-branch .mod-name {
  padding-left: 22px;
}

.mod-table tbody tr.is-branch {
  background: #f8fbf9;
}

.mod-table tbody tr.is-branch:hover {
  background: #f2f8f5;
}

.mod-details-row.is-branch-details .mod-details-cell {
  background: #f8fbf9;
}

.branch-group-separator-row td {
  height: 9px;
  padding: 0 !important;
  border-top: 2px solid #86ad9d;
  background: #ffffff;
}

.batch-selection-summary input,
.selection-heading input,
.mod-selection input {
  width: 16px;
  height: 16px;
  margin: 0;
  accent-color: #24745b;
  cursor: pointer;
}

.batch-selection-summary input:disabled,
.selection-heading input:disabled,
.mod-selection input:disabled {
  cursor: not-allowed;
}

.batch-action-button,
.batch-clear-button {
  min-height: 32px;
  padding: 0 10px;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #334b44;
  background: #ffffff;
  font: inherit;
  font-size: 0.78rem;
  font-weight: 700;
  cursor: pointer;
}

.batch-action-button:hover:not(:disabled),
.batch-action-button:focus-visible,
.batch-clear-button:hover:not(:disabled),
.batch-clear-button:focus-visible {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.batch-action-button.danger {
  border-color: #e0c3bc;
  color: #9a392c;
}

.batch-action-button.danger:hover:not(:disabled),
.batch-action-button.danger:focus-visible {
  border-color: #c86557;
  color: #7f2e23;
  background: #fff2ef;
}

.batch-action-button:disabled,
.batch-clear-button:disabled {
  color: #8a9894;
  background: #f3f6f5;
  cursor: not-allowed;
}

.mod-table-scroll {
  position: relative;
  width: 100%;
  max-width: 100%;
  overflow: auto;
  border: 1px solid #dfe7e3;
  border-radius: 0 0 6px 6px;
  border-top: 0;
}

.mod-table {
  width: 100%;
  min-width: 900px;
  table-layout: fixed;
  border-collapse: collapse;
  background: #ffffff;
}

.status-column {
  width: 48px;
}

.selection-column {
  width: 42px;
}

.index-column {
  width: 68px;
}

.name-column {
  width: 22%;
}

.category-column {
  width: 14%;
}

.replacement-column {
  width: 27%;
}

.note-column {
  width: 12%;
}

.actions-column {
  width: 110px;
}

.mod-table th,
.mod-table td {
  padding: 10px 12px;
  color: #435650;
  font-size: 0.86rem;
  text-align: left;
  vertical-align: middle;
}

.mod-table tbody > tr:not(.mod-details-row) {
  border-bottom: 1px solid #e7eeeb;
}

.reorder-indicator {
  position: absolute;
  z-index: 4;
  right: 0;
  left: 0;
  height: 2px;
  background: #24745b;
  pointer-events: none;
}

.mod-table th {
  border-bottom: 1px solid #e7eeeb;
  color: #61756f;
  background: #f7faf8;
  font-size: 0.76rem;
  font-weight: 750;
  white-space: nowrap;
}

.mod-table th:nth-child(1),
.mod-table th:nth-child(2),
.mod-table th:nth-child(3) {
  text-align: center;
}

.mod-table tbody tr:hover:not(.mod-details-row) {
  background: #f9fcfa;
}

.mod-table tbody tr.is-selected:not(.mod-details-row),
.mod-table tbody tr.is-selected:hover:not(.mod-details-row) {
  background: #eef6f2;
}

.name-editor,
.mod-action-buttons {
  display: flex;
  align-items: center;
}

.mod-status {
  text-align: center !important;
}

.mod-status .status-button {
  margin: 0 auto;
}

.selection-heading,
.mod-selection {
  text-align: center !important;
}

.selection-heading input,
.mod-selection input {
  display: block;
  margin: 0 auto;
}

.mod-index {
  color: #72837e !important;
  text-align: center !important;
  font-variant-numeric: tabular-nums;
  padding-right: 8px !important;
  padding-left: 8px !important;
}

.mod-index-content {
  display: inline-grid;
  width: 48px;
  height: 28px;
  grid-template-columns: 24px 18px;
  column-gap: 6px;
  align-items: center;
  vertical-align: middle;
}

.mod-index-number {
  text-align: right;
}

.drag-handle {
  display: grid;
  width: 18px;
  height: 28px;
  padding: 0;
  place-items: center;
  border: 0;
  color: #83948f;
  background: transparent;
  cursor: grab;
  touch-action: none;
  user-select: none;
}

.drag-handle:active {
  cursor: grabbing;
}

.drag-handle:hover,
.drag-handle:focus-visible {
  color: #24745b;
}

.is-dragging .drag-handle,
.is-drop-target .drag-handle {
  color: #17613f;
}

.drag-handle:focus-visible {
  outline: 2px solid #8cbca8;
  outline-offset: 1px;
}

.drag-grip {
  display: grid;
  width: 8px;
  height: 13px;
  grid-template-columns: repeat(2, 3px);
  grid-template-rows: repeat(3, 3px);
  gap: 2px;
}

.drag-grip span {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
}

.status-button,
.icon-button {
  position: relative;
  display: grid;
  width: 30px;
  height: 30px;
  padding: 0;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #61756f;
  background: #ffffff;
  font: inherit;
  cursor: pointer;
}

.status-button {
  width: 34px;
  height: 34px;
  line-height: 1;
}

.power-symbol {
  position: relative;
  display: block;
  width: 14px;
  height: 14px;
  border: 1.8px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
}

.power-symbol::after {
  position: absolute;
  top: -4px;
  left: 50%;
  width: 2px;
  height: 7px;
  border-radius: 1px;
  background: currentColor;
  content: "";
  transform: translateX(-50%);
}

.status-button.enabled {
  border-color: #24745b;
  color: #ffffff;
  background: #24745b;
}

.status-button:hover:not(:disabled),
.status-button:focus-visible,
.icon-button:hover:not(:disabled),
.icon-button:focus-visible,
.icon-button.active {
  border-color: #8cbca8;
  color: #17613f;
  background: #edf5f1;
}

.status-button.enabled:hover:not(:disabled),
.status-button.enabled:focus-visible {
  border-color: #17613f;
  color: #ffffff;
  background: #17613f;
}

.status-button:disabled,
.icon-button:disabled {
  color: #72837e;
  background: #f1f5f3;
  cursor: not-allowed;
}

.status-button.busy span,
.icon-button.busy span {
  animation: subtle-pulse 1.1s ease-in-out infinite;
}

.name-editor {
  min-width: 0;
  gap: 7px;
}

.inline-value-button {
  min-width: 0;
  padding: 0;
  border: 0;
  color: inherit;
  background: transparent;
  font: inherit;
  text-align: left;
  cursor: text;
}

.inline-value-button:focus-visible {
  outline: 2px solid #8cbca8;
  outline-offset: 2px;
}

.inline-value-button:disabled {
  cursor: not-allowed;
}

.mod-name-value {
  overflow-wrap: anywhere;
  color: #17211f;
  font-weight: 750;
}

.note-value {
  display: block;
  width: 100%;
  overflow: hidden;
  color: #52645f;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.inline-editor {
  width: 100%;
  min-height: 34px;
  padding: 0 9px;
  border: 1px solid #8cbca8;
  border-radius: 5px;
  color: #17211f;
  background: #ffffff;
  font: inherit;
}

.inline-editor:focus {
  outline: 2px solid #c6e0d3;
  outline-offset: 0;
}

.category-editor {
  position: relative;
}

.category-tag-button {
  display: flex;
  width: 100%;
  min-height: 34px;
  max-height: 48px;
  gap: 4px;
  padding: 0;
  overflow: hidden;
  border: 0;
  background: transparent;
  cursor: pointer;
  flex-wrap: wrap;
  align-content: center;
  text-align: left;
}

.category-tag-button:focus-visible {
  outline: 2px solid #8cbca8;
  outline-offset: 2px;
}

.category-tag-button.active {
  outline: 2px solid #8cbca8;
  outline-offset: 2px;
}

.category-tag-button:disabled {
  cursor: not-allowed;
}

.category-tag {
  max-width: 100%;
  padding: 3px 6px;
  overflow: hidden;
  border: 1px solid #b9d8ca;
  border-radius: 4px;
  color: #17613f;
  background: #edf5f1;
  font-size: 0.72rem;
  font-weight: 700;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.category-tag.unknown-category-tag {
  border-color: #d7e1dd;
  color: #72837e;
  background: #f7faf8;
}

.category-menu {
  position: absolute;
  z-index: 8;
  top: calc(100% + 6px);
  left: -8px;
  display: grid;
  width: min(240px, 70vw);
  max-height: 280px;
  overflow: auto;
  padding: 6px;
  border: 1px solid #cbd8d4;
  border-radius: 6px;
  background: #ffffff;
  box-shadow: 0 12px 30px rgba(23, 33, 31, 0.16);
}

.category-option {
  display: flex;
  width: 100%;
  min-height: 34px;
  gap: 8px;
  align-items: center;
  padding: 5px 7px;
  border: 0;
  border-radius: 4px;
  color: #334b44;
  background: transparent;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.category-option:hover {
  background: #f0f6f3;
}

.category-checkbox {
  position: relative;
  flex: 0 0 auto;
  width: 15px;
  height: 15px;
  border: 1px solid #87938f;
  border-radius: 3px;
  background: #ffffff;
}

.category-checkbox.checked,
.category-checkbox.mixed {
  border-color: #24745b;
  background: #24745b;
}

.category-checkbox.checked::after {
  position: absolute;
  top: 2px;
  left: 4px;
  width: 4px;
  height: 7px;
  border: solid #ffffff;
  border-width: 0 2px 2px 0;
  content: "";
  transform: rotate(45deg);
}

.category-checkbox.mixed::after {
  position: absolute;
  top: 6px;
  left: 3px;
  width: 7px;
  height: 2px;
  background: #ffffff;
  content: "";
}

.empty-category-options {
  margin: 0;
  padding: 8px;
  color: #72837e;
  font-size: 0.78rem;
}

.category-menu-actions {
  display: flex;
  gap: 6px;
  justify-content: space-between;
  padding: 7px 3px 1px;
  border-top: 1px solid #e7eeeb;
}

.category-menu-actions button {
  min-height: 30px;
  padding: 0 8px;
  border: 1px solid #cbd8d4;
  border-radius: 4px;
  color: #315e52;
  background: #ffffff;
  font: inherit;
  font-size: 0.74rem;
  cursor: pointer;
}

.category-menu-actions button:hover:not(:disabled) {
  border-color: #8cbca8;
  background: #edf5f1;
}

.category-menu-actions button:disabled {
  color: #9aa8a4;
  cursor: not-allowed;
}

.conflict-indicator {
  display: grid;
  width: 18px;
  height: 18px;
  flex: none;
  place-items: center;
  border: 1px solid #c76a26;
  border-radius: 50%;
  color: #9a4b16;
  background: #fff8ed;
  font-size: 0.7rem;
  font-weight: 800;
  line-height: 1;
}

.partial-override-indicator {
  border-color: #d19a45;
  color: #8a5a13;
  background: #fff8eb;
}

.metadata-error {
  margin: 5px 0 0 29px;
  color: #b42318;
  font-size: 0.74rem;
  line-height: 1.35;
}

.replacement-summary {
  color: #334b44 !important;
  line-height: 1.4;
}

.replacement-summary.has-remap-entry {
  padding: 5px 7px;
}

.replacement-entry-button {
  position: relative;
  display: grid;
  width: 100%;
  min-height: 48px;
  grid-template-columns: minmax(0, 1fr) 30px;
  gap: 8px;
  align-items: center;
  padding: 5px 7px 5px 9px;
  border: 1px solid #c9dbd4;
  border-radius: 6px;
  color: #334b44;
  background: #f8fbfa;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.replacement-entry-button:hover:not(:disabled),
.replacement-entry-button:focus-visible {
  border-color: #78a995;
  background: #edf6f2;
}

.replacement-entry-button:disabled {
  border-color: #dce5e2;
  color: #61756f;
  background: #f4f7f6;
  cursor: not-allowed;
}

.replacement-entry-content {
  min-width: 0;
}

.replacement-entry-icon {
  display: grid !important;
  width: 28px;
  height: 28px;
  place-items: center;
  border-left: 1px solid #d5e2dd;
  color: #24745b;
  font-size: 1rem;
  font-weight: 700;
}

.replacement-summary span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.replacement-summary .replacement-more {
  display: inline;
  color: #61756f;
  font-size: 0.76rem;
  font-weight: 700;
}

.mod-actions {
  padding-right: 6px !important;
  padding-left: 6px !important;
  white-space: nowrap;
}

.mod-action-buttons {
  gap: 3px;
  justify-content: flex-start;
}

.icon-button {
  color: #24745b;
  font-size: 0.9rem;
}

.icon-button.danger-icon {
  color: #b42318;
  font-size: 1rem;
}

.icon-button[data-tooltip]::after {
  position: absolute;
  z-index: 5;
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

.mod-details-row:hover {
  background: #ffffff;
}

.mod-details-cell {
  padding: 0 !important;
  background: #fbfdfc;
}

.mod-details-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0;
  border-bottom: 1px solid #e7eeeb;
}

.mod-details-grid section,
.replacement-details,
.file-details {
  padding: 12px;
}

.mod-details-grid section {
  min-width: 0;
  border-right: 1px solid #e7eeeb;
  border-bottom: 1px solid #e7eeeb;
}

.mod-details-grid section:nth-child(3n) {
  border-right: 0;
}

.mod-details-grid h4,
.replacement-details h4 {
  margin: 0 0 5px;
  color: #61756f;
  font-size: 0.74rem;
}

.replacement-heading {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 28px;
  margin-bottom: 4px;
}

.replacement-heading h4 {
  margin: 0;
}

.remap-count {
  color: #17613f;
  font-size: 0.72rem;
  font-weight: 700;
}

.detail-icon-button {
  position: relative;
  display: inline-grid;
  width: 28px;
  height: 28px;
  margin-left: auto;
  padding: 0;
  place-items: center;
  border: 1px solid #cbd8d4;
  border-radius: 6px;
  color: #315e52;
  background: #ffffff;
  font-size: 1rem;
}

.detail-icon-button:hover:not(:disabled),
.detail-icon-button:focus-visible {
  border-color: #78a99a;
  background: #eef7f3;
}

.mod-details-grid p,
.replacement-details p {
  margin: 0;
  color: #24332f;
  font-size: 0.8rem;
  line-height: 1.45;
  overflow-wrap: anywhere;
}

.mod-details-grid .detail-command {
  min-height: 34px;
  margin-top: 8px;
  padding: 0 10px;
  border: 1px solid #b8cec5;
  border-radius: 4px;
  color: #286b55;
  background: #fff;
  font: inherit;
  font-size: 0.82rem;
  font-weight: 700;
  cursor: pointer;
}

.detail-path {
  margin-top: 4px !important;
  color: #72837e !important;
  font-size: 0.72rem !important;
}

.replacement-details {
  border-bottom: 1px solid #e7eeeb;
}

.replacement-details ul,
.file-details ul {
  display: grid;
  gap: 7px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.replacement-details li {
  display: grid;
  gap: 2px;
  padding: 8px 0;
  border-bottom: 1px solid #edf1f0;
}

.replacement-details li:last-child {
  border-bottom: 0;
}

.replacement-details strong {
  color: #24332f;
  font-size: 0.82rem;
}

.replacement-details span,
.replacement-details small {
  color: #52645f;
  font-size: 0.76rem;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.file-details {
  color: #52645f;
  font-size: 0.78rem;
}

.file-details summary {
  color: #435650;
  font-weight: 700;
  cursor: pointer;
}

.file-details ul {
  margin-top: 10px;
}

.file-details li {
  overflow-wrap: anywhere;
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

@media (max-width: 980px) {
  .batch-toolbar {
    align-items: flex-start;
    flex-direction: column;
  }

  .batch-actions {
    flex-wrap: wrap;
  }
}

@media (max-width: 760px) {
  .mod-details-grid {
    grid-template-columns: 1fr;
  }

  .mod-details-grid section,
  .mod-details-grid section:nth-child(3n) {
    border-right: 0;
  }
}
</style>
