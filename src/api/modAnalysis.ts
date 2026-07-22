import { invoke } from "@tauri-apps/api/core";

export interface ModAnalysisEvidence {
  kind: string;
  detail: string;
  confidence: number;
}

export interface AnalyzedModFile {
  fileId: string;
  libraryRelativePath: string;
  sourceDeployRelativePath: string;
  effectiveDeployRelativePath: string;
  extension: string;
  sizeBytes: number;
  role: string;
  roleLabel: string;
  componentId: string;
  componentLabel: string;
  replacementTargets: string[];
  references: string[];
  evidence: ModAnalysisEvidence[];
  confidence: number;
  excludedFromDeployment: boolean;
}

export interface ModResourceComponent {
  componentId: string;
  kind: string;
  label: string;
  fileCount: number;
  fileIds: string[];
  roles: string[];
  replacementTargets: string[];
  confidence: number;
}

export interface ModResourceEdge {
  fromFileId: string;
  toFileId: string | null;
  targetReference: string;
  relation: string;
  relationLabel: string;
  evidence: string;
  confidence: number;
}

export interface ModKnowledgeEvidence {
  resultId: string;
  title: string;
  snippet: string;
  gameVersion: string;
  confidence: number;
  sourceTitle: string | null;
  sourceUrl: string | null;
  packId: string;
  packVersion: string;
}

export interface ModAnalysisReport {
  schemaVersion: number;
  analyzerVersion: number;
  modId: string;
  modName: string;
  inventorySha256: string;
  contentSha256: string;
  knowledgeSignature: string;
  fileCount: number;
  totalSizeBytes: number;
  recognizedFileCount: number;
  unknownFileCount: number;
  componentCount: number;
  files: AnalyzedModFile[];
  components: ModResourceComponent[];
  edges: ModResourceEdge[];
  knowledgeEvidence: ModKnowledgeEvidence[];
  warnings: string[];
  cacheHit: boolean;
  message: string;
}

/** Rust 通过稳定 MOD ID 恢复受控文件清单，前端不传递本地路径。 */
export function analyzeInstalledMod(modId: string): Promise<ModAnalysisReport> {
  return invoke<ModAnalysisReport>("analyze_installed_mod", { modId });
}
