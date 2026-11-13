import type { TimelineAsset } from "../contracts";
import type { TimelineAssetPosition, TimelineViewState } from "../controllers/timeline-controller.svelte";

interface RemovedLightboxPosition extends TimelineAssetPosition {
  assetId: string;
}

/** Coordinates Favorites-mode Lightbox removal tracking, former-position navigation, and post-close error reporting. */
export function createFavoritesLightboxState(getTimeline: () => TimelineViewState) {
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

  function reset() {
    close();
    updateError = "";
  }

  function clearError() {
    updateError = "";
  }

  function confirmRemoval(assetId: string) {
    const timeline = getTimeline();
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
  }

  async function updateFavorite(
    assetId: string,
    isFavorite: boolean,
    setAssetFavorite: (assetId: string, isFavorite: boolean) => Promise<void>,
    getErrorMessage: () => string,
    onConfirmedRemoval?: (assetId: string) => void,
  ) {
    updateError = "";
    try {
      await setAssetFavorite(assetId, isFavorite);
      if (!isFavorite) {
        confirmRemoval(assetId);
        onConfirmedRemoval?.(assetId);
      }
    } catch (error) {
      if (activeAssetId !== assetId) {
        updateError = getErrorMessage();
      }
      throw error;
    }
  }

  async function navigate(
    delta: number,
    current: TimelineAsset,
    resolveFavorite: (asset: TimelineAsset) => boolean,
  ): Promise<TimelineAsset | null> {
    const navigation = ++navigationGeneration;
    const timeline = getTimeline();
    const former = removedPosition?.assetId === current.id ? removedPosition : null;
    const next = await timeline.stepAsset?.(current.id, delta, former);
    if (!next || navigation !== navigationGeneration || activeAssetId !== current.id) {
      return null;
    }
    removedPosition = null;
    activeAssetId = next.id;
    return { ...next, isFavorite: resolveFavorite(next) };
  }

  return {
    get updateError() { return updateError; },
    open,
    close,
    reset,
    clearError,
    confirmRemoval,
    updateFavorite,
    navigate,
  };
}

