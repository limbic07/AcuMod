import { invoke } from "@tauri-apps/api/core";
import type { GameTextLanguage } from "../domain/gameText";

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

export interface GameTextSettings {
  language: GameTextLanguage;
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

export function getGameTextSettings(): Promise<GameTextSettings> {
  return invoke<GameTextSettings>("get_game_text_settings");
}

export function saveGameTextLanguage(language: GameTextLanguage): Promise<GameTextSettings> {
  return invoke<GameTextSettings>("save_game_text_language", { language });
}
