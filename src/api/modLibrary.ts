import { invoke } from "@tauri-apps/api/core";

export interface ModLibraryStatus {
  softwareDataPath: string;
  modsPath: string;
  installedPath: string;
  stagingPath: string;
  importStagingPath: string;
  isReady: boolean;
  message: string;
}

export interface ModImportFilePreview {
  sourcePath: string;
  sourceRelativePath: string;
  deployRelativePath: string;
}

export interface ModImportCandidate {
  rootPath: string;
  relativePath: string;
  detectionMethod: string;
  deployRoot: string;
  fileCount: number;
}

export interface ModImportPreview {
  sourcePath: string;
  status: string;
  detectionMethod: string;
  deployRoot: string;
  contentRootPath: string | null;
  requiresGameRootConfirmation: boolean;
  message: string;
  fileCount: number;
  files: ModImportFilePreview[];
  candidates: ModImportCandidate[];
  warnings: string[];
}

export interface InstalledModFile {
  sourceRelativePath: string;
  deployRelativePath: string;
  libraryRelativePath: string;
}

export interface ModelReplacement {
  modelKind: string;
  subKind: string;
  modelPart: string;
  modelId: string;
  gameIds: string[];
  variantIds: string[];
  displayNames: string[];
  affectedParts: string[];
  associations: ModelAssociation[];
  matchedFiles: string[];
  recognitionSource: string;
}

export interface ModelAssociation {
  modelKind: string;
  modelId: string;
  displayNames: string[];
  matchedFiles: string[];
  recognitionSource: string;
}

export interface ModInstallResult {
  modId: string;
  name: string;
  alreadyInstalled: boolean;
  modPath: string;
  contentPath: string;
  manifestPath: string;
  fileCount: number;
  files: InstalledModFile[];
  modelReplacements: ModelReplacement[];
  message: string;
}

export interface ModArchiveImportOutcome {
  status: string;
  sourcePath: string;
  originalArchivePath: string;
  preview: ModImportPreview | null;
  installResult: ModInstallResult | null;
  message: string;
}

export interface LegacyBoxModFile {
  sourceRelativePath: string;
  fileSizeBytes: number;
}

export interface LegacyBoxDeploymentStatus {
  status: "fullyMatched" | "partiallyMatched" | "notDeployed" | "different" | "unavailable";
  totalFileCount: number;
  matchingFileCount: number;
  missingFileCount: number;
  differentFileCount: number;
}

export interface LegacyBoxMod {
  moduleId: string;
  name: string;
  boxEnabled: boolean;
  boxIndex: number | null;
  modType: string;
  installTime: string;
  installSource: string;
  modulePath: string;
  filesPath: string;
  fileCount: number;
  totalSizeBytes: number;
  files: LegacyBoxModFile[];
  deployment: LegacyBoxDeploymentStatus;
}

export interface LegacyBoxScan {
  boxPath: string;
  boxGamePath: string | null;
  isBoxGamePathValid: boolean;
  acumodGamePath: string | null;
  gamePathsMatch: boolean | null;
  mods: LegacyBoxMod[];
  warnings: string[];
  message: string;
}

export interface LegacyBoxImportItem {
  moduleId: string;
  name: string;
  status: "imported" | "alreadyInstalled" | "failed";
  modId: string | null;
  message: string;
}

export interface LegacyBoxImportResult {
  items: LegacyBoxImportItem[];
  importedCount: number;
  alreadyInstalledCount: number;
  failedCount: number;
  message: string;
}

export interface InstalledModSummary {
  id: string;
  name: string;
  originalName: string;
  note: string;
  categoryIds: string[];
  categories: ModCategory[];
  modPath: string;
  contentPath: string;
  manifestPath: string;
  fileCount: number;
  files: InstalledModFile[];
  enabled: boolean;
  deployRoot: string;
  detectionMethod: string;
  installedAtUnixSeconds: number;
  modelReplacements: ModelReplacement[];
  originalModelReplacements: ModelReplacement[];
  modelRemapCount: number;
}

