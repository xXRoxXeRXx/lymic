<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import FailedSyncsModal from "./FailedSyncsModal.svelte";
  import FolderList from "./FolderList.svelte";
  import ActivityLog from "./ActivityLog.svelte";
  import type { FailedSyncEntry, LogEntry, SyncStatus, WatchedFolder } from "./types";

  interface Props {
    syncStatus: SyncStatus;
    lastSync: string;
    progress: number;
    processed: number;
    syncTotal: number;
    currentFile: string;
    transferredBytes?: number;
    transferRateBytesPerSecond?: number | null;
    estimatedSecondsRemaining?: number | null;
    serverUrl: string;
    watchedFolders: WatchedFolder[];
    failedSyncs: FailedSyncEntry[];
    logs: LogEntry[];
    isSyncActionPending: boolean;
    onSyncAction: () => void;
    onRetryFailedSyncs: () => void;
    onAddFolder: () => void;
    onRemoveFolder: (id: number) => void;
  }

  let {
    syncStatus,
    lastSync,
    progress,
    processed,
    syncTotal,
    currentFile,
    transferredBytes = 0,
    transferRateBytesPerSecond = null,
    estimatedSecondsRemaining = null,
    serverUrl,
    watchedFolders,
    failedSyncs,
    logs,
    isSyncActionPending,
    onSyncAction,
    onRetryFailedSyncs,
    onAddFolder,
    onRemoveFolder
  }: Props = $props();

  const numberLocale = $derived($locale === 'de' ? 'de-DE' : 'en-US');
  function formatBytes(bytes: number) {
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
    return unit === 0 ? `${value} ${units[unit]}` : `${value.toLocaleString(numberLocale, { maximumFractionDigits: 1 })} ${units[unit]}`;
  }
  function formatDuration(seconds: number) {
    if (seconds >= 3600) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
    return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, '0')}s`;
  }
</script>

<section class="glass-pane space-y-8 animate-in">
  <div class="flex items-center justify-between">
    <div class="space-y-1">
      <div class="flex items-center gap-2">
        <div class="w-2 h-2 rounded-full {syncStatus === 'syncing' ? 'bg-blue-500 animate-pulse' : syncStatus === 'paused' ? 'bg-amber-500' : syncStatus === 'error' ? 'bg-red-500' : 'bg-emerald-500'}"></div>
        <span class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('status')}</span>
      </div>
      <h3 class="text-3xl font-bold text-slate-900 dark:text-slate-100">
        {syncStatus === 'syncing' ? $t('sync_running') : syncStatus === 'paused' ? $t('sync_paused') : syncStatus === 'error' ? $t('sync_completed_with_errors') : $t('all_up_to_date')}
      </h3>
    </div>
    <button
      class="btn-action"
      onclick={onSyncAction}
      disabled={isSyncActionPending || ((syncStatus !== 'syncing' && syncStatus !== 'paused') && watchedFolders.length === 0)}
      title={watchedFolders.length === 0 ? $t('sync_requires_folder') : undefined}
      aria-describedby={watchedFolders.length === 0 ? 'sync-requires-folder' : undefined}
    >
      {syncStatus === 'syncing' ? $t('pause_sync') : syncStatus === 'paused' ? $t('resume_sync') : $t('sync_now')}
    </button>
    {#if watchedFolders.length === 0}
      <span id="sync-requires-folder" class="sr-only">{$t('sync_requires_folder')}</span>
    {/if}
  </div>
  {#if syncStatus === 'syncing' || syncStatus === 'paused'}
    <div class="space-y-3">
      <div class="glass-progress"><div class="glass-progress-fill" style="width: {progress}%;"></div></div>
      <div class="flex justify-between text-xs font-medium text-slate-400 dark:text-slate-400">
        <span>{progress}% {$t('completed')}</span>
        <span class="truncate max-w-[250px]">{currentFile}</span>
      </div>
      <p class="text-xs font-medium text-slate-400 dark:text-slate-400">
        {$t('processed')}: {processed.toLocaleString($locale === 'de' ? 'de-DE' : 'en-US')} / {syncTotal.toLocaleString($locale === 'de' ? 'de-DE' : 'en-US')}
      </p>
      {#if transferredBytes > 0 || transferRateBytesPerSecond !== null || estimatedSecondsRemaining !== null}
        <p class="text-xs font-medium text-slate-400 dark:text-slate-400">
          {#if transferredBytes > 0}{$t('transferred')}: {formatBytes(transferredBytes)}{/if}
          {#if transferRateBytesPerSecond !== null} {transferredBytes > 0 ? ' · ' : ''}{$t('transfer_rate')}: {(transferRateBytesPerSecond / 1_000_000).toLocaleString(numberLocale, { maximumFractionDigits: 1 })} MB/s{/if}
          {#if estimatedSecondsRemaining !== null} {(transferredBytes > 0 || transferRateBytesPerSecond !== null) ? ' · ' : ''}{$t('remaining_time')}: {formatDuration(estimatedSecondsRemaining)}{/if}
        </p>
      {/if}
      {#if syncStatus === 'paused'}
        <p class="text-xs text-amber-600 dark:text-amber-400">{$t('sync_paused_hint')}</p>
      {/if}
    </div>
  {:else}
    <div class="flex gap-10 pt-2 border-t border-black/5 dark:border-white/10">
      <div class="space-y-1"><p class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('last_sync')}</p><p class="font-semibold text-slate-600 dark:text-slate-300">{lastSync === 'Noch nie' ? $t('never') : lastSync}</p></div>
      <div class="space-y-1"><p class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('server')}</p><p class="font-semibold text-slate-600 dark:text-slate-300 truncate max-w-[150px]">{serverUrl.replace(/https?:\/\//, '')}</p></div>
    </div>
  {/if}
  <FailedSyncsModal {failedSyncs} {syncStatus} onRetry={onRetryFailedSyncs} />
</section>
<div class="grid grid-cols-1 xl:grid-cols-2 gap-8">
  <FolderList {watchedFolders} {onAddFolder} {onRemoveFolder} />
  <ActivityLog {logs} />
</div>
