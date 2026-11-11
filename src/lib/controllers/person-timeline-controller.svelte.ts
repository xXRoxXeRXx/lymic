import type { AssetDesktopAdapter, PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineViewState } from "./timeline-controller.svelte";

export function createPersonTimelineController(
  adapter: Pick<PeopleDesktopAdapter, "getPersonTimelineBuckets" | "getPersonTimelineBucketAssets"> & Pick<AssetDesktopAdapter, "downloadAsset">,
  personId: string,
): TimelineViewState {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getPersonTimelineBuckets(personId),
    getTimelineBucketAssets: (timeBucket) => adapter.getPersonTimelineBucketAssets(personId, timeBucket),
    downloadAsset: adapter.downloadAsset,
  });
}
