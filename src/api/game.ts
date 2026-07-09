import { invoke } from "@tauri-apps/api/core";

export interface GameDirectoryStatus {
  path: string | null;
  isConfigured: boolean;
  isValid: boolean;
  message: string;
  executablePath: string | null;
  nativePcPath: string | null;
  configPath: string;
  source: string;
}

export function getGameDirectoryStatus(): Promise<GameDirectoryStatus> {
  return invoke<GameDirectoryStatus>("get_game_directory_status");
}

export function detectGameDirectory(): Promise<GameDirectoryStatus> {
  return invoke<GameDirectoryStatus>("detect_game_directory");
}

export function saveGameDirectory(path: string): Promise<GameDirectoryStatus> {
  return invoke<GameDirectoryStatus>("save_game_directory", { path });
}
