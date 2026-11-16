<script lang="ts">
  import { Search, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { SearchController } from "../controllers/search-controller.svelte";

  /**
   * The search bar at the top of the content area of every page. Enter (or the Search button) runs the
   * Search; the clear control resets it. The Search Mode choice and the Search Filters belong in the panel
   * that later changes add beside the field.
   */
  let { search, onSubmit, onClear }: {
    search: SearchController;
    onSubmit: () => void;
    onClear: () => void;
  } = $props();

  const canClear = $derived(search.text !== "" || search.submittedText !== "");
</script>

<form
  role="search"
  aria-label={$t("search_label")}
  class="flex items-center gap-2"
  onsubmit={(event) => { event.preventDefault(); onSubmit(); }}
>
  <div class="relative min-w-0 flex-1">
    <Search class="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-slate-400 dark:text-zinc-500" aria-hidden="true" />
    <input
      type="search"
      class="glass-input !py-2 !pl-9 !pr-9 text-sm [&::-webkit-search-cancel-button]:hidden"
      aria-label={$t("search_label")}
      placeholder={$t("search_placeholder")}
      autocomplete="off"
      spellcheck="false"
      bind:value={search.text}
    />
    {#if canClear}
      <button
        type="button"
        class="absolute right-2 top-2 rounded p-0.5 text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200"
        aria-label={$t("search_clear")}
        title={$t("search_clear")}
        onclick={onClear}
      >
        <X class="h-4 w-4" aria-hidden="true" />
      </button>
    {/if}
  </div>
  <button
    type="submit"
    class="shrink-0 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:bg-sky-600 dark:hover:bg-sky-500"
  >{$t("search_submit")}</button>
</form>
