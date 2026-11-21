import type { AlbumAssignmentResult } from "../contracts";
import type { RemoteAssetDownloadOutcome } from "./remote-asset-actions.svelte";
import { archiveFeedbackFor, type ArchiveAction, type ArchiveFeedback, type RemoteAssetArchiveOutcome } from "./remote-asset-archive";
import { restoreFeedbackFor, type RemoteAssetRestoreOutcome, type RestoreFeedback } from "./remote-asset-restore";
import { trashFeedbackFor, type RemoteAssetTrashOutcome, type TrashFeedback } from "./remote-asset-trash";

/** The bulk actions of a timeline selection; at most one runs at a time. */
export type SelectionAction = "favorite" | "album" | "download" | "restore" | "trash" | "archive" | "unarchive";

/** How many of the selected Remote Assets a bulk action handled. */
export interface SelectionCounts {
  succeeded: number;
  total: number;
}

/** What a finished favorite, add-to-Album or Download bulk action reports (Restore, Move to Trash, Archive and Unarchive report through their own feedback). */
export interface SelectionResult extends SelectionCounts {
  action: Exclude<SelectionAction, "restore" | "trash" | "archive" | "unarchive">;
}

/** What the user is told about the last Archive or Unarchive of the selection. */
export interface SelectionArchiveFeedback {
  action: ArchiveAction;
  feedback: ArchiveFeedback;
}

export interface TimelineSelectionActionsOptions {
  /** Updates each asset independently; null means `isCurrent` turned false mid-update (see `RemoteAssetFavorites.updateMany`). */
  readonly updateFavorites: (
    assetIds: string[],
    isFavorite: boolean,
    isCurrent: () => boolean,
  ) => Promise<SelectionCounts | null>;
  readonly clearFavoriteError: () => void;
  /** Rejects on failure; anything but "saved" (cancelled, stale) is not reported. */
  readonly download: (assetIds: string[], isCurrent: () => boolean) => Promise<RemoteAssetDownloadOutcome>;
  readonly addAssetsToAlbum: (assetIds: string[], albumId: string) => Promise<AlbumAssignmentResult>;
  /** Restore of Trashed Assets; leave it out where the timeline offers no Restore. Never rejects. */
  readonly restore?: (assetIds: string[]) => Promise<RemoteAssetRestoreOutcome>;
  /** Move to Trash (the caller has asked for confirmation); leave it out where the timeline offers none. Never rejects. */
  readonly trash?: (assetIds: string[]) => Promise<RemoteAssetTrashOutcome>;
  /** Archive of Remote Assets; leave it out where the timeline offers no Archive. Never rejects. */
  readonly archive?: (assetIds: string[]) => Promise<RemoteAssetArchiveOutcome>;
  /** Unarchive of Archived Assets; leave it out where the timeline offers no Unarchive. Never rejects. */
  readonly unarchive?: (assetIds: string[]) => Promise<RemoteAssetArchiveOutcome>;
}

/**
 * Owns the multi-selection of a timeline and its bulk actions (favorite, add to Album, Download, Restore, Move to Trash, Archive, Unarchive):
 * the selected set, which single action is running, and the last result. Call `reset` when the timeline
 * refreshed or was replaced: it ends the running action and drops its result when it arrives.
 */
