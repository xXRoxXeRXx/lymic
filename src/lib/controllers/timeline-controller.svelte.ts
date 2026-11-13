import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";

export interface TimelineViewState {
  readonly buckets: TimelineBucket[];
  readonly refreshGeneration: number;
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  readonly hasInitialLoadError: boolean;
  assetsFor(timeBucket: string): TimelineAsset[];
  activateBucket(timeBucket: string): Promise<void>;
  refresh(): Promise<void>;
  clear(): void;
  removeAsset?(assetId: string): void;
}

export function createTimelineController(adapter: TimelineDesktopAdapter): TimelineViewState {
  let buckets = $state<TimelineBucket[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let hasInitialLoadError = $state(false);
  let refreshGeneration = $state(0);
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
    if (isLoading) hasInitialLoadError = false;
    isRefreshing = true;
    try {
      const loaded = await adapter.getTimelineBuckets();
      if (request === latestRequest) {
        const list = Array.isArray(loaded) ? loaded : [];
        const nextBuckets = [...list].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
        const activeKeys = new Set(nextBuckets.map((bucket) => bucket.timeBucket));
        const prunedAssets: Record<string, TimelineAsset[]> = {};
        staleBuckets.clear();
        for (const [timeBucket, assets] of Object.entries(assetsByBucket)) {
          if (!activeKeys.has(timeBucket)) continue;
          prunedAssets[timeBucket] = assets;
          staleBuckets.add(timeBucket);
        }
        buckets = nextBuckets;
        assetsByBucket = prunedAssets;
        refreshGeneration += 1;
        // Keep rendered assets visible while fresh bucket details arrive. A
        // generation makes requests started before this refresh unable to
        // overwrite the newer session view.
        assetGeneration += 1;
        pendingBuckets.clear();
        isOffline = false;
        hasInitialLoadError = false;
      }
    } catch (error) {
      if (request === latestRequest) {
        console.warn("Failed to refresh Photo Timeline", error);
        isOffline = true;
        hasInitialLoadError = buckets.length === 0;
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
    refreshGeneration += 1;
    isLoading = false;
    isRefreshing = false;
    isOffline = false;
    hasInitialLoadError = false;
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

  function removeAsset(assetId: string) {
    let changed = false;
    const nextBuckets: TimelineBucket[] = [];
    const nextAssetsByBucket: Record<string, TimelineAsset[]> = {};

    for (const bucket of buckets) {
      const assets = assetsByBucket[bucket.timeBucket];
      if (!assets) {
        nextBuckets.push(bucket);
        continue;
      }
      const remaining = assets.filter((asset) => asset.id !== assetId);
      const removedCount = assets.length - remaining.length;
      if (removedCount === 0) {
        nextBuckets.push(bucket);
        nextAssetsByBucket[bucket.timeBucket] = assets;
        continue;
      }
      changed = true;
      if (remaining.length === 0) {
        staleBuckets.delete(bucket.timeBucket);
        continue;
      }
      nextBuckets.push({
        ...bucket,
        count: Math.max(remaining.length, bucket.count - removedCount),
      });
      nextAssetsByBucket[bucket.timeBucket] = remaining;
    }

    if (!changed) return;
    buckets = nextBuckets;
    assetsByBucket = nextAssetsByBucket;
  }

  return {
    get buckets() { return buckets; },
    get refreshGeneration() { return refreshGeneration; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    get hasInitialLoadError() { return hasInitialLoadError; },
    assetsFor,
    activateBucket,
    refresh,
    clear,
    removeAsset,
  };
}
