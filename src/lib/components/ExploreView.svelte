<script lang="ts">
  import { t } from "svelte-i18n";
  import type { Place, RecognizedPerson } from "../contracts";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { PlacesViewState } from "../controllers/places-controller.svelte";
  import type { ExploreViewState } from "../controllers/explore-controller.svelte";
  import ExploreSection from "./ExploreSection.svelte";
  import PlaceTile from "./PlaceTile.svelte";
  import RecognizedPersonTile from "./RecognizedPersonTile.svelte";

  let { explore, people, places, onViewAllPeople, onSelectPerson, onViewAllPlaces, onSelectPlace }: {
    explore: ExploreViewState;
    people: PeopleViewState;
    places: PlacesViewState;
    onViewAllPeople: () => void;
    onSelectPerson: (person: RecognizedPerson) => void;
    onViewAllPlaces: () => void;
    onSelectPlace?: (place: Place) => void;
  } = $props();

  // One row of tiles: as many as fit the measured width, with a fixed fallback when unmeasured.
  const TILE_MIN_WIDTH = 144;
  const TILE_GAP = 16;
  const FALLBACK_COLUMNS = 6;
  let rowColumns = $state(FALLBACK_COLUMNS);

  function measureColumns(node: HTMLElement) {
    if (typeof ResizeObserver === "undefined") return {};
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0;
      if (width > 0) rowColumns = Math.max(1, Math.floor((width + TILE_GAP) / (TILE_MIN_WIDTH + TILE_GAP)));
    });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<div class="flex flex-col gap-8" aria-label={$t("explore_title")}>
  <ExploreSection
    id="people"
    title={$t("people_title")}
    section={explore.people}
    loadingMessage={$t("people_loading")}
    errorMessage={$t("people_error")}
    unavailableMessage={$t("people_unavailable")}
    offlineMessage={$t("people_offline")}
    onViewAll={onViewAllPeople}
  >
    <div use:measureColumns class="grid gap-4" style="grid-template-columns: repeat({rowColumns}, minmax(0, 1fr));">
      {#each people.people.slice(0, rowColumns) as person (person.id)}
        <RecognizedPersonTile {person} onSelect={onSelectPerson} />
      {/each}
    </div>
  </ExploreSection>

  <ExploreSection
    id="places"
    title={$t("places_title")}
    section={explore.places}
    loadingMessage={$t("places_loading")}
    errorMessage={$t("places_error")}
    unavailableMessage={$t("places_unavailable")}
    permissionDeniedMessage={$t("places_permission_denied")}
    offlineMessage={$t("places_offline")}
    onViewAll={onViewAllPlaces}
  >
    <div use:measureColumns class="grid gap-4" style="grid-template-columns: repeat({rowColumns}, minmax(0, 1fr));">
      {#each places.places.slice(0, rowColumns) as place (place.thumbnailAssetId)}
        <PlaceTile {place} onSelect={onSelectPlace} />
      {/each}
    </div>
  </ExploreSection>

  {#if explore.isAllEmpty}
    <p class="py-12 text-center text-sm text-slate-500 dark:text-zinc-400">{$t("explore_empty")}</p>
  {/if}
</div>
