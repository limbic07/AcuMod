<script setup lang="ts">
import { openUrl } from "@tauri-apps/plugin-opener";
import MarkdownIt from "markdown-it";
import { nextTick, ref, watch } from "vue";
import {
  clearAgentSession,
  getAgentSettings,
  startAgentTurn,
  type AgentEvent,
  type AgentSettings,
} from "../api/agent";

interface ChatMessage {
  id: number;
  role: "user" | "assistant";
  text: string;
  complete: boolean;
  renderedHtml: string;
}

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  openPanel: [];
  close: [];
  openSettings: [];
}>();

const settings = ref<AgentSettings | null>(null);
const messages = ref<ChatMessage[]>([]);
const input = ref("");
const statusMessage = ref("");
const error = ref("");
const isLoadingSettings = ref(false);
const isSending = ref(false);
const messageList = ref<HTMLElement | null>(null);
let nextMessageId = 1;
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
    error.value = "AI 回答的 Markdown 渲染失败，已保留原始文本。";
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
    await openUrl(url.toString());
  } catch {
    error.value = "无法打开回答中的链接。";
  }
}

async function scrollToLatest() {
  await nextTick();
  if (messageList.value) {
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
  if (event.kind === "failed") {
    error.value = event.message ?? "AI 回答失败。";
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
  };
  const assistantMessage: ChatMessage = {
    id: nextMessageId++,
    role: "assistant",
    text: "",
    complete: false,
    renderedHtml: "",
  };
  messages.value.push(userMessage, assistantMessage);
  input.value = "";
  error.value = "";
  statusMessage.value = "正在连接 DeepSeek V4";
  isSending.value = true;
  await scrollToLatest();

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
</script>

<template>
  <button
    v-if="!open"
    type="button"
    class="agent-launcher"
    title="打开 AI 助手"
    @click="$emit('openPanel')"
  >
    AI
  </button>

  <aside v-else class="agent-panel" aria-label="AI 助手">
    <header>
      <div>
        <p>Acumen MOD Manager</p>
        <h2>AI 助手</h2>
      </div>
      <div class="header-actions">
        <button type="button" title="清空当前对话" :disabled="isSending" @click="clearConversation">
          清空
        </button>
        <button type="button" title="收起 AI 助手" @click="$emit('close')">收起</button>
      </div>
    </header>

    <div v-if="isLoadingSettings && !settings" class="agent-empty">正在读取 AI 设置...</div>
    <div v-else-if="!settings?.apiKeyConfigured" class="agent-empty">
      <strong>尚未配置 DeepSeek 访问密钥</strong>
      <span>配置后可查询本地 MOD、冲突和 MHW 游戏术语。</span>
      <button type="button" @click="openSettings">前往设置</button>
    </div>
    <template v-else>
      <div ref="messageList" class="message-list" aria-live="polite">
        <div v-if="messages.length === 0" class="welcome-message">
          <strong>DeepSeek V4 已就绪</strong>
          <span>可以询问已安装 MOD、启用状态、冲突和替换目标。</span>
        </div>
        <div
          v-for="message in messages"
          :key="message.id"
          class="chat-message"
          :class="message.role"
        >
          <span>{{ message.role === "user" ? "你" : "AI" }}</span>
          <div
            v-if="message.role === 'assistant' && message.complete !== false"
            class="message-content markdown-body"
            @click="openMarkdownLink"
            v-html="renderedMarkdown(message)"
          />
          <p v-else>{{ message.text || "正在整理回答..." }}</p>
        </div>
      </div>

      <div v-if="statusMessage" class="agent-status">{{ statusMessage }}</div>
      <div v-if="error" class="agent-error">{{ error }}</div>

      <form class="agent-composer" @submit.prevent="sendMessage">
        <textarea
          v-model="input"
          rows="3"
          maxlength="4000"
          :disabled="isSending"
          placeholder="询问本地 MOD 或 MHW 游戏术语"
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
  display: grid;
  width: 48px;
  height: 48px;
  place-items: center;
  border: 1px solid #1d6f55;
  border-radius: 50%;
  color: #ffffff;
  background: #24745b;
  font: inherit;
  font-size: 0.82rem;
  font-weight: 800;
  cursor: pointer;
  box-shadow: 0 12px 26px rgba(23, 97, 63, 0.2);
}

.agent-launcher:hover {
  background: #17613f;
}

.agent-panel {
  display: flex;
  width: min(380px, calc(100vw - 32px));
  height: min(560px, calc(100vh - 92px));
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
  gap: 6px;
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
  overflow-x: auto;
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
  display: block;
  max-width: 100%;
  overflow-x: auto;
  border-collapse: collapse;
}

.markdown-body :deep(th),
.markdown-body :deep(td) {
  padding: 5px 7px;
  border: 1px solid #ccd9d4;
  text-align: left;
  vertical-align: top;
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
