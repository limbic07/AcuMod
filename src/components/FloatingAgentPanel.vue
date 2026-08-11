<script setup lang="ts">
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDirectoryDialog } from "@tauri-apps/plugin-dialog";
import MarkdownIt from "markdown-it";
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  cancelAgentActionPlan,
  clearAgentSession,
  confirmAgentActionPlan,
  createAgentCleanupPlan,
  getAgentSettings,
  startAgentTurn,
  type AgentActionPlan,
  type AgentActionResult,
  type AgentCleanupReview,
  type AgentCleanupReviewItem,
  type AgentEvent,
  type AgentKnowledgeEvidence,
  type AgentSettings,
} from "../api/agent";
import { installModFromArchive, openModCleanupCandidateFolder } from "../api/modLibrary";
import type { ModArchiveImportOutcome } from "../api/modLibrary";
import {
  listenDownloadWatch,
  startDownloadWatch,
  type DownloadWatchEvent,
} from "../api/downloadWatch";

interface ChatMessage {
  id: number;
  role: "user" | "assistant";
  text: string;
  complete: boolean;
  renderedHtml: string;
  actionPlans: VisibleActionPlan[];
  cleanupReviews: VisibleCleanupReview[];
  knowledgeEvidence: AgentKnowledgeEvidence[];
}

interface VisibleActionPlan {
  plan: AgentActionPlan;
  status: "pending" | "executing" | "completed" | "partiallyFailed" | "cancelled" | "failed";
  result: AgentActionResult | null;
  error: string;
}

interface VisibleCleanupReview {
  review: AgentCleanupReview;
  selectedCandidateIds: string[];
  status: "selecting" | "creating" | "planned" | "failed";
  error: string;
}

interface DetectedDownload {
  watchId: string;
  sourceUrl: string;
  filePath: string;
  fileName: string;
  sizeBytes: number;
  error: string;
}

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  openPanel: [];
  close: [];
  openSettings: [];
  workspaceChanged: [];
  archiveImportReady: [outcome: ModArchiveImportOutcome];
}>();

const settings = ref<AgentSettings | null>(null);
const messages = ref<ChatMessage[]>([]);
const input = ref("");
const statusMessage = ref("");
const error = ref("");
const isLoadingSettings = ref(false);
const isSending = ref(false);
const activePlanId = ref("");
const activeCleanupReviewId = ref("");
const openingCleanupCandidateId = ref("");
const downloadWatchDirectory = ref("");
const detectedDownloads = ref<DetectedDownload[]>([]);
const importingDownloadWatchId = ref("");
const messageList = ref<HTMLElement | null>(null);
const shouldAutoScroll = ref(true);
let nextMessageId = 1;
let stopDownloadWatchListener: (() => void) | undefined;
const markdown = new MarkdownIt({
  html: false,
  breaks: true,
  linkify: false,
  typographer: false,
});

// AI 回答不加载远程图片，避免 Markdown 内容在用户不知情时发起额外网络请求。
markdown.renderer.rules.image = (tokens, index) =>
  markdown.utils.escapeHtml(tokens[index]?.content ?? "");

function visibleError(value: unknown) {
  return value instanceof Error ? value.message : String(value);
}

function renderedMarkdown(message: ChatMessage) {
  return message.renderedHtml || markdown.render(message.text || "正在整理回答...");
}

function renderAssistantMarkdown(message: ChatMessage) {
  try {
    message.renderedHtml = markdown.render(message.text || "正在整理回答...");
  } catch {
    message.renderedHtml = "";
    error.value = "AcuAI 回答的 Markdown 渲染失败，已保留原始文本。";
  }
}

async function openMarkdownLink(event: MouseEvent) {
  const target = event.target instanceof Element ? event.target.closest("a") : null;
  const href = target?.getAttribute("href");
  if (!href) {
    return;
  }

  event.preventDefault();
  try {
    const url = new URL(href);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      throw new Error("不支持的链接协议");
    }
    // 知识库证据链接只用于阅读资料，不应被误当成下载入口创建监听会话。
    const isKnowledgeEvidenceLink = Boolean(target?.closest(".knowledge-evidence-card"));
    if (!isKnowledgeEvidenceLink && isModSourceUrl(url)) {
      await openModSourceAndWaitForDownload(url.toString());
    } else {
      await openUrl(url.toString());
    }
  } catch {
    error.value = "无法打开回答中的链接。";
  }
}

function isModSourceUrl(url: URL) {
  return new Set([
    "nexusmods.com",
    "www.nexusmods.com",
    "moddb.com",
    "www.moddb.com",
    "github.com",
    "www.curseforge.com",
    "caimogu.cc",
    "www.caimogu.cc",
    "caimogu.org",
    "www.caimogu.org",
    "mod.3dmgame.com",
    "dl.3dmgame.com",
  ]).has(url.hostname.toLowerCase());
}

/** 下载发现卡片只展示来源域名，避免较长链接挤占操作区域。 */
function downloadSourceLabel(sourceUrl: string) {
  try {
    return new URL(sourceUrl).hostname;
  } catch {
    return "来源页面";
  }
}