export interface ModelRemapTarget {
  targetId: string;
  modelId: string;
  modelPaths: string[];
  gameIds: string[];
  displayNames: string[];
  affectedParts: string[];
}

export interface ModelRemapGroup {
  groupKey: string;
  modelKind: string;
  subKind: string;
  sourceModelIds: string[];
  sourceGameIds: string[];
  sourceDisplayNames: string[];
  sourceAffectedParts: string[];
  originalTargetId: string | null;
  selectedTargetId: string | null;
  allowsManualTarget: boolean;
  targets: ModelRemapTarget[];
}

export interface ModRemapDetails {
  modId: string;
  name: string;
  enabled: boolean;
  groups: ModelRemapGroup[];
  warnings: string[];
  message: string;
}

export interface ModRemapPlanFile {
  sourceDeployRelativePath: string;
  effectiveDeployRelativePath: string;
  pathChanged: boolean;
  mrl3RewriteCount: number;
  evamRewriteCount: number;
}

export interface ModRemapPlan {
  modId: string;
  name: string;
  groupKey: string;
  sourceLabel: string;
  targetId: string | null;
  targetLabel: string;
  changedFileCount: number;
  mrl3RewriteCount: number;
  evamRewriteCount: number;
  files: ModRemapPlanFile[];
  warnings: string[];
  message: string;
}

export interface ModRemapApplyResult {
  modId: string;
  name: string;
  groupKey: string;
  targetId: string | null;
  selectionCount: number;
  changedFileCount: number;
  mrl3RewriteCount: number;
  evamRewriteCount: number;
  message: string;
}

export interface InstalledModList {
  mods: InstalledModSummary[];
  warnings: string[];
  message: string;
}

export interface ModDeploymentPlanFile {
  deployRelativePath: string;
  sourcePath: string;
  targetPath: string;
  targetExists: boolean;
  targetManagedByCurrentMod: boolean;
  targetManagedByOtherMod: boolean;
  targetManagedModId: string | null;
}

export interface ModDeploymentPlan {
  modId: string;
  name: string;
  status: string;
  message: string;
  fileCount: number;
  files: ModDeploymentPlanFile[];
  warnings: string[];
  requiresOverwriteConfirmation: boolean;
}

export interface DeployedModFile {
  deployRelativePath: string;
  deployedPath: string;
  deployedAtUnixSeconds: number;
}

export interface ModDeploymentResult {
  modId: string;
  name: string;
  enabled: boolean;
  affectedFileCount: number;
  files: DeployedModFile[];
  warnings: string[];
  message: string;
}

export interface ModDisablePlan {
  modId: string;
  name: string;
  enabled: boolean;
  fileCount: number;
  files: DeployedModFile[];
  warnings: string[];
  message: string;
}

export interface ModUninstallPlan {
  modId: string;
  name: string;
  enabled: boolean;
  deployedFileCount: number;
  libraryFileCount: number;
  deployedFiles: DeployedModFile[];
  libraryFiles: InstalledModFile[];
  warnings: string[];
  message: string;
}

export interface ModUninstallResult {
  modId: string;
  name: string;
  removedDeployedFileCount: number;
  removedLibraryFileCount: number;
  warnings: string[];
  message: string;
}

export interface RestoreModPlanItem {
  modId: string;
  name: string;
  enabled: boolean;
  deployedFileCount: number;
}

export interface RestoreAllPlan {
  affectedModCount: number;
  deployedFileCount: number;
  mods: RestoreModPlanItem[];
  warnings: string[];
  message: string;
}

export interface RestoreAllResult {
  affectedModCount: number;
  removedDeployedFileCount: number;
  mods: RestoreModPlanItem[];
  warnings: string[];
  message: string;
}

export interface ModConflictParticipant {
  modId: string;
  name: string;
  enabled: boolean;
  order: number;
}

export interface SharedModelTarget {
  modelKind: string;
  subKind: string;
  modelId: string;
  displayNames: string[];
}

