import { Channel, invoke } from "@tauri-apps/api/core";

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

export type AgentEventKind =
  | "started"
  | "textDelta"
  | "toolStarted"
  | "toolFinished"
  | "planReady"
  | "completed"
  | "failed";

export interface AgentEvent {
  turnId: string;
  sequence: number;
  kind: AgentEventKind;
  text: string | null;
  toolName: string | null;
  message: string | null;
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
