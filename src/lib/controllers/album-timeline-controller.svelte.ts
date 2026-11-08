import type { TimelineAsset } from "../contracts";
import type { AlbumTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineViewState } from "./timeline-controller.svelte";

/**
 * Adapts one complete album search result to the shared TimelineView seam.
 * The album request completes before its bucket index is published; activated
 * months then read from that already complete result rather than the network.
 */
export function createAlbumTimelineController(
  adapter: AlbumTimelineDesktopAdapter,
  albumId: string,
): TimelineViewState {
  let assetsByBucket: Record<string, TimelineAsset[]> = {};

  return createTimelineController({
    async getTimelineBuckets() {
      const timeline = await adapter.getAlbumTimeline(albumId);
      assetsByBucket = timeline.assetsByBucket;
      return timeline.buckets;
    },
    getTimelineBucketAssets(timeBucket) {
      return Promise.resolve(assetsByBucket[timeBucket] ?? []);
    },
  });
}
