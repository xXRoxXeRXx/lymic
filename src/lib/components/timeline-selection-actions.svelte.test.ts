import { describe, expect, it, vi } from "vitest";
import { createTimelineSelectionActions, type TimelineSelectionActionsOptions } from "./timeline-selection-actions.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function createSelection(overrides: Partial<TimelineSelectionActionsOptions> = {}) {
  return createTimelineSelectionActions({
    updateFavorites: async (assetIds) => ({ succeeded: assetIds.length, total: assetIds.length }),
    clearFavoriteError: () => {},
    download: async () => "saved",
    addAssetsToAlbum: async (assetIds) => ({ succeeded: assetIds.length, failed: 0 }),
    ...overrides,
  });
}

describe("timeline selection", () => {
  it("toggles assets in and out of the selection", () => {
    const selection = createSelection();

    expect(selection.isSelectionMode).toBe(false);
    selection.toggle("a");
    selection.toggle("b");
    expect(selection.count).toBe(2);
    expect(selection.isSelected("a")).toBe(true);
    expect(selection.isSelectionMode).toBe(true);

    selection.toggle("a");
    expect(selection.isSelected("a")).toBe(false);
    expect(selection.count).toBe(1);

    selection.clear();
    expect(selection.isSelectionMode).toBe(false);
  });

  it("drops an asset removed from the timeline from the selection", () => {
    const selection = createSelection();
    selection.toggle("a");
    selection.toggle("b");

    selection.forget("a");
    selection.forget("not-selected");

    expect(selection.isSelected("a")).toBe(false);
    expect([...selection.selectedIds]).toEqual(["b"]);
  });

  it("clears the previous result when the selection changes", async () => {
    const selection = createSelection();
    selection.toggle("a");
    await selection.setFavorite(true);
    expect(selection.result).not.toBeNull();

    selection.toggle("b");
    expect(selection.result).toBeNull();
  });
});

describe("selection favorite", () => {
  it("reports succeeded and total counts, then clears the selection", async () => {
    const updateFavorites = vi.fn(async (assetIds: string[]) => ({ succeeded: assetIds.length - 1, total: assetIds.length }));
    const clearFavoriteError = vi.fn();
    const selection = createSelection({ updateFavorites, clearFavoriteError });
    selection.toggle("a");
    selection.toggle("b");
    selection.toggle("c");

    await selection.setFavorite(true);

    expect(updateFavorites).toHaveBeenCalledWith(["a", "b", "c"], true, expect.any(Function));
    expect(clearFavoriteError).toHaveBeenCalled();
    expect(selection.result).toEqual({ action: "favorite", succeeded: 2, total: 3 });
    expect(selection.isSelectionMode).toBe(false);
    expect(selection.busyAction).toBeNull();
  });

  it("does nothing without a selection", async () => {
    const updateFavorites = vi.fn();
    const selection = createSelection({ updateFavorites });

    await selection.setFavorite(true);

    expect(updateFavorites).not.toHaveBeenCalled();
    expect(selection.result).toBeNull();
  });
});

describe("selection add to Album", () => {
  it("reports the succeeded count of a partial assignment out of the selection total", async () => {
    const addAssetsToAlbum = vi.fn(async () => ({ succeeded: 1, failed: 1 }));
    const selection = createSelection({ addAssetsToAlbum });
    selection.toggle("a");
    selection.toggle("b");

    const applied = await selection.addToAlbum("album-1");

    expect(applied).toBe(true);
    expect(addAssetsToAlbum).toHaveBeenCalledWith(["a", "b"], "album-1");
    expect(selection.result).toEqual({ action: "album", succeeded: 1, total: 2 });
    expect(selection.isSelectionMode).toBe(false);
  });

  it("reports a rejected assignment as zero succeeded", async () => {
    const selection = createSelection({ addAssetsToAlbum: async () => { throw new Error("offline"); } });
    selection.toggle("a");

    await selection.addToAlbum("album-1");

    expect(selection.result).toEqual({ action: "album", succeeded: 0, total: 1 });
    expect(selection.busyAction).toBeNull();
  });
});

