<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Folder, FolderPlus, Trash2 } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { Album, WatchedFolder } from "./types";

  interface Props {
    watchedFolders: WatchedFolder[];
    onAddFolder: () => void;
    onRemoveFolder: (id: number) => void;
  }

  let { watchedFolders, onAddFolder, onRemoveFolder }: Props = $props();
  let albums = $state<Album[]>([]);
  let latestAlbumRequest = 0;
  let targetAlbumIds = new Set<string>();

  const albumNamesById = $derived(new Map(albums.map((album) => [album.id, album.albumName])));

  async function fetchAlbums() {
    const request = ++latestAlbumRequest;
    try {
      const loadedAlbums = await invoke<Album[]>("get_albums");
      if (request === latestAlbumRequest) albums = loadedAlbums;
    } catch {
      // Retain the persisted ID if the album list is temporarily unavailable.
    }
  }

  $effect(() => {
    const nextTargetAlbumIds = new Set(
      watchedFolders.flatMap((folder) => folder.target_album_id ? [folder.target_album_id] : [])
    );
    const hasNewTargetAlbum = [...nextTargetAlbumIds].some((id) => !targetAlbumIds.has(id));
    targetAlbumIds = nextTargetAlbumIds;

    if (hasNewTargetAlbum || latestAlbumRequest === 0) {
      void fetchAlbums();
    }
  });
</script>

<div class="space-y-6 animate-in" style="animation-delay: 100ms">
  <div class="flex items-center justify-between px-4">
    <h4 class="text-sm font-bold text-slate-800 dark:text-slate-100 uppercase tracking-widest">
      {$t('synced_folders')}
    </h4>
    <button
      class="flex h-9 w-9 items-center justify-center rounded-lg border border-blue-600 bg-blue-600 text-white transition-colors hover:bg-blue-700"
      aria-label={$t('select_folder')}
      onclick={onAddFolder}
    >
      <FolderPlus class="w-4 h-4" />
    </button>
  </div>
  <div class="space-y-4">
    {#each watchedFolders as folder}
      <div class="glass-pane !p-4 flex items-center justify-between group">
        <div class="flex items-center gap-4 min-w-0">
          <div class="w-10 h-10 rounded-xl bg-slate-100 dark:bg-slate-800 flex items-center justify-center text-slate-400 dark:text-slate-400 group-hover:bg-blue-50 dark:group-hover:bg-blue-950/50 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
            <Folder class="w-5 h-5" />
          </div>
          <div class="min-w-0">
            <p class="text-sm font-bold text-slate-900 dark:text-slate-100 truncate">{folder.path.split(/[/\\]/).pop()}</p>
            <p class="text-[10px] font-mono text-slate-400 dark:text-slate-400 truncate">{folder.path}</p>
            {#if folder.target_album_id}
              <p class="text-[10px] font-medium text-blue-600 dark:text-blue-400 truncate">
                {$t('target_album')}: {albumNamesById.get(folder.target_album_id) ?? folder.target_album_id}
              </p>
            {/if}
          </div>
        </div>
        <button
          class="rounded-lg border border-red-200 bg-red-50 p-2 text-red-600 transition-colors hover:bg-red-100 dark:border-red-900/70 dark:bg-red-950/30 dark:text-red-400 dark:hover:bg-red-950/60"
          aria-label={`${$t('remove_folder')}: ${folder.path}`}
          onclick={() => onRemoveFolder(folder.id)}
        >
          <Trash2 class="w-4 h-4" />
        </button>
      </div>
    {/each}
    {#if watchedFolders.length === 0}
      <div class="glass-pane !p-12 text-center border-dashed border-2">
        <p class="text-sm text-slate-400 dark:text-slate-400 italic">{$t('no_folders')}</p>
      </div>
    {/if}
  </div>
</div>