async function openModSourceAndWaitForDownload(sourceUrl: string) {
  if (!downloadWatchDirectory.value) {
    const selected = await openDirectoryDialog({
      title: "选择浏览器下载 MOD 使用的目录",
      directory: true,
      multiple: false,
    });
    if (typeof selected !== "string") {
      return;
    }
    downloadWatchDirectory.value = selected;
  }
  const watch = await startDownloadWatch(downloadWatchDirectory.value, sourceUrl);
  statusMessage.value = watch.message;
  await openUrl(sourceUrl);
}

function handleDownloadWatch(event: DownloadWatchEvent) {
  if (event.status === "found" && event.filePath && event.fileName && event.sizeBytes !== null) {
    // 同一文件可能被两个来源点击会话同时发现；文件路径才是用户需要确认的唯一对象。
    if (!detectedDownloads.value.some((item) => item.filePath === event.filePath)) {
      detectedDownloads.value.push({
        watchId: event.watchId,
        sourceUrl: event.sourceUrl,
        filePath: event.filePath,
        fileName: event.fileName,
        sizeBytes: event.sizeBytes,
        error: "",
      });
    }
    statusMessage.value = event.message;
  } else {
    statusMessage.value = event.message;
  }
  void scrollToLatest();
}

function dismissDetectedDownloads() {
  // 仅移除界面提示，绝不触碰浏览器下载文件或 MOD 库内容。
  detectedDownloads.value = [];
  statusMessage.value = "已关闭下载文件提示。";
}

async function importDetectedDownload(download: DetectedDownload) {
  if (importingDownloadWatchId.value) {
    return;
  }
  importingDownloadWatchId.value = download.watchId;
  download.error = "";
  try {
    const outcome = await installModFromArchive(download.filePath, false);
    detectedDownloads.value = detectedDownloads.value.filter(
      (item) => item.watchId !== download.watchId,
    );
    if (outcome.status === "ambiguous") {
      statusMessage.value = "已识别下载归档，需要选择导入分支，请在导入页面继续。";
      emit("archiveImportReady", outcome);
    } else {
      const modName = outcome.installResult?.name ?? download.fileName;
      statusMessage.value =
        outcome.status === "alreadyInstalled" ? `MOD 已在库中：${modName}` : `已成功导入 MOD：${modName}`;
      emit("workspaceChanged");
    }
  } catch (value) {
    download.error = visibleError(value);
  } finally {
    importingDownloadWatchId.value = "";
  }
}

function handleMessageListScroll(event: Event) {
  const list = event.currentTarget as HTMLElement;
  // 用户主动上滑阅读时，不再让后续流式片段抢回滚动位置。
  shouldAutoScroll.value = list.scrollHeight - list.scrollTop - list.clientHeight <= 24;
}

async function scrollToLatest(force = false) {
  await nextTick();
  if (messageList.value && (force || shouldAutoScroll.value)) {
    messageList.value.scrollTop = messageList.value.scrollHeight;
  }
}

async function loadSettings() {
  isLoadingSettings.value = true;
  error.value = "";
  try {
    settings.value = await getAgentSettings();
  } catch (value) {
    error.value = visibleError(value);
  } finally {
    isLoadingSettings.value = false;
  }
}

function applyAgentEvent(event: AgentEvent, assistant: ChatMessage) {
  if (event.kind === "textReset") {
    assistant.text = "";
    assistant.renderedHtml = "";
  }
  if (event.kind === "textDelta" && event.text) {
    assistant.text += event.text;
    void scrollToLatest();
  }
  if (event.kind === "started" || event.kind === "toolStarted" || event.kind === "toolFinished") {
    statusMessage.value = event.message ?? "";
  }
  if (event.kind === "completed") {
    statusMessage.value = "";
  }
  if (event.kind === "planReady" && event.plan) {
    const known = assistant.actionPlans.some((item) => item.plan.planId === event.plan?.planId);
    if (!known) {
      // 计划属于生成它的这一轮回复，后续对话不会把历史计划挤到列表末尾。
      assistant.actionPlans.push({
        plan: event.plan,
        status: "pending",
        result: null,
        error: "",
      });
      void scrollToLatest();
    }
  }
  if (event.kind === "cleanupReviewReady" && event.cleanupReview) {
    const known = assistant.cleanupReviews.some(
      (item) => item.review.reviewId === event.cleanupReview?.reviewId,
    );
    if (!known) {
      assistant.cleanupReviews.push({
        review: event.cleanupReview,
        selectedCandidateIds: event.cleanupReview.items
          .filter((item) => item.selectedByDefault)
          .map((item) => item.candidateId),
        status: "selecting",
        error: "",
      });
      void scrollToLatest();
    }
  }
  if (event.kind === "knowledgeEvidenceReady") {
    const known = new Set(assistant.knowledgeEvidence.map((item) => item.evidenceId));
    for (const item of event.knowledgeEvidence) {
      if (!known.has(item.evidenceId)) {
        assistant.knowledgeEvidence.push(item);
      }
    }
    void scrollToLatest();
  }
  if (event.kind === "failed") {
    error.value = event.message ?? "AcuAI 回答失败。";
    statusMessage.value = "";
  }
}

