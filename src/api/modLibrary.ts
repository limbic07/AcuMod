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

export interface InstalledModSummary {
  id: string;
  name: string;
  modPath: string;
  contentPath: string;
  manifestPath: string;
  fileCount: number;
  enabled: boolean;
  deployRoot: string;
  detectionMethod: string;
  installedAtUnixSeconds: number;
  modelReplacements: ModelReplacement[];
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

export interface ModConflictGroup {
  groupId: string;
  participantCount: number;
  conflictFileCount: number;
  enabledParticipantCount: number;
  participants: ModConflictParticipant[];
}

export interface ModConflictReport {
  conflictCount: number;
  conflictFileCount: number;
  groups: ModConflictGroup[];
  warnings: string[];
  message: string;
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

export function getModLibraryStatus(): Promise<ModLibraryStatus> {
  return invoke<ModLibraryStatus>("get_mod_library_status");
}

export function listInstalledMods(): Promise<InstalledModList> {
  return invoke<InstalledModList>("list_installed_mods");
}

export function openInstalledModFolder(modId: string): Promise<void> {
  return invoke<void>("open_installed_mod_folder", {
    modId,
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
