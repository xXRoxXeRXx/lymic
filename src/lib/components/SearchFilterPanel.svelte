<script lang="ts">
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchFilterDate from "./SearchFilterDate.svelte";
  import SearchFilterMediaType from "./SearchFilterMediaType.svelte";

  /**
   * The panel that opens from the search bar. The Search Mode choice goes in the first section, above the
   * Search Filters; each Search Filter is one component below, edited in place on `search.filters`.
   */
  let { search, id }: { search: SearchController; id: string } = $props();
</script>

<div
  {id}
  class="absolute inset-x-0 top-full z-30 mt-2 space-y-4 rounded-xl border border-slate-200 bg-white p-4 shadow-lg dark:border-zinc-700 dark:bg-zinc-900"
>
  <!-- Search Mode: the exclusive choice goes here. -->
  <SearchFilterDate {search} />
  <SearchFilterMediaType {search} />
</div>
