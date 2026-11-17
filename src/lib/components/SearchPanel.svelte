<script lang="ts">
  import { locale, t } from "svelte-i18n";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import { chipContext } from "../search-filters/chip-context";
  import { chipSummary, clearChip, searchChips, type SearchEditorId } from "../search-filters/chips";
  import SearchFilterCamera from "./SearchFilterCamera.svelte";
  import SearchFilterChip from "./SearchFilterChip.svelte";
  import SearchFilterDate from "./SearchFilterDate.svelte";
  import SearchFilterDisplay from "./SearchFilterDisplay.svelte";
  import SearchFilterLocation from "./SearchFilterLocation.svelte";
  import SearchFilterMediaType from "./SearchFilterMediaType.svelte";
  import SearchFilterPerson from "./SearchFilterPerson.svelte";
  import SearchFilterRating from "./SearchFilterRating.svelte";
  import SearchFilterTag from "./SearchFilterTag.svelte";
  import SearchModeSelector from "./SearchModeSelector.svelte";

  /**
   * The panel below the search field: the Search Mode chip, then one Filter Chip per Search Filter. A chip
   * opens the editor of its filter below the chips; only one editor is open at a time, and none is open
   * when the panel opens, so the panel stays as low as the window allows. The panel never grows beyond
   * 45% of the window height; a taller editor scrolls inside it.
   */
  let { search, people, tags, id, editor = $bindable(null), onOpenArchive = () => {} }: {
    search: SearchController;
    people: PeopleViewState;
    tags: TagsViewState;
    id: string;
    /** The editor that is open, or null. */
    editor?: SearchEditorId | null;
    onOpenArchive?: () => void;
  } = $props();

  const editorId = $derived(`${id}-editor`);
  const context = $derived(chipContext($t, $locale, people.people));

  // The server's features are read when the panel opens, so a disabled OCR or Context mode is known before the first Search.
  $effect(() => { void search.loadFeatures(); });

  function toggle(next: SearchEditorId) {
    editor = editor === next ? null : next;
  }

  // The Person and Tag filters need a permission that only the server's answer reveals; once it said no, the chip says why.
  function unavailable(chipId: SearchEditorId): string | null {
    if (chipId === "person" && people.error === "permission_denied") return $t("search_filter_person_permission_denied");
    if (chipId === "tag" && tags.error === "permission_denied") return $t("search_filter_tag_permission_denied");
    return null;
  }
</script>

<div {id} class="mt-2 flex max-h-[45vh] flex-col gap-3 overflow-y-auto rounded-xl border border-slate-200/80 bg-white/70 p-3 dark:border-zinc-800 dark:bg-zinc-900/60">
  <div class="flex flex-wrap gap-2">
    <SearchFilterChip
      editor="mode"
      label={$t("search_mode_label")}
      summary={$t(`search_mode_${search.mode}`)}
      expanded={editor === "mode"}
      controls={editorId}
      onToggle={() => toggle("mode")}
    />
    {#each searchChips as chip (chip.id)}
      <SearchFilterChip
        editor={chip.id}
        label={$t(chip.labelKey)}
        summary={chipSummary(chip, search.filters, context)}
        expanded={editor === chip.id}
        controls={editorId}
        invalid={chip.keys.some((key) => key in search.filterErrors)}
        unavailable={unavailable(chip.id)}
        onToggle={() => toggle(chip.id)}
        onRemove={() => clearChip(chip, search.filters)}
      />
    {/each}
  </div>
  {#if editor}
    <div id={editorId} role="region" aria-labelledby="search-chip-{editor}">
      {#if editor === "mode"}
        <SearchModeSelector {search} />
      {:else if editor === "person"}
        <SearchFilterPerson {search} {people} />
      {:else if editor === "tag"}
        <SearchFilterTag {search} {tags} />
      {:else if editor === "date"}
        <SearchFilterDate {search} />
      {:else if editor === "mediaType"}
        <SearchFilterMediaType {search} />
      {:else if editor === "camera"}
        <SearchFilterCamera {search} />
      {:else if editor === "location"}
        <SearchFilterLocation {search} />
      {:else if editor === "rating"}
        <SearchFilterRating {search} />
      {:else if editor === "display"}
        <SearchFilterDisplay {search} {onOpenArchive} />
      {/if}
    </div>
  {/if}
</div>
