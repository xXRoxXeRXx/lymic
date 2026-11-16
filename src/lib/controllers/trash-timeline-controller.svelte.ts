import type { TrashTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** Creates a TimelineController backed by the user's Immich trashed-asset timeline endpoints. */
export function createTrashTimelineController(
  adapter: Pick<TrashTimelineDesktopAdapter, "getTrashTimelineBuckets" | "getTrashTimelineBucketAssets">,
): TimelineController {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getTrashTimelineBuckets(),
    getTimelineBucketAssets: (timeBucket) => adapter.getTrashTimelineBucketAssets(timeBucket),
  });
}
