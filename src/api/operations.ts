import { listen } from "@tauri-apps/api/event";

export const OPERATION_PROGRESS_EVENT = "acumod://operation-progress";

export interface OperationProgress {
  operationId: string;
  kind: string;
  title: string;
  phase: string;
  completed: number;
  total: number | null;
  currentItem: string | null;
  elapsedMillis: number;
  terminal: boolean;
}

export function listenOperationProgress(
  handler: (progress: OperationProgress) => void,
) {
  return listen<OperationProgress>(OPERATION_PROGRESS_EVENT, (event) => {
    handler(event.payload);
  });
}
