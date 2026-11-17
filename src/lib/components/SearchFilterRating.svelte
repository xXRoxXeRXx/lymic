<script lang="ts">
  import { Star } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { SearchRating } from "../contracts";
  import type { SearchController } from "../controllers/search-controller.svelte";

  /** The Rating Search Filter: one exact star level; selecting the selected level again clears it. */
  let { search }: { search: SearchController } = $props();

  const levels: SearchRating[] = [1, 2, 3, 4, 5];
  const label = (level: SearchRating) => (level === 1 ? $t("search_filter_rating_one") : $t("search_filter_rating_other", { values: { count: level } }));
</script>

<div role="group" aria-labelledby="search-filter-rating-label" class="space-y-2">
  <span id="search-filter-rating-label" class="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{$t("search_filter_rating")}</span>
  <div class="flex gap-1">
    {#each levels as level (level)}
      {@const selected = search.filters.rating === level}
      <button
        type="button"
        class="flex items-center gap-1 rounded-lg border px-2 py-1 text-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 {selected ? 'border-blue-600 bg-blue-50 text-blue-700 dark:border-sky-400 dark:bg-sky-950/40 dark:text-sky-300' : 'border-slate-200/80 text-slate-600 hover:bg-slate-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800'}"
        aria-label={label(level)}
        title={label(level)}
        aria-pressed={selected}
        onclick={() => search.toggleRating(level)}
      >
        <span aria-hidden="true">{level}</span>
        <Star class="h-3.5 w-3.5 {selected ? 'fill-current' : ''}" aria-hidden="true" />
      </button>
    {/each}
  </div>
</div>