export interface ModConflictGroup {
  groupId: string;
  participantCount: number;
  conflictFileCount: number;
  conflictFiles: string[];
  enabledParticipantCount: number;
  participants: ModConflictParticipant[];
  sharedModelTargets: SharedModelTarget[];
}

export interface ModConflictReport {
  conflictCount: number;
  conflictFileCount: number;
  groups: ModConflictGroup[];
  warnings: string[];
  message: string;
}

export interface ModWorkspaceSnapshot {
  installedMods: InstalledModList;
  categories: ModCategoryList;
  conflictReport: ModConflictReport;
}

export interface ModConflictMoveResult {
  groupId: string;
  modId: string;
  direction: string;
  moved: boolean;
  message: string;
}

export interface ApplyConflictOrderPlan {
  groupId: string;
  conflictFileCount: number;
  applicableFileCount: number;
  enabledParticipantCount: number;
  requiresOverwriteConfirmation: boolean;
  warnings: string[];
  message: string;
}

export interface ApplyConflictOrderResult {
  groupId: string;
  appliedFileCount: number;
  skippedFileCount: number;
  warnings: string[];
  message: string;
}

export interface ModMetadataUpdateResult {
  modId: string;
  name: string;
  originalName: string;
  note: string;
  categoryIds: string[];
  categories: ModCategory[];
  message: string;
}

export interface ModMetadataPatch {
  displayName?: string;
  note?: string;
  categoryIds?: string[];
}

export interface ModCategory {
  id: string;
  name: string;
  parentId: string | null;
  createdAtUnixSeconds: number;
}

export interface ModCategoryList {
  categories: ModCategory[];
  message: string;
}

export interface ModCategoryDeleteResult {
  categoryId: string;
  affectedModCount: number;
  message: string;
}

export function getModLibraryStatus(): Promise<ModLibraryStatus> {
  return invoke<ModLibraryStatus>("get_mod_library_status");
}

export function listInstalledMods(): Promise<InstalledModList> {
  return invoke<InstalledModList>("list_installed_mods");
}

export function getModWorkspaceSnapshot(): Promise<ModWorkspaceSnapshot> {
  return invoke<ModWorkspaceSnapshot>("get_mod_workspace_snapshot");
}

export function updateModMetadata(
  modId: string,
  patch: ModMetadataPatch,
): Promise<ModMetadataUpdateResult> {
  return invoke<ModMetadataUpdateResult>("update_mod_metadata", {
    modId,
    patch,
  });
}

export function listModCategories(): Promise<ModCategoryList> {
  return invoke<ModCategoryList>("list_mod_categories");
}

export function createModCategory(
  name: string,
  parentId: string | null,
): Promise<ModCategory> {
  return invoke<ModCategory>("create_mod_category", { name, parentId });
}

export function moveModLibraryItem(
  modId: string,
  targetModId: string,
  placeAfter: boolean,
): Promise<void> {
  return invoke<void>("move_mod_library_item", {
    modId,
    targetModId,
    placeAfter,
  });
}

export function renameModCategory(
  categoryId: string,
  name: string,
): Promise<ModCategory> {
  return invoke<ModCategory>("rename_mod_category", {
    categoryId,
    name,
  });
}

export function deleteModCategory(
  categoryId: string,
): Promise<ModCategoryDeleteResult> {
  return invoke<ModCategoryDeleteResult>("delete_mod_category", {
    categoryId,
  });
}

export function openInstalledModFolder(modId: string): Promise<void> {
  return invoke<void>("open_installed_mod_folder", {
    modId,
  });
}

export function getModRemapDetails(modId: string): Promise<ModRemapDetails> {
  return invoke<ModRemapDetails>("get_mod_remap_details", { modId });
}

export function previewModRemap(
  modId: string,
  groupKey: string,
  targetId: string | null,
): Promise<ModRemapPlan> {
  return invoke<ModRemapPlan>("preview_mod_remap", {
    modId,
    groupKey,
    targetId,
  });
}

