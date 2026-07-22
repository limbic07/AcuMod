import { invoke } from "@tauri-apps/api/core";

export type KnowledgePackKind = "mhw-modding" | "mhw-game-facts" | "mhw-game-guides";

export interface KnowledgePackSummary {
  packId: string;
  displayName: string;
  kind: KnowledgePackKind;
  version: string;
  gameVersion: string;
  locale: string;
  description: string;
  sha256: string;
  sizeBytes: number;
  installedAtUnixSeconds: number;
  entityCount: number;
  relationCount: number;
  documentCount: number;
  sourceCount: number;
  active: boolean;
  healthy: boolean;
  error: string | null;
}

export interface KnowledgeStatus {
  packs: KnowledgePackSummary[];
  activePackCount: number;
  totalSizeBytes: number;
  message: string;
}

export interface KnowledgeInstallResult {
  message: string;
  installedPack: KnowledgePackSummary;
  status: KnowledgeStatus;
}

export interface KnowledgeSearchMatch {
  resultId: string;
  resultKind: "entity" | "document";
  domain: string;
  title: string;
  snippet: string;
  gameVersion: string;
  confidence: number;
  sourceTitle: string | null;
  sourceUrl: string | null;
  packId: string;
  packVersion: string;
  packKind: KnowledgePackKind;
}

export interface KnowledgeSearchResponse {
  query: string;
  searchedPackCount: number;
  matches: KnowledgeSearchMatch[];
  warnings: string[];
}

export interface KnowledgeEntityAlias {
  locale: string;
  alias: string;
}

export interface KnowledgeEntityMatch {
  entityId: string;
  kind: string;
  domain: string;
  canonicalName: string;
  nameZhHans: string | null;
  nameZhHant: string | null;
  summary: string;
  gameVersion: string;
  confidence: number;
  data: Record<string, unknown>;
  aliases: KnowledgeEntityAlias[];
  sourceTitle: string | null;
  sourceUrl: string | null;
  packId: string;
  packVersion: string;
}

export interface KnowledgeEntityLookupResponse {
  query: string;
  searchedPackCount: number;
  matches: KnowledgeEntityMatch[];
  warnings: string[];
}

export type KnowledgeRelationDirection = "outgoing" | "incoming" | "both";

export interface KnowledgeRelationMatch {
  relationId: string;
  subjectId: string;
  subjectName: string;
  predicate: string;
  objectId: string;
  objectName: string;
  gameVersion: string;
  confidence: number;
  data: Record<string, unknown>;
  sourceTitle: string | null;
  sourceUrl: string | null;
  packId: string;
  packVersion: string;
}

export interface KnowledgeRelationResponse {
  entityId: string;
  direction: KnowledgeRelationDirection;
  searchedPackCount: number;
  relations: KnowledgeRelationMatch[];
  warnings: string[];
}

export function getKnowledgeStatus(): Promise<KnowledgeStatus> {
  return invoke<KnowledgeStatus>("get_knowledge_status");
}

export function installKnowledgePack(sourcePath: string): Promise<KnowledgeInstallResult> {
  return invoke<KnowledgeInstallResult>("install_knowledge_pack", { sourcePath });
}

export function deleteKnowledgePack(packId: string): Promise<KnowledgeStatus> {
  return invoke<KnowledgeStatus>("delete_knowledge_pack", { packId });
}

/** 直接查询入口用于开发验收；AcuAI 使用同一个 Rust service 的受控工具。 */
export function searchKnowledge(
  query: string,
  domains?: KnowledgePackKind[],
  limit = 20,
): Promise<KnowledgeSearchResponse> {
  return invoke<KnowledgeSearchResponse>("search_knowledge", { query, domains, limit });
}

/** 精确实体查询与全文检索分开，保证 AcuAI 可以取得稳定 ID 和类型化字段。 */
export function lookupGameEntities(
  query: string,
  kinds?: string[],
  limit = 20,
): Promise<KnowledgeEntityLookupResponse> {
  return invoke<KnowledgeEntityLookupResponse>("lookup_game_entities", { query, kinds, limit });
}

export function getGameEntityRelations(
  entityId: string,
  predicates?: string[],
  direction: KnowledgeRelationDirection = "both",
  limit = 30,
): Promise<KnowledgeRelationResponse> {
  return invoke<KnowledgeRelationResponse>("get_game_entity_relations", {
    entityId,
    predicates,
    direction,
    limit,
  });
}
