import type { FavoritesTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createFilteredTimelineController, type FilteredTimelineController } from "./filtered-timeline-controller.svelte";

/** The Favorites Timeline over the user's Immich favorite timeline requests. */
export function createFavoritesTimelineController(
  adapter: Pick<FavoritesTimelineDesktopAdapter, "getFavoritesTimelineBuckets" | "getFavoritesTimelineBucketAssets">,
): FilteredTimelineController {
  return createFilteredTimelineController({
    getBuckets: () => adapter.getFavoritesTimelineBuckets(),
    getBucketAssets: (timeBucket) => adapter.getFavoritesTimelineBucketAssets(timeBucket),
  });
}
