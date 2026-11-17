import type { TrashTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createFilteredTimelineController, type FilteredTimelineController } from "./filtered-timeline-controller.svelte";

/** The Trash Timeline over the user's Immich Trashed Asset timeline requests; any failure, 403 included, is a load error. */
export function createTrashTimelineController(
  adapter: Pick<TrashTimelineDesktopAdapter, "getTrashTimelineBuckets" | "getTrashTimelineBucketAssets">,
): FilteredTimelineController {
  return createFilteredTimelineController({
    getBuckets: () => adapter.getTrashTimelineBuckets(),
    getBucketAssets: (timeBucket) => adapter.getTrashTimelineBucketAssets(timeBucket),
  });
}
