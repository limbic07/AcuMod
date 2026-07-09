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

export interface ModInstallResult {
  modId: string;
  name: string;
  modPath: string;
  contentPath: string;
  manifestPath: string;
  fileCount: number;
  files: InstalledModFile[];
  message: string;
}

export function getModLibraryStatus(): Promise<ModLibraryStatus> {
  return invoke<ModLibraryStatus>("get_mod_library_status");
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
