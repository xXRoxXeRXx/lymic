import type { TrashRestoreResult } from "../contracts";
import { classifyExploreError } from "../controllers/explore-error";
import type { TimelineController } from "../controllers/timeline-controller.svelte";

/** What a Restore did. `failure` is set only when the whole request failed; a partial result leaves it null. */
export interface RemoteAssetRestoreOutcome {
  restoredAssetIds: string[];
  /** Trashed Assets that are still in the Immich trash; every requested asset when the request failed. */
  failedAssetIds: string[];
  /** `permission` when Immich refused the Restore (HTTP 403), `error` for any other failed request. */
  failure: "permission" | "error" | null;
}

export interface RemoteAssetRestoreOptions {
  readonly timeline: TimelineController;
  readonly restoreTrashedAssets: (assetIds: string[]) => Promise<TrashRestoreResult>;
  /** Called after a restored Remote Asset left the Trash Timeline. */
  onRemoved?: (assetId: string) => void;
}

/**
 * Shares Restore of Trashed Assets between the Lightbox and timeline selections: one request for the
 * given Remote Assets, then the restored ones leave the Trash Timeline at once and the rest stay.
 * Never rejects, so a failed Restore cannot take the view down.
 */
export function createRemoteAssetRestore(options: RemoteAssetRestoreOptions) {
  async function restore(assetIds: string[]): Promise<RemoteAssetRestoreOutcome> {
    let result: TrashRestoreResult;
    try {
      result = await options.restoreTrashedAssets(assetIds);
    } catch (error) {
      // The backend reports a refusal as a credential-free code, so the error text is never shown.
      const refused = classifyExploreError(error, "trash_restore") === "permission_denied";
      return { restoredAssetIds: [], failedAssetIds: [...assetIds], failure: refused ? "permission" : "error" };
    }
    const requested = new Set(assetIds);
    const restoredAssetIds = result.restoredAssetIds.filter((assetId) => requested.has(assetId));
    const restored = new Set(restoredAssetIds);
    // Anything not confirmed as restored stays visible, whatever the backend said about it.
    const failedAssetIds = assetIds.filter((assetId) => !restored.has(assetId));
    for (const assetId of restoredAssetIds) {
      options.timeline.removeAsset(assetId);
      options.onRemoved?.(assetId);
    }
    return { restoredAssetIds, failedAssetIds, failure: null };
  }

  return { restore };
}

export type RemoteAssetRestore = ReturnType<typeof createRemoteAssetRestore>;
