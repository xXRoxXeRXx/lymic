import type { AssetVisibility, AssetVisibilityResult } from "../contracts";
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

/** What Lymic changes about a Remote Asset's archive status. */
export type ArchiveAction = "archive" | "unarchive";

/** What an Archive or Unarchive did: the shared bulk outcome, where "changed" means the visibility was written. */
export type RemoteAssetArchiveOutcome = BulkAssetOutcome;

/** What the user is told about an Archive or Unarchive of `total` Remote Assets. */
export type ArchiveFeedback = BulkAssetFeedback;

/** Maps an Archive or Unarchive outcome to its feedback. */
export const archiveFeedbackFor = bulkAssetFeedbackFor;

/** The only visibility each action writes: Lymic never sends `locked` or `hidden`. */
const VISIBILITY: Record<ArchiveAction, AssetVisibility> = {
  archive: "archive",
  unarchive: "timeline",
};

const FEEDBACK_KEYS: Record<ArchiveAction, BulkAssetFeedbackKeys> = {
  archive: {
    success: "archive_archive_success",
    partial: "archive_archive_partial",
    none: "archive_archive_none",
    permission: "archive_archive_permission",
    failed: "archive_archive_failed",
  },
  unarchive: {
    success: "archive_unarchive_success",
    partial: "archive_unarchive_partial",
    none: "archive_unarchive_none",
    permission: "archive_unarchive_permission",
    failed: "archive_unarchive_failed",
  },
};

type Translate = (key: string, options?: { values: Record<string, string | number> }) => string;

/** The localized message for the feedback of `action`; counts are formatted for `numberLocale`. */
export function archiveFeedbackMessage(action: ArchiveAction, feedback: ArchiveFeedback, translate: Translate, numberLocale: string): string {
  return bulkAssetFeedbackMessage(feedback, FEEDBACK_KEYS[action], translate, numberLocale);
}

export interface RemoteAssetArchiveOptions {
  readonly timeline: TimelineController;
  readonly setAssetsVisibility: (assetIds: string[], visibility: AssetVisibility) => Promise<AssetVisibilityResult>;
  /** Called after a changed Remote Asset left the view. */
  onRemoved?: (assetId: string) => void;
  /**
   * For a view that shows archived and other assets together (Album, Person, Place, Memory, Tag, Search):
   * an archived Remote Asset stays in the list, so nothing is removed after an Archive.
   */
  assetsStayInView?: boolean;
  /** Whether an Archive may write this Remote Asset; one that may not (a `hidden` one) is left out of the request and counts as failed. */
  isArchivable?: (assetId: string) => boolean;
}

/**
 * Shares Archive and Unarchive between the Lightbox and timeline selections: one request for the given
 * Remote Assets, then the changed ones leave the view at once and the rest stay. Never rejects, so a
 * failed request cannot take the view down.
 */
export function createRemoteAssetArchive(options: RemoteAssetArchiveOptions) {
  async function apply(action: ArchiveAction, assetIds: string[]): Promise<RemoteAssetArchiveOutcome> {
    const writable = action === "archive" && options.isArchivable ? assetIds.filter(options.isArchivable) : assetIds;
    const skipped = new Set(assetIds.filter((assetId) => !writable.includes(assetId)));
    if (writable.length === 0) return { changedAssetIds: [], failedAssetIds: [...assetIds], failure: null };
    let result: AssetVisibilityResult;
    try {
      result = await options.setAssetsVisibility(writable, VISIBILITY[action]);
    } catch (error) {
      return bulkAssetOutcomeFromFailure(assetIds, error, "archive");
    }
    // Anything not confirmed as changed stays visible, whatever the backend said about it.
    const written = bulkAssetOutcomeFromResult(writable, result.changedAssetIds);
    const outcome: RemoteAssetArchiveOutcome = {
      ...written,
      failedAssetIds: assetIds.filter((assetId) => skipped.has(assetId) || written.failedAssetIds.includes(assetId)),
    };
    if (options.assetsStayInView) return outcome;
    for (const assetId of outcome.changedAssetIds) {
      options.timeline.removeAsset(assetId);
      options.onRemoved?.(assetId);
    }
    return outcome;
  }

  return { apply };
}
