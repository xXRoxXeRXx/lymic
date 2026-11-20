import type { TrashMoveResult } from "../contracts";
import { classifyRequestFailure } from "../controllers/request-failure";
import type { TimelineController } from "../controllers/timeline-controller.svelte";

/** What a Move to Trash did. `failure` is set only when the whole request failed. */
export interface RemoteAssetTrashOutcome {
  movedAssetIds: string[];
  /** Remote Assets that are still in the library; every requested asset when the request failed. */
  failedAssetIds: string[];
  /** `permission` when Immich refused the Move to Trash (HTTP 403), `error` for any other failed request. */
  failure: "permission" | "error" | null;
}

/** What the user is told about a Move to Trash. */
export type TrashFeedback =
  | { kind: "success"; count: number }
  | { kind: "failed" }
  | { kind: "permission" };

/** Maps a Move to Trash outcome to its feedback, the one place that decides success, permission or failed. */
export function trashFeedbackFor(outcome: RemoteAssetTrashOutcome): TrashFeedback {
  if (outcome.failure === "permission") return { kind: "permission" };
  if (outcome.failure === "error" || outcome.failedAssetIds.length > 0) return { kind: "failed" };
  return { kind: "success", count: outcome.movedAssetIds.length };
}

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The localized message for Move to Trash feedback; counts are formatted for `numberLocale`. */
export function trashFeedbackMessage(feedback: TrashFeedback, translate: Translate, numberLocale: string): string {
  switch (feedback.kind) {
    case "success": return translate("trash_move_success", { values: { count: feedback.count.toLocaleString(numberLocale) } });
    case "permission": return translate("trash_move_permission");
    case "failed": return translate("trash_move_failed");
  }
}

export interface RemoteAssetTrashOptions {
  readonly timeline: TimelineController;
  readonly trashAssets: (assetIds: string[]) => Promise<TrashMoveResult>;
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
    let result: TrashMoveResult;
    try {
      result = await options.trashAssets(assetIds);
    } catch (error) {
      // The backend reports a refusal as a credential-free code, so the error text is never shown.
      const refused = classifyRequestFailure(error, "trash_move") === "permission_denied";
      return { movedAssetIds: [], failedAssetIds: [...assetIds], failure: refused ? "permission" : "error" };
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
    return { movedAssetIds, failedAssetIds, failure: null };
  }

  return { trash };
}
