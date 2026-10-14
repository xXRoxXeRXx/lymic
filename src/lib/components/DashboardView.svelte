<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import FailedSyncsModal from "./FailedSyncsModal.svelte";
  import FolderList from "./FolderList.svelte";
  import ActivityLog from "./ActivityLog.svelte";
  import TargetAlbumModal from "./TargetAlbumModal.svelte";
  import type { DashboardViewState } from "../controllers/sync-controller.svelte";

  interface Props {
    dashboard: DashboardViewState;
    serverUrl: string;
  }

  let { dashboard, serverUrl }: Props = $props();

  const transferredBytes = $derived(dashboard.transferredBytes);
  const transferRateBytesPerSecond = $derived(dashboard.transferRateBytesPerSecond);
  const estimatedSecondsRemaining = $derived(dashboard.estimatedSecondsRemaining);

  const numberLocale = $derived(toBcp47Locale($locale));
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
        <div class="w-2 h-2 rounded-full {dashboard.syncStatus === 'syncing' ? 'bg-blue-500 animate-pulse' : dashboard.syncStatus === 'paused' ? 'bg-amber-500' : dashboard.syncStatus === 'error' ? 'bg-red-500' : 'bg-emerald-500'}"></div>
        <span class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('status')}</span>
      </div>
      <h3 class="text-3xl font-bold text-slate-900 dark:text-slate-100">
        {dashboard.syncStatus === 'syncing' ? $t('sync_running') : dashboard.syncStatus === 'paused' ? $t('sync_paused') : dashboard.syncStatus === 'error' ? $t('sync_completed_with_errors') : $t('all_up_to_date')}
      </h3>
    </div>
    <button
      class="btn-action"
      onclick={dashboard.onSyncAction}
      disabled={dashboard.isSyncActionPending || ((dashboard.syncStatus !== 'syncing' && dashboard.syncStatus !== 'paused') && dashboard.watchedFolders.length === 0)}
      title={dashboard.watchedFolders.length === 0 ? $t('sync_requires_folder') : undefined}
      aria-describedby={dashboard.watchedFolders.length === 0 ? 'sync-requires-folder' : undefined}
    >
      {dashboard.syncStatus === 'syncing' ? $t('pause_sync') : dashboard.syncStatus === 'paused' ? $t('resume_sync') : $t('sync_now')}
    </button>
    {#if dashboard.watchedFolders.length === 0}
      <span id="sync-requires-folder" class="sr-only">{$t('sync_requires_folder')}</span>
    {/if}
  </div>
  {#if dashboard.syncStatus === 'syncing' || dashboard.syncStatus === 'paused'}
    <div class="space-y-3">
      <div class="glass-progress"><div class="glass-progress-fill" style="width: {dashboard.progress}%;"></div></div>
      <div class="flex justify-between text-xs font-medium text-slate-400 dark:text-slate-400">
        <span>{dashboard.progress}% {$t('completed')}</span>
        <span class="truncate max-w-[250px]">{dashboard.currentFile}</span>
      </div>
      <p class="text-xs font-medium text-slate-400 dark:text-slate-400">
        {$t('processed')}: {dashboard.processed.toLocaleString(numberLocale)} / {dashboard.syncTotal.toLocaleString(numberLocale)}
      </p>
      {#if dashboard.transferredBytes > 0 || dashboard.transferRateBytesPerSecond !== null || dashboard.estimatedSecondsRemaining !== null}
        <p class="text-xs font-medium text-slate-400 dark:text-slate-400">
          {#if dashboard.transferredBytes > 0}{$t('transferred')}: {formatBytes(dashboard.transferredBytes)}{/if}
          {#if transferRateBytesPerSecond !== null} {transferredBytes > 0 ? ' Â· ' : ''}{$t('transfer_rate')}: {(transferRateBytesPerSecond / 1_000_000).toLocaleString(numberLocale, { maximumFractionDigits: 1 })} MB/s{/if}
          {#if estimatedSecondsRemaining !== null} {(transferredBytes > 0 || transferRateBytesPerSecond !== null) ? ' Â· ' : ''}{$t('remaining_time')}: {formatDuration(estimatedSecondsRemaining)}{/if}
        </p>
      {/if}
      {#if dashboard.syncStatus === 'paused'}
        <p class="text-xs text-amber-600 dark:text-amber-400">{$t('sync_paused_hint')}</p>
      {/if}
    </div>
  {:else}
    <div class="flex gap-10 pt-2 border-t border-black/5 dark:border-white/10">
      <div class="space-y-1"><p class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('last_sync')}</p><p class="font-semibold text-slate-600 dark:text-slate-300">{dashboard.lastSync ?? $t('never')}</p></div>
      <div class="space-y-1"><p class="text-xs font-bold text-slate-400 dark:text-slate-400 uppercase tracking-wider">{$t('server')}</p><p class="font-semibold text-slate-600 dark:text-slate-300">{serverUrl.replace(/https?:\/\//, '')}</p></div>
    </div>
  {/if}
  <FailedSyncsModal failedSyncs={dashboard.failedSyncs} syncStatus={dashboard.syncStatus} onRetry={dashboard.onRetryFailedSyncs} />
</section>
<div class="grid grid-cols-1 xl:grid-cols-2 gap-8">
  <FolderList watchedFolders={dashboard.watchedFolders} onAddFolder={dashboard.onAddFolder} onRemoveFolder={dashboard.onRemoveFolder} />
  <ActivityLog logs={dashboard.logs} />
</div>

<TargetAlbumModal
  isOpen={dashboard.isTargetAlbumModalOpen}
  folderPath={dashboard.pendingFolderPath ?? ""}
  onConfirm={dashboard.onConfirmAddFolder}
  onCancel={dashboard.onCancelAddFolder}
/>