describe("selection download", () => {
  it("reports every selected asset as saved", async () => {
    const download = vi.fn(async () => "saved" as const);
    const selection = createSelection({ download });
    selection.toggle("a");
    selection.toggle("b");

    await selection.download();

    expect(download).toHaveBeenCalledWith(["a", "b"], expect.any(Function));
    expect(selection.result).toEqual({ action: "download", succeeded: 2, total: 2 });
    expect(selection.busyAction).toBeNull();
  });

  it("keeps the selection and reports nothing when the user cancels", async () => {
    const selection = createSelection({ download: async () => "cancelled" });
    selection.toggle("a");

    await selection.download();

    expect(selection.result).toBeNull();
    expect(selection.isSelected("a")).toBe(true);
    expect(selection.busyAction).toBeNull();
  });

  it("reports a failed download as zero succeeded and keeps the selection", async () => {
    const selection = createSelection({ download: async () => { throw new Error("disk full"); } });
    selection.toggle("a");
    selection.toggle("b");

    await selection.download();

    expect(selection.result).toEqual({ action: "download", succeeded: 0, total: 2 });
    expect(selection.isSelected("a")).toBe(true);
    expect(selection.busyAction).toBeNull();
  });
});

describe("one bulk action at a time", () => {
  it("ignores a second action while one runs", async () => {
    const pending = deferred<{ succeeded: number; failed: number }>();
    const addAssetsToAlbum = vi.fn(() => pending.promise);
    const download = vi.fn(async () => "saved" as const);
    const updateFavorites = vi.fn(async () => ({ succeeded: 1, total: 1 }));
    const selection = createSelection({ addAssetsToAlbum, download, updateFavorites });
    selection.toggle("a");

    const running = selection.addToAlbum("album-1");
    expect(selection.busyAction).toBe("album");
    expect(selection.isBusy).toBe(true);

    await selection.download();
    await selection.setFavorite(true);
    expect(await selection.addToAlbum("album-2")).toBe(false);
    expect(download).not.toHaveBeenCalled();
    expect(updateFavorites).not.toHaveBeenCalled();
    expect(addAssetsToAlbum).toHaveBeenCalledTimes(1);

    pending.resolve({ succeeded: 1, failed: 0 });
    await running;
    expect(selection.busyAction).toBeNull();
  });
});

