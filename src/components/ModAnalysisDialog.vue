<script setup lang="ts">
import { computed, ref } from "vue";
import type { ModAnalysisReport } from "../api/modAnalysis";

const props = defineProps<{
  modName: string;
  report: ModAnalysisReport | null;
  loading: boolean;
  error: string;
}>();

defineEmits<{
  close: [];
  retry: [];
}>();

const query = ref("");
const visibleFileLimit = 200;

const filesById = computed(() =>
  new Map((props.report?.files ?? []).map((file) => [file.fileId, file])),
);

const filteredFiles = computed(() => {
  const normalized = query.value.trim().toLocaleLowerCase();
  if (!normalized) {
    return props.report?.files ?? [];
  }
  return (props.report?.files ?? []).filter((file) =>
    [
      file.effectiveDeployRelativePath,
      file.roleLabel,
      file.componentLabel,
      ...file.replacementTargets,
      ...file.references,
    ].some((value) => value.toLocaleLowerCase().includes(normalized)),
  );
});

const visibleFiles = computed(() => filteredFiles.value.slice(0, visibleFileLimit));

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function confidenceLabel(value: number) {
  if (value >= 0.95) return "已解析";
  if (value >= 0.75) return "高可信";
  if (value >= 0.5) return "路径推断";
  return "待确认";
}

function edgeSourcePath(fileId: string) {
  return filesById.value.get(fileId)?.effectiveDeployRelativePath ?? fileId;
}
</script>