export function createTimelineSelectionActions(options: TimelineSelectionActionsOptions) {
  let selectedIds = $state<Set<string>>(new Set());
  let busyAction = $state<SelectionAction | null>(null);
  let result = $state<SelectionResult | null>(null);
  let restoreFeedback = $state<RestoreFeedback | null>(null);
  let trashFeedback = $state<TrashFeedback | null>(null);
  let archiveFeedback = $state<SelectionArchiveFeedback | null>(null);
  let generation = 0;

  function select(assetIds: Iterable<string>) {
    selectedIds = new Set(assetIds);
  }

  function toggle(assetId: string) {
    result = null;
    const next = new Set(selectedIds);
    if (next.has(assetId)) next.delete(assetId);
    else next.add(assetId);
    selectedIds = next;
  }

  function clear() {
    select([]);
  }

  /** Selecting is over: drops the selection and reports what the action did. */
  function finish(action: SelectionResult["action"], counts: SelectionCounts) {
    clear();
    result = { action, ...counts };
  }

  /** Call when a Remote Asset left the timeline. */
  function forget(assetId: string) {
    if (!selectedIds.has(assetId)) return;
    const next = new Set(selectedIds);
    next.delete(assetId);
    selectedIds = next;
  }

  function clearResult() {
    result = null;
  }

  /** Ends any running action (its result will be dropped), and clears the selection and the results. */
  function reset() {
    generation += 1;
    busyAction = null;
    clearResult();
    restoreFeedback = null;
    trashFeedback = null;
    archiveFeedback = null;
    clear();
  }

  /** Tells whether the timeline was neither refreshed nor replaced (`reset`) since this call. */
  function currentGeneration() {
    const started = generation;
    return () => started === generation;
  }

  /** Starts `action` on the selection unless another one runs; the callback tells whether its result still applies. */
  async function run(action: SelectionAction, perform: (assetIds: string[], isCurrent: () => boolean) => Promise<void>) {
    const assetIds = [...selectedIds];
    if (assetIds.length === 0 || busyAction !== null) return false;
    const isCurrent = currentGeneration();
    clearResult();
    busyAction = action;
    try {
      await perform(assetIds, isCurrent);
    } finally {
      if (isCurrent()) busyAction = null;
    }
    return isCurrent();
  }

  async function setFavorite(isFavorite: boolean) {
    await run("favorite", async (assetIds, isCurrent) => {
      options.clearFavoriteError();
      const outcome = await options.updateFavorites(assetIds, isFavorite, isCurrent);
      if (!outcome || !isCurrent()) return;
      finish("favorite", outcome);
    });
  }

  /** Adds the selection to an Album; true when the result was applied, false when it was dropped or nothing ran. */
  async function addToAlbum(albumId: string) {
    return run("album", async (assetIds, isCurrent) => {
      let succeeded = 0;
      try {
        succeeded = (await options.addAssetsToAlbum(assetIds, albumId)).succeeded;
      } catch { /* Reported as zero succeeded below. */ }
      if (!isCurrent()) return;
      finish("album", { succeeded, total: assetIds.length });
    });
  }

  async function download() {
    await run("download", async (assetIds, isCurrent) => {
      try {
        const outcome = await options.download(assetIds, isCurrent);
        if (outcome !== "saved" || !isCurrent()) return;
        result = { action: "download", succeeded: assetIds.length, total: assetIds.length };
      } catch {
        if (!isCurrent()) return;
        result = { action: "download", succeeded: 0, total: assetIds.length };
      }
    });
  }

  /** Runs a bulk write that shrinks the selection to the failed assets and reports its outcome. */
  async function runBulkWrite<TOutcome extends { failedAssetIds: string[] }, TFeedback>(
    action: "restore" | "trash" | ArchiveAction,
    write: ((assetIds: string[]) => Promise<TOutcome>) | undefined,
    setFeedback: (value: TFeedback | null) => void,
    toFeedback: (outcome: TOutcome, total: number) => TFeedback,
  ) {
    if (!write) return;
    await run(action, async (assetIds, isCurrent) => {
      setFeedback(null);
      const outcome = await write(assetIds);
      if (!isCurrent()) return;
      select(outcome.failedAssetIds);
      setFeedback(toFeedback(outcome, assetIds.length));
    });
  }

  /** Runs a single-asset write (from the Lightbox) outside the selection busy state and tells whether its result still applies. */
  async function runSingleWrite<TOutcome extends { changedAssetIds: string[] }, TFeedback>(
    label: string,
    assetId: string,
    write: ((assetIds: string[]) => Promise<TOutcome>) | undefined,
    setFeedback: (value: TFeedback | null) => void,
    toFeedback: ((outcome: TOutcome) => TFeedback) | null,
  ) {
    if (!write) throw new Error(`This timeline offers no ${label}.`);
    const isCurrent = currentGeneration();
    if (toFeedback) setFeedback(null);
    const outcome = await write([assetId]);
    const applied = isCurrent() && outcome.changedAssetIds.includes(assetId);
    if (applied && toFeedback) setFeedback(toFeedback(outcome));
    return { outcome, applied };
  }

  /** Restores the selection; Trashed Assets that were not restored stay selected so the user can retry. */
  async function restore() {
    await runBulkWrite("restore", options.restore, (value) => { restoreFeedback = value; }, restoreFeedbackFor);
  }

  /** Archives or unarchives the selection; Remote Assets that did not change stay selected so the user can retry. */
  async function changeSelectionArchive(action: ArchiveAction, change: ((assetIds: string[]) => Promise<RemoteAssetArchiveOutcome>) | undefined) {
    await runBulkWrite(
      action,
      change,
      (value: SelectionArchiveFeedback | null) => { archiveFeedback = value; },
      (outcome, total) => ({ action, feedback: archiveFeedbackFor(outcome, total) }),
    );
  }

  const archive = () => changeSelectionArchive("archive", options.archive);
  const unarchive = () => changeSelectionArchive("unarchive", options.unarchive);

  /** Restores one Remote Asset (from the Lightbox). `applied` is false when the timeline changed meanwhile. */
  async function restoreOne(assetId: string) {
    return runSingleWrite(
      "Restore",
      assetId,
      options.restore,
      (value: RestoreFeedback | null) => { restoreFeedback = value; },
      (outcome) => restoreFeedbackFor(outcome, 1),
    );
  }

  /** Archives or unarchives one Remote Asset (from the Lightbox). The Lightbox reports its own single-asset message while open. */
  async function archiveOne(action: ArchiveAction, assetId: string) {
    const { outcome } = await runSingleWrite(
      action === "archive" ? "Archive" : "Unarchive",
      assetId,
      options[action],
      (value: SelectionArchiveFeedback | null) => { archiveFeedback = value; },
      null,
    );
    return outcome;
  }

  /** Moves the selection to the Immich trash; Remote Assets that were not moved stay selected so the user can retry. */
  async function trash() {
    await runBulkWrite("trash", options.trash, (value) => { trashFeedback = value; }, trashFeedbackFor);
  }

  /**
   * Moves one Remote Asset (from the Lightbox, which reports a failure itself). `applied` is false when the
   * asset was not moved or the timeline changed meanwhile; only an applied success is reported as `trashFeedback`.
   */
  async function trashOne(assetId: string) {
    const trashAssets = options.trash;
    if (!trashAssets) throw new Error("This timeline offers no Move to Trash.");
    const isCurrent = currentGeneration();
    trashFeedback = null;
    const outcome = await trashAssets([assetId]);
    const applied = isCurrent() && outcome.movedAssetIds.includes(assetId);
    if (applied) trashFeedback = trashFeedbackFor(outcome, 1);
    return { outcome, applied };
  }

  return {
    get selectedIds() { return selectedIds; },
    get count() { return selectedIds.size; },
    get isSelectionMode() { return selectedIds.size > 0; },
    get busyAction() { return busyAction; },
    get isBusy() { return busyAction !== null; },
    get result() { return result; },
    get restoreFeedback() { return restoreFeedback; },
    get trashFeedback() { return trashFeedback; },
    get archiveFeedback() { return archiveFeedback; },
    clearResult,
    clearTrashFeedback: () => { trashFeedback = null; },
    isSelected: (assetId: string) => selectedIds.has(assetId),
    toggle,
    clear,
    forget,
    reset,
    setFavorite,
    addToAlbum,
    download,
    restore,
    restoreOne,
    trash,
    trashOne,
    archive,
    unarchive,
    archiveOne,
  };
}