async function sendMessage() {
  const message = input.value.trim();
  if (!message || isSending.value) {
    return;
  }

  await loadSettings();
  if (!settings.value?.apiKeyConfigured) {
    error.value = "请先在设置中配置 DeepSeek 访问密钥。";
    return;
  }

  const userMessage: ChatMessage = {
    id: nextMessageId++,
    role: "user",
    text: message,
    complete: true,
    renderedHtml: "",
    actionPlans: [],
    cleanupReviews: [],
    knowledgeEvidence: [],
  };
  const assistantMessage: ChatMessage = {
    id: nextMessageId++,
    role: "assistant",
    text: "",
    complete: false,
    renderedHtml: "",
    actionPlans: [],
    cleanupReviews: [],
    knowledgeEvidence: [],
  };
  messages.value.push(userMessage, assistantMessage);
  input.value = "";
  error.value = "";
  statusMessage.value = "正在连接 DeepSeek V4";
  isSending.value = true;
  await scrollToLatest(true);

  try {
    const result = await startAgentTurn(message, (event) => {
      applyAgentEvent(event, assistantMessage);
    });
    if (!assistantMessage.text.trim()) {
      assistantMessage.text = result.message;
    }
  } catch (value) {
    if (!error.value) {
      error.value = visibleError(value);
    }
  } finally {
    isSending.value = false;
    statusMessage.value = "";
    if (!assistantMessage.text.trim() && error.value) {
      messages.value = messages.value.filter((item) => item.id !== assistantMessage.id);
    } else {
      renderAssistantMarkdown(assistantMessage);
      assistantMessage.complete = true;
    }
    await scrollToLatest();
  }
}

async function sendQuickPrompt(prompt: string) {
  if (isSending.value) {
    return;
  }
  input.value = prompt;
  await sendMessage();
}

function cleanupReviewGroups(review: AgentCleanupReview) {
  const groups = new Map<string, { modId: string; modName: string; items: AgentCleanupReviewItem[] }>();
  for (const item of review.items.filter((item) => item.recommendation !== "keep")) {
    const group = groups.get(item.modId) ?? {
      modId: item.modId,
      modName: item.modName,
      items: [],
    };
    group.items.push(item);
    groups.set(item.modId, group);
  }
  return [...groups.values()];
}

function cleanupRecommendationLabel(item: AgentCleanupReviewItem) {
  if (item.recommendation === "remove") {
    return "建议清理";
  }
  if (item.recommendation === "review") {
    return "需要确认";
  }
  return "建议保留";
}

function cleanupDecisionSourceLabel(item: AgentCleanupReviewItem) {
  return item.decisionSource === "localRule" ? "本地规则" : "AcuAI";
}

function cleanupRiskLabel(item: AgentCleanupReviewItem) {
  if (item.riskLevel === "high") {
    return "高风险";
  }
  if (item.riskLevel === "medium") {
    return "需谨慎";
  }
  return "低风险";
}

function knowledgeEvidenceMeta(item: AgentKnowledgeEvidence) {
  const tier = {
    localVerified: "本地数值资料",
    localReference: "本地参考资料",
    localAnalysis: "本地文件分析",
    webReference: "联网参考资料",
  }[item.sourceTier];
  return `${tier} · ${item.sourceTitle || item.packId} · ${item.gameVersion} · 可信度 ${Math.round(item.confidence * 100)}%`;
}

