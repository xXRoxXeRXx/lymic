import type { FavoritesTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

export type FavoritesTimelineViewState = TimelineController;

/** Creates a FavoritesTimelineViewState backed by the user's Immich favorite timeline endpoints. */
export function createFavoritesTimelineController(
  adapter: Pick<FavoritesTimelineDesktopAdapter, "getFavoritesTimelineBuckets" | "getFavoritesTimelineBucketAssets">,
): FavoritesTimelineViewState {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getFavoritesTimelineBuckets(),
    getTimelineBucketAssets: (timeBucket) => adapter.getFavoritesTimelineBucketAssets(timeBucket),
  });
}

