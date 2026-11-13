import type { FavoritesTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineViewState } from "./timeline-controller.svelte";

/** Creates a TimelineViewState backed by the user's Immich favorite timeline endpoints. */
export function createFavoritesTimelineController(
  adapter: Pick<FavoritesTimelineDesktopAdapter, "getFavoritesTimelineBuckets" | "getFavoritesTimelineBucketAssets">,
): TimelineViewState {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getFavoritesTimelineBuckets(),
    getTimelineBucketAssets: (timeBucket) => adapter.getFavoritesTimelineBucketAssets(timeBucket),
  });
}

