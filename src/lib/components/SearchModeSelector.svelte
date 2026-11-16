<script lang="ts">
  import { t } from "svelte-i18n";
  import type { SearchMode } from "../contracts";
  import type { SearchController } from "../controllers/search-controller.svelte";

  /**
   * The Search Mode choice of the search panel: exactly one of Filename, Description, Folder Path and OCR.
   * OCR is disabled, with an explanation, while the server has it switched off. Native radio buttons give
   * the arrow-key navigation and the screen reader announcements of an exclusive choice.
   */
  let { search }: { search: SearchController } = $props();

  const modes: SearchMode[] = ["filename", "description", "folderPath", "ocr"];
  const name = "search-mode";

  // The features are read when the panel opens, so a disabled OCR is explained before the first Search.
  $effect(() => { void search.loadFeatures(); });

  function isDisabled(mode: SearchMode) {
    return mode === "ocr" && !search.ocrAvailable;
  }
</script>

<fieldset class="min-w-0">
  <legend class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{$t("search_mode_label")}</legend>
  <div class="flex flex-wrap gap-2">
    {#each modes as mode (mode)}
      <label
        class="flex cursor-pointer items-center gap-2 rounded-lg border border-slate-200/80 px-3 py-1.5 text-sm text-slate-700 has-[:checked]:border-blue-500 has-[:checked]:bg-blue-50 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50 has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-blue-600 dark:border-zinc-700 dark:text-zinc-200 dark:has-[:checked]:border-sky-500 dark:has-[:checked]:bg-sky-950/40"
      >
        <input
          type="radio"
          class="accent-blue-600 dark:accent-sky-500"
          {name}
          value={mode}
          checked={search.mode === mode}
          disabled={isDisabled(mode)}
          aria-describedby={isDisabled(mode) ? "search-mode-ocr-unavailable" : undefined}
          onchange={() => { search.mode = mode; }}
        />
        {$t(`search_mode_${mode}`)}
      </label>
    {/each}
  </div>
  {#if !search.ocrAvailable}
    <p id="search-mode-ocr-unavailable" class="mt-1.5 text-xs text-slate-500 dark:text-zinc-400">{$t("search_mode_ocr_unavailable")}</p>
  {:else if search.featuresFailed}
    <p class="mt-1.5 text-xs text-slate-500 dark:text-zinc-400">{$t("search_features_unavailable")}</p>
  {/if}
</fieldset>
