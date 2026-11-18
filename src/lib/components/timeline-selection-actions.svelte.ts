import type { AlbumAssignmentResult } from "../contracts";
import type { RemoteAssetDownloadOutcome } from "./remote-asset-actions.svelte";
import { restoreFeedbackFor, type RemoteAssetRestoreOutcome, type RestoreFeedback } from "./remote-asset-restore";

/** The bulk actions of a timeline selection; at most one runs at a time. */
export type SelectionAction = "favorite" | "album" | "download" | "restore";

/** What a finished favorite, add-to-Album or Download bulk action reports: `succeeded` of `total` selected Remote Assets. */
export interface SelectionResult {
  action: "favorite" | "album" | "download";
  succeeded: number;
  total: number;
}

export interface TimelineSelectionActionsOptions {
  /** Updates each asset independently; null means `isCurrent` turned false mid-update (see `RemoteAssetFavorites.updateMany`). */
  readonly updateFavorites: (
    assetIds: string[],
    isFavorite: boolean,
    isCurrent: () => boolean,
  ) => Promise<{ succeeded: number; total: number } | null>;
  readonly clearFavoriteError: () => void;
  /** Rejects on failure; anything but "saved" (cancelled, stale) is not reported. */
  readonly download: (assetIds: string[], isCurrent: () => boolean) => Promise<RemoteAssetDownloadOutcome>;
  readonly addAssetsToAlbum: (assetIds: string[], albumId: string) => Promise<AlbumAssignmentResult>;
  /** Restore of Trashed Assets; leave it out where the timeline offers no Restore. Never rejects. */
  readonly restore?: (assetIds: string[]) => Promise<RemoteAssetRestoreOutcome>;
}

/**
 * Owns the multi-selection of a timeline and its bulk actions (favorite, add to Album, Download, Restore):
 * the selected set, which single action is running, and the last result. Call `reset` when the timeline
 * refreshed or was replaced: it ends the running action and drops its result when it arrives.
 */
export function createTimelineSelectionActions(options: TimelineSelectionActionsOptions) {
  let selectedIds = $state<Set<string>>(new Set());
  let busyAction = $state<SelectionAction | null>(null);
  let result = $state<SelectionResult | null>(null);
  let restoreFeedback = $state<RestoreFeedback | null>(null);
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
    clear();
  }

  /** Starts `action` on the selection unless another one runs; the callback tells whether its result still applies. */
  async function run(action: SelectionAction, perform: (assetIds: string[], isCurrent: () => boolean) => Promise<void>) {
    const assetIds = [...selectedIds];
    if (assetIds.length === 0 || busyAction !== null) return false;
    const started = generation;
    const isCurrent = () => started === generation;
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
      clear();
      result = { action: "favorite", ...outcome };
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
      clear();
      result = { action: "album", succeeded, total: assetIds.length };
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

  /** Restores the selection; Trashed Assets that were not restored stay selected so the user can retry. */
  async function restore() {
    const restoreAssets = options.restore;
    if (!restoreAssets) return;
    await run("restore", async (assetIds, isCurrent) => {
      restoreFeedback = null;
      const outcome = await restoreAssets(assetIds);
      if (!isCurrent()) return;
      select(outcome.failedAssetIds);
      restoreFeedback = restoreFeedbackFor(outcome, assetIds.length);
    });
  }

  /** Restores one Remote Asset (from the Lightbox). `applied` is false when the timeline changed meanwhile. */
  async function restoreOne(assetId: string) {
    const started = generation;
    restoreFeedback = null;
    const outcome = await options.restore!([assetId]);
    const applied = started === generation && outcome.restoredAssetIds.includes(assetId);
    if (applied) restoreFeedback = restoreFeedbackFor(outcome, 1);
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
    clearResult,
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
  };
}
