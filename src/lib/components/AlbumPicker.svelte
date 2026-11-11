<script lang="ts">
  import { Album, AlertCircle, Check, Folder, Loader2, Plus, RefreshCw, Search, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import { formatError } from "../controllers/feedback-controller.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

  interface Props {
    isOpen: boolean;
    catalogue: TargetAlbumCatalogue;
    allowNoAlbum?: boolean;
    onProceedWithoutAlbum?: () => void;
    selectedAlbumId?: string | null;
    isCreating?: boolean;
  }

  let {
    isOpen,
    catalogue,
    allowNoAlbum = false,
    onProceedWithoutAlbum,
    selectedAlbumId = $bindable<string | null>(null),
    isCreating = $bindable(false),
  }: Props = $props();

  let searchQuery = $state("");
  let newAlbumName = $state("");
  let createError = $state<string | null>(null);
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

{#if catalogue.error}
  <div class="space-y-3 rounded-lg border border-red-200/80 bg-red-50/70 p-4 dark:border-red-900/50 dark:bg-red-950/30">
    <div class="flex items-start gap-2.5 text-xs text-red-600 dark:text-red-400">
      <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
      <div class="space-y-1">
        <p class="font-bold">{$t("target_album_load_error")}</p>
        <p class="text-slate-600 dark:text-zinc-300">{catalogue.error}</p>
      </div>
    </div>
    <div class="flex items-center gap-3 pt-1">
      <button type="button" class="flex items-center gap-1.5 rounded-lg bg-red-100 px-3 py-1.5 text-xs font-semibold text-red-700 hover:bg-red-200 dark:bg-red-900/60 dark:text-red-200" onclick={catalogue.refresh}>
        <RefreshCw class="h-3.5 w-3.5" />
        <span>{$t("target_album_retry")}</span>
      </button>
      {#if onProceedWithoutAlbum}
        <button type="button" class="text-xs font-medium text-slate-600 hover:underline dark:text-zinc-400" onclick={onProceedWithoutAlbum}>
          {$t("target_album_proceed_without_album")}
        </button>
      {/if}
    </div>
  </div>
{:else}
  <div class="relative">
    <Search class="absolute left-3 top-2.5 h-4 w-4 text-slate-400 dark:text-zinc-500" />
    <input type="text" class="glass-input !py-2 !pl-9 !pr-8 text-xs font-normal" placeholder={$t("target_album_search_placeholder")} bind:value={searchQuery} />
    {#if searchQuery}
      <button type="button" class="absolute right-2.5 top-2.5 rounded p-0.5 text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200" aria-label={$t("target_album_close")} onclick={() => searchQuery = ""}>
        <X class="h-3.5 w-3.5" />
      </button>
    {/if}
  </div>

  <div class="max-h-60 space-y-1.5 overflow-y-auto pr-1">
    {#if allowNoAlbum}
      <button type="button" class="flex w-full items-center justify-between rounded-lg border p-2.5 text-left transition-all {selectedAlbumId === null ? 'border-blue-500/80 bg-blue-50/70 dark:border-blue-500/80 dark:bg-blue-950/40' : 'border-slate-200/80 bg-white hover:border-slate-300 hover:bg-slate-50 dark:border-zinc-800 dark:bg-zinc-900/50 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/50'}" onclick={() => selectedAlbumId = null}>
        <span class="flex min-w-0 items-center gap-2.5"><span class="shrink-0 rounded-md p-2 {selectedAlbumId === null ? 'bg-blue-100 text-blue-600 dark:bg-blue-900/60 dark:text-sky-400' : 'bg-slate-100 text-slate-400 dark:bg-zinc-800 dark:text-zinc-500'}"><Folder class="h-4 w-4" /></span><span class="min-w-0 space-y-0.5"><span class="block truncate text-sm font-bold {selectedAlbumId === null ? 'text-blue-600 dark:text-sky-400' : 'text-slate-800 dark:text-slate-200'}">{$t("target_album_none")}</span><span class="block truncate text-xs text-slate-500 dark:text-zinc-400">{$t("target_album_none_description")}</span></span></span>
        {#if selectedAlbumId === null}<span class="ml-2 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-blue-600 text-white dark:bg-sky-500"><Check class="h-3 w-3 stroke-[3]" /></span>{/if}
      </button>
    {/if}

    {#if catalogue.isLoading}
      <div class="flex flex-col items-center gap-2 py-8 text-xs text-slate-400 dark:text-zinc-500"><Loader2 class="h-4 w-4 animate-spin text-blue-600 dark:text-sky-400" />{$t("target_album_loading")}</div>
    {:else}
      {#each filteredAlbums as album}
        <button type="button" class="flex w-full items-center justify-between rounded-lg border p-2.5 text-left transition-all {selectedAlbumId === album.id ? 'border-blue-500/80 bg-blue-50/70 dark:border-blue-500/80 dark:bg-blue-950/40' : 'border-slate-200/80 bg-white hover:border-slate-300 hover:bg-slate-50 dark:border-zinc-800 dark:bg-zinc-900/50 dark:hover:border-zinc-700 dark:hover:bg-zinc-800/50'}" aria-pressed={selectedAlbumId === album.id} onclick={() => selectedAlbumId = album.id}>
          <span class="flex min-w-0 items-center gap-2.5"><span class="shrink-0 rounded-md p-2 {selectedAlbumId === album.id ? 'bg-blue-100 text-blue-600 dark:bg-blue-900/60 dark:text-sky-400' : 'bg-slate-100 text-slate-400 dark:bg-zinc-800 dark:text-zinc-500'}"><Album class="h-4 w-4" /></span><span class="truncate text-sm font-semibold {selectedAlbumId === album.id ? 'text-blue-600 dark:text-sky-400' : 'text-slate-800 dark:text-slate-200'}">{album.albumName}</span></span>
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
