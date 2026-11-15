import type { ArchiveTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** Creates a TimelineController backed by the user's Immich archive-visibility timeline endpoints. */
export function createArchiveTimelineController(
  adapter: Pick<ArchiveTimelineDesktopAdapter, "getArchiveTimelineBuckets" | "getArchiveTimelineBucketAssets">,
): TimelineController {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getArchiveTimelineBuckets(),
    getTimelineBucketAssets: (timeBucket) => adapter.getArchiveTimelineBucketAssets(timeBucket),
  });
}
