<script lang="ts">
  import { t } from "svelte-i18n";
  import type { SearchLocation } from "../contracts";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import { setLocationPart } from "../search-filters/location";
  import SearchFilterSuggestion from "./SearchFilterSuggestion.svelte";

  /** The Location Search Filter: country, state or region and city, each optional. Country and state are chosen from Immich's suggestions (the state's are limited by the chosen country); the city from the city listing of the Explore Places, which Immich's suggestions do not offer. */
  let { search }: { search: SearchController } = $props();

  const location = $derived(search.filters.location);

  function set(part: keyof SearchLocation) {
    return (value: string | undefined) => { search.filters.location = setLocationPart(search.filters.location, part, value); };
  }
</script>

<div role="group" aria-labelledby="search-filter-location-label" class="space-y-2">
  <span id="search-filter-location-label" class="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{$t("search_filter_location")}</span>
  <div class="grid gap-2 sm:grid-cols-2">
    <SearchFilterSuggestion
      {search}
      id="search-filter-location-country"
      label={$t("search_filter_location_country")}
      request={{ type: "country" }}
      value={location?.country}
      onChange={set("country")}
    />
    <SearchFilterSuggestion
      {search}
      id="search-filter-location-state"
      label={$t("search_filter_location_state")}
      request={location?.country ? { type: "state", country: location.country } : { type: "state" }}
      value={location?.state}
      onChange={set("state")}
    />
    <SearchFilterSuggestion
      {search}
      id="search-filter-location-city"
      label={$t("search_filter_location_city")}
      source={search.cities}
      value={location?.city}
      onChange={set("city")}
    />
  </div>
</div>
