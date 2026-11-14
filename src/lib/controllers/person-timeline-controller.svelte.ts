import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

export function createPersonTimelineController(
  adapter: Pick<PeopleDesktopAdapter, "getPersonTimelineBuckets" | "getPersonTimelineBucketAssets">,
  personId: string,
): TimelineController {
  return createTimelineController({
    getTimelineBuckets: () => adapter.getPersonTimelineBuckets(personId),
    getTimelineBucketAssets: (timeBucket) => adapter.getPersonTimelineBucketAssets(personId, timeBucket),
  });
}