export function applyModRemap(
  modId: string,
  groupKey: string,
  targetId: string | null,
): Promise<ModRemapApplyResult> {
  return invoke<ModRemapApplyResult>("apply_mod_remap", {
    modId,
    groupKey,
    targetId,
  });
}

export function previewModImport(
  path: string,
  allowGameRoot: boolean,
): Promise<ModImportPreview> {
  return invoke<ModImportPreview>("preview_mod_import", {
    path,
    allowGameRoot,
  });
}

export function installModFromFolder(
  path: string,
  allowGameRoot: boolean,
): Promise<ModInstallResult> {
  return invoke<ModInstallResult>("install_mod_from_folder", {
    path,
    allowGameRoot,
  });
}

export function installModFromArchive(
  path: string,
  allowGameRoot: boolean,
): Promise<ModArchiveImportOutcome> {
  return invoke<ModArchiveImportOutcome>("install_mod_from_archive", {
    path,
    allowGameRoot,
  });
}

export function installModFromCandidate(
  sourcePath: string,
  candidateRootPath: string,
  originalArchivePath: string | null,
): Promise<ModInstallResult> {
  return invoke<ModInstallResult>("install_mod_from_candidate", {
    sourcePath,
    candidateRootPath,
    originalArchivePath,
  });
}

// 盒子扫描和导入分别对应 Rust 的只读核验与本地库复制，避免前端误把扫描结果当作接管状态。
export function scanLegacyBoxMods(boxPath: string): Promise<LegacyBoxScan> {
  return invoke<LegacyBoxScan>("scan_legacy_box_mods", { boxPath });
}

export function importLegacyBoxMods(
  boxPath: string,
  moduleIds: string[],
): Promise<LegacyBoxImportResult> {
  return invoke<LegacyBoxImportResult>("import_legacy_box_mods", {
    boxPath,
    moduleIds,
  });
}

export function previewEnableMod(modId: string): Promise<ModDeploymentPlan> {
  return invoke<ModDeploymentPlan>("preview_enable_mod", {
    modId,
  });
}

export function enableMod(
  modId: string,
  confirmOverwrite: boolean,
): Promise<ModDeploymentResult> {
  return invoke<ModDeploymentResult>("enable_mod", {
    modId,
    confirmOverwrite,
  });
}

export function disableMod(modId: string): Promise<ModDeploymentResult> {
  return invoke<ModDeploymentResult>("disable_mod", {
    modId,
  });
}

export function previewDisableMod(modId: string): Promise<ModDisablePlan> {
  return invoke<ModDisablePlan>("preview_disable_mod", {
    modId,
  });
}

export function previewUninstallMod(modId: string): Promise<ModUninstallPlan> {
  return invoke<ModUninstallPlan>("preview_uninstall_mod", {
    modId,
  });
}

export function uninstallMod(modId: string): Promise<ModUninstallResult> {
  return invoke<ModUninstallResult>("uninstall_mod", {
    modId,
  });
}

export function previewRestoreAllMods(): Promise<RestoreAllPlan> {
  return invoke<RestoreAllPlan>("preview_restore_all_mods");
}

export function restoreAllMods(): Promise<RestoreAllResult> {
  return invoke<RestoreAllResult>("restore_all_mods");
}

export function getModConflictReport(): Promise<ModConflictReport> {
  return invoke<ModConflictReport>("get_mod_conflict_report");
}

export function moveConflictParticipant(
  groupId: string,
  modId: string,
  direction: "up" | "down",
): Promise<ModConflictMoveResult> {
  return invoke<ModConflictMoveResult>("move_conflict_participant", {
    groupId,
    modId,
    direction,
  });
}

export function previewApplyConflictOrder(
  groupId: string,
): Promise<ApplyConflictOrderPlan> {
  return invoke<ApplyConflictOrderPlan>("preview_apply_conflict_order", {
    groupId,
  });
}

export function applyConflictOrder(
  groupId: string,
  confirmOverwrite: boolean,
): Promise<ApplyConflictOrderResult> {
  return invoke<ApplyConflictOrderResult>("apply_conflict_order", {
    groupId,
    confirmOverwrite,
  });
}
