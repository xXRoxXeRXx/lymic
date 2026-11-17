import type { TimelineAsset, TimelineBucket } from "../contracts";
import { classifyRequestFailure } from "./request-failure";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** The two requests of one filtered main view (Favorites, Archive, Trash), already bound to its filter. */
export interface FilteredTimelineSource {
  getBuckets(): Promise<TimelineBucket[]>;
  getBucketAssets(timeBucket: string): Promise<TimelineAsset[]>;
  /**
   * The backend error code prefix of the view's missing-permission failure (`<prefix>_permission_denied:`),
   * when the backend reports one; without it `permissionDenied` is always false.
   */
  permissionPrefix?: string;
}

/** A filtered main view's timeline, which also reports whether the latest failed load was a missing API key permission. */
export interface FilteredTimelineController extends TimelineController {
  /** True while the latest request was rejected with the source's permission-denied code. */
  readonly permissionDenied: boolean;
}

/** Creates the shared timeline controller over the requests of one filtered main view. */
export function createFilteredTimelineController(source: FilteredTimelineSource): FilteredTimelineController {
  let permissionDenied = $state(false);
  // Bumped by clear() so a request still in flight when the session ends cannot
  // mark the next account's timeline as permission-denied.
  let generation = 0;

  // The shared timeline controller reports only that a load failed, so the
  // requests record whether the backend's credential-free permission code was the reason.
  async function tracked<T>(request: Promise<T>): Promise<T> {
    const requestGeneration = generation;
    try {
      const result = await request;
      if (requestGeneration === generation) permissionDenied = false;
      return result;
    } catch (error) {
      if (requestGeneration === generation && source.permissionPrefix !== undefined) {
        permissionDenied = classifyRequestFailure(error, source.permissionPrefix) === "permission_denied";
      }
      throw error;
    }
  }

  const timeline = createTimelineController({
    getTimelineBuckets: () => tracked(source.getBuckets()),
    getTimelineBucketAssets: (timeBucket) => tracked(source.getBucketAssets(timeBucket)),
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
