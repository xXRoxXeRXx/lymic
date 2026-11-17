<script lang="ts">
  import { Archive } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchFilterHeading from "./SearchFilterHeading.svelte";

  /**
   * The Display group: Favorite, Archived and Unfiled. A switched-off option is unset, not false, so it adds
   * no condition and no filter indicator. "Open Archive" leaves the Search like a click on the Archive
   * navigation entry; the Search session stays in memory.
   */
  let { search, onOpenArchive }: { search: SearchController; onOpenArchive: () => void } = $props();

  const options = [
    { key: "favorite", label: "search_filter_favorite" },
    { key: "archived", label: "search_filter_archived" },
    { key: "unfiled", label: "search_filter_unfiled" },
  ] as const;
</script>

<div role="group" aria-labelledby="search-filter-display-label" class="space-y-2">
  <SearchFilterHeading id="search-filter-display-label" text={$t("search_filter_display")} />
  <div class="flex flex-wrap gap-x-4 gap-y-2">
    {#each options as option (option.key)}
      <label class="flex items-center gap-2 text-sm text-slate-700 dark:text-zinc-300">
        <input
          type="checkbox"
          checked={search.filters[option.key] === true}
          aria-describedby={option.key === "archived" ? "search-filter-archived-hint" : undefined}
          onchange={(event) => { search.filters[option.key] = event.currentTarget.checked || undefined; }}
        />
        <span>{$t(option.label)}</span>
      </label>
    {/each}
  </div>
  <p id="search-filter-archived-hint" class="text-xs text-slate-500 dark:text-zinc-400">{$t("search_filter_archived_hint")}</p>
  <button
    type="button"
    class="flex items-center gap-1.5 rounded text-sm font-medium text-blue-600 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:text-sky-400"
    onclick={onOpenArchive}
  >
    <Archive class="h-4 w-4" aria-hidden="true" />
    <span>{$t("search_filter_open_archive")}</span>
  </button>
</div>