describe("a result arriving after the timeline changed", () => {
  it("is dropped by reset for a favorite update and leaves no busy state", async () => {
    const pending = deferred<{ succeeded: number; total: number } | null>();
    let isCurrent: () => boolean = () => true;
    const selection = createSelection({
      updateFavorites: (_assetIds, _isFavorite, current) => { isCurrent = current; return pending.promise; },
    });
    selection.toggle("a");

    const running = selection.setFavorite(true);
    expect(selection.busyAction).toBe("favorite");
    selection.reset();
    expect(isCurrent()).toBe(false);
    expect(selection.busyAction).toBeNull();

    pending.resolve({ succeeded: 1, total: 1 });
    await running;

    expect(selection.result).toBeNull();
    expect(selection.busyAction).toBeNull();
  });

  it("is dropped by reset for an Album assignment", async () => {
    const pending = deferred<{ succeeded: number; failed: number }>();
    const selection = createSelection({ addAssetsToAlbum: () => pending.promise });
    selection.toggle("a");

    const running = selection.addToAlbum("album-1");
    selection.reset();
    pending.resolve({ succeeded: 1, failed: 0 });

    expect(await running).toBe(false);
    expect(selection.result).toBeNull();
    expect(selection.busyAction).toBeNull();
  });

  it("is dropped by reset for a rejected Album assignment", async () => {
    const pending = deferred<{ succeeded: number; failed: number }>();
    const selection = createSelection({ addAssetsToAlbum: () => pending.promise });
    selection.toggle("a");

    const running = selection.addToAlbum("album-1");
    selection.reset();
    pending.reject(new Error("offline"));
    await running;

    expect(selection.result).toBeNull();
    expect(selection.busyAction).toBeNull();
  });

  it("is dropped by reset for a download, and a new action can start straight away", async () => {
    const pending = deferred<"saved">();
    const selection = createSelection({ download: () => pending.promise });
    selection.toggle("a");

    const running = selection.download();
    selection.reset();
    selection.toggle("b");
    const next = selection.setFavorite(true);
    pending.resolve("saved");
    await running;
    await next;

    expect(selection.result).toEqual({ action: "favorite", succeeded: 1, total: 1 });
    expect(selection.busyAction).toBeNull();
  });

  it("does not let a dropped download clear the busy state of a newer action", async () => {
    const stale = deferred<"saved">();
    const fresh = deferred<{ succeeded: number; failed: number }>();
    const selection = createSelection({ download: () => stale.promise, addAssetsToAlbum: () => fresh.promise });
    selection.toggle("a");

    const staleRun = selection.download();
    selection.reset();
    selection.toggle("a");
    const freshRun = selection.addToAlbum("album-1");
    stale.resolve("saved");
    await staleRun;

    expect(selection.busyAction).toBe("album");
    fresh.resolve({ succeeded: 1, failed: 0 });
    await freshRun;
    expect(selection.busyAction).toBeNull();
  });

  it("reset clears the selection, the result and the restore feedback", async () => {
    const selection = createSelection();
    selection.toggle("a");
    await selection.setFavorite(true);
    selection.toggle("b");

    selection.reset();

    expect(selection.isSelectionMode).toBe(false);
    expect(selection.result).toBeNull();
    expect(selection.restoreFeedback).toBeNull();
  });
});

describe("selection restore", () => {
  it("is unavailable without a restore", async () => {
    const selection = createSelection();
    selection.toggle("a");

    await selection.restore();

    expect(selection.busyAction).toBeNull();
    expect(selection.isSelected("a")).toBe(true);
  });

  it("keeps the assets that were not restored selected and reports partial feedback", async () => {
    const restore = vi.fn(async () => ({ changedAssetIds: ["a"], failedAssetIds: ["b"], failure: null }));
    const selection = createSelection({ restore });
    selection.toggle("a");
    selection.toggle("b");

    await selection.restore();

    expect(restore).toHaveBeenCalledWith(["a", "b"]);
    expect([...selection.selectedIds]).toEqual(["b"]);
    expect(selection.restoreFeedback).toEqual({ kind: "partial", failed: 1, total: 2 });
    expect(selection.busyAction).toBeNull();
  });

  it("is dropped by reset and leaves no busy state", async () => {
    const pending = deferred<{ changedAssetIds: string[]; failedAssetIds: string[]; failure: null }>();
    const selection = createSelection({ restore: () => pending.promise });
    selection.toggle("a");

    const running = selection.restore();
    expect(selection.busyAction).toBe("restore");
    selection.reset();
    pending.resolve({ changedAssetIds: ["a"], failedAssetIds: [], failure: null });
    await running;

    expect(selection.restoreFeedback).toBeNull();
    expect(selection.busyAction).toBeNull();
  });

  it("restores a single asset outside the busy state and reports whether the result still applies", async () => {
    const restore = vi.fn(async (ids: string[]) => ({ changedAssetIds: ids, failedAssetIds: [], failure: null }));
    const selection = createSelection({ restore });

    const { outcome, applied } = await selection.restoreOne("a");

    expect(outcome.changedAssetIds).toEqual(["a"]);
    expect(applied).toBe(true);
    expect(selection.restoreFeedback).toEqual({ kind: "success", count: 1 });

    const pending = deferred<{ changedAssetIds: string[]; failedAssetIds: string[]; failure: null }>();
    const stale = createSelection({ restore: () => pending.promise });
    const running = stale.restoreOne("a");
    stale.reset();
    pending.resolve({ changedAssetIds: ["a"], failedAssetIds: [], failure: null });
    expect((await running).applied).toBe(false);
    expect(stale.restoreFeedback).toBeNull();
  });
});

