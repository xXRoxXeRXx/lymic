import type { TrashRestoreResult } from "../contracts";
import { classifyRequestFailure } from "../controllers/request-failure";
import type { TimelineController } from "../controllers/timeline-controller.svelte";

/** What a Restore did. `failure` is set only when the whole request failed; a partial result leaves it null. */
export interface RemoteAssetRestoreOutcome {
  restoredAssetIds: string[];
  /** Trashed Assets that are still in the Immich trash; every requested asset when the request failed. */
  failedAssetIds: string[];
  /** `permission` when Immich refused the Restore (HTTP 403), `error` for any other failed request. */
  failure: "permission" | "error" | null;
}

/** What the user is told about a Restore of `total` Trashed Assets. */
export type RestoreFeedback =
  | { kind: "success"; count: number }
  | { kind: "partial"; failed: number; total: number }
  | { kind: "none"; failed: number; total: number }
  | { kind: "failed" }
  | { kind: "permission" };

/** Maps a Restore outcome to its feedback, the one place that decides success, partial, none, permission or failed. */
export function restoreFeedbackFor(outcome: RemoteAssetRestoreOutcome, total: number): RestoreFeedback {
  if (outcome.failure === "permission") return { kind: "permission" };
  if (outcome.failure === "error") return { kind: "failed" };
  if (outcome.failedAssetIds.length === 0) return { kind: "success", count: outcome.restoredAssetIds.length };
  if (outcome.restoredAssetIds.length === 0) return { kind: "none", failed: outcome.failedAssetIds.length, total };
  return { kind: "partial", failed: outcome.failedAssetIds.length, total };
}

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The localized message for Restore feedback; counts are formatted for `numberLocale`. */
export function restoreFeedbackMessage(feedback: RestoreFeedback, translate: Translate, numberLocale: string): string {
  const count = (value: number) => value.toLocaleString(numberLocale);
  switch (feedback.kind) {
    case "success": return translate("trash_restore_success", { values: { count: count(feedback.count) } });
    case "partial": return translate("trash_restore_partial", { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "none": return translate("trash_restore_none", { values: { failed: count(feedback.failed), count: count(feedback.total) } });
    case "permission": return translate("trash_restore_permission");
    case "failed": return translate("trash_restore_failed");
  }
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
      const refused = classifyRequestFailure(error, "trash_restore") === "permission_denied";
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
