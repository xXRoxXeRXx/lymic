import type {
  FailedSyncEntry,
  LogEntry,
  SyncSnapshot,
  SyncStatus,
  SyncSummary,
  TransferStatsEvent,
  UiLogMessage,
} from "../contracts";
import type { SyncDesktopAdapter } from "../tauri/desktop-adapter";
import { subscribeToSyncEvents } from "../tauri/sync-event-subscription";
import { toBcp47Locale } from "../i18n";

export interface SyncStatusViewState {
  readonly syncStatus: SyncStatus;
  readonly canStartSync: boolean;
  readonly lastSync: string | null;
  readonly progress: number;
  readonly processed: number;
  readonly syncTotal: number;
  readonly currentFile: string;
  readonly transferredBytes: number;
  readonly transferRateBytesPerSecond: number | null;
  readonly estimatedSecondsRemaining: number | null;
  readonly failedSyncs: FailedSyncEntry[];
  readonly isSyncActionPending: boolean;
  onSyncAction(): Promise<void>;
  onRetryFailedSyncs(): Promise<void>;
}

export interface SyncController {
  readonly status: SyncStatusViewState;
  readonly isActive: boolean;
  readonly logs: LogEntry[];
  startLifecycle(): void;
  hydrate(): Promise<void>;
  startSubscriptions(): void;
  refreshFailedSyncs(): Promise<void>;
  addLog(payload: UiLogMessage): void;
  clearUserState(): void;
  destroy(): void;
}

export interface SyncControllerOptions {
  adapter: SyncDesktopAdapter;
  reportError(error: unknown): void;
  locale(): string;
  canStartSync?(): boolean;
  onSyncSettled?(): Promise<void> | void;
}

