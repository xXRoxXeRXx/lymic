<script lang="ts">
  import { X } from "lucide-svelte";
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

<div class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/40 p-4" role="presentation">
  <dialog open class="m-0 w-full max-w-lg rounded-2xl border-0 bg-white p-6 shadow-2xl dark:bg-slate-900" aria-labelledby="update-dialog-title">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h2 id="update-dialog-title" class="text-lg font-bold text-slate-900 dark:text-slate-100">{$t("update_available")}</h2>
        <p class="mt-1 text-sm text-slate-600 dark:text-slate-300">{$t("update_version", { values: { version } })}</p>
      </div>
      <button type="button" class="rounded-lg p-1 text-slate-500 hover:bg-slate-100 disabled:opacity-50 dark:hover:bg-slate-800" aria-label={$t("update_later")} onclick={onLater} disabled={isInstalling}>
        <X class="h-5 w-5" />
      </button>
    </div>

    {#if notes}
      <div class="mt-5 max-h-48 overflow-y-auto rounded-xl bg-slate-50 p-4 text-sm whitespace-pre-wrap text-slate-700 dark:bg-slate-800 dark:text-slate-200">
        <p class="mb-2 font-semibold">{$t("update_notes")}</p>
        {notes}
      </div>
    {/if}

    {#if installState === "downloading"}
      <p class="mt-4 text-sm text-slate-600 dark:text-slate-300">
        {$t("update_downloading")}{progress === null ? "" : ` ${progress}%`}
      </p>
    {:else if installState === "installing"}
      <p class="mt-4 text-sm text-slate-600 dark:text-slate-300">{$t("update_installing")}</p>
    {:else if installState === "error"}
      <p class="mt-4 text-sm text-red-600 dark:text-red-300">{$t("update_install_failed")}</p>
    {:else if isSyncActive}
      <p class="mt-4 text-sm text-amber-700 dark:text-amber-300">{$t("update_sync_blocked")}</p>
    {/if}

    <div class="mt-6 flex justify-end gap-3">
      <button type="button" class="rounded-lg px-4 py-2 text-sm font-bold text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:text-slate-300 dark:hover:bg-slate-800" onclick={onLater} disabled={isInstalling}>
        {$t("update_later")}
      </button>
      <button type="button" class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-bold text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50" onclick={onInstall} disabled={isSyncActive || isInstalling}>
        {installState === "error" ? $t("update_retry") : $t("update_install")}
      </button>
    </div>
  </dialog>
</div>
