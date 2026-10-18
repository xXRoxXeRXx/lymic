<script lang="ts">
  import { Album, Folder, FolderPlus, Trash2 } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import type { WatchedFolder } from "./types";

  interface Props {
    watchedFolders: WatchedFolder[];
    onAddFolder: () => void;
    onRemoveFolder: (id: number) => void;
    catalogue: TargetAlbumCatalogue;
  }

  let { watchedFolders, onAddFolder, onRemoveFolder, catalogue }: Props = $props();
  let targetAlbumIds = new Set<string>();

  $effect(() => {
    const nextTargetAlbumIds = new Set(
      watchedFolders.flatMap((folder) => folder.target_album_id ? [folder.target_album_id] : [])
    );
    const hasNewTargetAlbum = [...nextTargetAlbumIds].some((id) => !targetAlbumIds.has(id));
    targetAlbumIds = nextTargetAlbumIds;

    if (hasNewTargetAlbum || !catalogue.hasRequested) {
      void catalogue.refresh();
    }
  });
</script>

<div class="space-y-4 animate-in">
  <div class="flex items-center justify-between px-1">
    <h4 class="text-xs font-bold text-slate-400 dark:text-zinc-500 uppercase tracking-wider flex items-center gap-2">
      <Folder class="w-3.5 h-3.5" />
      <span>{$t('synced_folders')}</span>
    </h4>
    <button
      type="button"
      class="flex h-8 items-center gap-1.5 rounded-lg bg-blue-600 px-3 text-xs font-semibold text-white hover:bg-blue-700 transition-colors shadow-sm cursor-pointer"
      aria-label={$t('add_folder')}
      onclick={onAddFolder}
    >
      <FolderPlus class="w-4 h-4 shrink-0" />
      <span>{$t('add_folder')}</span>
    </button>
  </div>

  <div class="space-y-3">
    {#each watchedFolders as folder}
      <div class="settings-card flex items-center justify-between gap-4">
        <div class="flex items-center gap-3.5 min-w-0">
          <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0">
            <Folder class="w-4 h-4" />
          </div>
          <div class="min-w-0 space-y-0.5">
            <p class="text-sm font-semibold text-slate-900 dark:text-slate-100 truncate">{folder.path.split(/[/\\]/).pop()}</p>
            <p class="text-[11px] font-mono text-slate-400 dark:text-zinc-500 truncate">{folder.path}</p>
            {#if folder.target_album_id}
              <p class="text-[11px] font-medium text-blue-600 dark:text-sky-400 truncate flex items-center gap-1.5">
                <Album class="w-3 h-3 shrink-0" />
                <span class="truncate">{$t('target_album')}: {catalogue.nameFor(folder.target_album_id) ?? $t('target_album_unavailable')}</span>
              </p>
            {/if}
          </div>
        </div>
        <button
          class="rounded-lg border border-red-200 bg-red-50 p-2 text-red-600 transition-colors hover:bg-red-100 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-400 dark:hover:bg-red-950/60 shrink-0 cursor-pointer"
          aria-label={`${$t('remove_folder')}: ${folder.path}`}
          onclick={() => onRemoveFolder(folder.id)}
        >
          <Trash2 class="w-4 h-4" />
        </button>
      </div>
    {/each}
    {#if watchedFolders.length === 0}
      <div class="settings-card !p-10 text-center border-dashed border-2 border-slate-200 dark:border-zinc-800">
        <p class="text-xs text-slate-400 dark:text-zinc-500 italic">{$t('no_folders')}</p>
      </div>
    {/if}
  </div>
</div>
