import type { ArchiveTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyExploreError } from "./explore-error";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** The Archive Timeline, which also reports whether the latest failed load was a missing API key permission. */
export interface ArchiveTimelineController extends TimelineController {
  /** True while the latest archive request was rejected for a missing `asset.read` permission. */
  readonly permissionDenied: boolean;
}

/** Creates a TimelineController backed by the user's Immich archive-visibility timeline endpoints. */
export function createArchiveTimelineController(
  adapter: Pick<ArchiveTimelineDesktopAdapter, "getArchiveTimelineBuckets" | "getArchiveTimelineBucketAssets">,
): ArchiveTimelineController {
  let permissionDenied = $state(false);

  // The shared timeline controller reports only that a load failed, so the
  // archive adapter calls record whether the backend's credential-free error
  // code (`archive_permission_denied:<permission>`) was the reason.
  async function tracked<T>(request: Promise<T>): Promise<T> {
    try {
      const result = await request;
      permissionDenied = false;
      return result;
    } catch (error) {
      permissionDenied = classifyExploreError(error, "archive") === "permission_denied";
      throw error;
    }
  }

  const timeline = createTimelineController({
    getTimelineBuckets: () => tracked(adapter.getArchiveTimelineBuckets()),
    getTimelineBucketAssets: (timeBucket) => tracked(adapter.getArchiveTimelineBucketAssets(timeBucket)),
  });

  const clear = timeline.clear;
  return Object.defineProperties(timeline, {
    permissionDenied: { get: () => permissionDenied, enumerable: true },
    clear: {
      value: () => {
        clear();
        permissionDenied = false;
      },
    },
  }) as ArchiveTimelineController;
}