<template>
  <div class="analysis-backdrop" role="presentation" @click.self="$emit('close')">
    <section class="analysis-dialog" role="dialog" aria-modal="true" aria-labelledby="analysis-title">
      <header>
        <div>
          <p>MOD 文件分析</p>
          <h2 id="analysis-title">{{ modName }}</h2>
        </div>
        <button type="button" class="close-button" aria-label="关闭" data-tooltip="关闭" @click="$emit('close')">
          ×
        </button>
      </header>

      <div v-if="loading" class="analysis-state">
        <strong>正在分析文件结构</strong>
        <p>首次分析会计算内容指纹，进度显示在程序顶部。</p>
      </div>
      <div v-else-if="error" class="analysis-state error">
        <strong>分析失败</strong>
        <p>{{ error }}</p>
        <button type="button" @click="$emit('retry')">重新分析</button>
      </div>

      <div v-else-if="report" class="analysis-content">
        <section class="summary-strip" aria-label="分析概况">
          <div><span>文件</span><strong>{{ report.fileCount }}</strong></div>
          <div><span>已识别</span><strong>{{ report.recognizedFileCount }}</strong></div>
          <div><span>待确认</span><strong>{{ report.unknownFileCount }}</strong></div>
          <div><span>资源组件</span><strong>{{ report.componentCount }}</strong></div>
          <div><span>总大小</span><strong>{{ formatBytes(report.totalSizeBytes) }}</strong></div>
        </section>

        <p class="analysis-message">
          {{ report.message }}
          <span v-if="report.cacheHit">本次使用已有分析结果。</span>
        </p>

        <section class="analysis-section">
          <h3>资源组件</h3>
          <div class="component-table">
            <div v-for="component in report.components" :key="component.componentId" class="component-row">
              <strong>{{ component.label }}</strong>
              <span>{{ component.roles.join("、") || "待确认" }}</span>
              <span>{{ component.fileCount }} 个文件</span>
              <small v-if="component.replacementTargets.length">
                {{ component.replacementTargets.join("、") }}
              </small>
            </div>
          </div>
        </section>

        <section class="analysis-section">
          <div class="section-heading">
            <h3>文件作用</h3>
            <input v-model="query" type="search" placeholder="筛选路径、作用或替换目标" />
          </div>
          <div class="file-table" role="table">
            <div class="file-row file-header" role="row">
              <span>文件路径</span><span>作用</span><span>组件</span><span>依据</span>
            </div>
            <div v-for="file in visibleFiles" :key="file.fileId" class="file-row" role="row">
              <div class="file-path">
                <code>{{ file.effectiveDeployRelativePath }}</code>
                <small v-if="file.sourceDeployRelativePath !== file.effectiveDeployRelativePath">
                  原路径：{{ file.sourceDeployRelativePath }}
                </small>
                <small v-if="file.excludedFromDeployment">已排除部署</small>
              </div>
              <div>
                <strong>{{ file.roleLabel }}</strong>
                <small v-if="file.replacementTargets.length">{{ file.replacementTargets.join("、") }}</small>
              </div>
              <span>{{ file.componentLabel }}</span>
              <details>
                <summary>{{ confidenceLabel(file.confidence) }}</summary>
                <ul>
                  <li v-for="item in file.evidence" :key="`${item.kind}-${item.detail}`">{{ item.detail }}</li>
                </ul>
                <p v-if="file.references.length">引用：{{ file.references.join("、") }}</p>
              </details>
            </div>
          </div>
          <p v-if="filteredFiles.length > visibleFileLimit" class="limit-note">
            当前显示前 {{ visibleFileLimit }} 项，请使用筛选框缩小范围。
          </p>
          <p v-else-if="!filteredFiles.length" class="limit-note">没有符合条件的文件。</p>
        </section>

        <section v-if="report.edges.length" class="analysis-section">
          <h3>资源依赖</h3>
          <ul class="dependency-list">
            <li v-for="edge in report.edges.slice(0, 200)" :key="`${edge.fromFileId}-${edge.relation}-${edge.targetReference}`">
              <code>{{ edgeSourcePath(edge.fromFileId) }}</code>
              <strong>{{ edge.relationLabel }}</strong>
              <code>{{ edge.toFileId ? edgeSourcePath(edge.toFileId) : edge.targetReference }}</code>
              <small>{{ edge.evidence }}</small>
            </li>
          </ul>
          <p v-if="report.edges.length > 200" class="limit-note">依赖较多，当前显示前 200 条。</p>
        </section>

        <section v-if="report.knowledgeEvidence.length" class="analysis-section">
          <h3>技术资料</h3>
          <ul class="knowledge-list">
            <li v-for="item in report.knowledgeEvidence" :key="`${item.packId}-${item.resultId}`">
              <strong>{{ item.title }}</strong>
              <span>{{ item.snippet }}</span>
              <small>{{ item.sourceTitle || "知识包资料" }} · 游戏版本 {{ item.gameVersion }} · 包 {{ item.packVersion }}</small>
            </li>
          </ul>
        </section>

        <details v-if="report.warnings.length" class="warnings">
          <summary>查看 {{ report.warnings.length }} 条提示</summary>
          <ul><li v-for="warning in report.warnings" :key="warning">{{ warning }}</li></ul>
        </details>
      </div>
    </section>
  </div>
</template>

<style scoped>
.analysis-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgb(22 37 32 / 34%);
}

.analysis-dialog {
  width: min(1180px, 96vw);
  max-height: 92vh;
  overflow: auto;
  border: 1px solid #bfcfc9;
  border-radius: 6px;
  background: #fff;
  box-shadow: 0 18px 52px rgb(20 49 39 / 20%);
}

.analysis-dialog > header {
  position: sticky;
  top: 0;
  z-index: 2;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 22px;
  border-bottom: 1px solid #dce6e2;
  background: #fff;
}

.analysis-dialog header p,
.analysis-dialog header h2,
.analysis-message,
.analysis-state p,
.limit-note {
  margin: 0;
}

.analysis-dialog header p {
  color: #35745f;
  font-size: 0.82rem;
  font-weight: 700;
}

.analysis-dialog header h2 {
  margin-top: 4px;
  font-size: 1.25rem;
  letter-spacing: 0;
}

.close-button {
  width: 38px;
  height: 38px;
  border: 1px solid #c8d5d0;
  border-radius: 4px;
  color: #375b50;
  background: #fff;
  font: inherit;
  font-size: 1.45rem;
  line-height: 1;
  cursor: pointer;
}

.analysis-state,
.analysis-content {
  padding: 22px;
}

.analysis-state {
  min-height: 240px;
  display: grid;
  place-content: center;
  gap: 10px;
  text-align: center;
  color: #536b63;
}

