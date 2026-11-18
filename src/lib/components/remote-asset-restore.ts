import type { TrashRestoreResult } from "../contracts";
import {
  bulkAssetFeedbackFor,
  bulkAssetFeedbackMessage,
  bulkAssetOutcomeFromFailure,
  bulkAssetOutcomeFromResult,
  type BulkAssetFeedback,
  type BulkAssetFeedbackKeys,
  type BulkAssetOutcome,
} from "./bulk-asset-outcome";
import type { TimelineController } from "../controllers/timeline-controller.svelte";

/** What a Restore did: the shared bulk outcome, where "changed" means restored from the trash. */
export type RemoteAssetRestoreOutcome = BulkAssetOutcome;

/** What the user is told about a Restore of `total` Trashed Assets. */
export type RestoreFeedback = BulkAssetFeedback;

/** Maps a Restore outcome to its feedback. */
export const restoreFeedbackFor = bulkAssetFeedbackFor;

const RESTORE_FEEDBACK_KEYS: BulkAssetFeedbackKeys = {
  success: "trash_restore_success",
  partial: "trash_restore_partial",
  none: "trash_restore_none",
  permission: "trash_restore_permission",
  failed: "trash_restore_failed",
};

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The localized message for Restore feedback; counts are formatted for `numberLocale`. */
export function restoreFeedbackMessage(feedback: RestoreFeedback, translate: Translate, numberLocale: string): string {
  return bulkAssetFeedbackMessage(feedback, RESTORE_FEEDBACK_KEYS, translate, numberLocale);
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
      return bulkAssetOutcomeFromFailure(assetIds, error, "trash_restore");
    }
    // Anything not confirmed as restored stays visible, whatever the backend said about it.
    const outcome = bulkAssetOutcomeFromResult(assetIds, result.restoredAssetIds);
    for (const assetId of outcome.changedAssetIds) {
      options.timeline.removeAsset(assetId);
      options.onRemoved?.(assetId);
    }
    return outcome;
  }

  return { restore };
}
