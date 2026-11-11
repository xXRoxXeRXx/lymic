import type { PeopleDesktopAdapter, TimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineViewState } from "./timeline-controller.svelte";

export function createPersonTimelineController(
  adapter: Pick<PeopleDesktopAdapter, "getPersonTimelineBuckets" | "getPersonTimelineBucketAssets"> & Pick<TimelineDesktopAdapter, "setAssetFavorite">,
  personId: string,
): TimelineViewState {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getPersonTimelineBuckets(personId),
    getTimelineBucketAssets: (timeBucket) => adapter.getPersonTimelineBucketAssets(personId, timeBucket),
    setAssetFavorite: adapter.setAssetFavorite,
  });
}
