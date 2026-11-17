import { FILTERED_TIMELINES, type FilteredTimeline } from "../contracts";
import type { FilteredTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure } from "./request-failure";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** A Filtered Timeline's timeline, which also reports whether the latest failed load was a missing API key permission. */
export interface FilteredTimelineController extends TimelineController {
  /**
   * True while the latest request was rejected with the backend's `<timeline>_permission_denied:` code.
   * Only a Filtered Timeline whose backend request has a permission code (the Archive Timeline) can report it.
   */
  readonly permissionDenied: boolean;
}

/** Creates the shared timeline controller over the requests of one Filtered Timeline. */
export function createFilteredTimelineController(
  adapter: FilteredTimelineDesktopAdapter,
  filteredTimeline: FilteredTimeline,
): FilteredTimelineController {
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
      if (requestGeneration === generation) {
        permissionDenied = classifyRequestFailure(error, filteredTimeline) === "permission_denied";
      }
      throw error;
    }
  }

  const timeline = createTimelineController({
    getTimelineBuckets: () => tracked(adapter.getFilteredTimelineBuckets(filteredTimeline)),
    getTimelineBucketAssets: (timeBucket) => tracked(adapter.getFilteredTimelineBucketAssets(filteredTimeline, timeBucket)),
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

/** One controller per Filtered Timeline (Favorites, Archive, Trash). */
export function createFilteredTimelineControllers(
  adapter: FilteredTimelineDesktopAdapter,
): Record<FilteredTimeline, FilteredTimelineController> {
  return Object.fromEntries(
    FILTERED_TIMELINES.map((filteredTimeline) => [filteredTimeline, createFilteredTimelineController(adapter, filteredTimeline)]),
  ) as Record<FilteredTimeline, FilteredTimelineController>;
}