export function createSyncController(options: SyncControllerOptions): SyncController {
  const { adapter } = options;
  const canStartSync = options.canStartSync ?? (() => true);

  let syncStatus = $state<SyncStatus>("idle");
  let lastSync = $state<string | null>(null);
  let progress = $state(0);
  let processed = $state(0);
  let syncTotal = $state(0);
  let currentFile = $state("");
  let transferredBytes = $state(0);
  let transferRateBytesPerSecond = $state<number | null>(null);
  let estimatedSecondsRemaining = $state<number | null>(null);
  let failedSyncs = $state<FailedSyncEntry[]>([]);
  let logs = $state<LogEntry[]>([]);
  let isSyncActionPending = $state(false);
  let mounted = false;
  let logCounter = 0;
  let progressResetTimer: ReturnType<typeof setTimeout> | undefined;
  let stopEvents: (() => void) | undefined;

  function resetTransferStats() {
    transferredBytes = 0;
    transferRateBytesPerSecond = null;
    estimatedSecondsRemaining = null;
  }

  function isSyncActive() {
    return syncStatus === "syncing" || syncStatus === "paused";
  }

  function addLog(payload: UiLogMessage) {
    let level: LogEntry["level"] = "info";
    if (payload.level === "ERROR") level = "error";
    if (payload.level === "SUCCESS") level = "success";
    if (payload.level === "WARN") level = "warn";

    const timestamp = typeof payload.timestamp === "number" && Number.isFinite(payload.timestamp)
      ? payload.timestamp
      : Date.now();
    const time = new Date(timestamp).toLocaleTimeString(toBcp47Locale(options.locale()), {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    logs = [{
      id: ++logCounter,
      time,
      level,
      message: payload.message ?? "",
      key: payload.key,
      params: payload.params,
    }, ...logs].slice(0, 50);
  }

  function applySyncSnapshot(snapshot: SyncSnapshot | null | undefined) {
    if (!snapshot || !mounted) return;

    syncStatus = snapshot.status === "FAILED"
      ? "error"
      : snapshot.status === "PAUSED"
      ? "paused"
      : snapshot.status === "RUNNING"
        ? "syncing"
        : snapshot.failed > 0 ? "error" : "idle";
    processed = snapshot.succeeded + snapshot.failed;
    syncTotal = snapshot.total;
    progress = snapshot.totalBytes && snapshot.totalBytes > 0
      ? Math.min(100, Math.round(((snapshot.completedBytes ?? 0) / snapshot.totalBytes) * 100))
      : snapshot.total > 0
        ? Math.min(100, Math.round(((snapshot.succeeded + snapshot.failed) / snapshot.total) * 100))
        : 0;
    if (snapshot.currentPath) currentFile = snapshot.currentPath.split(/[/\\]/).pop() || "";
  }

  async function refreshFailedSyncs() {
    try {
      failedSyncs = await adapter.getFailedSyncs();
      if (failedSyncs.length > 0 && syncStatus === "idle") syncStatus = "error";
    } catch (error) {
      console.error("Failed to load sync failures", error);
      options.reportError(error);
    }
  }

  async function hydrate() {
    try {
      applySyncSnapshot(await adapter.getSyncStatus());
    } catch (error) {
      console.warn("Failed to load sync status", error);
    }
  }

  async function finishSync(summary?: SyncSummary) {
    if (!mounted) return;
    await refreshFailedSyncs();
    if (!mounted) return;
    await options.onSyncSettled?.();
    if (!mounted) return;

    if (syncStatus === "error" || !!summary?.failed || failedSyncs.length > 0) {
      syncStatus = "error";
      progress = 0;
      return;
    }

    syncStatus = "idle";
    progress = 100;
    lastSync = new Date().toLocaleTimeString(toBcp47Locale(options.locale()), {
      hour: "2-digit",
      minute: "2-digit",
    });
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = setTimeout(() => {
      if (mounted) progress = 0;
      progressResetTimer = undefined;
    }, 2000);
  }

  async function startSync() {
    if (isSyncActive() || isSyncActionPending || !canStartSync()) return;
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    syncStatus = "syncing";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    resetTransferStats();
    try {
      await finishSync(await adapter.startSync());
    } catch (error) {
      console.error("Sync failed", error);
      syncStatus = "error";
    }
  }

  async function syncAction() {
    if (isSyncActionPending) return;
    if (!isSyncActive()) return startSync();

    isSyncActionPending = true;
    try {
      applySyncSnapshot(await (syncStatus === "syncing" ? adapter.pauseSync() : adapter.resumeSync()));
    } catch (error) {
      options.reportError(error);
    } finally {
      isSyncActionPending = false;
    }
  }

  async function retryFailedSyncs() {
    if (syncStatus === "syncing" || failedSyncs.length === 0) return;
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    syncStatus = "syncing";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    resetTransferStats();
    try {
      await finishSync(await adapter.retryFailedSyncs());
    } catch (error) {
      console.error("Failed to retry sync failures", error);
      options.reportError(error);
      await refreshFailedSyncs();
      syncStatus = "error";
    }
  }

  function startSubscriptions() {
    if (!mounted || stopEvents) return;
    stopEvents = subscribeToSyncEvents({
      onProgress(path) {
        if (!mounted) return;
        syncStatus = "syncing";
        currentFile = path.split(/[/\\]/).pop() || "";
      },
      onProgressPercent(value) {
        if (mounted) progress = value;
      },
      onStarted() {
        if (!mounted) return;
        if (progressResetTimer) clearTimeout(progressResetTimer);
        progressResetTimer = undefined;
        syncStatus = "syncing";
        resetTransferStats();
      },
      onTransferStats(stats: TransferStatsEvent) {
        if (!mounted) return;
        transferredBytes = stats.transferredBytes ?? 0;
        transferRateBytesPerSecond = stats.transferRateBytesPerSecond > 0 ? stats.transferRateBytesPerSecond : null;
        estimatedSecondsRemaining = stats.estimatedSecondsRemaining;
      },
      onSnapshot: applySyncSnapshot,
      onError() {
        if (!mounted) return;
        syncStatus = "error";
        progress = 0;
      },
      onTriggerSync: startSync,
      onLog: addLog,
      onIdle() {
        void finishSync();
      },
    }, () => mounted);
  }

  function startLifecycle() {
    mounted = true;
  }

  function clearUserState() {
    syncStatus = "idle";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    currentFile = "";
    resetTransferStats();
    failedSyncs = [];
    logs = [];
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
  }

  function destroy() {
    mounted = false;
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    stopEvents?.();
    stopEvents = undefined;
  }

  return {
    get status(): SyncStatusViewState {
      return {
        syncStatus,
        canStartSync: canStartSync(),
        lastSync,
        progress,
        processed,
        syncTotal,
        currentFile,
        transferredBytes,
        transferRateBytesPerSecond,
        estimatedSecondsRemaining,
        failedSyncs,
        isSyncActionPending,
        onSyncAction: syncAction,
        onRetryFailedSyncs: retryFailedSyncs,
      };
    },
    get isActive() { return isSyncActive(); },
    get logs() { return logs; },
    startLifecycle,
    hydrate,
    startSubscriptions,
    refreshFailedSyncs,
    addLog,
    clearUserState,
    destroy,
  };
}
