<script lang="ts">
  import { Album, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import AlbumPicker from "./AlbumPicker.svelte";

  interface Props {
    isOpen: boolean;
    folderPath: string;
    onConfirm: (targetAlbumId: string | null) => void;
    onCancel: () => void;
    catalogue: TargetAlbumCatalogue;
  }

  let { isOpen, folderPath, onConfirm, onCancel, catalogue }: Props = $props();
  let selectedAlbumId = $state<string | null>(null);
  let isCreating = $state(false);

  const folderName = $derived(folderPath ? folderPath.split(/[/\\]/).pop() || folderPath : "");

  function handleKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === "Escape" && !isCreating) {
      event.preventDefault();
      onCancel();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) onCancel();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 animate-in" role="presentation" onclick={handleBackdropClick}>
    <!-- svelte-ignore a11y_no_redundant_roles The issue requires an explicit dialog role. -->
    <dialog open role="dialog" class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 dark:border-zinc-800 bg-white p-6 shadow-xl dark:bg-zinc-900 text-slate-900 dark:text-slate-100 space-y-5" aria-labelledby="target-album-dialog-title" aria-modal="true">
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0"><Album class="w-5 h-5" /></div>
          <div class="min-w-0">
            <h2 id="target-album-dialog-title" class="text-base font-bold text-slate-900 dark:text-slate-100 truncate">{$t("target_album_modal_title")}</h2>
            <p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400 truncate">{$t("target_album_modal_subtitle", { values: { folder: folderName } })}</p>
          </div>
        </div>
        <button type="button" class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200 transition-colors cursor-pointer shrink-0" aria-label={$t("target_album_close")} onclick={onCancel}><X class="h-4 w-4" /></button>
      </div>

      <AlbumPicker isOpen={isOpen} {catalogue} allowNoAlbum onProceedWithoutAlbum={() => onConfirm(null)} bind:selectedAlbumId bind:isCreating />

      <div class="pt-4 flex items-center justify-end gap-3 border-t border-slate-100 dark:border-zinc-800">
        <button type="button" class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 dark:text-zinc-300 dark:hover:bg-zinc-800 transition-colors cursor-pointer" onclick={onCancel}>{$t("target_album_cancel")}</button>
        <button type="button" class="rounded-lg bg-blue-600 hover:bg-blue-700 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors cursor-pointer" onclick={() => onConfirm(selectedAlbumId)}>{$t("target_album_confirm")}</button>
      </div>
    </dialog>
  </div>
{/if}
