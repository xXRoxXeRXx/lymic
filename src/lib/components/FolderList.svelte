<script lang="ts">
  import { Album, Folder, FolderPlus, Trash2 } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import type { WatchedFolder } from "./types";

  interface Props {
    watchedFolders: WatchedFolder[];
    onAddFolder: () => void;
    onRemoveFolder: (id: number) => void;
    onUpdateFolderExclusionPatterns: (id: number, patterns: string[]) => void;
    catalogue: TargetAlbumCatalogue;
  }

  let { watchedFolders, onAddFolder, onRemoveFolder, onUpdateFolderExclusionPatterns, catalogue }: Props = $props();
  let targetAlbumIds = new Set<string>();
  let drafts = $state<Record<number, string[]>>({});
  let newPatterns = $state<Record<number, string>>({});
  let validationErrors = $state<Record<number, string>>({});

  function isValidPattern(pattern: string) {
    return pattern.length > 0
      && !pattern.includes("\\\\")
      && !pattern.startsWith("/")
      && !pattern.includes("..")
      && !/[?\[\]{}!]/.test(pattern);
  }

  function patternsFor(folder: WatchedFolder) {
    return drafts[folder.id] ?? folder.exclusion_patterns ?? [];
  }

  function addPattern(folder: WatchedFolder) {
    const pattern = (newPatterns[folder.id] ?? "").trim();
    if (!isValidPattern(pattern)) {
      validationErrors[folder.id] = $t('folder_exclusions_invalid_pattern');
      return;
    }
    const patterns = patternsFor(folder);
    if (!patterns.some((existing) => existing.toLowerCase() === pattern.toLowerCase())) {
      drafts[folder.id] = [...patterns, pattern];
    }
    newPatterns[folder.id] = "";
    validationErrors[folder.id] = "";
  }

  function removePattern(folder: WatchedFolder, pattern: string) {
    drafts[folder.id] = patternsFor(folder).filter((candidate) => candidate !== pattern);
  }

  function savePatterns(folder: WatchedFolder) {
    const patterns = patternsFor(folder);
    if (patterns.some((pattern) => !isValidPattern(pattern))) {
      validationErrors[folder.id] = $t('folder_exclusions_invalid_pattern');
      return;
    }
    onUpdateFolderExclusionPatterns(folder.id, patterns);
  }

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
        <details class="shrink-0">
          <summary class="cursor-pointer text-xs text-blue-600 dark:text-sky-400">{$t('folder_exclusions')}</summary>
          <div class="mt-2 w-64 space-y-2">
            <label class="block text-xs text-slate-500" for={`exclusion-${folder.id}`}>{$t('folder_exclusions_help')}</label>
            <div class="flex gap-1">
              <input id={`exclusion-${folder.id}`} class="min-w-0 flex-1 rounded border p-2 text-xs" placeholder={$t('folder_exclusions_example')} value={newPatterns[folder.id] ?? ""} oninput={(event) => { newPatterns[folder.id] = event.currentTarget.value; validationErrors[folder.id] = ""; }} />
              <button type="button" class="rounded border px-2 text-xs" onclick={() => addPattern(folder)}>{$t('folder_exclusions_add')}</button>
            </div>
            {#if validationErrors[folder.id]}
              <p class="text-xs text-red-600" role="alert">{validationErrors[folder.id]}</p>
            {/if}
            <ul class="space-y-1">
              {#each patternsFor(folder) as pattern}
                <li class="flex items-center justify-between gap-2 rounded bg-slate-100 px-2 py-1 text-xs dark:bg-zinc-800"><code>{pattern}</code><button type="button" aria-label={`${$t('folder_exclusions_remove')}: ${pattern}`} onclick={() => removePattern(folder, pattern)}>×</button></li>
              {/each}
            </ul>
            {#if patternsFor(folder).some((pattern) => pattern === '**' || pattern === '**/*')}
              <p class="text-xs text-amber-600">{$t('folder_exclusions_all_warning')}</p>
            {/if}
            <button type="button" class="rounded bg-blue-600 px-2 py-1 text-xs text-white" onclick={() => savePatterns(folder)}>{$t('folder_exclusions_save')}</button>
          </div>
        </details>
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
