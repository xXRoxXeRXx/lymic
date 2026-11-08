import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

export interface PersonTimelineViewState {
  readonly buckets: TimelineBucket[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  assetsFor(timeBucket: string): TimelineAsset[];
  activateBucket(timeBucket: string): Promise<void>;
  refresh(): Promise<void>;
  clear(): void;
}

function bucketFor(takenAt: string | null): string {
  const date = takenAt ? new Date(takenAt) : new Date(0);
  return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, "0")}-01T00:00:00.000Z`;
}

export function createPersonTimelineController(adapter: Pick<PeopleDesktopAdapter, "getPersonTimelineAssets">, personId: string): PersonTimelineViewState {
  let buckets = $state<TimelineBucket[]>([]);
  let assetsByBucket = $state<Record<string, TimelineAsset[]>>({});
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let latestRequest = 0;

  async function refresh() {
    const request = ++latestRequest;
    isLoading = buckets.length === 0;
    isRefreshing = true;
    try {
      const assets = await adapter.getPersonTimelineAssets(personId);
      if (request !== latestRequest) return;
      const nextAssets: Record<string, TimelineAsset[]> = {};
      for (const asset of assets) {
        const bucket = bucketFor(asset.takenAt);
        (nextAssets[bucket] ??= []).push(asset);
      }
      for (const bucketAssets of Object.values(nextAssets)) bucketAssets.sort((a, b) => (b.takenAt ?? "").localeCompare(a.takenAt ?? ""));
      assetsByBucket = nextAssets;
      buckets = Object.keys(nextAssets).sort((a, b) => b.localeCompare(a)).map((timeBucket) => ({ timeBucket, count: nextAssets[timeBucket].length }));
      isOffline = false;
    } catch (error) {
      if (request === latestRequest) {
        console.warn("Failed to refresh Recognized Person Photo Timeline", error);
        isOffline = true;
      }
    } finally {
      if (request === latestRequest) { isLoading = false; isRefreshing = false; }
    }
  }

  function clear() { latestRequest += 1; buckets = []; assetsByBucket = {}; isLoading = false; isRefreshing = false; isOffline = false; }
  return { get buckets() { return buckets; }, get isLoading() { return isLoading; }, get isRefreshing() { return isRefreshing; }, get isOffline() { return isOffline; }, assetsFor: (timeBucket) => assetsByBucket[timeBucket] ?? [], activateBucket: () => Promise.resolve(), refresh, clear };
}
