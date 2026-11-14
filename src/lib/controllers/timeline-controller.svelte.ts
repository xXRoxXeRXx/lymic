import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";

export interface TimelineAssetPosition {
  timeBucket: string;
  index: number;
}

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
  locateAsset?(assetId: string): TimelineAssetPosition | null;
  stepAsset?(currentId: string, delta: number, formerPosition?: TimelineAssetPosition | null): Promise<TimelineAsset | null>;
}

export interface TimelineController extends TimelineViewState {
  removeAsset(assetId: string): void;
  locateAsset(assetId: string): TimelineAssetPosition | null;
  stepAsset(currentId: string, delta: number, formerPosition?: TimelineAssetPosition | null): Promise<TimelineAsset | null>;
}

export interface CompleteTimelineSource {
  getTimeline(): Promise<{ buckets: TimelineBucket[]; assetsByBucket: Record<string, TimelineAsset[]> }>;
}

export function createTimelineController(
  adapter: TimelineDesktopAdapter | CompleteTimelineSource,
): TimelineController {
  let buckets = $state<TimelineBucket[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let hasInitialLoadError = $state(false);
  let refreshGeneration = $state(0);
  let latestRequest = 0;
  let assetGeneration = 0;
  let assetsByBucket = $state<Record<string, TimelineAsset[]>>({});
  let completeAssetsByBucket: Record<string, TimelineAsset[]> = {};
  const pendingBuckets = new Map<string, Promise<void>>();
  const staleBuckets = new Set<string>();
  let removalVersion = 0;
  const removalVersions = new Map<string, number>();

  const source = "getTimeline" in adapter
    ? {
        load: () => adapter.getTimeline(),
        loadBucket: (timeBucket: string) => Promise.resolve(completeAssetsByBucket[timeBucket] ?? []),
      }
    : {
        load: async () => ({ buckets: await adapter.getTimelineBuckets(), assetsByBucket: undefined }),
        loadBucket: (timeBucket: string) => adapter.getTimelineBucketAssets(timeBucket),
      };

  async function refresh() {
    const request = ++latestRequest;
    const requestVersion = removalVersion;
    // A background refresh must not replace an already usable timeline with a
    // loading state.  Keep the cached view interactive while the request runs.
    isLoading = buckets.length === 0;
    if (isLoading) hasInitialLoadError = false;
    isRefreshing = true;
    try {
      let loaded: TimelineBucket[];
      let loadedAssetsByBucket: Record<string, TimelineAsset[]> | undefined;
      let version: number;
      do {
        version = removalVersion;
        const snapshot = await source.load();
        loaded = snapshot.buckets;
        loadedAssetsByBucket = snapshot.assetsByBucket;
        if (request !== latestRequest) return;
      } while (version !== removalVersion);
      if (request === latestRequest) {
        const list = Array.isArray(loaded) ? loaded : [];
        const nextBuckets = [...list].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
        const activeKeys = new Set(nextBuckets.map((bucket) => bucket.timeBucket));
        const prunedAssets: Record<string, TimelineAsset[]> = {};
        staleBuckets.clear();
        if (loadedAssetsByBucket) {
          completeAssetsByBucket = loadedAssetsByBucket;
          for (const timeBucket of activeKeys) {
            if (timeBucket in assetsByBucket) {
              prunedAssets[timeBucket] = loadedAssetsByBucket[timeBucket] ?? [];
            }
          }
        } else {
          for (const [timeBucket, assets] of Object.entries(assetsByBucket)) {
            if (!activeKeys.has(timeBucket)) continue;
            prunedAssets[timeBucket] = assets;
            staleBuckets.add(timeBucket);
          }
        }
        buckets = nextBuckets;
        assetsByBucket = prunedAssets;
        refreshGeneration += 1;
        // Keep rendered assets visible while fresh bucket details arrive. A
        // generation makes requests started before this refresh unable to
        // overwrite the newer session view.
        assetGeneration += 1;
        pendingBuckets.clear();
        for (const [id, version] of removalVersions) {
          if (version <= requestVersion) removalVersions.delete(id);
        }
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
    completeAssetsByBucket = {};
    pendingBuckets.clear();
    staleBuckets.clear();
    removalVersions.clear();
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
    const requestVersion = removalVersion;
    const request = source.loadBucket(timeBucket).then((assets) => {
      if (generation !== assetGeneration) return;
      const removedDuringRequest = new Set(
        [...removalVersions].filter(([, version]) => version > requestVersion).map(([id]) => id),
      );
      assetsByBucket = { ...assetsByBucket, [timeBucket]: assets.filter((asset) => !removedDuringRequest.has(asset.id)) };
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
      if (remaining.length === 0 && bucket.count <= removedCount) {
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
    removalVersions.set(assetId, ++removalVersion);
    buckets = nextBuckets;
    assetsByBucket = nextAssetsByBucket;
  }

  function locateAsset(assetId: string): TimelineAssetPosition | null {
    for (const bucket of buckets) {
      const assets = assetsByBucket[bucket.timeBucket];
      if (!assets) continue;
      const index = assets.findIndex((asset) => asset.id === assetId);
      if (index !== -1) return { timeBucket: bucket.timeBucket, index };
    }
    return null;
  }

  async function stepAsset(
    currentId: string,
    delta: number,
    formerPosition: TimelineAssetPosition | null = null,
  ): Promise<TimelineAsset | null> {
    if (!buckets.length) return null;
    const currentPos = locateAsset(currentId);
    const anchor = currentPos?.timeBucket ?? formerPosition?.timeBucket ?? null;
    const anchorIndex = anchor ? buckets.findIndex((bucket) => bucket.timeBucket === anchor) : -1;
    if (anchorIndex !== -1) {
      const assets = assetsFor(buckets[anchorIndex].timeBucket);
      const index = currentPos
        ? currentPos.index + delta
        : (formerPosition?.index ?? 0) + (delta < 0 ? -1 : 0);
      if (index >= 0 && index < assets.length) return assets[index];
    }
    const insertion = anchor ? buckets.findIndex((bucket) => bucket.timeBucket < anchor) : -1;
    const fallbackBoundary = insertion < 0 ? buckets.length : insertion;
    const start = anchorIndex !== -1
      ? anchorIndex + delta
      : delta > 0
        ? fallbackBoundary
        : fallbackBoundary - 1;
    for (let step = 0; step < buckets.length; step++) {
      const index = ((start + step * delta) % buckets.length + buckets.length) % buckets.length;
      const bucket = buckets[index];
      try {
        await activateBucket(bucket.timeBucket);
      } catch {
        return null;
      }
      const assets = assetsFor(bucket.timeBucket);
      if (!assets.length) continue;
      return delta > 0 ? assets[0] : assets[assets.length - 1];
    }
    return null;
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
    locateAsset,
    stepAsset,
  };
}