describe("selection unarchive", () => {
  const outcome = (changedAssetIds: string[], failedAssetIds: string[]) => ({ changedAssetIds, failedAssetIds, failure: null });

  it("is unavailable without an unarchive", async () => {
    const selection = createSelection();
    selection.toggle("a");

    await selection.unarchive();

    expect(selection.busyAction).toBeNull();
    expect(selection.isSelected("a")).toBe(true);
    expect(selection.archiveFeedback).toBeNull();
  });

  it("keeps the assets that were not unarchived selected and reports partial feedback for the action", async () => {
    const unarchive = vi.fn(async () => outcome(["a"], ["b"]));
    const selection = createSelection({ unarchive });
    selection.toggle("a");
    selection.toggle("b");

    await selection.unarchive();

    expect(unarchive).toHaveBeenCalledWith(["a", "b"]);
    expect([...selection.selectedIds]).toEqual(["b"]);
    expect(selection.archiveFeedback).toEqual({ action: "unarchive", feedback: { kind: "partial", failed: 1, total: 2 } });
    expect(selection.busyAction).toBeNull();
  });

  it("ignores a second action while one runs", async () => {
    const pending = deferred<ReturnType<typeof outcome>>();
    const unarchive = vi.fn(() => pending.promise);
    const selection = createSelection({ unarchive });
    selection.toggle("a");

    const running = selection.unarchive();
    expect(selection.busyAction).toBe("unarchive");
    expect(selection.isBusy).toBe(true);
    await selection.unarchive();
    await selection.download();
    expect(unarchive).toHaveBeenCalledTimes(1);

    pending.resolve(outcome(["a"], []));
    await running;
    expect(selection.archiveFeedback).toEqual({ action: "unarchive", feedback: { kind: "success", count: 1 } });
  });

  it("clears the previous feedback when a new action starts and when the selection is reset", async () => {
    const pending = deferred<ReturnType<typeof outcome>>();
    const unarchive = vi.fn()
      .mockResolvedValueOnce(outcome([], ["a"]))
      .mockImplementationOnce(() => pending.promise);
    const selection = createSelection({ unarchive });
    selection.toggle("a");
    await selection.unarchive();
    expect(selection.archiveFeedback?.feedback).toEqual({ kind: "none", failed: 1, total: 1 });

    const running = selection.unarchive();
    expect(selection.archiveFeedback).toBeNull();
    selection.reset();
    pending.resolve(outcome(["a"], []));
    await running;

    expect(selection.archiveFeedback).toBeNull();
    expect(selection.busyAction).toBeNull();
  });

  it("unarchives a single asset outside the busy state and keeps its feedback only while the timeline is the same", async () => {
    const unarchive = vi.fn(async (ids: string[]) => outcome(ids, []));
    const selection = createSelection({ unarchive });

    const result = await selection.unarchiveOne("a");

    expect(unarchive).toHaveBeenCalledWith(["a"]);
    expect(result.changedAssetIds).toEqual(["a"]);
    expect(selection.busyAction).toBeNull();
    expect(selection.archiveFeedback).toEqual({ action: "unarchive", feedback: { kind: "success", count: 1 } });

    const pending = deferred<ReturnType<typeof outcome>>();
    const stale = createSelection({ unarchive: () => pending.promise });
    const running = stale.unarchiveOne("a");
    stale.reset();
    pending.resolve(outcome(["a"], []));
    await running;
    expect(stale.archiveFeedback).toBeNull();

    const failing = createSelection({ unarchive: async () => outcome([], ["a"]) });
    await failing.unarchiveOne("a");
    expect(failing.archiveFeedback).toBeNull();
  });
});
