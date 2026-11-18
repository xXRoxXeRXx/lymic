import { describe, expect, it, vi } from "vitest";
import type { TimelineController } from "../controllers/timeline-controller.svelte";
import { archiveFeedbackMessage, createRemoteAssetArchive } from "./remote-asset-archive";

function setup(
  setAssetsVisibility: (assetIds: string[], visibility: "archive" | "timeline") => Promise<{ changedAssetIds: string[] }>,
  extra: { assetsStayInView?: boolean; isArchivable?: (assetId: string) => boolean } = {},
) {
  const removeAsset = vi.fn();
  const onRemoved = vi.fn();
  const archive = createRemoteAssetArchive({
    timeline: { removeAsset } as unknown as TimelineController,
    setAssetsVisibility,
    onRemoved,
    ...extra,
  });
  return { archive, removeAsset, onRemoved };
}

describe("Unarchive", () => {
  it("writes the timeline visibility and removes the unarchived assets from the view", async () => {
    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const { archive, removeAsset, onRemoved } = setup(setAssetsVisibility);

    const outcome = await archive.apply("unarchive", ["a", "b"]);

    expect(setAssetsVisibility).toHaveBeenCalledWith(["a", "b"], "timeline");
    expect(outcome).toEqual({ changedAssetIds: ["a", "b"], failedAssetIds: [], failure: null });
    expect(removeAsset.mock.calls).toEqual([["a"], ["b"]]);
    expect(onRemoved.mock.calls).toEqual([["a"], ["b"]]);
  });

  it("keeps assets the backend did not confirm, including ones it was never asked about", async () => {
    const { archive, removeAsset } = setup(async () => ({ changedAssetIds: ["a", "stranger"] }));

    const outcome = await archive.apply("unarchive", ["a", "b", "c"]);

    expect(outcome).toEqual({ changedAssetIds: ["a"], failedAssetIds: ["b", "c"], failure: null });
    expect(removeAsset.mock.calls).toEqual([["a"]]);
  });

  it("never rejects: a missing asset.update permission is reported as permission, anything else as an error", async () => {
    const refused = setup(async () => { throw "archive_permission_denied:asset.update"; });
    expect(await refused.archive.apply("unarchive", ["a"])).toEqual({ changedAssetIds: [], failedAssetIds: ["a"], failure: "permission" });
    expect(refused.removeAsset).not.toHaveBeenCalled();

    const broken = setup(async () => { throw new Error("Failed with key secret-key"); });
    expect(await broken.archive.apply("unarchive", ["a", "b"])).toEqual({ changedAssetIds: [], failedAssetIds: ["a", "b"], failure: "error" });
    expect(broken.removeAsset).not.toHaveBeenCalled();
  });
});

describe("Archive", () => {
  it("writes the archive visibility and removes the archived assets from the view", async () => {
    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const { archive, removeAsset, onRemoved } = setup(setAssetsVisibility);

    const outcome = await archive.apply("archive", ["a", "b"]);

    expect(setAssetsVisibility).toHaveBeenCalledWith(["a", "b"], "archive");
    expect(outcome).toEqual({ changedAssetIds: ["a", "b"], failedAssetIds: [], failure: null });
    expect(removeAsset.mock.calls).toEqual([["a"], ["b"]]);
    expect(onRemoved.mock.calls).toEqual([["a"], ["b"]]);
  });

  it("keeps assets the backend did not confirm and reports a missing permission without removing anything", async () => {
    const partial = setup(async () => ({ changedAssetIds: ["a"] }));
    expect(await partial.archive.apply("archive", ["a", "b"])).toEqual({ changedAssetIds: ["a"], failedAssetIds: ["b"], failure: null });
    expect(partial.removeAsset.mock.calls).toEqual([["a"]]);

    const refused = setup(async () => { throw "archive_permission_denied:asset.update"; });
    expect(await refused.archive.apply("archive", ["a"])).toEqual({ changedAssetIds: [], failedAssetIds: ["a"], failure: "permission" });
    expect(refused.removeAsset).not.toHaveBeenCalled();
  });
});

