<script lang="ts">
  import { Clock } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { LogEntry } from "../contracts";

  interface Props { logs: LogEntry[]; }
  let { logs }: Props = $props();

  function formatLogMessage(log: LogEntry): string {
    if (!log.key) return log.message;
    return $t(log.key, { values: log.params, default: log.message });
  }
</script>

<div class="space-y-4 animate-in">
  <h4 class="text-xs font-bold text-slate-400 dark:text-zinc-500 uppercase tracking-wider flex items-center gap-2 px-1">
    <Clock class="w-3.5 h-3.5" />
    <span>{$t('activity_log')}</span>
  </h4>
  <div class="desktop-card !p-4 h-[calc(100vh-14rem)] min-h-[360px] overflow-y-auto space-y-1.5 selectable">
    {#if logs.length === 0}
      <div class="h-full flex items-center justify-center">
        <p class="text-xs text-slate-400 dark:text-zinc-500 italic">{$t('no_activity')}</p>
      </div>
    {:else}
      {#each logs as log (log.id)}
        <div class="flex items-start gap-3 text-xs leading-relaxed group py-1 border-b border-black/[0.03] dark:border-white/[0.03] last:border-0">
          <span class="text-slate-400 dark:text-zinc-500 font-mono text-[10px] w-12 shrink-0 pt-0.5">{log.time}</span>
          <span class="text-slate-700 dark:text-zinc-300 font-normal">
            {formatLogMessage(log)}
          </span>
        </div>
      {/each}
    {/if}
  </div>
</div>
