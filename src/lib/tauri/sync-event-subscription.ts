import { listen } from "@tauri-apps/api/event";
import type { SyncSnapshot, TransferStatsEvent, UiLogMessage } from "../controllers/types";

export interface SyncEventHandlers {
  onProgress(path: string): void;
  onProgressPercent(progress: number): void;
  onSnapshot(snapshot: SyncSnapshot): void;
  onStarted(): void;
  onTransferStats(stats: TransferStatsEvent): void;
  onError(): void;
  onTriggerSync(): void;
  onLog(message: UiLogMessage): void;
  onIdle(): void;
}

export function subscribeToSyncEvents(handlers: SyncEventHandlers, isActive: () => boolean) {
  const resolved: (() => void)[] = [];
  let disposed = false;

  function add(event: string, handler: (event: { payload: unknown }) => void) {
    void listen(event, handler)
      .then((unlisten) => {
        if (disposed || !isActive()) unlisten();
        else resolved.push(unlisten);
      })
      .catch((error) => console.error(`Failed to subscribe to ${event}`, error));
  }

  add("sync-progress", (event) => handlers.onProgress(event.payload as string));
  add("sync-progress-percent", (event) => handlers.onProgressPercent(event.payload as number));
  add("sync-started", () => handlers.onStarted());
  add("sync-transfer-stats", (event) => handlers.onTransferStats(event.payload as TransferStatsEvent));
  add("sync-progress-snapshot", (event) => handlers.onSnapshot(event.payload as SyncSnapshot));
  add("sync-paused", (event) => handlers.onSnapshot(event.payload as SyncSnapshot));
  add("sync-resumed", (event) => handlers.onSnapshot(event.payload as SyncSnapshot));
  add("sync-error", () => handlers.onError());
  add("trigger-sync", () => handlers.onTriggerSync());
  add("log-message", (event) => handlers.onLog(event.payload as UiLogMessage));
  add("sync-idle", () => handlers.onIdle());

  return () => {
    disposed = true;
    resolved.splice(0).forEach((unlisten) => unlisten());
  };
}
