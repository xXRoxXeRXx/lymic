import type { TagTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

export function createTagTimelineController(adapter: TagTimelineDesktopAdapter, tagId: string): TimelineController {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getTagTimelineBuckets(tagId),
    getTimelineBucketAssets: (timeBucket) => adapter.getTagTimelineBucketAssets(tagId, timeBucket),
  });
}
