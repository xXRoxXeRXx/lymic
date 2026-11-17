<script lang="ts">
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchFilterCamera from "./SearchFilterCamera.svelte";
  import SearchFilterDate from "./SearchFilterDate.svelte";
  import SearchFilterDisplay from "./SearchFilterDisplay.svelte";
  import SearchFilterLocation from "./SearchFilterLocation.svelte";
  import SearchFilterMediaType from "./SearchFilterMediaType.svelte";
  import SearchFilterPerson from "./SearchFilterPerson.svelte";
  import SearchFilterRating from "./SearchFilterRating.svelte";
  import SearchModeSelector from "./SearchModeSelector.svelte";

  /**
   * The panel below the search field. Each Search option is one section here: the Search Mode choice now,
   * the Search Filters as they are added.
   */
  let { search, people, id, onOpenArchive = () => {} }: { search: SearchController; people: PeopleViewState; id: string; onOpenArchive?: () => void } = $props();
</script>

<div {id} class="mt-2 flex flex-col gap-4 rounded-xl border border-slate-200/80 bg-white/70 p-3 dark:border-zinc-800 dark:bg-zinc-900/60">
  <SearchModeSelector {search} />
  <SearchFilterPerson {search} {people} />
  <SearchFilterDate {search} />
  <SearchFilterMediaType {search} />
  <SearchFilterCamera {search} />
  <SearchFilterLocation {search} />
  <SearchFilterRating {search} />
  <SearchFilterDisplay {search} {onOpenArchive} />
</div>
