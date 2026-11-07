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
  let assetGeneration = 0;
  let assetsByBucket = $state<Record<string, TimelineAsset[]>>({});
  const pendingBuckets = new Map<string, Promise<void>>();
  const staleBuckets = new Set<string>();

  async function refresh() {
    const request = ++latestRequest;
    // A background refresh must not replace an already usable timeline with a
    // loading state.  Keep the cached view interactive while the request runs.
    isLoading = buckets.length === 0;
    isRefreshing = true;
    try {
      const loaded = await adapter.getTimelineBuckets();
      if (request === latestRequest) {
        const list = Array.isArray(loaded) ? loaded : [];
        buckets = [...list].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
        // Keep rendered assets visible while fresh bucket details arrive. A
        // generation makes requests started before this refresh unable to
        // overwrite the newer session view.
        assetGeneration += 1;
        staleBuckets.clear();
        for (const timeBucket of Object.keys(assetsByBucket)) staleBuckets.add(timeBucket);
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
    staleBuckets.clear();
    assetGeneration += 1;
  }

  function assetsFor(timeBucket: string) {
    return assetsByBucket[timeBucket] ?? [];
  }

  function activateBucket(timeBucket: string) {
    if (timeBucket in assetsByBucket && !staleBuckets.has(timeBucket)) return Promise.resolve();
    const pending = pendingBuckets.get(timeBucket);
    if (pending) return pending;
    const generation = assetGeneration;
    const request = adapter.getTimelineBucketAssets(timeBucket).then((assets) => {
      if (generation !== assetGeneration) return;
      assetsByBucket = { ...assetsByBucket, [timeBucket]: assets };
      staleBuckets.delete(timeBucket);
      isOffline = false;
    }).catch((error) => {
      isOffline = true;
      throw error;
    }).finally(() => {
      if (pendingBuckets.get(timeBucket) === request) pendingBuckets.delete(timeBucket);
    });
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
