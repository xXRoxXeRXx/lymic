<script lang="ts">
  import { Search, SlidersHorizontal, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchFilterPanel from "./SearchFilterPanel.svelte";

  /**
   * The search bar at the top of the content area of every page. Enter (or the Search button) runs the
   * Search; the clear control resets text, Search Mode and Search Filters. The filter panel opens from the
   * Filters button; a Search can consist of filters alone.
   */
  let { search, onSubmit, onClear }: {
    search: SearchController;
    onSubmit: () => void;
    onClear: () => void;
  } = $props();

  const hasFilters = $derived(Object.values(search.filters).some((value) => value !== undefined));
  const canClear = $derived(search.text !== "" || hasFilters || search.revision > 0);
  const panelId = "search-filter-panel";
  let panelOpen = $state(false);
  let filtersButton = $state<HTMLButtonElement | null>(null);

  function submit() {
    // A filter that cannot be searched keeps the panel open, where its message is.
    if (Object.keys(search.filterErrors).length === 0) panelOpen = false;
    onSubmit();
  }

  function closeOnEscape(event: KeyboardEvent) {
    if (event.key !== "Escape" || !panelOpen) return;
    event.stopPropagation();
    panelOpen = false;
    filtersButton?.focus();
  }
</script>

<!-- The wrapper only listens for Escape bubbling up from the controls inside the panel. -->
<div role="presentation" class="relative" onkeydown={closeOnEscape}>
<form
  role="search"
  aria-label={$t("search_label")}
  class="flex items-center gap-2"
  onsubmit={(event) => { event.preventDefault(); submit(); }}
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
    type="button"
    bind:this={filtersButton}
    class="relative flex shrink-0 items-center gap-2 rounded-lg border border-slate-300 px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:border-zinc-600 dark:text-zinc-200 dark:hover:bg-zinc-800"
    aria-expanded={panelOpen}
    aria-controls={panelId}
    onclick={() => { panelOpen = !panelOpen; }}
  >
    <SlidersHorizontal class="h-4 w-4" aria-hidden="true" />
    <span>{$t("search_filters")}</span>
    {#if hasFilters}<span class="h-2 w-2 rounded-full bg-blue-600 dark:bg-sky-400" aria-hidden="true"></span>{/if}
  </button>
  <button
    type="submit"
    class="shrink-0 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:bg-sky-600 dark:hover:bg-sky-500"
  >{$t("search_submit")}</button>
  {#if panelOpen}
    <SearchFilterPanel {search} id={panelId} />
  {/if}
</form>
</div>