describe("Archive in a view that shows archived and other assets together", () => {
  it("keeps the archived assets in the view and reports them as changed, including ones that were already archived", async () => {
    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const { archive, removeAsset, onRemoved } = setup(setAssetsVisibility, { assetsStayInView: true });

    const outcome = await archive.apply("archive", ["a", "already-archived"]);

    expect(setAssetsVisibility).toHaveBeenCalledWith(["a", "already-archived"], "archive");
    expect(outcome).toEqual({ changedAssetIds: ["a", "already-archived"], failedAssetIds: [], failure: null });
    expect(removeAsset).not.toHaveBeenCalled();
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("leaves hidden assets out of the request and reports them as not archived", async () => {
    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const { archive } = setup(setAssetsVisibility, { assetsStayInView: true, isArchivable: (assetId) => assetId !== "live-video" });

    const outcome = await archive.apply("archive", ["a", "live-video", "b"]);

    expect(setAssetsVisibility).toHaveBeenCalledWith(["a", "b"], "archive");
    expect(outcome).toEqual({ changedAssetIds: ["a", "b"], failedAssetIds: ["live-video"], failure: null });
  });

  it("does not call the backend when no selected asset can be archived", async () => {
    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const { archive } = setup(setAssetsVisibility, { assetsStayInView: true, isArchivable: () => false });

    expect(await archive.apply("archive", ["live-video"])).toEqual({ changedAssetIds: [], failedAssetIds: ["live-video"], failure: null });
    expect(setAssetsVisibility).not.toHaveBeenCalled();
  });

  it("keeps a failed request's assets failed and does not skip anything for an Unarchive", async () => {
    const broken = setup(async () => { throw new Error("boom"); }, { assetsStayInView: true, isArchivable: (assetId) => assetId !== "hidden" });
    expect(await broken.archive.apply("archive", ["a", "hidden"])).toEqual({ changedAssetIds: [], failedAssetIds: ["a", "hidden"], failure: "error" });

    const setAssetsVisibility = vi.fn(async (assetIds: string[]) => ({ changedAssetIds: assetIds }));
    const unarchive = setup(setAssetsVisibility, { isArchivable: () => false });
    await unarchive.archive.apply("unarchive", ["a"]);
    expect(setAssetsVisibility).toHaveBeenCalledWith(["a"], "timeline");
  });
});

describe("Archive feedback", () => {
  const translate = (key: string, options?: { values: Record<string, string | number> }) =>
    `${key}${options ? `:${JSON.stringify(options.values)}` : ""}`;

  it("uses the Archive locale keys with locale-formatted counts", () => {
    expect(archiveFeedbackMessage("archive", { kind: "success", count: 1234 }, translate, "en")).toBe('archive_archive_success:{"count":"1,234"}');
    expect(archiveFeedbackMessage("archive", { kind: "partial", failed: 2, total: 3 }, translate, "en")).toBe('archive_archive_partial:{"failed":"2","count":"3"}');
    expect(archiveFeedbackMessage("archive", { kind: "none", failed: 1, total: 1 }, translate, "en")).toBe('archive_archive_none:{"failed":"1","count":"1"}');
    expect(archiveFeedbackMessage("archive", { kind: "permission" }, translate, "en")).toBe("archive_archive_permission");
    expect(archiveFeedbackMessage("archive", { kind: "failed" }, translate, "en")).toBe("archive_archive_failed");
  });
});

describe("Unarchive feedback", () => {
  const translate = (key: string, options?: { values: Record<string, string | number> }) =>
    `${key}${options ? `:${JSON.stringify(options.values)}` : ""}`;

  it("uses the Unarchive locale keys with locale-formatted counts", () => {
    expect(archiveFeedbackMessage("unarchive", { kind: "success", count: 1234 }, translate, "en")).toBe('archive_unarchive_success:{"count":"1,234"}');
    expect(archiveFeedbackMessage("unarchive", { kind: "partial", failed: 2, total: 3 }, translate, "en")).toBe('archive_unarchive_partial:{"failed":"2","count":"3"}');
    expect(archiveFeedbackMessage("unarchive", { kind: "none", failed: 1, total: 1 }, translate, "en")).toBe('archive_unarchive_none:{"failed":"1","count":"1"}');
    expect(archiveFeedbackMessage("unarchive", { kind: "permission" }, translate, "en")).toBe("archive_unarchive_permission");
    expect(archiveFeedbackMessage("unarchive", { kind: "failed" }, translate, "en")).toBe("archive_unarchive_failed");
  });
});
