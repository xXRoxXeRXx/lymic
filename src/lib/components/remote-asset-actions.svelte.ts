import type { AssetDetails, TimelineAsset } from "../contracts";

export interface RemoteAssetActionsOptions {
  readonly getAssetDetails: (assetId: string) => Promise<AssetDetails | null>;
  readonly chooseDownloadDestination: (originalFileName: string) => Promise<string | null>;
  readonly downloadAsset: (assetId: string, destinationPath: string) => Promise<void>;
  readonly downloadArchive: (assetIds: string[]) => Promise<void>;
}

export interface RemoteAssetDetailsLoad {
  details: AssetDetails | null;
  /** True when the details were requested before the last `invalidate()`; they were not cached and should be ignored. */
  stale: boolean;
}

/** `saved` finished, `cancelled` means no destination was chosen, `stale` means the caller's context ended first. */
export type RemoteAssetDownloadOutcome = "saved" | "cancelled" | "stale";

/** Cached details beyond this count evict the oldest entry. */
export const maxCachedDetails = 500;

/**
 * Shares Remote Asset detail retrieval, caching and Download between the Lightbox, timeline selection and Map View.
 *
 * Details are cached and requested once at a time per asset. `invalidate()` drops the cache and marks requests still in flight as stale.
 * A favorite status reflected here also patches cached and later-loaded details, so every view sees one favorite state.
 */
export function createRemoteAssetActions(options: RemoteAssetActionsOptions) {
  const cache = new Map<string, AssetDetails | null>();
  const inFlight = new Map<string, Promise<RemoteAssetDetailsLoad>>();
  let generation = $state(0);
  let favoriteOverrides = $state<Record<string, boolean>>({});

  function store(assetId: string, details: AssetDetails | null) {
    cache.delete(assetId);
    if (cache.size >= maxCachedDetails) cache.delete(cache.keys().next().value!);
    cache.set(assetId, details);
  }

  function withOverride(assetId: string, details: AssetDetails | null) {
    const favorite = favoriteOverrides[assetId];
    return details && favorite !== undefined ? { ...details, isFavorite: favorite } : details;
  }

  /** Cached details; `undefined` when not cached and `null` when the asset is known to have none. */
  function cachedDetails(assetId: string) {
    return cache.get(assetId);
  }

  function loadDetails(assetId: string): Promise<RemoteAssetDetailsLoad> {
    if (cache.has(assetId)) return Promise.resolve({ details: cache.get(assetId) ?? null, stale: false });
    const existing = inFlight.get(assetId);
    if (existing) return existing;

    const requestGeneration = generation;
    const request: Promise<RemoteAssetDetailsLoad> = options.getAssetDetails(assetId)
      .then((received) => {
        if (requestGeneration !== generation) return { details: received, stale: true };
        const details = withOverride(assetId, received);
        store(assetId, details);
        return { details, stale: false };
      }, (error: unknown) => {
        if (requestGeneration !== generation) return { details: null, stale: true };
        throw error;
      })
      .finally(() => {
        if (inFlight.get(assetId) === request) inFlight.delete(assetId);
      });
    inFlight.set(assetId, request);
    return request;
  }

  function isFavorite(asset: Pick<TimelineAsset, "id" | "isFavorite">) {
    return favoriteOverrides[asset.id] ?? asset.isFavorite;
  }

  /** Records a favorite status the UI should show before or after the server confirms it. */
  function reflectFavorite(assetId: string, isFavorite: boolean) {
    favoriteOverrides = { ...favoriteOverrides, [assetId]: isFavorite };
    const cached = cache.get(assetId);
    if (cached) cache.set(assetId, { ...cached, isFavorite });
  }

  /** Drops cached details and in-flight results, e.g. after a timeline refresh; keeps favorite overrides for assets whose update is still in flight. */
  function invalidate(retainFavoriteFor: readonly string[] = []) {
    generation += 1;
    cache.clear();
    inFlight.clear();
    favoriteOverrides = Object.fromEntries(
      retainFavoriteFor.filter((assetId) => assetId in favoriteOverrides).map((assetId) => [assetId, favoriteOverrides[assetId]]),
    );
  }

  /**
   * Downloads one asset's original file to a chosen destination, or several assets as one archive.
   * Rejects on failure. `isCurrent` lets the caller cancel: once it returns false the result is `stale` and no further step runs.
   */
  async function download(assetIds: string[], isCurrent: () => boolean = () => true): Promise<RemoteAssetDownloadOutcome> {
    if (assetIds.length === 1) {
      const assetId = assetIds[0];
      let loaded = await loadDetails(assetId);
      // An invalidation during the request says nothing about this Download, so ask again.
      while (loaded.stale && isCurrent()) loaded = await loadDetails(assetId);
      const { details } = loaded;
      if (!isCurrent()) return "stale";
      if (!details?.originalFileName) throw new Error("Original file name is unavailable");
      const destinationPath = await options.chooseDownloadDestination(details.originalFileName);
      if (!destinationPath) return "cancelled";
      if (!isCurrent()) return "stale";
      await options.downloadAsset(assetId, destinationPath);
    } else {
      await options.downloadArchive(assetIds);
    }
    return isCurrent() ? "saved" : "stale";
  }

  return {
    get generation() { return generation; },
    cachedDetails,
    loadDetails,
    isFavorite,
    reflectFavorite,
    invalidate,
    download,
  };
}

export type RemoteAssetActions = ReturnType<typeof createRemoteAssetActions>;
