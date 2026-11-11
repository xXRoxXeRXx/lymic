<script lang="ts">
  import { Album, Loader2, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import AlbumPicker from "./AlbumPicker.svelte";

  interface Props {
    isOpen: boolean;
    catalogue: TargetAlbumCatalogue;
    isAssigning: boolean;
    assignmentError: string;
    onConfirm: (albumId: string) => void;
    onCancel: () => void;
  }

  let { isOpen, catalogue, isAssigning, assignmentError, onConfirm, onCancel }: Props = $props();
  let selectedAlbumId = $state<string | null>(null);
  let isCreating = $state(false);

  function handleKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === "Escape" && !isCreating && !isAssigning) {
      event.preventDefault();
      onCancel();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget && !isAssigning) onCancel();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/40 p-4 backdrop-blur-xs" role="presentation" onclick={handleBackdropClick}>
    <!-- svelte-ignore a11y_no_redundant_roles The issue requires an explicit dialog role. -->
    <dialog open role="dialog" aria-modal="true" aria-labelledby="add-to-album-dialog-title" class="static m-0 w-full max-w-lg space-y-5 rounded-xl border border-slate-200 bg-white p-6 text-slate-900 shadow-xl dark:border-zinc-800 dark:bg-zinc-900 dark:text-slate-100">
      <div class="flex items-start justify-between gap-4">
        <div class="flex min-w-0 items-center gap-3">
          <div class="shrink-0 rounded-lg bg-blue-50 p-2.5 text-blue-600 dark:bg-blue-950/40 dark:text-sky-400"><Album class="h-5 w-5" /></div>
          <div class="min-w-0"><h2 id="add-to-album-dialog-title" class="truncate text-base font-bold">{$t("timeline_add_to_album_modal_title")}</h2><p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400">{$t("timeline_add_to_album_modal_subtitle")}</p></div>
        </div>
        <button type="button" class="shrink-0 rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 disabled:opacity-50 dark:hover:bg-zinc-800 dark:hover:text-zinc-200" aria-label={$t("target_album_close")} disabled={isAssigning} onclick={onCancel}><X class="h-4 w-4" /></button>
      </div>

      <AlbumPicker isOpen={isOpen} {catalogue} bind:selectedAlbumId bind:isCreating />

      {#if assignmentError}<p role="alert" class="text-xs font-medium text-red-600 dark:text-red-400">{assignmentError}</p>{/if}
      <div class="flex items-center justify-end gap-3 border-t border-slate-100 pt-4 dark:border-zinc-800">
        <button type="button" class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:text-zinc-300 dark:hover:bg-zinc-800" disabled={isAssigning} onclick={onCancel}>{$t("target_album_cancel")}</button>
        <button type="button" class="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white shadow-sm disabled:opacity-50" disabled={selectedAlbumId === null || isAssigning} aria-busy={isAssigning} onclick={() => selectedAlbumId && onConfirm(selectedAlbumId)}>{#if isAssigning}<Loader2 class="h-4 w-4 animate-spin" />{/if}{$t("timeline_add_to_album_confirm")}</button>
      </div>
    </dialog>
  </div>
{/if}
