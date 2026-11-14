import { describe, expect, it, vi } from "vitest";
import type { TimelineAsset } from "../contracts";
import { createTimelineController, type TimelineController } from "../controllers/timeline-controller.svelte";
import { createRemoteAssetFavorites } from "./remote-asset-favorites.svelte";

const march = "2025-03-01T00:00:00.000Z";
const february = "2025-02-01T00:00:00.000Z";

function asset(id: string, isFavorite = true): TimelineAsset {
  return { id, width: 800, height: 600, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite };
}

async function favoritesTimeline(assetsByBucket: Record<string, TimelineAsset[]>): Promise<TimelineController> {
  const timeline = createTimelineController({
    getTimelineBuckets: async () => Object.entries(assetsByBucket).map(([timeBucket, assets]) => ({ timeBucket, count: assets.length })),
    getTimelineBucketAssets: async (timeBucket) => assetsByBucket[timeBucket],
  });
  await timeline.refresh();
  for (const timeBucket of Object.keys(assetsByBucket)) await timeline.activateBucket(timeBucket);
  return timeline;
}

function createFavorites(options: {
  timeline: TimelineController;
  setAssetFavorite?: (assetId: string, isFavorite: boolean) => Promise<void>;
  removeUnfavorited?: boolean;
  onReflect?: (assetId: string, isFavorite: boolean) => void;
  onRemoved?: (assetId: string) => void;
}) {
  return createRemoteAssetFavorites({
    timeline: options.timeline,
    setAssetFavorite: options.setAssetFavorite ?? (async () => {}),
    removeUnfavorited: options.removeUnfavorited ?? false,
    errorMessage: "favorite update failed",
    onReflect: options.onReflect,
    onRemoved: options.onRemoved,
  });
}

function bucketIds(timeline: TimelineController, timeBucket: string) {
  return timeline.assetsFor(timeBucket).map((candidate) => candidate.id);
}

describe("Remote Asset favorites in a Favorites Timeline", () => {
  it("removes an asset from the timeline and reports it once its unfavorite is confirmed", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b")] });
    const onRemoved = vi.fn();
    const favorites = createFavorites({ timeline, removeUnfavorited: true, onRemoved });

    await favorites.updateOne("a", false);

    expect(bucketIds(timeline, march)).toEqual(["b"]);
    expect(onRemoved).toHaveBeenCalledWith("a");
  });

  it("keeps the asset and rethrows when an unfavorite fails, reporting the error only when the Lightbox is not showing that asset", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b")] });
    const onRemoved = vi.fn();
    const favorites = createFavorites({
      timeline,
      removeUnfavorited: true,
      onRemoved,
      setAssetFavorite: async () => { throw new Error("offline"); },
    });

    favorites.open("a");
    await expect(favorites.updateOne("a", false)).rejects.toThrow("offline");
    expect(favorites.updateError).toBe("");

    await expect(favorites.updateOne("b", false)).rejects.toThrow("offline");
    expect(favorites.updateError).toBe("favorite update failed");
    favorites.clearError();
    expect(favorites.updateError).toBe("");

    expect(bucketIds(timeline, march)).toEqual(["a", "b"]);
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("navigates the Lightbox from a removed asset's former position, in both directions and across buckets", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b"), asset("c")], [february]: [asset("d")] });
    const favorites = createFavorites({ timeline, removeUnfavorited: true });

    favorites.open("b");
    await favorites.updateOne("b", false);
    expect((await favorites.navigate(1, asset("b")))?.id).toBe("c");

    favorites.open("c");
    await favorites.updateOne("c", false);
    expect((await favorites.navigate(1, asset("c")))?.id).toBe("d");

    favorites.open("d");
    await favorites.updateOne("d", false);
    expect((await favorites.navigate(-1, asset("d")))?.id).toBe("a");
  });

  it("keeps the former position correct when an earlier asset in the same bucket is removed afterwards", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b"), asset("c"), asset("d")] });
    const favorites = createFavorites({ timeline, removeUnfavorited: true });

    favorites.open("b");
    await favorites.updateOne("b", false);
    await favorites.updateOne("a", false);

    expect((await favorites.navigate(1, asset("b")))?.id).toBe("c");
  });

  it("abandons navigation once the Lightbox closes", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b")] });
    const favorites = createFavorites({ timeline, removeUnfavorited: true });

    favorites.open("a");
    const pending = favorites.navigate(1, asset("a"));
    favorites.close();
    expect(await pending).toBeNull();
  });

  it("removes only the selected assets that were confirmed and reports a partial failure", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a"), asset("b"), asset("c")] });
    const onRemoved = vi.fn();
    const favorites = createFavorites({
      timeline,
      removeUnfavorited: true,
      onRemoved,
      setAssetFavorite: async (assetId) => { if (assetId === "b") throw new Error("rejected"); },
    });

    const result = await favorites.updateMany(["a", "b", "c"], false, () => true);

    expect(result).toEqual({ succeeded: 2, total: 3 });
    expect(bucketIds(timeline, march)).toEqual(["b"]);
    expect(onRemoved.mock.calls).toEqual([["a"], ["c"]]);
    expect(favorites.updateError).toBe("");
  });
});

