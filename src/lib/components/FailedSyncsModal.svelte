<script lang="ts">
  import { AlertCircle, ChevronRight } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { FailedSyncEntry, SyncStatus } from "./types";

  let { failedSyncs, syncStatus, onRetry }: { failedSyncs: FailedSyncEntry[]; syncStatus: SyncStatus; onRetry: () => void } = $props();
  let showFailedSyncs = $state(false);
</script>

{#if failedSyncs.length > 0}
  <div class="border-t border-red-100 dark:border-red-900/40 pt-4 space-y-3">
    <div class="flex items-center justify-between gap-4">
      <button
        type="button"
        class="flex items-center gap-2 text-xs font-semibold text-red-600 dark:text-red-400 hover:text-red-700 dark:hover:text-red-300 cursor-pointer"
        aria-expanded={showFailedSyncs}
        aria-controls="failed-sync-details-list"
        onclick={() => showFailedSyncs = !showFailedSyncs}
      >
        <AlertCircle class="w-4 h-4 shrink-0" />
        <span>{$t('failed_sync_count', { values: { count: failedSyncs.length } })}</span>
        <ChevronRight class="w-3.5 h-3.5 transition-transform {showFailedSyncs ? 'rotate-90' : ''}" />
      </button>
      <button
        type="button"
        class="text-xs font-semibold text-blue-600 dark:text-sky-400 hover:text-blue-700 dark:hover:text-sky-300 disabled:text-slate-300 dark:disabled:text-slate-600 cursor-pointer"
        onclick={onRetry}
        disabled={syncStatus === 'syncing'}
      >
        {$t('retry_failed_syncs')}
      </button>
    </div>
    {#if showFailedSyncs}
      <div id="failed-sync-details-list" class="max-h-48 overflow-y-auto space-y-2 pr-2" aria-label={$t('failed_sync_details')}>
        {#each failedSyncs as failedSync}
          <div class="rounded-lg bg-red-50/70 dark:bg-red-950/30 border border-red-200/60 dark:border-red-900/40 p-2.5 text-xs">
            <p class="break-all font-mono text-[11px] text-red-800 dark:text-red-300">{failedSync.localPath}</p>
            <p class="mt-0.5 text-red-600 dark:text-red-400 text-[11px]">{failedSync.failureReason || $t('failure_details_unavailable')}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
