<script lang="ts">
  import { Folder, FolderPlus, Trash2 } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { WatchedFolder } from "./types";

  interface Props {
    watchedFolders: WatchedFolder[];
    onAddFolder: () => void;
    onRemoveFolder: (id: number) => void;
  }

  let { watchedFolders, onAddFolder, onRemoveFolder }: Props = $props();
</script>

<div class="space-y-6 animate-in" style="animation-delay: 100ms">
  <div class="flex items-center justify-between px-2">
    <h4 class="text-sm font-bold text-slate-800 dark:text-slate-100 uppercase tracking-widest">
      {$t('synced_folders')}
    </h4>
    <button
      class="w-8 h-8 rounded-full bg-blue-600 text-white flex items-center justify-center hover:bg-blue-700 transition-colors"
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
          </div>
        </div>
        <button
          class="p-2 text-slate-300 dark:text-slate-400 hover:text-red-500 dark:hover:text-red-400 opacity-0 group-hover:opacity-100 transition-all"
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
