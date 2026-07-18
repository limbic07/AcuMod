import { Channel, invoke } from "@tauri-apps/api/core";
import type { ModArchiveImportOutcome } from "./modLibrary";

export type DeepSeekModel = "v4Flash" | "v4Pro";

export interface AgentSettings {
  model: DeepSeekModel;
  modelApiName: string;
  apiKeyConfigured: boolean;
  apiKeyHint: string | null;
  apiKeySource: "credentialManager" | "environment" | null;
  nexusApiKeyConfigured: boolean;
  nexusApiKeyHint: string | null;
  nexusApiKeySource: "credentialManager" | "environment" | null;
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
    | "cleanupRestore"
    | "nexusDownloadInstall";
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

export interface NexusConnectionResult {
  userName: string;
  isPremium: boolean;
  message: string;
}

export type AgentEventKind =
  | "started"
  | "textDelta"
  | "toolStarted"
  | "toolFinished"
  | "planReady"
  | "cleanupReviewReady"
  | "completed"
  | "failed";

export interface AgentEvent {
  turnId: string;
  sequence: number;
  kind: AgentEventKind;
  text: string | null;
  toolName: string | null;
  message: string | null;
  plan: AgentActionPlan | null;
  cleanupReview: AgentCleanupReview | null;
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

export function setNexusApiKey(apiKey: string): Promise<AgentSettings> {
  return invoke<AgentSettings>("set_nexus_api_key", { apiKey });
}

export function deleteNexusApiKey(): Promise<AgentSettings> {
  return invoke<AgentSettings>("delete_nexus_api_key");
}

export function testNexusConnection(): Promise<NexusConnectionResult> {
  return invoke<NexusConnectionResult>("test_nexus_connection");
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
