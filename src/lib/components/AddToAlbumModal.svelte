<script lang="ts">
  import { Album, AlertCircle, Check, Loader2, Plus, RefreshCw, Search, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import { formatError } from "../controllers/feedback-controller.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

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
  let searchQuery = $state("");
  let newAlbumName = $state("");
  let createError = $state<string | null>(null);
  let isCreating = $state(false);
  let isCreatingOpen = $state(false);

  const filteredAlbums = $derived(
    searchQuery.trim() === ""
      ? catalogue.albums
      : catalogue.albums.filter((album) => album.albumName.toLowerCase().includes(searchQuery.trim().toLowerCase())),
  );

  async function handleCreateAlbum() {
    createError = null;
    const trimmed = newAlbumName.trim();
    if (!trimmed) {
      createError = $t("target_album_name_required");
      return;
    }

    isCreating = true;
    try {
      const created = await catalogue.create(trimmed);
      selectedAlbumId = created.id;
      newAlbumName = "";
      isCreatingOpen = false;
    } catch (error) {
      createError = formatError(error, "Failed to create album");
    } finally {
      isCreating = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === "Escape" && !isCreating && !isAssigning) {
      event.preventDefault();
      onCancel();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget && !isAssigning) onCancel();
  }

  $effect(() => {
    if (isOpen) {
      selectedAlbumId = null;
      searchQuery = "";
      newAlbumName = "";
      createError = null;
      isCreatingOpen = false;
      void catalogue.refresh();
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/40 p-4 backdrop-blur-xs" role="presentation" onclick={handleBackdropClick}>
    <!-- svelte-ignore a11y_no_redundant_roles The issue requires an explicit dialog role. -->
    <dialog open role="dialog" aria-modal="true" aria-labelledby="add-to-album-dialog-title" class="static m-0 w-full max-w-lg space-y-5 rounded-xl border border-slate-200 bg-white p-6 text-slate-900 shadow-xl dark:border-zinc-800 dark:bg-zinc-900 dark:text-slate-100">
      <div class="flex items-start justify-between gap-4">
        <div class="flex min-w-0 items-center gap-3">
          <div class="shrink-0 rounded-lg bg-blue-50 p-2.5 text-blue-600 dark:bg-blue-950/40 dark:text-sky-400"><Album class="h-5 w-5" /></div>
          <div class="min-w-0">
            <h2 id="add-to-album-dialog-title" class="truncate text-base font-bold">{$t("timeline_add_to_album_modal_title")}</h2>
            <p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400">{$t("timeline_add_to_album_modal_subtitle")}</p>
          </div>
        </div>
        <button type="button" class="shrink-0 rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 disabled:opacity-50 dark:hover:bg-zinc-800 dark:hover:text-zinc-200" aria-label={$t("target_album_close")} disabled={isAssigning} onclick={onCancel}><X class="h-4 w-4" /></button>
      </div>

      {#if catalogue.error}
        <div class="space-y-3 rounded-lg border border-red-200/80 bg-red-50/70 p-4 dark:border-red-900/50 dark:bg-red-950/30">
          <div class="flex items-start gap-2.5 text-xs text-red-600 dark:text-red-400"><AlertCircle class="mt-0.5 h-4 w-4 shrink-0" /><div><p class="font-bold">{$t("target_album_load_error")}</p><p class="text-slate-600 dark:text-zinc-300">{catalogue.error}</p></div></div>
          <button type="button" class="flex items-center gap-1.5 rounded-lg bg-red-100 px-3 py-1.5 text-xs font-semibold text-red-700 hover:bg-red-200 dark:bg-red-900/60 dark:text-red-200" onclick={catalogue.refresh}><RefreshCw class="h-3.5 w-3.5" />{$t("target_album_retry")}</button>
        </div>
      {:else}
        <div class="relative">
          <Search class="absolute left-3 top-2.5 h-4 w-4 text-slate-400 dark:text-zinc-500" />
          <input type="text" class="glass-input !py-2 !pl-9 !pr-8 text-xs font-normal" placeholder={$t("target_album_search_placeholder")} bind:value={searchQuery} />
          {#if searchQuery}<button type="button" class="absolute right-2.5 top-2.5 rounded p-0.5 text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200" aria-label={$t("target_album_close")} onclick={() => searchQuery = ""}><X class="h-3.5 w-3.5" /></button>{/if}
        </div>

        <div class="max-h-60 space-y-1.5 overflow-y-auto pr-1">
          {#if catalogue.isLoading}
            <div class="flex flex-col items-center gap-2 py-8 text-xs text-slate-400 dark:text-zinc-500"><Loader2 class="h-4 w-4 animate-spin text-blue-600 dark:text-sky-400" />{$t("target_album_loading")}</div>
          {:else}
            {#each filteredAlbums as album}
              <button type="button" class="flex w-full items-center justify-between rounded-lg border p-2.5 text-left transition-all {selectedAlbumId === album.id ? 'border-blue-500/80 bg-blue-50/70 dark:border-blue-500/80 dark:bg-blue-950/40' : 'border-slate-200/80 bg-white hover:border-slate-300 hover:bg-slate-50 dark:border-zinc-800 dark:bg-zinc-900/50 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/50'}" aria-pressed={selectedAlbumId === album.id} onclick={() => selectedAlbumId = album.id}>
                <span class="flex min-w-0 items-center gap-2.5"><span class="shrink-0 rounded-md p-2 {selectedAlbumId === album.id ? 'bg-blue-100 text-blue-600 dark:bg-blue-900/60 dark:text-sky-400' : 'bg-slate-100 text-slate-400 dark:bg-zinc-800 dark:text-zinc-500'}"><Album class="h-4 w-4" /></span><span class="truncate text-sm font-semibold">{album.albumName}</span></span>
                {#if selectedAlbumId === album.id}<span class="ml-2 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-blue-600 text-white dark:bg-sky-500"><Check class="h-3 w-3 stroke-[3]" /></span>{/if}
              </button>
            {/each}
            {#if filteredAlbums.length === 0 && searchQuery.trim() !== ""}<p class="py-6 text-center text-xs italic text-slate-400 dark:text-zinc-500">{$t("target_album_no_albums_found")}</p>{/if}
          {/if}
        </div>

        <div class="space-y-2 border-t border-slate-100 pt-3 dark:border-zinc-800">
          {#if !isCreatingOpen}
            <button type="button" class="inline-flex items-center gap-1.5 text-xs font-semibold text-blue-600 hover:text-blue-700 dark:text-sky-400 dark:hover:text-sky-300" onclick={() => isCreatingOpen = true}><Plus class="h-3.5 w-3.5" />{$t("target_album_create_new")}</button>
          {:else}
            <div class="space-y-2"><div class="flex gap-2"><input type="text" class="glass-input !flex-1 !px-3 !py-1.5 text-xs" placeholder={$t("target_album_name_placeholder")} bind:value={newAlbumName} onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); void handleCreateAlbum(); } }} /><button type="button" class="flex shrink-0 items-center gap-1.5 rounded-lg bg-blue-600 px-3.5 py-1.5 text-xs font-semibold text-white disabled:opacity-50" disabled={isCreating} onclick={handleCreateAlbum}>{#if isCreating}<Loader2 class="h-3 w-3 animate-spin" />{/if}{$t("target_album_create_button")}</button><button type="button" class="shrink-0 rounded-lg px-2 py-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200" aria-label={$t("target_album_close")} onclick={() => { isCreatingOpen = false; createError = null; }}><X class="h-4 w-4" /></button></div>{#if createError}<p class="text-xs font-medium text-red-600 dark:text-red-400">{createError}</p>{/if}</div>
          {/if}
        </div>
      {/if}

      {#if assignmentError}<p role="alert" class="text-xs font-medium text-red-600 dark:text-red-400">{assignmentError}</p>{/if}
      <div class="flex items-center justify-end gap-3 border-t border-slate-100 pt-4 dark:border-zinc-800">
        <button type="button" class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:text-zinc-300 dark:hover:bg-zinc-800" disabled={isAssigning} onclick={onCancel}>{$t("target_album_cancel")}</button>
        <button type="button" class="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white shadow-sm disabled:opacity-50" disabled={selectedAlbumId === null || isAssigning} aria-busy={isAssigning} onclick={() => selectedAlbumId && onConfirm(selectedAlbumId)}>{#if isAssigning}<Loader2 class="h-4 w-4 animate-spin" />{/if}{$t("timeline_add_to_album_confirm")}</button>
      </div>
    </dialog>
  </div>
{/if}
