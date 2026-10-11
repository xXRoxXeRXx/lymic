import { invoke } from "@tauri-apps/api/core";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import type { FailedSyncEntry, LogEntry, SyncStatus, WatchedFolder } from "../components/types";
import { subscribeToSyncEvents } from "../tauri/sync-event-subscription";
import type { RemoveFolderResult, SyncSnapshot, SyncSummary, TransferStatsEvent, UiLogMessage } from "./types";

export function createSyncController(options: {
  reportError(error: unknown): void;
  t(key: string): string;
  locale(): string;
}) {
  let syncStatus = $state<SyncStatus>("idle");
  let lastSync = $state("Noch nie");
  let progress = $state(0);
  let processed = $state(0);
  let syncTotal = $state(0);
  let currentFile = $state("");
  let transferredBytes = $state(0);
  let transferRateBytesPerSecond = $state<number | null>(null);
  let estimatedSecondsRemaining = $state<number | null>(null);
  let watchedFolders = $state<WatchedFolder[]>([]);
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
    const time = new Date(timestamp).toLocaleTimeString(options.locale() === "de" ? "de-DE" : "en-US", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    logs = [{ id: ++logCounter, time, level, message: payload.message ?? "" }, ...logs].slice(0, 50);
  }

  function applySyncSnapshot(snapshot: SyncSnapshot | null | undefined) {
    if (!snapshot || !mounted) return;

    syncStatus = snapshot.status === "PAUSED"
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
      failedSyncs = await invoke<FailedSyncEntry[]>("get_failed_syncs");
      if (failedSyncs.length > 0 && syncStatus === "idle") syncStatus = "error";
    } catch (error) {
      console.error("Failed to load sync failures", error);
      options.reportError(error);
    }
  }

  async function refreshFolders() {
    try {
      watchedFolders = await invoke<WatchedFolder[]>("get_folders");
    } catch (error) {
      console.error("Failed to load folders", error);
      options.reportError(error);
    }
  }

  async function hydrate() {
    await refreshFolders();
    try {
      applySyncSnapshot(await invoke<SyncSnapshot>("get_sync_status"));
    } catch (error) {
      console.warn("Failed to load sync status", error);
    }
  }

  async function finishSync(summary?: SyncSummary) {
    if (!mounted) return;
    await refreshFailedSyncs();
    if (!mounted) return;

    if (syncStatus === "error" || !!summary?.failed || failedSyncs.length > 0) {
      syncStatus = "error";
      progress = 0;
      return;
    }

    syncStatus = "idle";
    progress = 100;
    lastSync = new Date().toLocaleTimeString(options.locale() === "de" ? "de-DE" : "en-US", {
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
    if (isSyncActive() || isSyncActionPending || watchedFolders.length === 0) return;
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    syncStatus = "syncing";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    resetTransferStats();
    try {
      await finishSync(await invoke<SyncSummary>("start_sync"));
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
      applySyncSnapshot(await invoke<SyncSnapshot>(syncStatus === "syncing" ? "pause_sync" : "resume_sync"));
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
      await finishSync(await invoke<SyncSummary>("retry_failed_syncs"));
    } catch (error) {
      console.error("Failed to retry sync failures", error);
      options.reportError(error);
      await refreshFailedSyncs();
      syncStatus = "error";
    }
  }

  async function addFolder() {
    try {
      const selected = await open({ directory: true, multiple: false, title: options.t("select_folder") });
      if (typeof selected === "string") {
        await invoke<number>("add_folder", { path: selected });
        await refreshFolders();
      }
    } catch (error) {
      console.error("Failed to add folder", error);
      options.reportError(error);
    }
  }

  async function removeFolder(id: number) {
    try {
      const result = await invoke<RemoveFolderResult>("remove_folder", { id });
      if (result?.requiresSyncCancellation) {
        const confirmed = await confirm(options.t("remove_folder_sync_warning"), {
          title: options.t("remove_folder"),
          kind: "warning",
        });
        if (!confirmed) return;
        await invoke<RemoveFolderResult>("remove_folder", { id, confirmCancellation: true });
      }
      await refreshFolders();
      await refreshFailedSyncs();
    } catch (error) {
      console.error("Failed to remove folder", error);
      options.reportError(error);
    }
  }

  function startSubscriptions() {
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
  }

  return {
    get syncStatus() { return syncStatus; },
    get isActive() { return isSyncActive(); },
    get lastSync() { return lastSync; },
    get progress() { return progress; },
    get processed() { return processed; },
    get syncTotal() { return syncTotal; },
    get currentFile() { return currentFile; },
    get transferredBytes() { return transferredBytes; },
    get transferRateBytesPerSecond() { return transferRateBytesPerSecond; },
    get estimatedSecondsRemaining() { return estimatedSecondsRemaining; },
    get watchedFolders() { return watchedFolders; },
    get failedSyncs() { return failedSyncs; },
    get logs() { return logs; },
    get isSyncActionPending() { return isSyncActionPending; },
    startLifecycle,
    addLog,
    refreshFailedSyncs,
    hydrate,
    startSubscriptions,
    destroy,
    clearUserState,
    syncAction,
    retryFailedSyncs,
    addFolder,
    removeFolder,
  };
}
