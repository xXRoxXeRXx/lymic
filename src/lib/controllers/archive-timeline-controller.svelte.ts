import type { ArchiveTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure } from "./request-failure";
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
  // Bumped by clear() so a request still in flight when the session ends cannot
  // mark the next account's Archive Timeline as permission-denied.
  let generation = 0;

  // The shared timeline controller reports only that a load failed, so the
  // archive adapter calls record whether the backend's credential-free error
  // code (`archive_permission_denied:<permission>`) was the reason.
  async function tracked<T>(request: Promise<T>): Promise<T> {
    const requestGeneration = generation;
    try {
      const result = await request;
      if (requestGeneration === generation) permissionDenied = false;
      return result;
    } catch (error) {
      if (requestGeneration === generation) {
        permissionDenied = classifyRequestFailure(error, "archive") === "permission_denied";
      }
      throw error;
    }
  }

  const timeline = createTimelineController({
    getTimelineBuckets: () => tracked(adapter.getArchiveTimelineBuckets()),
    getTimelineBucketAssets: (timeBucket) => tracked(adapter.getArchiveTimelineBucketAssets(timeBucket)),
  });

  return {
    get buckets() { return timeline.buckets; },
    get refreshGeneration() { return timeline.refreshGeneration; },
    get isLoading() { return timeline.isLoading; },
    get isRefreshing() { return timeline.isRefreshing; },
    get isOffline() { return timeline.isOffline; },
    get hasInitialLoadError() { return timeline.hasInitialLoadError; },
    get permissionDenied() { return permissionDenied; },
    assetsFor: timeline.assetsFor,
    activateBucket: timeline.activateBucket,
    refresh: timeline.refresh,
    removeAsset: timeline.removeAsset,
    locateAsset: timeline.locateAsset,
    stepAsset: timeline.stepAsset,
    clear() {
      generation += 1;
      timeline.clear();
      permissionDenied = false;
    },
  };
}
