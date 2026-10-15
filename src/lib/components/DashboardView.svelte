<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import FailedSyncsModal from "./FailedSyncsModal.svelte";
  import FolderList from "./FolderList.svelte";
  import ActivityLog from "./ActivityLog.svelte";
  import TargetAlbumModal from "./TargetAlbumModal.svelte";
  import { createTargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import type { DashboardViewState } from "../controllers/sync-controller.svelte";

  interface Props {
    dashboard: DashboardViewState;
    serverUrl: string;
  }

  let { dashboard, serverUrl }: Props = $props();
  const targetAlbumCatalogue = createTargetAlbumCatalogue();

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

<div class="space-y-8 animate-in">
  <!-- Top Status Card -->
  <section class="desktop-card p-6 space-y-6">
    <div class="flex items-center justify-between gap-4">
      <div class="space-y-1">
        <div class="flex items-center gap-2">
          <div class="w-2.5 h-2.5 rounded-full {dashboard.syncStatus === 'syncing' ? 'bg-blue-600 animate-pulse' : dashboard.syncStatus === 'paused' ? 'bg-amber-500' : dashboard.syncStatus === 'error' ? 'bg-red-500' : 'bg-emerald-500'}"></div>
          <span class="text-xs font-semibold text-slate-400 dark:text-zinc-500 uppercase tracking-wider">{$t('status')}</span>
        </div>
        <h3 class="text-2xl font-bold text-slate-900 dark:text-slate-100">
          {dashboard.syncStatus === 'syncing' ? $t('sync_running') : dashboard.syncStatus === 'paused' ? $t('sync_paused') : dashboard.syncStatus === 'error' ? $t('sync_completed_with_errors') : $t('all_up_to_date')}
        </h3>
      </div>
      <button
        class="px-5 py-2.5 rounded-lg font-semibold text-sm transition-colors cursor-pointer shadow-sm disabled:opacity-50 {dashboard.syncStatus === 'syncing' ? 'bg-slate-200 dark:bg-zinc-700 text-slate-800 dark:text-slate-100 hover:bg-slate-300 dark:hover:bg-zinc-600' : dashboard.syncStatus === 'paused' ? 'bg-amber-600 hover:bg-amber-700 text-white' : 'bg-blue-600 hover:bg-blue-700 text-white'}"
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
      <div class="space-y-3 pt-2">
        <div class="w-full h-2 rounded-full bg-slate-100 dark:bg-zinc-800 border border-slate-200/60 dark:border-zinc-700/60 overflow-hidden">
          <div class="h-full bg-blue-600 dark:bg-sky-500 rounded-full transition-all duration-300" style="width: {dashboard.progress}%;"></div>
        </div>
        <div class="flex justify-between text-xs font-medium text-slate-500 dark:text-zinc-400">
          <span>{dashboard.progress}% {$t('completed')}</span>
          <span class="truncate max-w-[280px] font-mono text-[11px]">{dashboard.currentFile}</span>
        </div>
        <p class="text-xs font-medium text-slate-500 dark:text-zinc-400">
          {$t('processed')}: {dashboard.processed.toLocaleString(numberLocale)} / {dashboard.syncTotal.toLocaleString(numberLocale)}
        </p>
        {#if dashboard.transferredBytes > 0 || dashboard.transferRateBytesPerSecond !== null || dashboard.estimatedSecondsRemaining !== null}
          <p class="text-xs font-medium text-slate-500 dark:text-zinc-400">
            {#if dashboard.transferredBytes > 0}{$t('transferred')}: {formatBytes(dashboard.transferredBytes)}{/if}
            {#if transferRateBytesPerSecond !== null} {transferredBytes > 0 ? ' · ' : ''}{$t('transfer_rate')}: {(transferRateBytesPerSecond / 1_000_000).toLocaleString(numberLocale, { maximumFractionDigits: 1 })} MB/s{/if}
            {#if estimatedSecondsRemaining !== null} {(transferredBytes > 0 || transferRateBytesPerSecond !== null) ? ' · ' : ''}{$t('remaining_time')}: {formatDuration(estimatedSecondsRemaining)}{/if}
          </p>
        {/if}
        {#if dashboard.syncStatus === 'paused'}
          <p class="text-xs text-amber-600 dark:text-amber-400">{$t('sync_paused_hint')}</p>
        {/if}
      </div>
    {:else}
      <div class="flex gap-10 pt-4 border-t border-slate-100 dark:border-zinc-800">
        <div class="space-y-1">
          <p class="text-[11px] font-bold text-slate-400 dark:text-zinc-500 uppercase tracking-wider">{$t('last_sync')}</p>
          <p class="text-sm font-semibold text-slate-700 dark:text-zinc-300">{dashboard.lastSync ?? $t('never')}</p>
        </div>
        <div class="space-y-1">
          <p class="text-[11px] font-bold text-slate-400 dark:text-zinc-500 uppercase tracking-wider">{$t('server')}</p>
          <p class="text-sm font-semibold text-slate-700 dark:text-zinc-300 font-mono text-xs">{serverUrl.replace(/https?:\/\//, '')}</p>
        </div>
      </div>
    {/if}

    <FailedSyncsModal failedSyncs={dashboard.failedSyncs} syncStatus={dashboard.syncStatus} onRetry={dashboard.onRetryFailedSyncs} />
  </section>

  <!-- Bottom Grid: Folder List & Activity Log -->
  <div class="grid grid-cols-1 xl:grid-cols-2 gap-8 items-start">
    <FolderList watchedFolders={dashboard.watchedFolders} onAddFolder={dashboard.onAddFolder} onRemoveFolder={dashboard.onRemoveFolder} catalogue={targetAlbumCatalogue} />
    <ActivityLog logs={dashboard.logs} />
  </div>
</div>

<TargetAlbumModal
  isOpen={dashboard.isTargetAlbumModalOpen}
  folderPath={dashboard.pendingFolderPath ?? ""}
  onConfirm={dashboard.onConfirmAddFolder}
  onCancel={dashboard.onCancelAddFolder}
  catalogue={targetAlbumCatalogue}
/>
