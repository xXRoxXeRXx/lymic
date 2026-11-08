import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

const PAGE_SIZE = 250;

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
  let page = 0;
  let hasNextPage = false;
  let pendingPage: Promise<void> | null = null;

  function addAssets(assets: TimelineAsset[]) {
    const nextAssets = { ...assetsByBucket };
    for (const asset of assets) {
      const bucket = bucketFor(asset.takenAt);
      nextAssets[bucket] = [...(nextAssets[bucket] ?? []), asset];
    }
    for (const bucketAssets of Object.values(nextAssets)) bucketAssets.sort((a, b) => (b.takenAt ?? "").localeCompare(a.takenAt ?? ""));
    assetsByBucket = nextAssets;
    buckets = Object.keys(nextAssets).sort((a, b) => b.localeCompare(a)).map((timeBucket) => ({ timeBucket, count: nextAssets[timeBucket].length }));
  }

  async function loadPage(nextPage: number, replace: boolean) {
    if (pendingPage) return pendingPage;
    const request = latestRequest;
    const load = adapter.getPersonTimelineAssets(personId, nextPage, PAGE_SIZE).then((result) => {
      if (request !== latestRequest) return;
      if (replace) {
        assetsByBucket = {};
        buckets = [];
      }
      addAssets(result.assets);
      page = nextPage;
      hasNextPage = result.hasNextPage;
      isOffline = false;
    }).catch((error) => {
      if (request === latestRequest) {
        console.warn("Failed to load Recognized Person Photo Timeline", error);
        isOffline = true;
      }
      throw error;
    }).finally(() => {
      if (pendingPage === load) pendingPage = null;
    });
    pendingPage = load;
    return load;
  }

  async function refresh() {
    latestRequest += 1;
    isLoading = buckets.length === 0;
    isRefreshing = true;
    try {
      await loadPage(1, true);
    } catch {
      // The cached timeline remains usable while the offline state is shown.
    } finally {
      isLoading = false;
      isRefreshing = false;
    }
  }

  function activateBucket(timeBucket: string) {
    const oldestLoadedBucket = buckets.at(-1)?.timeBucket;
    if (hasNextPage && timeBucket === oldestLoadedBucket) return loadPage(page + 1, false);
    return Promise.resolve();
  }

  function clear() {
    latestRequest += 1;
    buckets = [];
    assetsByBucket = {};
    isLoading = false;
    isRefreshing = false;
    isOffline = false;
    page = 0;
    hasNextPage = false;
    pendingPage = null;
  }
  return { get buckets() { return buckets; }, get isLoading() { return isLoading; }, get isRefreshing() { return isRefreshing; }, get isOffline() { return isOffline; }, assetsFor: (timeBucket) => assetsByBucket[timeBucket] ?? [], activateBucket, refresh, clear };
}