.analysis-state.error strong,
.warnings {
  color: #9d493a;
}

.analysis-state button {
  justify-self: center;
  min-height: 38px;
  padding: 0 14px;
  border: 1px solid #afc9bf;
  border-radius: 4px;
  color: #266b54;
  background: #fff;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.summary-strip {
  display: grid;
  grid-template-columns: repeat(5, minmax(100px, 1fr));
  border: 1px solid #d6e2de;
}

.summary-strip div {
  display: grid;
  gap: 3px;
  padding: 11px 14px;
  border-right: 1px solid #d6e2de;
}

.summary-strip div:last-child {
  border-right: 0;
}

.summary-strip span,
.component-row span,
.component-row small,
.file-row small,
.dependency-list small,
.knowledge-list small,
.limit-note {
  color: #60736d;
  font-size: 0.8rem;
}

.analysis-message {
  margin-top: 12px;
  color: #536b63;
}

.analysis-section {
  margin-top: 24px;
}

.analysis-section h3 {
  margin: 0 0 10px;
  font-size: 1rem;
  letter-spacing: 0;
}

.component-table,
.file-table {
  border: 1px solid #d7e2de;
}

.component-row {
  display: grid;
  grid-template-columns: minmax(180px, 1.4fr) minmax(180px, 1fr) 100px minmax(180px, 1fr);
  align-items: center;
  gap: 12px;
  padding: 9px 12px;
  border-bottom: 1px solid #e2e9e6;
}

.component-row:last-child,
.file-row:last-child {
  border-bottom: 0;
}

.section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  margin-bottom: 10px;
}

.section-heading h3 {
  margin: 0;
}

.section-heading input {
  width: min(420px, 50%);
  min-height: 38px;
  padding: 0 10px;
  border: 1px solid #bdccc7;
  border-radius: 4px;
  color: #203b34;
  background: #fff;
  font: inherit;
}

.file-row {
  display: grid;
  grid-template-columns: minmax(280px, 2fr) minmax(140px, 0.8fr) minmax(160px, 1fr) minmax(100px, 0.6fr);
  gap: 12px;
  align-items: start;
  padding: 10px 12px;
  border-bottom: 1px solid #e2e9e6;
}

.file-header {
  color: #50665f;
  background: #f7faf9;
  font-size: 0.82rem;
  font-weight: 700;
}

.file-path,
.file-row > div,
.knowledge-list li {
  display: grid;
  gap: 4px;
  min-width: 0;
}

code {
  overflow-wrap: anywhere;
  color: #294c41;
  font-family: Consolas, monospace;
  font-size: 0.8rem;
}

.file-row details summary,
.warnings summary {
  cursor: pointer;
  color: #2a7159;
}

.file-row details ul,
.file-row details p,
.warnings ul {
  margin: 7px 0 0;
  padding-left: 18px;
  color: #536b63;
  font-size: 0.8rem;
}

.dependency-list,
.knowledge-list {
  margin: 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid #dce6e2;
}

.dependency-list li {
  display: grid;
  grid-template-columns: minmax(220px, 1fr) 90px minmax(220px, 1fr) minmax(140px, 0.7fr);
  gap: 10px;
  align-items: center;
  padding: 9px 0;
  border-bottom: 1px solid #e2e9e6;
}

.knowledge-list li {
  padding: 10px 0;
  border-bottom: 1px solid #e2e9e6;
}

.limit-note,
.warnings {
  margin-top: 10px;
}

@media (max-width: 860px) {
  .analysis-backdrop {
    padding: 8px;
  }

  .summary-strip,
  .component-row,
  .file-row,
  .dependency-list li {
    grid-template-columns: 1fr;
  }

  .summary-strip div {
    border-right: 0;
    border-bottom: 1px solid #d6e2de;
  }

  .summary-strip div:last-child {
    border-bottom: 0;
  }

  .section-heading {
    align-items: stretch;
    flex-direction: column;
  }

  .section-heading input {
    width: 100%;
  }

  .file-header {
    display: none;
  }
}
</style>
