import type { TimelineAsset } from "../contracts";
import type { TimelineAssetPosition, TimelineViewState } from "../controllers/timeline-controller.svelte";

interface RemovedLightboxPosition extends TimelineAssetPosition {
  assetId: string;
}

export interface RemoteAssetFavoritesOptions {
  getTimeline: () => TimelineViewState;
  setAssetFavorite: (assetId: string, isFavorite: boolean) => Promise<void>;
  /** True in the Favorites Timeline, where an unfavorited Remote Asset leaves the timeline. */
  removeUnfavorited: boolean;
  errorMessage: string;
  /** Called when a favorite status should be reflected in caches and open views. */
  onReflect?: (assetId: string, isFavorite: boolean) => void;
  /** Called after an asset is removed from the Favorites Timeline. */
  onRemoved?: (assetId: string) => void;
}

/** Shares Remote Asset favorite updates between the Lightbox and timeline selections, including Favorites Timeline removal. */
export function createRemoteAssetFavorites(options: RemoteAssetFavoritesOptions) {
  let activeAssetId = $state<string | null>(null);
  let navigationGeneration = 0;
  let removedPosition = $state<RemovedLightboxPosition | null>(null);
  let updateError = $state("");
  let overrides = $state<Record<string, boolean>>({});

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

  function confirmRemoval(assetId: string) {
    const timeline = options.getTimeline();
    const pos = timeline.locateAsset?.(assetId);
    if (pos) {
      if (activeAssetId === assetId) {
        removedPosition = { assetId, ...pos };
      } else if (removedPosition?.timeBucket === pos.timeBucket && pos.index < removedPosition.index) {
        removedPosition = {
          ...removedPosition,
          index: Math.max(0, removedPosition.index - 1),
        };
      }
    }
    timeline.removeAsset?.(assetId);
    options.onRemoved?.(assetId);
  }

  function isFavorite(asset: TimelineAsset) {
    return overrides[asset.id] ?? asset.isFavorite;
  }

  /** Records a favorite status the UI should show before or after the server confirms it. */
  function reflect(assetId: string, isFavorite: boolean) {
    if (!options.removeUnfavorited) overrides = { ...overrides, [assetId]: isFavorite };
    options.onReflect?.(assetId, isFavorite);
  }

  /** Drops overrides except for assets whose update is still in flight. */
  function retainOverrides(assetIds: string[]) {
    overrides = Object.fromEntries(
      assetIds.filter((assetId) => assetId in overrides).map((assetId) => [assetId, overrides[assetId]]),
    );
  }

  /** Closes the Lightbox tracking and clears the error; overrides are cleared with retainOverrides. */
  function reset() {
    close();
    clearError();
  }

  async function updateOne(assetId: string, isFavorite: boolean) {
    updateError = "";
    try {
      await options.setAssetFavorite(assetId, isFavorite);
      if (options.removeUnfavorited && !isFavorite) confirmRemoval(assetId);
    } catch (error) {
      if (options.removeUnfavorited && activeAssetId !== assetId) updateError = options.errorMessage;
      throw error;
    }
  }

  /** Updates each asset independently so a partial failure is reported; null means the caller's context ended mid-update. */
  async function updateMany(assetIds: string[], isFavorite: boolean, isCurrent: () => boolean) {
    if (options.removeUnfavorited) updateError = "";
    let succeeded = 0;
    for (const assetId of assetIds) {
      try {
        await options.setAssetFavorite(assetId, isFavorite);
        if (!isCurrent()) return null;
        if (options.removeUnfavorited) {
          if (!isFavorite) confirmRemoval(assetId);
        } else {
          reflect(assetId, isFavorite);
        }
        succeeded += 1;
      } catch {
        if (!isCurrent()) return null;
        // Continue so the result can accurately report a partial update.
      }
    }
    return isCurrent() ? { succeeded, total: assetIds.length } : null;
  }

  async function navigate(
    delta: number,
    current: TimelineAsset,
  ): Promise<TimelineAsset | null> {
    const navigation = ++navigationGeneration;
    const former = removedPosition?.assetId === current.id ? removedPosition : null;
    const next = await options.getTimeline().stepAsset?.(current.id, delta, former);
    if (!next || navigation !== navigationGeneration || activeAssetId !== current.id) {
      return null;
    }
    removedPosition = null;
    activeAssetId = next.id;
    return { ...next, isFavorite: isFavorite(next) };
  }

  return {
    get updateError() { return updateError; },
    open,
    close,
    clearError,
    isFavorite,
    reflect,
    retainOverrides,
    reset,
    updateOne,
    updateMany,
    navigate,
  };
}
