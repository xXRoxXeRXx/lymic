import type { FavoritesTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineViewState } from "./timeline-controller.svelte";

export interface FavoritesTimelineViewState extends TimelineViewState {
  removeAsset(assetId: string): void;
}

/** Creates a FavoritesTimelineViewState backed by the user's Immich favorite timeline endpoints. */
export function createFavoritesTimelineController(
  adapter: Pick<FavoritesTimelineDesktopAdapter, "getFavoritesTimelineBuckets" | "getFavoritesTimelineBucketAssets">,
): FavoritesTimelineViewState {
  const controller = createTimelineController({
    getTimelineBuckets: () => adapter.getFavoritesTimelineBuckets(),
    getTimelineBucketAssets: (timeBucket) => adapter.getFavoritesTimelineBucketAssets(timeBucket),
  });
  return {
    get buckets() { return controller.buckets; },
    get refreshGeneration() { return controller.refreshGeneration; },
    get isLoading() { return controller.isLoading; },
    get isRefreshing() { return controller.isRefreshing; },
    get isOffline() { return controller.isOffline; },
    get hasInitialLoadError() { return controller.hasInitialLoadError; },
    assetsFor: controller.assetsFor,
    activateBucket: controller.activateBucket,
    refresh: controller.refresh,
    clear: controller.clear,
    removeAsset: (assetId) => controller.removeAsset?.(assetId),
  };
}