function formatFileSize(sizeBytes: number) {
  if (sizeBytes < 1024) {
    return `${sizeBytes} B`;
  }
  if (sizeBytes < 1024 * 1024) {
    return `${(sizeBytes / 1024).toFixed(1)} KB`;
  }
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`;
}

function updateCleanupSelection(review: VisibleCleanupReview, candidateId: string, selected: boolean) {
  const selectedIds = new Set(review.selectedCandidateIds);
  if (selected) {
    selectedIds.add(candidateId);
  } else {
    selectedIds.delete(candidateId);
  }
  review.selectedCandidateIds = [...selectedIds];
}

function updateCleanupSelectionFromEvent(
  review: VisibleCleanupReview,
  candidateId: string,
  event: Event,
) {
  const target = event.target;
  updateCleanupSelection(
    review,
    candidateId,
    target instanceof HTMLInputElement && target.checked,
  );
}

function cleanupReviewStatusLabel(review: VisibleCleanupReview) {
  if (review.status === "creating") {
    return "正在生成计划";
  }
  if (review.status === "planned") {
    return "已生成计划";
  }
  if (review.status === "failed") {
    return "生成失败";
  }
  return "待选择";
}

async function createCleanupPlan(message: ChatMessage, review: VisibleCleanupReview) {
  if (
    isSending.value
    || activeCleanupReviewId.value
    || review.status !== "selecting"
    || review.selectedCandidateIds.length === 0
  ) {
    return;
  }
  activeCleanupReviewId.value = review.review.reviewId;
  review.status = "creating";
  review.error = "";
  try {
    const plan = await createAgentCleanupPlan(
      review.review.reviewId,
      review.selectedCandidateIds,
    );
    message.actionPlans.push({
      plan,
      status: "pending",
      result: null,
      error: "",
    });
    review.status = "planned";
    await scrollToLatest();
  } catch (value) {
    review.status = "failed";
    review.error = visibleError(value);
  } finally {
    activeCleanupReviewId.value = "";
  }
}

async function openCleanupCandidateFolder(
  review: VisibleCleanupReview,
  item: AgentCleanupReviewItem,
) {
  if (openingCleanupCandidateId.value) {
    return;
  }
  openingCleanupCandidateId.value = item.candidateId;
  review.error = "";
  try {
    await openModCleanupCandidateFolder(item.modId, item.candidateId);
  } catch (value) {
    review.error = visibleError(value);
  } finally {
    openingCleanupCandidateId.value = "";
  }
}

async function clearConversation() {
  if (isSending.value) {
    return;
  }
  error.value = "";
  try {
    await clearAgentSession();
    messages.value = [];
  } catch (value) {
    error.value = visibleError(value);
  }
}

function planExpiryLabel(plan: AgentActionPlan) {
  return new Date(plan.expiresAtUnixSeconds * 1000).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

async function confirmPlan(item: VisibleActionPlan) {
  if (activePlanId.value || item.status !== "pending") {
    return;
  }
  activePlanId.value = item.plan.planId;
  item.status = "executing";
  item.error = "";
  error.value = "";
  try {
    const result = await confirmAgentActionPlan(item.plan.planId);
    item.result = result;
    item.status = result.status;
    if (result.archiveImport?.status === "ambiguous") {
      emit("archiveImportReady", result.archiveImport);
    } else {
      emit("workspaceChanged");
    }
  } catch (value) {
    // 后端计划在确认时即被消费；状态漂移或业务失败后必须重新生成，不能重复提交旧计划。
    item.status = "failed";
    item.error = visibleError(value);
  } finally {
    activePlanId.value = "";
  }
}

async function cancelPlan(item: VisibleActionPlan) {
  if (activePlanId.value || item.status !== "pending") {
    return;
  }
  activePlanId.value = item.plan.planId;
  item.error = "";
  try {
    const result = await cancelAgentActionPlan(item.plan.planId);
    item.result = result;
    item.status = "cancelled";
  } catch (value) {
    item.error = visibleError(value);
  } finally {
    activePlanId.value = "";
  }
}

function openSettings() {
  emit("openSettings");
  emit("close");
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      void loadSettings();
      void scrollToLatest();
    }
  },
  { immediate: true },
);

onMounted(() => {
  void listenDownloadWatch(handleDownloadWatch)
    .then((unlisten) => {
      stopDownloadWatchListener = unlisten;
    })
    .catch(() => {
      // Browser-only Vite development has no Tauri event API.
    });
});

onBeforeUnmount(() => {
  stopDownloadWatchListener?.();
});
</script>

<template>
  <button
    v-if="!props.open"
    type="button"
    class="agent-launcher"
    title="打开 AcuAI"
    aria-label="打开 AcuAI"
    @click="$emit('openPanel')"
  >
    AcuAI
  </button>

  <aside v-else class="agent-panel" aria-label="AcuAI">
    <header>
      <div>
        <p>Acumen MOD Manager</p>
        <h2>AcuAI</h2>
      </div>
      <div class="header-actions">
        <button type="button" title="清空当前对话" :disabled="isSending" @click="clearConversation">
          清空
        </button>
        <button
          type="button"
          class="close-button"
          title="关闭 AcuAI"
          aria-label="关闭 AcuAI"
          @click="$emit('close')"
        >
          ×
        </button>
      </div>
    </header>

    <div v-if="isLoadingSettings && !settings" class="agent-empty">正在读取 AcuAI 设置...</div>
    <div v-else-if="!settings?.apiKeyConfigured" class="agent-empty">
      <strong>尚未配置 DeepSeek 访问密钥</strong>
      <span>配置后可查询本地 MOD、冲突和 MHW 游戏术语。</span>
      <button type="button" @click="openSettings">前往设置</button>
    </div>
    <template v-else>
      <section v-if="detectedDownloads.length" class="download-watch-card">
        <header class="download-watch-card__heading">
          <strong>发现浏览器下载文件</strong>
          <button
            type="button"
            class="download-watch-card__close"
            title="关闭下载文件提示"
            aria-label="关闭下载文件提示"
            @click="dismissDetectedDownloads"
          >
            ×
          </button>
        </header>
        <div v-for="download in detectedDownloads" :key="download.watchId">
          <span>
            {{ download.fileName }} · {{ formatFileSize(download.sizeBytes) }}
            · 来源：{{ downloadSourceLabel(download.sourceUrl) }}
          </span>
          <button
            type="button"
            :disabled="Boolean(importingDownloadWatchId)"
            @click="importDetectedDownload(download)"
          >
            {{ importingDownloadWatchId === download.watchId ? "正在导入" : "导入此文件" }}
          </button>
          <p v-if="download.error" class="plan-error">{{ download.error }}</p>
        </div>
      </section>
      <div ref="messageList" class="message-list" aria-live="polite" @scroll.passive="handleMessageListScroll">
        <div v-if="messages.length === 0" class="welcome-message">
          <strong>DeepSeek V4 已就绪</strong>
          <span>可以查询本地 MOD，也可以直接描述想要的 MOD，由助手联网搜索。</span>
          <div class="agent-quick-actions">
            <button type="button" @click="sendQuickPrompt('扫描全部已安装 MOD 中不影响 MOD 生效的冗余文件')">
              扫描可清理文件
            </button>
            <button type="button" @click="sendQuickPrompt('查看当前清理记录，并帮我恢复最近一次清理')">
              恢复最近清理
            </button>
          </div>
        </div>
        <template v-for="message in messages" :key="message.id">
          <div class="chat-message" :class="message.role">
            <span>{{ message.role === "user" ? "你" : "AcuAI" }}</span>
            <div
              v-if="message.role === 'assistant' && message.complete !== false"
              class="message-content markdown-body"
              @click="openMarkdownLink"
              v-html="renderedMarkdown(message)"
            />
            <p v-else>{{ message.text || "正在整理回答..." }}</p>
          </div>

          <details
            v-if="message.role === 'assistant' && message.knowledgeEvidence.length"
            class="knowledge-evidence-card"
          >
            <summary>本次实际资料来源（{{ message.knowledgeEvidence.length }}）</summary>
            <ul>
              <li v-for="item in message.knowledgeEvidence" :key="item.evidenceId">
                <a
                  v-if="item.sourceUrl"
                  :href="item.sourceUrl"
                  @click="openMarkdownLink"
                >{{ item.title }}</a>
                <span v-else>{{ item.title }}</span>
                <small>
                  {{ knowledgeEvidenceMeta(item) }}
                </small>
              </li>
            </ul>
          </details>

          <section
            v-for="cleanup in message.cleanupReviews"
            :key="cleanup.review.reviewId"
            class="cleanup-review-card"
          >
            <div class="cleanup-review-heading">
              <div>
                <strong>MOD 文件清理建议</strong>
                <span>{{ cleanup.review.candidateCount }} 个候选</span>
              </div>
              <span>{{ cleanupReviewStatusLabel(cleanup) }}</span>
            </div>
            <p>{{ cleanup.review.message }}</p>
            <div class="cleanup-audit-summary">
              <span>扫描 {{ cleanup.review.scannedFileCount }}</span>
              <span>本地建议 {{ cleanup.review.localRemoveCount }}</span>
              <span>AcuAI 审核 {{ cleanup.review.aiReviewCount }}</span>
            </div>
            <p class="cleanup-safety-note">只排除游戏目录部署，本地 MOD 库原始文件不会删除。</p>

            <div class="cleanup-review-groups">
              <details
                v-for="group in cleanupReviewGroups(cleanup.review)"
                :key="group.modId"
                open
              >
                <summary>
                  <strong>{{ group.modName }}</strong>
                  <span>{{ group.items.length }} 个文件</span>
                </summary>
                <div class="cleanup-review-items">
                  <div
                    v-for="item in group.items"
                    :key="item.candidateId"
                    class="cleanup-review-item"
                    :class="`recommendation-${item.recommendation}`"
                  >
                    <label class="cleanup-review-select">
                      <input
                        type="checkbox"
                        :checked="cleanup.selectedCandidateIds.includes(item.candidateId)"
                        :disabled="cleanup.status !== 'selecting'"
                        @change="updateCleanupSelectionFromEvent(
                          cleanup,
                          item.candidateId,
                          $event,
                        )"
                      />
                      <span class="cleanup-item-content">
                        <span class="cleanup-item-title">
                          <strong>{{ item.libraryRelativePath }}</strong>
                          <span :class="`recommendation-${item.recommendation}`">
                            {{ cleanupRecommendationLabel(item) }}
                          </span>
                        </span>
                        <span>{{ item.reason }}</span>
                        <small>
                          {{ cleanupDecisionSourceLabel(item) }}
                          · {{ cleanupRiskLabel(item) }}
                          ·
                          可信度 {{ Math.round(item.confidence * 100) }}%
                          · {{ formatFileSize(item.sizeBytes) }}
                          · {{ item.currentlyDeployed ? "当前已部署" : "当前未部署" }}
                        </small>
                      </span>
                    </label>
                    <button
                      type="button"
                      class="cleanup-folder-button"
                      :disabled="Boolean(openingCleanupCandidateId)"
                      :aria-label="openingCleanupCandidateId === item.candidateId
                        ? '正在打开所在文件夹'
                        : '打开候选文件所在文件夹'"
                      :data-tooltip="openingCleanupCandidateId === item.candidateId
                        ? '正在打开文件夹'
                        : '打开所在文件夹'"
                      @click="openCleanupCandidateFolder(cleanup, item)"
                    >
                      <span v-if="openingCleanupCandidateId === item.candidateId" aria-hidden="true">&#8987;</span>
                      <span v-else aria-hidden="true">&#128194;</span>
                    </button>
                  </div>
                </div>
              </details>
            </div>

            <p v-if="cleanup.error" class="plan-error">{{ cleanup.error }}</p>
            <div v-if="cleanup.status === 'selecting'" class="cleanup-review-actions">
              <span>已选择 {{ cleanup.selectedCandidateIds.length }} 项</span>
              <button
                type="button"
                :disabled="Boolean(activeCleanupReviewId) || cleanup.selectedCandidateIds.length === 0"
                @click="createCleanupPlan(message, cleanup)"
              >
                生成清理计划
              </button>
            </div>
          </section>

          <section
            v-for="item in message.actionPlans"
            :key="item.plan.planId"
            class="action-plan-card"
            :class="{ destructive: item.plan.destructive }"
          >
          <div class="plan-heading">
            <div>
              <strong>{{ item.plan.title }}</strong>
              <span>{{ item.plan.targetCount }} 个目标 · {{ planExpiryLabel(item.plan) }} 前有效</span>
            </div>
            <span class="plan-status">
              {{
                item.status === "pending"
                  ? "待确认"
                  : item.status === "executing"
                    ? "执行中"
                    : item.status === "cancelled"
                      ? "已取消"
                      : item.status === "failed"
                        ? "执行失败"
                      : item.status === "partiallyFailed"
                        ? "部分失败"
                        : "已完成"
              }}
            </span>
          </div>
          <p>{{ item.plan.summary }}</p>

          <details open>
            <summary>查看全部目标（{{ item.plan.targetCount }}）</summary>
            <ol class="plan-targets">
              <li v-for="target in item.plan.targets" :key="target.modId">
                <strong>{{ target.name }}</strong>
                <span>{{ target.detail }}</span>
              </li>
            </ol>
          </details>

          <details v-if="item.plan.warnings.length" open class="plan-warnings">
            <summary>注意事项（{{ item.plan.warnings.length }}）</summary>
            <ul>
              <li v-for="warning in item.plan.warnings" :key="warning">{{ warning }}</li>
            </ul>
          </details>

          <div v-if="item.result" class="plan-result">
            <strong>{{ item.result.message }}</strong>
            <span v-if="item.result.failedCount">
              成功 {{ item.result.succeededCount }}，失败 {{ item.result.failedCount }}
            </span>
            <ul v-if="item.result.warnings.length">
              <li v-for="warning in item.result.warnings" :key="warning">{{ warning }}</li>
            </ul>
          </div>
          <p v-if="item.error" class="plan-error">{{ item.error }}</p>

          <div v-if="item.status === 'pending'" class="plan-actions">
            <button
              type="button"
              :disabled="isSending || Boolean(activePlanId)"
              @click="cancelPlan(item)"
            >
              取消
            </button>
            <button
              type="button"
              class="confirm-plan-button"
              :class="{ danger: item.plan.destructive }"
              :disabled="isSending || Boolean(activePlanId)"
              @click="confirmPlan(item)"
            >
              确认执行
            </button>
          </div>
          </section>
        </template>
      </div>

      <div v-if="statusMessage" class="agent-status">{{ statusMessage }}</div>
      <div v-if="error" class="agent-error">{{ error }}</div>

      <form class="agent-composer" @submit.prevent="sendMessage">
        <textarea
          v-model="input"
          rows="3"
          maxlength="4000"
          :disabled="isSending"
          placeholder="询问本地 MOD，或描述想找的 MHW MOD"
          @keydown.enter.exact.prevent="sendMessage"
        />
        <button type="submit" :disabled="isSending || !input.trim()">
          {{ isSending ? "回答中" : "发送" }}
        </button>
      </form>
    </template>
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
  display: inline-flex;
  width: 48px;
  height: 48px;
  min-width: 48px;
  min-height: 48px;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  padding: 0;
  border: 1px solid #1d6f55;
  border-radius: 50%;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-size: 0.7rem;
  font-weight: 800;
  line-height: 1;
  text-align: center;
  white-space: nowrap;
  cursor: pointer;
  box-shadow: 0 12px 26px rgba(23, 97, 63, 0.2);
}

.agent-launcher:hover {
  background: #17613f;
}

.agent-panel {
  display: flex;
  width: min(760px, calc(100vw - 32px));
  height: min(780px, calc(100vh - 72px));
  flex-direction: column;
  overflow: hidden;
  border: 1px solid #c9d9d2;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 18px 44px rgba(23, 33, 31, 0.18);
}

.agent-panel header {
  display: flex;
  flex: 0 0 auto;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border-bottom: 1px solid #e3ebe7;
}

.agent-panel p,
.agent-panel h2 {
  margin: 0;
}

.agent-panel header p {
  color: #24745b;
  font-size: 0.7rem;
  font-weight: 750;
}

.agent-panel h2 {
  margin-top: 2px;
  color: #17211f;
  font-size: 1rem;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.agent-panel .close-button {
  display: grid;
  width: 32px;
  height: 32px;
  min-height: 32px;
  padding: 0;
  place-items: center;
  color: #526862;
  font-size: 1.25rem;
  font-weight: 400;
  line-height: 1;
}

.agent-panel .close-button:hover {
  color: #17211f;
  background: #f2f6f4;
}

.agent-panel button {
  min-height: 30px;
  padding: 0 9px;
  border: 1px solid #cbd8d4;
  border-radius: 5px;
  color: #24745b;
  background: #ffffff;
  font: inherit;
  font-size: 0.78rem;
  font-weight: 700;
  cursor: pointer;
}

.agent-panel button:disabled {
  cursor: default;
  opacity: 0.55;
}

.agent-empty,
.welcome-message {
  display: grid;
  gap: 7px;
  color: #61756f;
  font-size: 0.84rem;
}

.agent-quick-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 7px;
  margin-top: 5px;
}

.knowledge-evidence-card {
  display: grid;
  gap: 8px;
  padding: 9px 11px;
  border: 1px solid #d5e2dc;
  border-radius: 6px;
  color: #40574e;
  background: #fbfdfc;
  font-size: 0.76rem;
}

.knowledge-evidence-card summary {
  cursor: pointer;
  font-weight: 700;
}

.knowledge-evidence-card ul {
  display: grid;
  gap: 6px;
  margin: 0;
  padding-left: 17px;
}

.knowledge-evidence-card li {
  display: grid;
  gap: 2px;
}

.knowledge-evidence-card a {
  color: #1c6a52;
  font-weight: 700;
}

.knowledge-evidence-card small {
  color: #6b7f77;
}

.cleanup-review-card {
  display: grid;
  gap: 10px;
  padding: 12px;
  border: 1px solid #9ebcaf;
  border-radius: 6px;
  color: #263d36;
  background: #f7faf8;
  font-size: 0.8rem;
}

.download-watch-card {
  display: grid;
  gap: 9px;
  margin: 10px 12px 0;
  padding: 11px;
  border: 1px solid #9fc8b9;
  border-radius: 7px;
  background: #f1faf6;
}

.download-watch-card > div {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
  color: #385f52;
  font-size: 0.86rem;
}

.download-watch-card__heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.download-watch-card .download-watch-card__close {
  width: 28px;
  min-height: 28px;
  padding: 0;
  border-color: transparent;
  color: #4d6c60;
  background: transparent;
  font-size: 1.25rem;
  font-weight: 400;
  line-height: 1;
}

.download-watch-card .download-watch-card__close:hover {
  color: #1e3d32;
  background: #dcefe7;
}

.download-watch-card p {
  width: 100%;
  margin: 0;
}

.download-watch-card button {
  min-height: 32px;
  padding: 0 10px;
  border: 1px solid #24745b;
  border-radius: 4px;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.download-watch-card button:disabled {
  cursor: default;
  opacity: 0.55;
}

.cleanup-review-card p {
  margin: 0;
}

.cleanup-review-heading,
.cleanup-review-heading > div,
.cleanup-item-content {
  display: grid;
  gap: 3px;
}

.cleanup-review-heading {
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
}

.cleanup-review-heading span,
.cleanup-item-content > span,
.cleanup-item-content small {
  color: #647a73;
}

.cleanup-safety-note {
  padding: 7px 8px;
  border-left: 3px solid #4d8b72;
  background: #edf6f2;
}

.cleanup-audit-summary {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.cleanup-audit-summary span {
  padding: 2px 6px;
  border: 1px solid #c8d8d1;
  border-radius: 4px;
  color: #526b62;
  background: #ffffff;
  font-size: 0.72rem;
}

.cleanup-review-groups {
  display: grid;
  gap: 8px;
  max-height: 420px;
  overflow-y: auto;
}

.cleanup-review-groups details {
  border: 1px solid #d6e1dd;
  border-radius: 5px;
  background: #ffffff;
}

.cleanup-review-groups summary {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 10px;
  cursor: pointer;
}

.cleanup-review-items {
  display: grid;
  border-top: 1px solid #e2e9e6;
}

.cleanup-review-item {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 34px;
  align-items: start;
  gap: 9px;
  padding: 9px 10px;
}

.cleanup-review-item + .cleanup-review-item {
  border-top: 1px solid #edf1ef;
}

.cleanup-review-item:hover {
  background: #f3f7f5;
}

.cleanup-review-select {
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr);
  gap: 9px;
  min-width: 0;
  cursor: pointer;
}

.cleanup-review-select input {
  margin: 3px 0 0;
}

.cleanup-folder-button {
  width: 34px;
  height: 34px;
  padding: 0;
  align-self: start;
  color: #356d5b;
  border-color: #bfd2ca;
  background: #ffffff;
  font-size: 0.92rem;
}

.cleanup-item-title {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
}

.cleanup-item-title strong {
  min-width: 0;
  overflow-wrap: anywhere;
}

.cleanup-item-title > span {
  flex: 0 0 auto;
  padding: 1px 5px;
  border: 1px solid currentColor;
  border-radius: 4px;
  font-size: 0.7rem;
}

.recommendation-remove {
  color: #24745b;
}

.recommendation-review {
  color: #a8661f;
}

.recommendation-keep {
  color: #65746f;
}

.cleanup-review-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
}

.cleanup-review-actions > span {
  color: #647a73;
}

.cleanup-review-actions button {
  color: #ffffff;
  border-color: #24745b;
  background: #24745b;
}

.action-plan-card {
  display: grid;
  gap: 9px;
  padding: 11px;
  border: 1px solid #8fb6a7;
  border-radius: 6px;
  color: #263d36;
  background: #f4faf7;
  font-size: 0.78rem;
}

.action-plan-card.destructive {
  border-color: #d6a397;
  background: #fff8f6;
}

.action-plan-card p,
.action-plan-card ul,
.action-plan-card ol {
  margin: 0;
}

.plan-heading,
.plan-heading > div,
.plan-targets li,
.plan-result {
  display: grid;
  gap: 3px;
}

.plan-heading {
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 8px;
}

.plan-heading span,
.plan-targets span,
.plan-result span {
  color: #647a73;
}

.plan-status {
  padding: 2px 5px;
  border: 1px solid #b8cec5;
  border-radius: 4px;
  white-space: nowrap;
  background: #ffffff;
}

.action-plan-card details summary {
  cursor: pointer;
  color: #245e4b;
  font-weight: 700;
}

.plan-targets,
.plan-warnings ul,
.plan-result ul {
  display: grid;
  gap: 6px;
  max-height: 180px;
  overflow-y: auto;
  margin-top: 7px;
  padding-left: 20px;
}

.plan-warnings {
  color: #8c521d;
}

.plan-result {
  padding: 8px;
  border: 1px solid #c7d9d1;
  border-radius: 4px;
  background: #ffffff;
}

.plan-error {
  color: #a34133;
}

.plan-actions {
  display: flex;
  justify-content: flex-end;
  gap: 7px;
}

.agent-panel .confirm-plan-button {
  color: #ffffff;
  border-color: #24745b;
  background: #24745b;
}

.agent-panel .confirm-plan-button.danger {
  border-color: #a94b3d;
  background: #a94b3d;
}

.agent-empty {
  align-content: center;
  justify-items: start;
  min-height: 180px;
  padding: 20px 16px;
}

.agent-empty strong,
.welcome-message strong {
  color: #203b34;
}

.message-list {
  display: flex;
  min-height: 0;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
  padding: 16px;
}

.welcome-message {
  padding: 4px 0 10px;
}

.chat-message {
  display: grid;
  gap: 5px;
  max-width: 90%;
}

.chat-message.user {
  align-self: flex-end;
}

.chat-message > span {
  color: #71817c;
  font-size: 0.7rem;
  font-weight: 700;
}

.chat-message.user > span {
  text-align: right;
}

.chat-message > p,
.message-content {
  min-width: 0;
  padding: 9px 11px;
  border: 1px solid #dce6e2;
  border-radius: 6px;
  color: #263d36;
  background: #f7faf8;
  font-size: 0.86rem;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.chat-message.user > p {
  border-color: #bed6cc;
  background: #eaf4ef;
}

.markdown-body {
  min-width: 0;
  max-width: 100%;
  overflow-wrap: anywhere;
}

.markdown-body :deep(:first-child) {
  margin-top: 0;
}

.markdown-body :deep(:last-child) {
  margin-bottom: 0;
}

.markdown-body :deep(p),
.markdown-body :deep(ul),
.markdown-body :deep(ol),
.markdown-body :deep(pre),
.markdown-body :deep(blockquote),
.markdown-body :deep(table) {
  margin: 0 0 0.65em;
}

.markdown-body :deep(ul),
.markdown-body :deep(ol) {
  padding-left: 1.4em;
}

.markdown-body :deep(li + li) {
  margin-top: 0.25em;
}

.markdown-body :deep(h1),
.markdown-body :deep(h2),
.markdown-body :deep(h3),
.markdown-body :deep(h4) {
  margin: 0.85em 0 0.4em;
  color: #203b34;
  font-size: 0.95rem;
  line-height: 1.35;
}

.markdown-body :deep(code) {
  padding: 0.08em 0.28em;
  border-radius: 3px;
  background: #e8efec;
  font-family: Consolas, "Courier New", monospace;
  font-size: 0.82em;
}

.markdown-body :deep(pre) {
  overflow-x: hidden;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  padding: 8px;
  border: 1px solid #d7e2de;
  border-radius: 4px;
  background: #eef4f1;
}

.markdown-body :deep(pre code) {
  padding: 0;
  background: transparent;
}

.markdown-body :deep(blockquote) {
  padding-left: 9px;
  border-left: 3px solid #91b5a7;
  color: #587068;
}

.markdown-body :deep(a) {
  color: #176a50;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.markdown-body :deep(table) {
  display: table;
  width: 100%;
  min-width: 0;
  max-width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
}

.markdown-body :deep(th),
.markdown-body :deep(td) {
  padding: 5px 7px;
  border: 1px solid #ccd9d4;
  text-align: left;
  vertical-align: top;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.agent-status,
.agent-error {
  flex: 0 0 auto;
  padding: 8px 16px;
  border-top: 1px solid #edf2ef;
  color: #61756f;
  font-size: 0.78rem;
}

.agent-error {
  color: #a34133;
  background: #fff8f6;
}

.agent-composer {
  display: grid;
  flex: 0 0 auto;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  padding: 12px;
  border-top: 1px solid #e3ebe7;
}

.agent-composer textarea {
  min-width: 0;
  resize: none;
  padding: 9px 10px;
  border: 1px solid #bdccc7;
  border-radius: 5px;
  color: #17211f;
  background: #ffffff;
  font: inherit;
  font-size: 0.84rem;
  line-height: 1.4;
}

.agent-composer button {
  align-self: end;
  min-height: 38px;
  color: #ffffff;
  border-color: #24745b;
  background: #24745b;
}

@media (max-width: 520px) {
  .agent-launcher,
  .agent-panel {
    right: 16px;
    bottom: 16px;
  }

  .agent-panel {
    width: calc(100vw - 32px);
    height: min(520px, calc(100vh - 76px));
  }
}
</style>
