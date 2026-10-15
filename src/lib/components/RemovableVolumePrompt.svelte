<script lang="ts">
  import { HardDrive, ArrowRight, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { DetectedVolume } from "../controllers/types";

  let {
    volume,
    onAccept,
    onDismiss,
  }: {
    volume: DetectedVolume;
    onAccept: (volume: DetectedVolume) => void;
    onDismiss: (volumeId: string) => void;
  } = $props();
</script>

<div
  role="region"
  aria-label={$t("removable_card_detected_title")}
  class="w-full bg-white/80 dark:bg-slate-900/80 backdrop-blur-md border border-blue-200/80 dark:border-blue-900/50 rounded-2xl p-4 shadow-sm flex items-center justify-between gap-4 transition-all"
>
  <div class="flex items-center gap-3.5 min-w-0">
    <div class="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-950/50 text-blue-600 dark:text-blue-400 flex items-center justify-center shrink-0 border border-blue-100 dark:border-blue-900/40">
      <HardDrive class="w-5 h-5" />
    </div>
    <div class="min-w-0 flex flex-col">
      <div class="flex items-center gap-2">
        <span class="text-xs font-bold uppercase tracking-wider text-blue-600 dark:text-blue-400">
          {$t("removable_card_detected_title")}
        </span>
      </div>
      <p class="text-sm font-semibold text-slate-800 dark:text-slate-100 truncate">
        {volume.displayName}
        <span class="font-normal text-xs text-slate-500 dark:text-slate-400 ml-1">
          ({volume.mountPath})
        </span>
      </p>
    </div>
  </div>

  <div class="flex items-center gap-2 shrink-0">
    <button
      type="button"
      class="text-xs font-medium text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-200 px-3 py-2 rounded-xl transition-colors"
      aria-label={$t("removable_prompt_dismiss")}
      onclick={() => onDismiss(volume.volumeId)}
    >
      {$t("removable_prompt_dismiss")}
    </button>
    <button
      type="button"
      class="inline-flex items-center gap-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 px-3.5 py-2 rounded-xl shadow-sm transition-all hover:shadow"
      aria-label={$t("removable_prompt_sync")}
      onclick={() => onAccept(volume)}
    >
      <span>{$t("removable_prompt_sync")}</span>
      <ArrowRight class="w-3.5 h-3.5" />
    </button>
  </div>
</div>
