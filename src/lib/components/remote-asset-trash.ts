import type { TrashMoveResult } from "../contracts";
import { classifyRequestFailure } from "../controllers/request-failure";
import type { TimelineController } from "../controllers/timeline-controller.svelte";

/** What a Move to Trash did. `failure` is set when the request failed or stopped before it was done. */
export interface RemoteAssetTrashOutcome {
  movedAssetIds: string[];
  /** Remote Assets that are still in the library; every requested asset when the request failed before any was moved. */
  failedAssetIds: string[];
  /**
   * `permission` when Immich refused the Move to Trash (HTTP 403), `error` for any other failed request. It is also
   * set when the request stopped after some assets were already moved: `movedAssetIds` then lists them.
   */
  failure: "permission" | "error" | null;
}

/** What the user is told about a Move to Trash. */
export type TrashFeedback =
  | { kind: "success"; count: number }
  | { kind: "partial"; failed: number; total: number }
  | { kind: "none"; failed: number; total: number }
  | { kind: "failed" }
  | { kind: "permission" };

/**
 * Maps a Move to Trash outcome for `total` requested Remote Assets to its feedback, the one place that
 * decides success, partial, none, permission or failed. A request that failed or stopped (`failure`) is always
 * `permission` or `failed`, also when some assets were moved before it stopped: why it stopped matters more
 * than the counts, and the moved assets have already left the view. A single asset that was not moved is `failed`,
 * since "1 of 1" says nothing more.
 */
export function trashFeedbackFor(outcome: RemoteAssetTrashOutcome, total: number): TrashFeedback {
  if (outcome.failure === "permission") return { kind: "permission" };
  if (outcome.failure === "error") return { kind: "failed" };
  if (outcome.failedAssetIds.length === 0) return { kind: "success", count: outcome.movedAssetIds.length };
  if (outcome.movedAssetIds.length === 0) {
    return total === 1 ? { kind: "failed" } : { kind: "none", failed: outcome.failedAssetIds.length, total };
  }
  return { kind: "partial", failed: outcome.failedAssetIds.length, total };
}

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The localized message for Move to Trash feedback; counts are formatted for `numberLocale`. */
export function trashFeedbackMessage(feedback: TrashFeedback, translate: Translate, numberLocale: string): string {
  const count = (value: number) => value.toLocaleString(numberLocale);
  switch (feedback.kind) {
    case "success": return translate("trash_move_success", { values: { count: count(feedback.count) } });
    case "partial": return translate("trash_move_partial", { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "none": return translate("trash_move_none", { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "permission": return translate("trash_move_permission");
    case "failed": return translate("trash_move_failed");
  }
}

function failureOf(reason: unknown): "permission" | "error" {
  return classifyRequestFailure(reason, "trash_move") === "permission_denied" ? "permission" : "error";
}

export interface RemoteAssetTrashOptions {
  /** Whatever shows the assets: a timeline, or the Map View's markers. */
  readonly timeline: Pick<TimelineController, "removeAsset">;
  /**
   * Absent where the view offers no Move to Trash: `trash` then fails every asset without a request.
   * Rejects with a credential-free code when nothing was moved; resolves with `failureCode` when it stopped midway.
   */
  readonly trashAssets?: (assetIds: string[]) => Promise<TrashMoveResult>;
  /** Called after a moved Remote Asset left the timeline. */
  onRemoved?: (assetId: string) => void;
}

/**
 * Shares Move to Trash between the Lightbox and timeline selections: one request for the given Remote
 * Assets, then the ones Immich confirmed as moved leave the timeline at once and the rest stay.
 * Never rejects, so a failed Move to Trash cannot take the view down. Asking the user to confirm
 * is the caller's job.
 */
export function createRemoteAssetTrash(options: RemoteAssetTrashOptions) {
  async function trash(assetIds: string[]): Promise<RemoteAssetTrashOutcome> {
    const trashAssets = options.trashAssets;
    if (!trashAssets) return { movedAssetIds: [], failedAssetIds: [...assetIds], failure: "error" };
    let result: TrashMoveResult;
    try {
      result = await trashAssets(assetIds);
    } catch (error) {
      // The backend reports a refusal as a credential-free code, so the error text is never shown.
      return { movedAssetIds: [], failedAssetIds: [...assetIds], failure: failureOf(error) };
    }
    const requested = new Set(assetIds);
    const movedAssetIds = result.movedAssetIds.filter((assetId) => requested.has(assetId));
    const moved = new Set(movedAssetIds);
    // Anything not confirmed as moved stays visible, whatever the backend said about it.
    const failedAssetIds = assetIds.filter((assetId) => !moved.has(assetId));
    for (const assetId of movedAssetIds) {
      options.timeline.removeAsset(assetId);
      options.onRemoved?.(assetId);
    }
    // A request that stopped after some assets were moved still reports why; those assets left the view above.
    return { movedAssetIds, failedAssetIds, failure: result.failureCode ? failureOf(result.failureCode) : null };
  }

  return { trash };
}
