import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export const DOWNLOAD_WATCH_EVENT = "acumod://download-watch";

export interface DownloadWatchStart {
  watchId: string;
  directory: string;
  message: string;
}

export interface DownloadWatchEvent {
  watchId: string;
  status: "found" | "expired" | "failed";
  sourceUrl: string;
  filePath: string | null;
  fileName: string | null;
  sizeBytes: number | null;
  message: string;
}

/** 只启动目录观察；文件发现后仍需由用户确认进入既有归档导入流程。 */
export function startDownloadWatch(
  directory: string,
  sourceUrl: string,
): Promise<DownloadWatchStart> {
  return invoke<DownloadWatchStart>("start_download_watch", { directory, sourceUrl });
}

export function listenDownloadWatch(handler: (event: DownloadWatchEvent) => void) {
  return listen<DownloadWatchEvent>(DOWNLOAD_WATCH_EVENT, (event) => handler(event.payload));
}
