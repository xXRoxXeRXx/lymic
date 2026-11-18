import type { TimelineAsset } from "../contracts";
import type { TimelineAssetPosition, TimelineController } from "../controllers/timeline-controller.svelte";

interface RemovedLightboxPosition extends TimelineAssetPosition {
  assetId: string;
}

export interface RemoteAssetFavoritesOptions {
  readonly timeline: TimelineController;
  readonly setAssetFavorite: (assetId: string, isFavorite: boolean) => Promise<void>;
  /** True in the Favorites Timeline, where an unfavorited Remote Asset leaves the timeline. */
  readonly removeUnfavorited: boolean;
  readonly errorMessage: string;
  /** The favorite status to show for an asset, including changes not yet in the timeline data. */
  readonly isFavorite: (asset: TimelineAsset) => boolean;
  /** Called when a favorite status should be reflected in caches and open views. */
  onReflect?: (assetId: string, isFavorite: boolean) => void;
  /** Called after an asset is removed from the Favorites Timeline. */
  onRemoved?: (assetId: string) => void;
}

/** What differs between the Favorites Timeline and other timelines; chosen once per call from `removeUnfavorited`. */
interface FavoritesMode {
  /** A single-asset update was confirmed by the server. */
  singleConfirmed(assetId: string, isFavorite: boolean): void;
  /** A single-asset update failed while the Lightbox was not showing that asset. */
  singleFailed(): void;
  /** One asset of a selection update was confirmed by the server. */
  selectionConfirmed(assetId: string, isFavorite: boolean): void;
  /** Whether the grid shows reflected favorite changes before the timeline data catches up. */
  showsReflectedStatus: boolean;
  navigate(delta: number, current: TimelineAsset): Promise<TimelineAsset | null>;
}

/** Shares Remote Asset favorite updates between the Lightbox and timeline selections, including Favorites Timeline removal. */
export function createRemoteAssetFavorites(options: RemoteAssetFavoritesOptions) {
  let activeAssetId = $state<string | null>(null);
  let navigationGeneration = 0;
  let removedPosition = $state<RemovedLightboxPosition | null>(null);
  let updateError = $state("");

  function open(assetId: string) {
    removedPosition = null;
    activeAssetId = assetId;
  }

  function close() {
    navigationGeneration += 1;
    activeAssetId = null;
    removedPosition = null;
  }

  function clearError() {
    updateError = "";
  }

  function isFavorite(asset: TimelineAsset) {
    return mode().showsReflectedStatus ? options.isFavorite(asset) : asset.isFavorite;
  }

  function confirmRemoval(assetId: string, isFavorite: boolean) {
    if (isFavorite) return;
    const { timeline } = options;
    const located = timeline.locateAsset(assetId);
    if (located) {
      if (activeAssetId === assetId) {
        removedPosition = { assetId, ...located };
      } else if (removedPosition?.timeBucket === located.timeBucket && located.index < removedPosition.index) {
        removedPosition = { ...removedPosition, index: Math.max(0, removedPosition.index - 1) };
      }
    }
    timeline.removeAsset(assetId);
    options.onRemoved?.(assetId);
  }

  /** Remembers where the open Remote Asset sits before an action removes it from the timeline, so the Lightbox can step on from there. */
  function expectRemoval(assetId: string) {
    if (activeAssetId !== assetId) return;
    const located = options.timeline.locateAsset(assetId);
    if (located) removedPosition = { assetId, ...located };
  }

  /** The expected removal did not happen: the Lightbox keeps navigating from the asset itself. */
  function cancelExpectedRemoval() {
    removedPosition = null;
  }

  async function navigateFromFormerPosition(delta: number, current: TimelineAsset) {
    const navigation = ++navigationGeneration;
    const former = removedPosition?.assetId === current.id ? removedPosition : null;
    const next = await options.timeline.stepAsset(current.id, delta, former);
    if (!next || navigation !== navigationGeneration || activeAssetId !== current.id) return null;
    removedPosition = null;
    activeAssetId = next.id;
    return { ...next, isFavorite: options.isFavorite(next) };
  }

  async function navigateAround(delta: number, current: TimelineAsset) {
    const { timeline } = options;
    const assets = timeline.buckets.flatMap((bucket) => timeline.assetsFor(bucket.timeBucket));
    if (!assets.length) return null;
    const index = assets.findIndex((candidate) => candidate.id === current.id);
    const next = assets[(index + delta + assets.length) % assets.length];
    return { ...next, isFavorite: options.isFavorite(next) };
  }

  /** Reports a favorite status the UI should show before or after the server confirms it. */
  function reflect(assetId: string, isFavorite: boolean) {
    options.onReflect?.(assetId, isFavorite);
  }

  const favoritesTimelineMode: FavoritesMode = {
    showsReflectedStatus: false,
    singleConfirmed: confirmRemoval,
    singleFailed: () => { updateError = options.errorMessage; },
    selectionConfirmed: confirmRemoval,
    navigate: navigateFromFormerPosition,
  };

  // The Lightbox reflects single-asset changes itself, so only selections need to.
  const otherTimelineMode: FavoritesMode = {
    showsReflectedStatus: true,
    singleConfirmed: () => {},
    singleFailed: () => {},
    selectionConfirmed: reflect,
    // An asset removed by another action (Unarchive) leaves the Lightbox stepping on from where it was.
    navigate: (delta, current) => removedPosition?.assetId === current.id ? navigateFromFormerPosition(delta, current) : navigateAround(delta, current),
  };

  function mode() {
    return options.removeUnfavorited ? favoritesTimelineMode : otherTimelineMode;
  }

  /** Forgets Lightbox tracking and the error. */
  function reset() {
    close();
    clearError();
  }

  async function updateOne(assetId: string, isFavorite: boolean) {
    clearError();
    try {
      await options.setAssetFavorite(assetId, isFavorite);
      mode().singleConfirmed(assetId, isFavorite);
    } catch (error) {
      if (activeAssetId !== assetId) mode().singleFailed();
      throw error;
    }
  }

  /** Updates each asset independently so a partial failure is reported; null means the caller's context ended mid-update. */
  async function updateMany(assetIds: string[], isFavorite: boolean, isCurrent: () => boolean) {
    clearError();
    let succeeded = 0;
    for (const assetId of assetIds) {
      try {
        await options.setAssetFavorite(assetId, isFavorite);
        if (!isCurrent()) return null;
        mode().selectionConfirmed(assetId, isFavorite);
        succeeded += 1;
      } catch {
        if (!isCurrent()) return null;
        // Continue so the result can accurately report a partial update.
      }
    }
    return isCurrent() ? { succeeded, total: assetIds.length } : null;
  }

  function navigate(delta: number, current: TimelineAsset) {
    return mode().navigate(delta, current);
  }

  return {
    get updateError() { return updateError; },
    open,
    close,
    clearError,
    isFavorite,
    reflect,
    reset,
    expectRemoval,
    cancelExpectedRemoval,
    updateOne,
    updateMany,
    navigate,
  };
}
