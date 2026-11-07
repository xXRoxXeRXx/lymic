import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";

export interface TimelineViewState {
  readonly buckets: TimelineBucket[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  assetsFor(timeBucket: string): TimelineAsset[];
  activateBucket(timeBucket: string): Promise<void>;
  refresh(): Promise<void>;
  clear(): void;
}

export function createTimelineController(adapter: TimelineDesktopAdapter): TimelineViewState {
  let buckets = $state<TimelineBucket[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let latestRequest = 0;
  let assetsByBucket = $state<Record<string, TimelineAsset[]>>({});
  const pendingBuckets = new Map<string, Promise<void>>();

  async function refresh() {
    const request = ++latestRequest;
    isLoading = true;
    try {
      const loaded = await adapter.getTimelineBuckets();
      if (request === latestRequest) {
        buckets = [...loaded].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
        isOffline = false;
      }
    } catch (error) {
      if (request === latestRequest) {
        console.warn("Failed to refresh Photo Timeline", error);
        isOffline = true;
      }
    } finally {
      if (request === latestRequest) isLoading = false;
    }
  }

  function clear() {
    latestRequest += 1;
    buckets = [];
    isLoading = false;
    isOffline = false;
    assetsByBucket = {};
    pendingBuckets.clear();
  }

  function assetsFor(timeBucket: string) {
    return assetsByBucket[timeBucket] ?? [];
  }

  function activateBucket(timeBucket: string) {
    if (timeBucket in assetsByBucket) return Promise.resolve();
    const pending = pendingBuckets.get(timeBucket);
    if (pending) return pending;
    const request = adapter.getTimelineBucketAssets(timeBucket).then((assets) => {
      assetsByBucket = { ...assetsByBucket, [timeBucket]: assets };
    }).finally(() => pendingBuckets.delete(timeBucket));
    pendingBuckets.set(timeBucket, request);
    return request;
  }

  return {
    get buckets() { return buckets; },
    get isLoading() { return isLoading; },
    get isOffline() { return isOffline; },
    assetsFor,
    activateBucket,
    refresh,
    clear,
  };
}
