import { Channel, invoke } from "@tauri-apps/api/core";
import type { ModArchiveImportOutcome } from "./modLibrary";

export type DeepSeekModel = "v4Flash" | "v4Pro";

export interface AgentSettings {
  model: DeepSeekModel;
  modelApiName: string;
  apiKeyConfigured: boolean;
  apiKeyHint: string | null;
  apiKeySource: "credentialManager" | "environment" | null;
}

export interface AgentConnectionResult {
  model: DeepSeekModel;
  modelApiName: string;
  elapsedMillis: number;
  message: string;
}

export interface AgentActionTarget {
  modId: string;
  name: string;
  detail: string;
}

export interface AgentActionPlan {
  planId: string;
  kind:
    | "batchModAction"
    | "conflictOrder"
    | "modelRemap"
    | "cleanupExclusions"
    | "cleanupRestore";
  title: string;
  summary: string;
  stateVersion: string;
  expiresAtUnixSeconds: number;
  targetCount: number;
  targets: AgentActionTarget[];
  warnings: string[];
  destructive: boolean;
}

export type CleanupRecommendation = "remove" | "review" | "keep";

export interface AgentCleanupReviewItem {
  candidateId: string;
  modId: string;
  modName: string;
  libraryRelativePath: string;
  deployRelativePath: string;
  sizeBytes: number;
  currentlyDeployed: boolean;
  recommendation: CleanupRecommendation;
  reason: string;
  confidence: number;
  selectedByDefault: boolean;
  decisionSource: "localRule" | "acuAi";
  riskLevel: "low" | "medium" | "high";
}

export interface AgentCleanupReview {
  reviewId: string;
  candidateCount: number;
  scannedFileCount: number;
  localKeepCount: number;
  localRemoveCount: number;
  aiReviewCount: number;
  aiGroupCount: number;
  ruleVersion: number;
  items: AgentCleanupReviewItem[];
  message: string;
}

export interface AgentActionResult {
  planId: string;
  status: "completed" | "partiallyFailed" | "cancelled";
  title: string;
  message: string;
  succeededCount: number;
  failedCount: number;
  warnings: string[];
  archiveImport: ModArchiveImportOutcome | null;
}

export type AgentEventKind =
  | "started"
  | "textReset"
  | "textDelta"
  | "toolStarted"
  | "toolFinished"
  | "planReady"
  | "cleanupReviewReady"
  | "knowledgeEvidenceReady"
  | "completed"
  | "failed";

export interface AgentKnowledgeEvidence {
  evidenceId: string;
  title: string;
  gameVersion: string;
  confidence: number;
  sourceTitle: string | null;
  sourceUrl: string | null;
  packId: string;
  packVersion: string;
  sourceTier: "localVerified" | "localReference" | "localAnalysis" | "webReference";
}

export interface AgentWebSearchTestResult {
  model: DeepSeekModel;
  modelApiName: string;
  elapsedMillis: number;
  searchResultCount: number;
  pageReadSucceeded: boolean;
  source: string | null;
  message: string;
}

export interface AgentEvent {
  turnId: string;
  sequence: number;
  kind: AgentEventKind;
  text: string | null;
  toolName: string | null;
  message: string | null;
  plan: AgentActionPlan | null;
  cleanupReview: AgentCleanupReview | null;
  knowledgeEvidence: AgentKnowledgeEvidence[];
}

export interface AgentTurnResult {
  turnId: string;
  message: string;
}

export function getAgentSettings(): Promise<AgentSettings> {
  return invoke<AgentSettings>("get_agent_settings");
}

export function saveAgentModel(model: DeepSeekModel): Promise<AgentSettings> {
  return invoke<AgentSettings>("save_agent_model", { model });
}

export function setDeepSeekApiKey(apiKey: string): Promise<AgentSettings> {
  return invoke<AgentSettings>("set_deepseek_api_key", { apiKey });
}

export function deleteDeepSeekApiKey(): Promise<AgentSettings> {
  return invoke<AgentSettings>("delete_deepseek_api_key");
}

export function testAgentConnection(): Promise<AgentConnectionResult> {
  return invoke<AgentConnectionResult>("test_agent_connection");
}

/** 使用已保存的 Key 验证 DeepSeek 服务端搜索和白名单页面读取。 */
export function testAgentWebSearch(): Promise<AgentWebSearchTestResult> {
  return invoke<AgentWebSearchTestResult>("test_agent_web_search");
}

/** Channel 保证同一轮流式片段有序到达，前端不接触 DeepSeek 原始 SSE。 */
export function startAgentTurn(
  message: string,
  handleEvent: (event: AgentEvent) => void,
): Promise<AgentTurnResult> {
  const onEvent = new Channel<AgentEvent>();
  onEvent.onmessage = handleEvent;
  return invoke<AgentTurnResult>("start_agent_turn", { message, onEvent });
}

export function clearAgentSession(): Promise<void> {
  return invoke<void>("clear_agent_session");
}

export function confirmAgentActionPlan(planId: string): Promise<AgentActionResult> {
  return invoke<AgentActionResult>("confirm_agent_action_plan", { planId });
}

export function cancelAgentActionPlan(planId: string): Promise<AgentActionResult> {
  return invoke<AgentActionResult>("cancel_agent_action_plan", { planId });
}

export function createAgentCleanupPlan(
  reviewId: string,
  candidateIds: string[],
): Promise<AgentActionPlan> {
  return invoke<AgentActionPlan>("create_agent_cleanup_plan", { reviewId, candidateIds });
}