describe("Remote Asset favorites in a Photo Timeline", () => {
  it("keeps unfavorited assets in the timeline and reflects each confirmed selection update", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a", false), asset("b", false), asset("c", false)] });
    const onReflect = vi.fn();
    const onRemoved = vi.fn();
    const setAssetFavorite = vi.fn(async (assetId: string) => { if (assetId === "b") throw new Error("rejected"); });
    const favorites = createFavorites({ timeline, setAssetFavorite, onReflect, onRemoved });

    const result = await favorites.updateMany(["a", "b", "c"], true, () => true);

    expect(result).toEqual({ succeeded: 2, total: 3 });
    expect(bucketIds(timeline, march)).toEqual(["a", "b", "c"]);
    expect(timeline.assetsFor(march).map((candidate) => favorites.isFavorite(candidate))).toEqual([true, false, true]);
    expect(onReflect.mock.calls).toEqual([["a", true], ["c", true]]);
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("does not remove an asset when a single unfavorite is confirmed", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a")] });
    const setAssetFavorite = vi.fn(async () => {});
    const favorites = createFavorites({ timeline, setAssetFavorite });

    await favorites.updateOne("a", false);

    expect(setAssetFavorite).toHaveBeenCalledWith("a", false);
    expect(bucketIds(timeline, march)).toEqual(["a"]);
  });

  it("abandons a selection update once it is no longer current", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a", false), asset("b", false)] });
    const onReflect = vi.fn();
    let current = true;
    const setAssetFavorite = vi.fn(async () => { current = false; });
    const favorites = createFavorites({ timeline, setAssetFavorite, onReflect });

    const result = await favorites.updateMany(["a", "b"], true, () => current);

    expect(result).toBeNull();
    expect(setAssetFavorite).toHaveBeenCalledTimes(1);
    expect(onReflect).not.toHaveBeenCalled();
  });

  it("tracks Lightbox favorite changes as overrides and keeps only in-flight ones across a refresh", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a", false), asset("b", false)] });
    const onReflect = vi.fn();
    const favorites = createFavorites({ timeline, onReflect });

    favorites.reflect("a", true);
    favorites.reflect("b", true);
    expect(timeline.assetsFor(march).map((candidate) => favorites.isFavorite(candidate))).toEqual([true, true]);
    expect(onReflect.mock.calls).toEqual([["a", true], ["b", true]]);

    favorites.retainOverrides(["b"]);
    expect(timeline.assetsFor(march).map((candidate) => favorites.isFavorite(candidate))).toEqual([false, true]);

    favorites.reset();
    expect(timeline.assetsFor(march).map((candidate) => favorites.isFavorite(candidate))).toEqual([false, false]);
  });
});

describe("Remote Asset favorites navigation in a Photo Timeline", () => {
  it("wraps around the loaded assets and resolves each asset's favorite state", async () => {
    const timeline = await favoritesTimeline({ [march]: [asset("a", false), asset("b", false)] });
    const favorites = createFavorites({ timeline });
    favorites.reflect("b", true);

    expect(await favorites.navigate(1, asset("a", false))).toMatchObject({ id: "b", isFavorite: true });
    expect((await favorites.navigate(1, asset("b", false)))?.id).toBe("a");
    expect((await favorites.navigate(-1, asset("a", false)))?.id).toBe("b");
  });
});
