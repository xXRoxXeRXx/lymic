<script lang="ts">
  import { Search, SlidersHorizontal, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import type { SearchFilters } from "../contracts";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import { chipContext } from "../search-filters/chip-context";
  import { activeChips, clearChip, editorOfFilter, type SearchEditorId } from "../search-filters/chips";
  import SearchFilterChip from "./SearchFilterChip.svelte";
  import SearchPanel from "./SearchPanel.svelte";

  /**
   * The search bar at the top of the content area of every page. Enter (or the Search button) runs the
   * Search; the clear control resets text, Search Mode and Search Filters. The Search Mode choice and the
   * Search Filters live in the panel that the options button opens below the field; a Search can consist of
   * filters alone. While the panel is closed, one line below the field shows the active Search Filters as
   * Filter Chips; a chip opens the panel at its editor.
   */
  let { search, people, tags, onSubmit, onClear, onOpenArchive }: {
    search: SearchController;
    people: PeopleViewState;
    tags: TagsViewState;
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
  // The editor that is open in the panel; none when the panel opens.
  let editor = $state<SearchEditorId | null>(null);
  let optionsButton = $state<HTMLButtonElement | null>(null);
  let chipLine = $state<HTMLElement | null>(null);
  let hiddenChips = $state(0);
  const placeholder = $derived(search.mode === "filename" ? $t("search_placeholder") : $t(`search_placeholder_${search.mode}`));
  const context = $derived(chipContext($t, $locale, people.people));
  const active = $derived(activeChips(search.filters, context));

  function submit() {
    // A filter that cannot be searched blocks the Search, so the panel opens (or stays open) at that filter's editor: its message is there.
    const blocked = Object.keys(search.filterErrors) as (keyof SearchFilters)[];
    panelOpen = blocked.length > 0;
    editor = blocked.length > 0 ? editorOfFilter(blocked[0]) : null;
    onSubmit();
  }

  function closePanel() {
    panelOpen = false;
    editor = null;
  }

  function openArchive() {
    closePanel();
    onOpenArchive();
  }

  function openEditor(next: SearchEditorId) {
    panelOpen = true;
    editor = next;
  }

  // Escape closes the open editor first and the panel with the next press, handing the focus back to what opened it.
  function closeOnEscape(event: KeyboardEvent) {
    if (event.key !== "Escape" || !panelOpen) return;
    event.stopPropagation();
    if (editor) {
      const closed = editor;
      editor = null;
      document.getElementById(`search-chip-${closed}`)?.focus();
      return;
    }
    closePanel();
    optionsButton?.focus();
  }

  // The chip line is one line high: the chips that wrap into a second line are cut off, counted, and taken out of the tab order.
  $effect(() => {
    void active;
    if (!chipLine) return;
    const line = chipLine;
    const measure = () => {
      const chips = Array.from(line.children) as HTMLElement[];
      const firstTop = chips[0]?.offsetTop ?? 0;
      let hidden = 0;
      for (const chip of chips) {
        const overflowing = chip.offsetTop > firstTop;
        chip.inert = overflowing;
        if (overflowing) hidden += 1;
      }
      hiddenChips = hidden;
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(line);
    return () => observer.disconnect();
  });
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
    onclick={() => { if (panelOpen) closePanel(); else panelOpen = true; }}
  >
    <SlidersHorizontal class="h-4 w-4" aria-hidden="true" />
  </button>
  <button
    type="submit"
    class="shrink-0 rounded-lg bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:bg-sky-600 dark:hover:bg-sky-500"
  >{$t("search_submit")}</button>
  </div>
  {#if panelOpen}
    <SearchPanel {search} {people} {tags} id={panelId} bind:editor onOpenArchive={openArchive} />
  {:else if active.length > 0}
    <div role="group" aria-label={$t("search_active_filters")} class="mt-2 flex items-start gap-2">
      <div bind:this={chipLine} class="flex h-[1.875rem] min-w-0 flex-1 flex-wrap gap-2 overflow-hidden">
        {#each active as { chip, summary } (chip.id)}
          <SearchFilterChip
            editor={chip.id}
            label={$t(chip.labelKey)}
            {summary}
            expanded={false}
            controls={panelId}
            invalid={chip.keys.some((key) => key in search.filterErrors)}
            onToggle={() => openEditor(chip.id)}
            onRemove={() => clearChip(chip, search.filters)}
          />
        {/each}
      </div>
      {#if hiddenChips > 0}
        <button
          type="button"
          class="shrink-0 rounded-lg border border-slate-200/80 px-2.5 py-1 text-sm text-slate-600 hover:bg-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
          aria-label={$t("search_chip_more", { values: { count: hiddenChips } })}
          onclick={() => { panelOpen = true; }}
        >+{hiddenChips}</button>
      {/if}
    </div>
  {/if}
</form>
</div>
