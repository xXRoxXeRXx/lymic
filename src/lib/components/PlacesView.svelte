<script lang="ts">
  import { t } from "svelte-i18n";
  import type { Place } from "../contracts";
  import type { PlacesViewState } from "../controllers/places-controller.svelte";
  import PlaceTile from "./PlaceTile.svelte";

  /** "All Places": every Place in a grid. It has no navigation entry; Explore opens it. */
  let { places, onSelect }: { places: PlacesViewState; onSelect: (place: Place) => void } = $props();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4" aria-label={$t("places_all_title")}>
  {#if places.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("places_offline")}</p>{/if}
  {#if places.error}
    <div role="alert" class="flex items-center gap-3 text-sm text-red-600 dark:text-red-300">
      <p>{places.error === "permission_denied" ? $t("places_permission_denied") : places.error === "unavailable" ? $t("places_unavailable") : $t("places_error")}</p>
      <button type="button" class="rounded-lg px-2 py-1 font-medium hover:bg-slate-200 dark:hover:bg-zinc-800" onclick={() => void places.refresh()}>{$t("explore_retry")}</button>
    </div>
  {:else if places.isLoading}<p role="status">{$t("places_loading")}</p>
  {:else if places.places.length === 0}<p>{$t("places_empty")}</p>
  {:else}
    <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
      {#each places.places as place (place.thumbnailAssetId)}
        <PlaceTile {place} {onSelect} />
      {/each}
    </div>
  {/if}
</div>
