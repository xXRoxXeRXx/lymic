<script lang="ts">
  import { t } from "svelte-i18n";
  import type { SearchMediaType } from "../contracts";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import SearchFilterHeading from "./SearchFilterHeading.svelte";

  /** The media type Search Filter: All (no filter), Images or Videos. */
  let { search }: { search: SearchController } = $props();

  const options: { value: SearchMediaType | undefined; label: string }[] = [
    { value: undefined, label: "search_filter_media_all" },
    { value: "image", label: "search_filter_media_image" },
    { value: "video", label: "search_filter_media_video" },
  ];
</script>

<div class="space-y-2">
  <SearchFilterHeading id="search-filter-media-label" text={$t("search_filter_media_type")} />
  <div role="radiogroup" aria-labelledby="search-filter-media-label" class="flex flex-wrap gap-4">
    {#each options as option (option.label)}
      <label class="flex items-center gap-2 text-sm text-slate-700 dark:text-zinc-300">
        <input
          type="radio"
          name="search-filter-media-type"
          checked={search.filters.mediaType === option.value}
          onchange={() => { search.filters.mediaType = option.value; }}
        />
        <span>{$t(option.label)}</span>
      </label>
    {/each}
  </div>
</div>
