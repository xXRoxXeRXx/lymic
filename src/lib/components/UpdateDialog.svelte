<script lang="ts">
  import { AlertCircle, Loader2, Sparkles, X } from "lucide-svelte";
  import { t } from "svelte-i18n";

  interface Props {
    version: string;
    notes: string;
    isSyncActive: boolean;
    installState: "idle" | "downloading" | "installing" | "error";
    downloadedBytes: number;
    contentLength: number | null;
    onLater: () => void;
    onInstall: () => void;
  }

  let {
    version,
    notes,
    isSyncActive,
    installState,
    downloadedBytes,
    contentLength,
    onLater,
    onInstall
  }: Props = $props();

  const isInstalling = $derived(installState === "downloading" || installState === "installing");
  const progress = $derived(contentLength && contentLength > 0
    ? Math.min(100, Math.round((downloadedBytes / contentLength) * 100))
    : null);
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 animate-in" role="presentation">
  <dialog
    open
    class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 dark:border-zinc-800 bg-white p-6 shadow-xl dark:bg-zinc-900 text-slate-900 dark:text-slate-100 space-y-5"
    aria-labelledby="update-dialog-title"
    aria-modal="true"
  >
    <div class="flex items-start justify-between gap-4">
      <div class="flex items-center gap-3 min-w-0">
        <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0">
          <Sparkles class="w-5 h-5" />
        </div>
        <div class="min-w-0">
          <h2 id="update-dialog-title" class="text-base font-bold text-slate-900 dark:text-slate-100 truncate">
            {$t("update_available")}
          </h2>
          <p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400 truncate">
            {$t("update_version", { values: { version } })}
          </p>
        </div>
      </div>
      <button
        type="button"
        class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200 transition-colors cursor-pointer shrink-0 disabled:opacity-50"
        aria-label={$t("update_later")}
        onclick={onLater}
        disabled={isInstalling}
      >
        <X class="h-4 w-4" />
      </button>
    </div>

    {#if notes}
      <div class="rounded-lg border border-slate-200/80 dark:border-zinc-800 bg-slate-50/60 dark:bg-zinc-800/40 p-4 text-xs whitespace-pre-wrap text-slate-700 dark:text-zinc-300 max-h-48 overflow-y-auto leading-relaxed">
        <p class="mb-2 font-semibold text-slate-900 dark:text-slate-100">{$t("update_notes")}</p>
        {notes}
      </div>
    {/if}

    {#if installState === "downloading"}
      <div class="space-y-2">
        <p class="text-xs text-slate-600 dark:text-zinc-300">
          {$t("update_downloading")}{progress === null ? "" : ` ${progress}%`}
        </p>
        {#if progress !== null}
          <div class="w-full h-2 rounded-full bg-slate-100 dark:bg-zinc-800 border border-slate-200/60 dark:border-zinc-700/60 overflow-hidden">
            <div class="h-full bg-blue-600 dark:bg-sky-500 rounded-full transition-all duration-300" style="width: {progress}%;"></div>
          </div>
        {/if}
      </div>
    {:else if installState === "installing"}
      <div class="flex items-center gap-2.5 text-xs text-slate-600 dark:text-zinc-300">
        <Loader2 class="w-4 h-4 animate-spin text-blue-600 dark:text-sky-400 shrink-0" />
        <p>{$t("update_installing")}</p>
      </div>
    {:else if installState === "error"}
      <div class="p-3.5 rounded-lg bg-red-50/70 dark:bg-red-950/30 border border-red-200/80 dark:border-red-900/50 flex items-center gap-2.5 text-xs text-red-600 dark:text-red-400">
        <AlertCircle class="w-4 h-4 shrink-0" />
        <p>{$t("update_install_failed")}</p>
      </div>
    {:else if isSyncActive}
      <div class="p-3.5 rounded-lg bg-amber-50/70 dark:bg-amber-950/30 border border-amber-200/80 dark:border-amber-900/50 flex items-center gap-2.5 text-xs text-amber-700 dark:text-amber-300">
        <AlertCircle class="w-4 h-4 shrink-0" />
        <p>{$t("update_sync_blocked")}</p>
      </div>
    {/if}

    <div class="pt-4 flex items-center justify-end gap-3 border-t border-slate-100 dark:border-zinc-800">
      <button
        type="button"
        class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:text-zinc-300 dark:hover:bg-zinc-800 transition-colors cursor-pointer"
        onclick={onLater}
        disabled={isInstalling}
      >
        {$t("update_later")}
      </button>
      <button
        type="button"
        class="rounded-lg bg-blue-600 hover:bg-blue-700 px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-50 shadow-sm transition-colors cursor-pointer"
        onclick={onInstall}
        disabled={isSyncActive || isInstalling}
      >
        {installState === "error" ? $t("update_retry") : $t("update_install")}
      </button>
    </div>
  </dialog>
</div>
