<script lang="ts">
  import { t } from "svelte-i18n";
  import type { LogEntry } from "./types";
  interface Props { logs: LogEntry[]; }
  let { logs }: Props = $props();
</script>

<div class="space-y-6 animate-in" style="animation-delay: 200ms">
  <h4 class="text-sm font-bold text-slate-800 dark:text-slate-100 uppercase tracking-widest px-2">
    {$t('activity_log')}
  </h4>
  <div class="glass-pane !p-6 h-full max-h-[400px] overflow-y-auto space-y-4 selectable">
    {#if logs.length === 0}
      <p class="text-xs text-slate-400 dark:text-slate-400 italic">{$t('no_activity')}</p>
    {:else}
      {#each logs as log}
        <div class="flex items-start gap-4 text-[11px] leading-relaxed group">
          <span class="text-slate-400 dark:text-slate-400 font-mono w-14 shrink-0">{log.time}</span>
          <span class="text-slate-500 dark:text-slate-400 group-hover:text-slate-900 dark:group-hover:text-slate-100 transition-colors">{log.message}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>
