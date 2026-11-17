import type { ContextSearchPage, TimelineAsset, TimelineBucket } from "../contracts";
import type { TimelineAssetPosition, TimelineController } from "./timeline-controller.svelte";

/** The one pseudo-bucket that holds every Context Search Result: a flat list has no Timeline Buckets (ADR-0009). */
export const RELEVANCE_BUCKET = "relevance";

/**
 * Context Search Results: a flat, relevance-ordered list that further pages are appended to. It satisfies the
 * shared timeline contract through a single pseudo-bucket, so the shared timeline view, Lightbox and Remote
 * Asset actions show it; `refresh()` reloads the first page, `loadMore()` loads (or retries) the next one.
 */
export interface ContextResults extends TimelineController {
  /** Another page can be requested. */
  readonly hasNextPage: boolean;
  readonly isLoadingMore: boolean;
  /** A further page failed; the pages already shown are kept and `loadMore()` retries it. */
  readonly loadMoreFailed: boolean;
  loadMore(): Promise<void>;
}

export interface ContextResultsSource {
  /** Loads one page of the current Context Search; page 1 is the first. */
  loadPage(page: number): Promise<ContextSearchPage>;
}

export function createContextResultsController(source: ContextResultsSource): ContextResults {
  let assets = $state<TimelineAsset[]>([]);
  let nextPage = $state<number | null>(null);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let hasInitialLoadError = $state(false);
  let isLoadingMore = $state(false);
  let loadMoreFailed = $state(false);
  let refreshGeneration = $state(0);
  // Bumped by every refresh and by clear(), so a page that arrives late cannot touch newer results.
  let generation = 0;
  let pendingMore: Promise<void> | null = null;

  const buckets = $derived<TimelineBucket[]>(assets.length > 0 ? [{ timeBucket: RELEVANCE_BUCKET, count: assets.length }] : []);

  function append(loaded: TimelineAsset[], replace: boolean) {
    const known = new Set(replace ? [] : assets.map((asset) => asset.id));
    const fresh: TimelineAsset[] = [];
    for (const asset of loaded) {
      if (known.has(asset.id)) continue;
      known.add(asset.id);
      fresh.push(asset);
    }
    assets = replace ? fresh : [...assets, ...fresh];
  }

  async function refresh() {
    const requestGeneration = ++generation;
    pendingMore = null;
    isLoadingMore = false;
    loadMoreFailed = false;
    isLoading = assets.length === 0;
    if (isLoading) hasInitialLoadError = false;
    isRefreshing = true;
    try {
      const page = await source.loadPage(1);
      if (requestGeneration !== generation) return;
      append(page.assets, true);
      nextPage = page.nextPage;
      refreshGeneration += 1;
      isOffline = false;
      hasInitialLoadError = false;
    } catch {
      if (requestGeneration !== generation) return;
      isOffline = true;
      hasInitialLoadError = assets.length === 0;
    } finally {
      if (requestGeneration === generation) {
        isLoading = false;
        isRefreshing = false;
      }
    }
  }

  function loadMore() {
    if (pendingMore) return pendingMore;
    if (nextPage === null || isRefreshing) return Promise.resolve();
    const requestGeneration = generation;
    const page = nextPage;
    isLoadingMore = true;
    const request = source
      .loadPage(page)
      .then((loaded) => {
        if (requestGeneration !== generation) return;
        append(loaded.assets, false);
        nextPage = loaded.nextPage;
        loadMoreFailed = false;
      })
      .catch(() => {
        if (requestGeneration === generation) loadMoreFailed = true;
      })
      .finally(() => {
        if (requestGeneration !== generation) return;
        pendingMore = null;
        isLoadingMore = false;
      });
    pendingMore = request;
    return request;
  }

  function clear() {
    generation += 1;
    pendingMore = null;
    assets = [];
    nextPage = null;
    isLoading = false;
    isRefreshing = false;
    isOffline = false;
    hasInitialLoadError = false;
    isLoadingMore = false;
    loadMoreFailed = false;
    refreshGeneration += 1;
  }

  function assetsFor(timeBucket: string) {
    return timeBucket === RELEVANCE_BUCKET ? assets : [];
  }

  function removeAsset(assetId: string) {
    if (assets.some((asset) => asset.id === assetId)) assets = assets.filter((asset) => asset.id !== assetId);
  }

  function locateAsset(assetId: string): TimelineAssetPosition | null {
    const index = assets.findIndex((asset) => asset.id === assetId);
    return index === -1 ? null : { timeBucket: RELEVANCE_BUCKET, index };
  }

  /** Steps through the loaded results, wrapping at the ends; a removed asset steps from where it was. */
  async function stepAsset(currentId: string, delta: number, formerPosition: TimelineAssetPosition | null = null) {
    if (assets.length === 0) return null;
    const current = locateAsset(currentId);
    if (current) return assets[((current.index + delta) % assets.length + assets.length) % assets.length];
    const index = (formerPosition?.index ?? 0) + (delta < 0 ? -1 : 0);
    return assets[Math.min(Math.max(index, 0), assets.length - 1)];
  }

  return {
    get buckets() { return buckets; },
    get refreshGeneration() { return refreshGeneration; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    get hasInitialLoadError() { return hasInitialLoadError; },
    get hasNextPage() { return nextPage !== null; },
    get isLoadingMore() { return isLoadingMore; },
    get loadMoreFailed() { return loadMoreFailed; },
    assetsFor,
    activateBucket: () => Promise.resolve(),
    refresh,
    clear,
    removeAsset,
    locateAsset,
    stepAsset,
    loadMore,
  };
}
