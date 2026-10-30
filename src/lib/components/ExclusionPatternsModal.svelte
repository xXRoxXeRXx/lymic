<script lang="ts">
  import { AlertCircle, Folder, Plus, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { WatchedFolder } from "../contracts";

  interface Props {
    folder: WatchedFolder | null;
    onSave: (id: number, patterns: string[]) => Promise<void>;
    onClose: () => void;
  }

  let { folder, onSave, onClose }: Props = $props();
  let patterns = $state<string[]>([]);
  let newPattern = $state("");
  let validationError = $state("");

  function isValidPattern(pattern: string) {
    return pattern.length > 0 && !pattern.includes("\\") && !pattern.startsWith("/")
      && !pattern.includes("..") && !/[?\[\]{}!]/.test(pattern);
  }

  function excludesAllMedia() {
    return patterns.some((pattern) => ["*", "*.*", "**", "**/*"].includes(pattern));
  }

  function addPattern() {
    const pattern = newPattern.trim();
    if (!isValidPattern(pattern)) {
      validationError = $t("folder_exclusions_invalid_pattern");
      return;
    }
    if (!patterns.some((existing) => existing.toLowerCase() === pattern.toLowerCase())) {
      patterns = [...patterns, pattern];
    }
    newPattern = "";
    validationError = "";
  }

  async function save() {
    if (!folder) return;
    await onSave(folder.id, patterns);
    onClose();
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) onClose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (folder && event.key === "Escape") onClose();
  }

  $effect(() => {
    if (folder) {
      patterns = folder.exclusion_patterns ?? [];
      newPattern = "";
      validationError = "";
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if folder}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-xs p-4 animate-in" role="presentation" onclick={handleBackdropClick}>
    <dialog open class="static m-0 w-full max-w-lg rounded-xl border border-slate-200 bg-white p-6 shadow-xl dark:border-zinc-800 dark:bg-zinc-900 text-slate-900 dark:text-slate-100 space-y-5" aria-labelledby="exclusion-patterns-dialog-title" aria-modal="true">
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-center gap-3 min-w-0">
          <div class="p-2.5 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0"><Folder class="w-5 h-5" /></div>
          <div class="min-w-0"><h2 id="exclusion-patterns-dialog-title" class="text-base font-bold truncate">{$t("folder_exclusions")}</h2><p class="mt-0.5 text-xs text-slate-500 dark:text-zinc-400 truncate">{folder.path}</p></div>
        </div>
        <button type="button" class="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 dark:hover:bg-zinc-800" aria-label={$t("folder_exclusions_close")} onclick={onClose}><X class="w-4 h-4" /></button>
      </div>
      <div class="space-y-2">
        <p class="text-xs text-slate-500 dark:text-zinc-400">{$t("folder_exclusions_help")}</p>
        <div class="flex gap-2"><input class="glass-input !py-2 text-xs flex-1" placeholder={$t("folder_exclusions_example")} bind:value={newPattern} oninput={() => validationError = ""} onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); addPattern(); } }} /><button type="button" class="rounded-lg border px-3 text-xs font-semibold" onclick={addPattern}><Plus class="w-3.5 h-3.5 inline" /> {$t("folder_exclusions_add")}</button></div>
        {#if validationError}<p class="text-xs text-red-600 dark:text-red-400" role="alert">{validationError}</p>{/if}
      </div>
      <ul class="max-h-48 overflow-y-auto space-y-1.5">
        {#each patterns as pattern}<li class="flex items-center justify-between gap-2 rounded-lg bg-slate-100 px-3 py-2 text-xs dark:bg-zinc-800"><code>{pattern}</code><button type="button" class="text-slate-500 hover:text-red-600" aria-label={`${$t("folder_exclusions_remove")}: ${pattern}`} onclick={() => patterns = patterns.filter((candidate) => candidate !== pattern)}><X class="w-4 h-4" /></button></li>{/each}
      </ul>
      {#if excludesAllMedia()}<div class="flex gap-2 rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-700 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-300"><AlertCircle class="w-4 h-4 shrink-0" /><p>{$t("folder_exclusions_all_warning")}</p></div>{/if}
      <div class="pt-4 flex justify-end gap-3 border-t border-slate-100 dark:border-zinc-800"><button type="button" class="rounded-lg px-4 py-2 text-sm font-semibold text-slate-600 hover:bg-slate-100 dark:text-zinc-300 dark:hover:bg-zinc-800" onclick={onClose}>{$t("folder_exclusions_cancel")}</button><button type="button" class="rounded-lg bg-blue-600 hover:bg-blue-700 px-4 py-2 text-sm font-semibold text-white" onclick={save}>{$t("folder_exclusions_save")}</button></div>
    </dialog>
  </div>
{/if}
