<script setup lang="ts">
import { computed } from "vue";
import type { OperationProgress } from "../api/operations";

const props = defineProps<{
  operation: OperationProgress | null;
}>();

const hasKnownTotal = computed(
  () => props.operation?.total !== null && props.operation?.total !== undefined,
);

const completionPercent = computed(() => {
  if (!props.operation?.total || props.operation.total <= 0) {
    return 0;
  }

  return Math.min(100, Math.round((props.operation.completed / props.operation.total) * 100));
});

const elapsedLabel = computed(() => {
  const elapsedSeconds = Math.max(0, Math.floor((props.operation?.elapsedMillis ?? 0) / 1000));
  const minutes = Math.floor(elapsedSeconds / 60);
  const seconds = elapsedSeconds % 60;

  return minutes > 0 ? `${minutes} 分 ${seconds} 秒` : `${seconds} 秒`;
});
</script>

<template>
  <section v-if="operation" class="operation-status" aria-live="polite">
    <div class="operation-copy">
      <strong>{{ operation.title }}</strong>
      <span>{{ operation.phase }}</span>
      <small v-if="operation.currentItem" :title="operation.currentItem">
        {{ operation.currentItem }}
      </small>
    </div>

    <div class="operation-progress">
      <div v-if="hasKnownTotal" class="progress-track" aria-hidden="true">
        <span :style="{ width: `${completionPercent}%` }" />
      </div>
      <span v-if="hasKnownTotal" class="progress-count">
        {{ operation.completed }} / {{ operation.total }}
      </span>
      <span class="elapsed">{{ elapsedLabel }}</span>
    </div>
  </section>
</template>

<style scoped>
.operation-status {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  padding: 10px 24px;
  border-bottom: 1px solid #bfddcf;
  background: #edf7f1;
}

.operation-copy,
.operation-progress {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
}

.operation-copy strong {
  flex: 0 0 auto;
  color: #175c45;
  font-size: 0.84rem;
}

.operation-copy span {
  flex: 0 0 auto;
  color: #2f6653;
  font-size: 0.82rem;
}

.operation-copy small {
  overflow: hidden;
  color: #547268;
  font-size: 0.78rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.operation-progress {
  flex: 0 0 auto;
}

.progress-track {
  width: 124px;
  height: 6px;
  overflow: hidden;
  border-radius: 3px;
  background: #cfe4d8;
}

.progress-track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: #24745b;
  transition: width 120ms ease-out;
}

.progress-count,
.elapsed {
  color: #547268;
  font-size: 0.78rem;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

@media (max-width: 760px) {
  .operation-status {
    align-items: flex-start;
    flex-direction: column;
    gap: 8px;
    padding: 10px 16px;
  }

  .operation-copy {
    width: 100%;
  }

  .operation-progress {
    width: 100%;
  }

  .progress-track {
    flex: 1;
  }
}
</style>
