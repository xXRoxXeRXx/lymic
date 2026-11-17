<script lang="ts">
  import { Search, SlidersHorizontal, X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchPanel from "./SearchPanel.svelte";

  /**
   * The search bar at the top of the content area of every page. Enter (or the Search button) runs the
   * Search; the clear control resets text, Search Mode and Search Filters. The Search Mode choice and the
   * Search Filters live in the panel that the options button opens below the field; a Search can consist of
   * filters alone.
   */
  let { search, people, onSubmit, onClear, onOpenArchive }: {
    search: SearchController;
    people: PeopleViewState;
    onSubmit: () => void;
    onClear: () => void;
    /** Switches to the Archive main view, like a click on its navigation entry. */
    onOpenArchive: () => void;
  } = $props();

  const hasFilters = $derived(Object.values(search.filters).some((value) => value !== undefined));
  // The Search can be cleared whenever there is something of it to reset, including a failure or unsupported notice left over from it.
  const canClear = $derived(search.text !== "" || hasFilters || search.revision > 0 || search.unsupported !== null || search.failure !== null);
  const panelId = "search-panel";
  let panelOpen = $state(false);
  let optionsButton = $state<HTMLButtonElement | null>(null);
  const placeholder = $derived(search.mode === "filename" ? $t("search_placeholder") : $t(`search_placeholder_${search.mode}`));

  function submit() {
    // A filter that cannot be searched blocks the Search, so the panel opens (or stays open): its message is there.
    panelOpen = Object.keys(search.filterErrors).length > 0;
    onSubmit();
  }

  function openArchive() {
    panelOpen = false;
    onOpenArchive();
  }

  function closeOnEscape(event: KeyboardEvent) {
    if (event.key !== "Escape" || !panelOpen) return;
    event.stopPropagation();
    panelOpen = false;
    optionsButton?.focus();
  }
</script>

<!-- The wrapper only listens for Escape bubbling up from the controls inside the panel. -->
<div role="presentation" onkeydown={closeOnEscape}>
<form
  role="search"
  aria-label={$t("search_label")}
  onsubmit={(event) => { event.preventDefault(); submit(); }}
>
  <div class="flex items-center gap-2">
  <div class="relative min-w-0 flex-1">
    <Search class="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-slate-400 dark:text-zinc-500" aria-hidden="true" />
    <input
      type="search"
      class="glass-input !py-2 !pl-9 !pr-9 text-sm [&::-webkit-search-cancel-button]:hidden"
      aria-label={$t("search_label")}
      {placeholder}
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
    bind:this={optionsButton}
    class="relative shrink-0 rounded-lg border border-slate-200/80 p-2.5 text-slate-600 hover:bg-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
    aria-label={$t("search_options")}
    title={$t("search_options")}
    aria-expanded={panelOpen}
    aria-controls={panelId}
    onclick={() => { panelOpen = !panelOpen; }}
  >
    <SlidersHorizontal class="h-4 w-4" aria-hidden="true" />
    {#if hasFilters}<span class="absolute right-1 top-1 h-2 w-2 rounded-full bg-blue-600 dark:bg-sky-400" aria-hidden="true"></span>{/if}
  </button>
  <button
    type="submit"
    class="shrink-0 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:bg-sky-600 dark:hover:bg-sky-500"
  >{$t("search_submit")}</button>
  </div>
  {#if panelOpen}
    <SearchPanel {search} {people} id={panelId} onOpenArchive={openArchive} />
  {/if}
</form>
</div>
