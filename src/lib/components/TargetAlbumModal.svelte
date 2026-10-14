<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { AlertCircle, Check, Loader2, Plus, RefreshCw, Search, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { Album } from "./types";

  interface Props {
    isOpen: boolean;
    folderPath: string;
    onConfirm: (targetAlbumId: string | null) => void;
    onCancel: () => void;
  }

  let { isOpen, folderPath, onConfirm, onCancel }: Props = $props();

  let albums = $state<Album[]>([]);
  let isLoading = $state(true);
  let loadError = $state<string | null>(null);
  let selectedAlbumId = $state<string | null>(null);
  let searchQuery = $state("");
  let newAlbumName = $state("");
  let createError = $state<string | null>(null);
  let isCreating = $state(false);
  let isCreatingOpen = $state(false);

  const folderName = $derived(folderPath ? folderPath.split(/[/\\]/).pop() || folderPath : "");

  const filteredAlbums = $derived(
    searchQuery.trim() === ""
      ? albums
      : albums.filter((a) => a.albumName.toLowerCase().includes(searchQuery.trim().toLowerCase()))
  );

  async function fetchAlbums() {
    isLoading = true;
    loadError = null;
    try {
      albums = await invoke<Album[]>("get_albums");
    } catch (error) {
      loadError = typeof error === "string" ? error : (error as Error)?.message || "Failed to load albums";
    } finally {
      isLoading = false;
    }
  }

  async function handleCreateAlbum() {
    createError = null;
    const trimmed = newAlbumName.trim();
    if (!trimmed) {
      createError = $t("target_album_name_required");
      return;
    }
    isCreating = true;
    try {
      const created = await invoke<Album>("create_album", { name: trimmed });
      albums = [...albums, created].sort((a, b) => a.albumName.toLowerCase().localeCompare(b.albumName.toLowerCase()));
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
      void fetchAlbums();
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/40 p-4 backdrop-blur-xs animate-in"
    role="presentation"
    onclick={handleBackdropClick}
  >
    <dialog
      open
      class="static m-0 w-full max-w-lg rounded-2xl border-0 bg-white p-6 shadow-2xl dark:bg-slate-900 text-slate-900 dark:text-slate-100 space-y-5"
      aria-labelledby="target-album-dialog-title"
      aria-modal="true"
    >
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 id="target-album-dialog-title" class="text-lg font-bold text-slate-900 dark:text-slate-100">
            {$t("target_album_modal_title")}
          </h2>
          <p class="mt-1 text-xs text-slate-500 dark:text-slate-400">
            {$t("target_album_modal_subtitle", { values: { folder: folderName } })}
          </p>
        </div>
        <button
          type="button"
          class="rounded-lg p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          aria-label={$t("target_album_close")}
          onclick={onCancel}
        >
          <X class="h-5 w-5" />
        </button>
      </div>

      {#if loadError}
        <div class="p-4 rounded-xl bg-red-50 dark:bg-red-950/40 border border-red-200 dark:border-red-900/50 space-y-3">
          <div class="flex items-start gap-2.5 text-xs text-red-600 dark:text-red-400">
            <AlertCircle class="w-4 h-4 mt-0.5 shrink-0" />
            <div class="space-y-1">
              <p class="font-bold">{$t("target_album_load_error")}</p>
              <p class="text-slate-600 dark:text-slate-300">{loadError}</p>
            </div>
          </div>
          <div class="flex items-center gap-3 pt-1">
            <button
              type="button"
              class="flex items-center gap-1.5 rounded-lg bg-red-100 dark:bg-red-900/60 px-3 py-1.5 text-xs font-bold text-red-700 dark:text-red-200 hover:bg-red-200 dark:hover:bg-red-900"
              onclick={fetchAlbums}
            >
              <RefreshCw class="w-3.5 h-3.5" />
              {$t("target_album_retry")}
            </button>
            <button
              type="button"
              class="text-xs font-medium text-slate-600 dark:text-slate-400 hover:underline"
              onclick={() => onConfirm(null)}
            >
              {$t("target_album_proceed_without_album")}
            </button>
          </div>
        </div>
      {:else}
        <!-- Search bar -->
        <div class="relative">
          <Search class="absolute left-3 top-2.5 h-4 w-4 text-slate-400" />
          <input
            type="text"
            class="glass-input !pl-9 !py-2 text-sm"
            placeholder={$t("target_album_search_placeholder")}
            bind:value={searchQuery}
          />
        </div>

        <!-- Album list -->
        <div class="max-h-60 overflow-y-auto space-y-1.5 pr-1">
          <!-- "No album" default option -->
          <button
            type="button"
            class="w-full flex items-center justify-between p-3 rounded-xl border text-left transition-all {selectedAlbumId === null ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40' : 'border-slate-200 dark:border-slate-800 hover:bg-slate-50 dark:hover:bg-slate-800/50'}"
            onclick={() => selectedAlbumId = null}
          >
            <div>
              <p class="text-sm font-bold {selectedAlbumId === null ? 'text-blue-600 dark:text-blue-400' : 'text-slate-800 dark:text-slate-200'}">
                {$t("target_album_none")}
              </p>
              <p class="text-xs text-slate-400 dark:text-slate-500">
                {$t("target_album_none_description")}
              </p>
            </div>
            {#if selectedAlbumId === null}
              <Check class="w-4 h-4 text-blue-600 dark:text-blue-400" />
            {/if}
          </button>

          {#if isLoading}
            <div class="py-6 flex items-center justify-center gap-2 text-xs text-slate-400">
              <Loader2 class="w-4 h-4 animate-spin" />
              <span>{$t("target_album_loading")}</span>
            </div>
          {:else}
            {#each filteredAlbums as album}
              <button
                type="button"
                class="w-full flex items-center justify-between p-3 rounded-xl border text-left transition-all {selectedAlbumId === album.id ? 'border-blue-500 bg-blue-50/60 dark:bg-blue-950/40' : 'border-slate-200 dark:border-slate-800 hover:bg-slate-50 dark:hover:bg-slate-800/50'}"
                onclick={() => selectedAlbumId = album.id}
              >
                <span class="text-sm font-semibold {selectedAlbumId === album.id ? 'text-blue-600 dark:text-blue-400' : 'text-slate-800 dark:text-slate-200'}">
                  {album.albumName}
                </span>
                {#if selectedAlbumId === album.id}
                  <Check class="w-4 h-4 text-blue-600 dark:text-blue-400" />
                {/if}
              </button>
            {/each}

            {#if filteredAlbums.length === 0 && searchQuery.trim() !== ""}
              <p class="text-center py-4 text-xs text-slate-400 italic">
                {$t("target_album_no_albums_found")}
              </p>
            {/if}
          {/if}
        </div>

        <!-- Inline create album -->
        <div class="pt-2 border-t border-slate-100 dark:border-slate-800 space-y-2">
          {#if !isCreatingOpen}
            <button
              type="button"
              class="flex items-center gap-1.5 text-xs font-bold text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300"
              onclick={() => isCreatingOpen = true}
            >
              <Plus class="w-3.5 h-3.5" />
              {$t("target_album_create_new")}
            </button>
          {:else}
            <div class="space-y-2">
              <div class="flex gap-2">
                <input
                  type="text"
                  class="glass-input !py-1.5 text-xs flex-1"
                  placeholder={$t("target_album_name_placeholder")}
                  bind:value={newAlbumName}
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); void handleCreateAlbum(); } }}
                />
                <button
                  type="button"
                  class="rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-bold text-white hover:bg-blue-700 disabled:opacity-50 flex items-center gap-1"
                  onclick={handleCreateAlbum}
                  disabled={isCreating}
                >
                  {#if isCreating}
                    <Loader2 class="w-3 h-3 animate-spin" />
                  {/if}
                  {$t("target_album_create_button")}
                </button>
                <button
                  type="button"
                  class="rounded-lg px-2 py-1.5 text-xs text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
                  onclick={() => { isCreatingOpen = false; createError = null; }}
                >
                  <X class="w-4 h-4" />
                </button>
              </div>
              {#if createError}
                <p class="text-xs text-red-600 dark:text-red-400">{createError}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Action buttons -->
      <div class="pt-4 flex items-center justify-end gap-3 border-t border-slate-100 dark:border-slate-800">
        <button
          type="button"
          class="rounded-lg px-4 py-2 text-sm font-bold text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800"
          onclick={onCancel}
        >
          {$t("target_album_cancel")}
        </button>
        <button
          type="button"
          class="btn-action"
          onclick={() => onConfirm(selectedAlbumId)}
        >
          {$t("target_album_confirm")}
        </button>
      </div>
    </dialog>
  </div>
{/if}
