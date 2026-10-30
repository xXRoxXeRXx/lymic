<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import FailedSyncsModal from "./FailedSyncsModal.svelte";
  import FolderList from "./FolderList.svelte";
  import ExclusionPatternsModal from "./ExclusionPatternsModal.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import type { SyncStatusViewState } from "../controllers/sync-controller.svelte";
  import type { WatchedFoldersViewState } from "../controllers/watched-folders-controller.svelte";
  import type { WatchedFolder } from "../contracts";

  interface Props {
    status: SyncStatusViewState;
    folders: WatchedFoldersViewState;
    catalogue: TargetAlbumCatalogue;
  }

  let { status, folders, catalogue }: Props = $props();
  let exclusionSettingsFolder = $state<WatchedFolder | null>(null);

  const isSyncRunning = $derived(status.syncStatus === "syncing");
  const isSyncPaused = $derived(status.syncStatus === "paused");
  const isSyncActive = $derived(isSyncRunning || isSyncPaused);
  const isSyncDisabled = $derived(status.isSyncActionPending || (!isSyncActive && !status.canStartSync));
  const statusHeadingKey = $derived(
    isSyncRunning
      ? "sync_running"
      : isSyncPaused
        ? "sync_paused"
        : status.syncStatus === "error"
          ? "sync_completed_with_errors"
          : "all_up_to_date"
  );
  const syncActionKey = $derived(
    isSyncRunning ? "pause_sync" : isSyncPaused ? "resume_sync" : "sync_now"
  );

  const transferredBytes = $derived(status.transferredBytes);
  const transferRateBytesPerSecond = $derived(status.transferRateBytesPerSecond);
  const estimatedSecondsRemaining = $derived(status.estimatedSecondsRemaining);

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
      <div>
        <h3 class="text-2xl font-bold text-slate-900 dark:text-slate-100">
          {$t(statusHeadingKey)}
        </h3>
      </div>
      <button
        class="px-5 py-2.5 rounded-lg font-semibold text-sm transition-colors cursor-pointer shadow-sm disabled:opacity-50 {isSyncRunning ? 'bg-slate-200 dark:bg-zinc-700 text-slate-800 dark:text-slate-100 hover:bg-slate-300 dark:hover:bg-zinc-600' : isSyncPaused ? 'bg-amber-600 hover:bg-amber-700 text-white' : 'bg-blue-600 hover:bg-blue-700 text-white'}"
        onclick={status.onSyncAction}
        disabled={isSyncDisabled}
        title={!status.canStartSync ? $t('sync_requires_folder') : undefined}
        aria-describedby={!status.canStartSync ? 'sync-requires-folder' : undefined}
      >
        {$t(syncActionKey)}
      </button>
      {#if !status.canStartSync}
        <span id="sync-requires-folder" class="sr-only">{$t('sync_requires_folder')}</span>
      {/if}
    </div>

    {#if isSyncActive}
      <div class="space-y-3 pt-2">
        <div class="w-full h-2 rounded-full bg-slate-100 dark:bg-zinc-800 border border-slate-200/60 dark:border-zinc-700/60 overflow-hidden">
          <div class="h-full bg-blue-600 dark:bg-sky-500 rounded-full transition-all duration-300" style="width: {status.progress}%;"></div>
        </div>
        <div class="flex justify-between text-xs font-medium text-slate-500 dark:text-zinc-400">
          <span>{status.progress}% {$t('completed')}</span>
          <span class="truncate max-w-[280px] font-mono text-[11px]">{status.currentFile}</span>
        </div>
        <p class="text-xs font-medium text-slate-500 dark:text-zinc-400">
          {$t('processed')}: {status.processed.toLocaleString(numberLocale)} / {status.syncTotal.toLocaleString(numberLocale)}
        </p>
        {#if transferredBytes > 0 || transferRateBytesPerSecond !== null || estimatedSecondsRemaining !== null}
          <p class="text-xs font-medium text-slate-500 dark:text-zinc-400">
            {#if transferredBytes > 0}{$t('transferred')}: {formatBytes(transferredBytes)}{/if}
            {#if transferRateBytesPerSecond !== null} {transferredBytes > 0 ? ' · ' : ''}{$t('transfer_rate')}: {(transferRateBytesPerSecond / 1_000_000).toLocaleString(numberLocale, { maximumFractionDigits: 1 })} MB/s{/if}
            {#if estimatedSecondsRemaining !== null} {(transferredBytes > 0 || transferRateBytesPerSecond !== null) ? ' · ' : ''}{$t('remaining_time')}: {formatDuration(estimatedSecondsRemaining)}{/if}
          </p>
        {/if}
        {#if isSyncPaused}
          <p class="text-xs text-amber-600 dark:text-amber-400">{$t('sync_paused_hint')}</p>
        {/if}
      </div>
    {:else}
      <div class="pt-4 border-t border-slate-100 dark:border-zinc-800">
        <p class="text-xs text-slate-500 dark:text-zinc-400">
          {$t('last_sync')}: {status.lastSync ? `${status.lastSync} Uhr` : $t('never')}
        </p>
      </div>
    {/if}

    <FailedSyncsModal failedSyncs={status.failedSyncs} syncStatus={status.syncStatus} onRetry={status.onRetryFailedSyncs} />
  </section>

  <!-- Bottom: Folder List -->
  <FolderList watchedFolders={folders.watchedFolders} onAddFolder={folders.onAddFolder} onRemoveFolder={folders.onRemoveFolder} onOpenExclusionSettings={(folder) => exclusionSettingsFolder = folder} {catalogue} />
</div>


<ExclusionPatternsModal folder={exclusionSettingsFolder} onSave={folders.onUpdateFolderExclusionPatterns} onClose={() => exclusionSettingsFolder = null} />
