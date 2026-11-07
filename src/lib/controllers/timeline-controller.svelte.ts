import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";

export interface TimelineViewState {
  readonly buckets: TimelineBucket[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  assetsFor(timeBucket: string): TimelineAsset[];
  activateBucket(timeBucket: string): Promise<void>;
  refresh(): Promise<void>;
  clear(): void;
}

export function createTimelineController(adapter: TimelineDesktopAdapter): TimelineViewState {
  let buckets = $state<TimelineBucket[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let latestRequest = 0;
  let assetsByBucket = $state<Record<string, TimelineAsset[]>>({});
  const pendingBuckets = new Map<string, Promise<void>>();

  async function refresh() {
    const request = ++latestRequest;
    // A background refresh must not replace an already usable timeline with a
    // loading state.  Keep the cached view interactive while the request runs.
    isLoading = buckets.length === 0;
    isRefreshing = true;
    try {
      const loaded = await adapter.getTimelineBuckets();
      if (request === latestRequest) {
        buckets = [...loaded].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
        // A bucket can receive new assets after synchronization.  Drop only
        // successful-session metadata so visible sections fetch current data.
        assetsByBucket = {};
        pendingBuckets.clear();
        isOffline = false;
      }
    } catch (error) {
      if (request === latestRequest) {
        console.warn("Failed to refresh Photo Timeline", error);
        isOffline = true;
      }
    } finally {
      if (request === latestRequest) {
        isLoading = false;
        isRefreshing = false;
      }
    }
  }

  function clear() {
    latestRequest += 1;
    buckets = [];
    isLoading = false;
    isRefreshing = false;
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
      isOffline = false;
    }).catch((error) => {
      isOffline = true;
      throw error;
    }).finally(() => pendingBuckets.delete(timeBucket));
    pendingBuckets.set(timeBucket, request);
    return request;
  }

  return {
    get buckets() { return buckets; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    assetsFor,
    activateBucket,
    refresh,
    clear,
  };
}
