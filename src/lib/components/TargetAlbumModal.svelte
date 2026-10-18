<script lang="ts">
  import { Album, AlertCircle, Check, Folder, Loader2, Plus, RefreshCw, Search, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

  interface Props {
    isOpen: boolean;
    folderPath: string;
    onConfirm: (targetAlbumId: string | null) => void;
    onCancel: () => void;
    catalogue: TargetAlbumCatalogue;
  }

  let { isOpen, folderPath, onConfirm, onCancel, catalogue }: Props = $props();

  let selectedAlbumId = $state<string | null>(null);
  let searchQuery = $state("");
  let newAlbumName = $state("");
  let createError = $state<string | null>(null);
  let isCreating = $state(false);
  let isCreatingOpen = $state(false);

  const folderName = $derived(folderPath ? folderPath.split(/[/\\]/).pop() || folderPath : "");

  const filteredAlbums = $derived(
    searchQuery.trim() === ""
      ? catalogue.albums
      : catalogue.albums.filter((a) => a.albumName.toLowerCase().includes(searchQuery.trim().toLowerCase()))
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
      createError = typeof error === "string" ? error : (error as Error)?.message || "Failed to create album";
    } finally {
      isCreating = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!isOpen) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      onCancel();
    }
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
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 animate-in"
    role="presentation"
    onclick={handleBackdropClick}
  >
    <dialog
      open
      class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 dark:border-zinc-800 bg-white p-6 shadow-xl dark:bg-zinc-900 text-slate-900 dark:text-slate-100 space-y-5"
      aria-labelledby="target-album-dialog-title"
      aria-modal="true"
    >
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0">
            <Album class="w-5 h-5" />
          </div>
          <div class="min-w-0">
            <h2 id="target-album-dialog-title" class="text-base font-bold text-slate-900 dark:text-slate-100 truncate">
              {$t("target_album_modal_title")}
            </h2>
            <p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400 truncate">
              {$t("target_album_modal_subtitle", { values: { folder: folderName } })}
            </p>
          </div>
        </div>
        <button
          type="button"
          class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-zinc-800 dark:hover:text-zinc-200 transition-colors cursor-pointer shrink-0"
          aria-label={$t("target_album_close")}
          onclick={onCancel}
        >
          <X class="h-4 w-4" />
        </button>
      </div>

      {#if catalogue.error}
        <div class="p-4 rounded-lg bg-red-50/70 dark:bg-red-950/30 border border-red-200/80 dark:border-red-900/50 space-y-3">
          <div class="flex items-start gap-2.5 text-xs text-red-600 dark:text-red-400">
            <AlertCircle class="w-4 h-4 mt-0.5 shrink-0" />
            <div class="space-y-1">
              <p class="font-bold">{$t("target_album_load_error")}</p>
              <p class="text-slate-600 dark:text-zinc-300">{catalogue.error}</p>
            </div>
          </div>
          <div class="flex items-center gap-3 pt-1">
            <button
              type="button"
              class="flex items-center gap-1.5 rounded-lg bg-red-100 dark:bg-red-900/60 px-3 py-1.5 text-xs font-semibold text-red-700 dark:text-red-200 hover:bg-red-200 dark:hover:bg-red-900 transition-colors cursor-pointer"
              onclick={catalogue.refresh}
            >
              <RefreshCw class="w-3.5 h-3.5" />
              <span>{$t("target_album_retry")}</span>
            </button>
            <button
              type="button"
              class="text-xs font-medium text-slate-600 dark:text-zinc-400 hover:underline cursor-pointer"
              onclick={() => onConfirm(null)}
            >
              {$t("target_album_proceed_without_album")}
            </button>
          </div>
        </div>
      {:else}
        <!-- Search bar -->
        <div class="relative">
          <Search class="absolute left-3 top-2.5 h-4 w-4 text-slate-400 dark:text-zinc-500" />
          <input
            type="text"
            class="glass-input !pl-9 !py-2 !pr-8 text-xs font-normal"
            placeholder={$t("target_album_search_placeholder")}
            bind:value={searchQuery}
          />
          {#if searchQuery}
            <button
              type="button"
              class="absolute right-2.5 top-2.5 p-0.5 rounded text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200 transition-colors cursor-pointer"
              onclick={() => searchQuery = ""}
            >
              <X class="w-3.5 h-3.5" />
            </button>
          {/if}
        </div>

        <!-- Album list -->
        <div class="max-h-60 overflow-y-auto space-y-1.5 pr-1">
          <!-- "No album" default option -->
          <button
            type="button"
            class="w-full flex items-center justify-between p-2.5 rounded-lg border text-left transition-all cursor-pointer {selectedAlbumId === null ? 'border-blue-500/80 bg-blue-50/70 dark:bg-blue-950/40 dark:border-blue-500/80' : 'border-slate-200/80 dark:border-zinc-800 bg-white dark:bg-zinc-900/50 hover:border-slate-300 dark:hover:border-zinc-700 hover:bg-slate-50 dark:hover:bg-zinc-800/50'}"
            onclick={() => selectedAlbumId = null}
          >
            <div class="flex items-center gap-2.5 min-w-0">
              <div class="p-2 rounded-md {selectedAlbumId === null ? 'bg-blue-100 dark:bg-blue-900/60 text-blue-600 dark:text-sky-400' : 'bg-slate-100 dark:bg-zinc-800 text-slate-400 dark:text-zinc-500'} shrink-0">
                <Folder class="w-4 h-4" />
              </div>
              <div class="min-w-0 space-y-0.5">
                <p class="text-sm font-bold {selectedAlbumId === null ? 'text-blue-600 dark:text-sky-400' : 'text-slate-800 dark:text-slate-200'} truncate">
                  {$t("target_album_none")}
                </p>
                <p class="text-xs text-slate-500 dark:text-zinc-400 truncate">
                  {$t("target_album_none_description")}
                </p>
              </div>
            </div>
            {#if selectedAlbumId === null}
              <div class="w-5 h-5 rounded-full bg-blue-600 dark:bg-sky-500 text-white flex items-center justify-center shrink-0 ml-2">
                <Check class="w-3 h-3 stroke-[3]" />
              </div>
            {/if}
          </button>

          {#if catalogue.isLoading}
            <div class="py-8 flex flex-col items-center justify-center gap-2 text-xs text-slate-400 dark:text-zinc-500">
              <Loader2 class="w-4 h-4 animate-spin text-blue-600 dark:text-sky-400" />
              <span>{$t("target_album_loading")}</span>
            </div>
          {:else}
            {#each filteredAlbums as album}
              <button
                type="button"
                class="w-full flex items-center justify-between p-2.5 rounded-lg border text-left transition-all cursor-pointer {selectedAlbumId === album.id ? 'border-blue-500/80 bg-blue-50/70 dark:bg-blue-950/40 dark:border-blue-500/80' : 'border-slate-200/80 dark:border-zinc-800 bg-white dark:bg-zinc-900/50 hover:border-slate-300 dark:hover:border-zinc-700 hover:bg-slate-50 dark:hover:bg-zinc-800/50'}"
                onclick={() => selectedAlbumId = album.id}
              >
                <div class="flex items-center gap-2.5 min-w-0">
                  <div class="p-2 rounded-md {selectedAlbumId === album.id ? 'bg-blue-100 dark:bg-blue-900/60 text-blue-600 dark:text-sky-400' : 'bg-slate-100 dark:bg-zinc-800 text-slate-400 dark:text-zinc-500'} shrink-0">
                    <Album class="w-4 h-4" />
                  </div>
                  <span class="text-sm font-semibold {selectedAlbumId === album.id ? 'text-blue-600 dark:text-sky-400' : 'text-slate-800 dark:text-slate-200'} truncate">
                    {album.albumName}
                  </span>
                </div>
                {#if selectedAlbumId === album.id}
                  <div class="w-5 h-5 rounded-full bg-blue-600 dark:bg-sky-500 text-white flex items-center justify-center shrink-0 ml-2">
                    <Check class="w-3 h-3 stroke-[3]" />
                  </div>
                {/if}
              </button>
            {/each}

            {#if filteredAlbums.length === 0 && searchQuery.trim() !== ""}
              <p class="text-center py-6 text-xs text-slate-400 dark:text-zinc-500 italic">
                {$t("target_album_no_albums_found")}
              </p>
            {/if}
          {/if}
        </div>

        <!-- Inline create album -->
        <div class="pt-3 border-t border-slate-100 dark:border-zinc-800 space-y-2">
          {#if !isCreatingOpen}
            <button
              type="button"
              class="inline-flex items-center gap-1.5 text-xs font-semibold text-blue-600 dark:text-sky-400 hover:text-blue-700 dark:hover:text-sky-300 transition-colors cursor-pointer"
              onclick={() => isCreatingOpen = true}
            >
              <Plus class="w-3.5 h-3.5" />
              <span>{$t("target_album_create_new")}</span>
            </button>
          {:else}
            <div class="space-y-2">
              <div class="flex gap-2">
                <input
                  type="text"
                  class="glass-input !py-1.5 !px-3 text-xs flex-1"
                  placeholder={$t("target_album_name_placeholder")}
                  bind:value={newAlbumName}
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); void handleCreateAlbum(); } }}
                />
                <button
                  type="button"
                  class="rounded-lg bg-blue-600 hover:bg-blue-700 disabled:opacity-50 px-3.5 py-1.5 text-xs font-semibold text-white shadow-sm transition-colors flex items-center gap-1.5 cursor-pointer shrink-0"
                  onclick={handleCreateAlbum}
                  disabled={isCreating}
                >
                  {#if isCreating}
                    <Loader2 class="w-3 h-3 animate-spin" />
                  {/if}
                  <span>{$t("target_album_create_button")}</span>
                </button>
                <button
                  type="button"
                  class="rounded-lg px-2 py-1.5 text-xs text-slate-400 hover:text-slate-600 hover:bg-slate-100 dark:text-zinc-400 dark:hover:text-zinc-200 dark:hover:bg-zinc-800 transition-colors cursor-pointer shrink-0"
                  onclick={() => { isCreatingOpen = false; createError = null; }}
                >
                  <X class="w-4 h-4" />
                </button>
              </div>
              {#if createError}
                <p class="text-xs font-medium text-red-600 dark:text-red-400">{createError}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Action buttons -->
      <div class="pt-4 flex items-center justify-end gap-3 border-t border-slate-100 dark:border-zinc-800">
        <button
          type="button"
          class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 dark:text-zinc-300 dark:hover:bg-zinc-800 transition-colors cursor-pointer"
          onclick={onCancel}
        >
          {$t("target_album_cancel")}
        </button>
        <button
          type="button"
          class="rounded-lg bg-blue-600 hover:bg-blue-700 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors cursor-pointer"
          onclick={() => onConfirm(selectedAlbumId)}
        >
          {$t("target_album_confirm")}
        </button>
      </div>
    </dialog>
  </div>
{/if}
