<script lang="ts">
  import { AlertCircle, ChevronRight } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { FailedSyncEntry, SyncStatus } from "./types";

  let { failedSyncs, syncStatus, onRetry }: { failedSyncs: FailedSyncEntry[]; syncStatus: SyncStatus; onRetry: () => void } = $props();
  let showFailedSyncs = $state(false);
</script>

{#if failedSyncs.length > 0}
  <div class="border-t border-red-100 dark:border-red-900/60 pt-5 space-y-4">
    <div class="flex items-center justify-between gap-4">
      <button type="button" class="flex items-center gap-2 text-sm font-bold text-red-700 dark:text-red-300 hover:text-red-800 dark:hover:text-red-200" aria-expanded={showFailedSyncs} aria-controls="failed-sync-details-list" onclick={() => showFailedSyncs = !showFailedSyncs}>
        <AlertCircle class="w-4 h-4" />
        {$t('failed_sync_count', { values: { count: failedSyncs.length } })}
        <ChevronRight class="w-4 h-4 transition-transform {showFailedSyncs ? 'rotate-90' : ''}" />
      </button>
      <button type="button" class="text-xs font-bold text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300 disabled:text-slate-300 dark:disabled:text-slate-600" onclick={onRetry} disabled={syncStatus === 'syncing'}>
        {$t('retry_failed_syncs')}
      </button>
    </div>
    {#if showFailedSyncs}
      <div id="failed-sync-details-list" class="max-h-48 overflow-y-auto space-y-3 pr-2" aria-label={$t('failed_sync_details')}>
        {#each failedSyncs as failedSync}
          <div class="rounded-lg bg-red-50 dark:bg-red-950/50 p-3 text-xs">
            <p class="break-all font-mono text-red-800 dark:text-red-200">{failedSync.localPath}</p>
            <p class="mt-1 text-red-600 dark:text-red-300">{failedSync.failureReason || $t('failure_details_unavailable')}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
