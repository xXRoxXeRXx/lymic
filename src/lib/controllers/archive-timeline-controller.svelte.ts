import type { ArchiveTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createFilteredTimelineController, type FilteredTimelineController } from "./filtered-timeline-controller.svelte";

/**
 * The Archive Timeline over the user's Immich archive-visibility timeline requests. Its
 * `permissionDenied` reports the backend's `archive_permission_denied:<permission>` code.
 */
export function createArchiveTimelineController(
  adapter: Pick<ArchiveTimelineDesktopAdapter, "getArchiveTimelineBuckets" | "getArchiveTimelineBucketAssets">,
): FilteredTimelineController {
  return createFilteredTimelineController({
    getBuckets: () => adapter.getArchiveTimelineBuckets(),
    getBucketAssets: (timeBucket) => adapter.getArchiveTimelineBucketAssets(timeBucket),
    permissionPrefix: "archive",
  });
}
