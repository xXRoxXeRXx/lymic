import { describe, expect, it, vi } from "vitest";
import type { TimelineAsset, TimelineBucket } from "../contracts";
import type { FilteredTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createFilteredTimelineController, createFilteredTimelineControllers } from "./filtered-timeline-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const february = "2025-02-01T00:00:00.000Z";
const ARCHIVE_PERMISSION_DENIED = "archive_permission_denied:asset.read";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function asset(id: string, takenAt: string | null = null): TimelineAsset {
  return { id, width: 800, height: 600, type: "IMAGE", takenAt, fileSize: null, duration: null, isFavorite: true };
}

/** An adapter that answers every Filtered Timeline with `getBuckets` and `getBucketAssets(timeBucket)`. */
function adapterOf(
  getBuckets: () => Promise<TimelineBucket[]>,
  getBucketAssets: (timeBucket: string) => Promise<TimelineAsset[]> = vi.fn(),
): FilteredTimelineDesktopAdapter {
  return {
    getFilteredTimelineBuckets: () => getBuckets(),
    getFilteredTimelineBucketAssets: (_timeline, timeBucket) => getBucketAssets(timeBucket),
  };
}

describe("Filtered Timeline controller", () => {
  it("requests its own Filtered Timeline's buckets and the assets of an activated bucket, newest first", async () => {
    const adapter: FilteredTimelineDesktopAdapter = {
      getFilteredTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: february, count: 1 }, { timeBucket: march, count: 2 }]),
      getFilteredTimelineBucketAssets: vi.fn().mockResolvedValue([asset("fav-photo"), asset("fav-video")]),
    };
    const favorites = createFilteredTimelineController(adapter, "favorites");

    await favorites.refresh();
    await favorites.activateBucket(march);

    expect(adapter.getFilteredTimelineBuckets).toHaveBeenCalledExactlyOnceWith("favorites");
    expect(adapter.getFilteredTimelineBucketAssets).toHaveBeenCalledExactlyOnceWith("favorites", march);
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 2 }, { timeBucket: february, count: 1 }]);
    expect(favorites.assetsFor(march).map((entry) => entry.id)).toEqual(["fav-photo", "fav-video"]);
  });

  it("creates one controller per Filtered Timeline, each over its own requests", async () => {
    const adapter: FilteredTimelineDesktopAdapter = {
      getFilteredTimelineBuckets: vi.fn().mockResolvedValue([]),
      getFilteredTimelineBucketAssets: vi.fn(),
    };
    const controllers = createFilteredTimelineControllers(adapter);

    await controllers.archive.refresh();
    await controllers.trash.refresh();

    expect(Object.keys(controllers)).toEqual(["favorites", "archive", "trash"]);
    expect(adapter.getFilteredTimelineBuckets).toHaveBeenNthCalledWith(1, "archive");
    expect(adapter.getFilteredTimelineBuckets).toHaveBeenNthCalledWith(2, "trash");
  });

  it("distinguishes an initial load error from an empty timeline and keeps cached buckets when a re-entry refresh fails", async () => {
    const getBuckets = vi.fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockRejectedValueOnce(new Error("offline"));
    const favorites = createFilteredTimelineController(adapterOf(getBuckets), "favorites");

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(true);
    expect(favorites.buckets).toEqual([]);

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(false);
    expect(favorites.isOffline).toBe(false);
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 1 }]);

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(false);
    expect(favorites.isOffline).toBe(true);
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 1 }]);
  });

  it("removes a confirmed unfavorited asset, updates counts, drops empty buckets, and ignores unknown assets", async () => {
    const favorites = createFilteredTimelineController(adapterOf(
      vi.fn().mockResolvedValue([{ timeBucket: march, count: 2 }, { timeBucket: february, count: 1 }]),
      vi.fn(async (timeBucket: string) => timeBucket === march
        ? [asset("fav-1", "2025-03-05T12:00:00.000Z"), asset("fav-2", "2025-03-04T12:00:00.000Z")]
        : [asset("fav-3", "2025-02-10T12:00:00.000Z")]),
    ), "favorites");

    await favorites.refresh();
    await favorites.activateBucket(march);
    await favorites.activateBucket(february);

    favorites.removeAsset("does-not-exist");
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 2 }, { timeBucket: february, count: 1 }]);

    favorites.removeAsset("fav-1");
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 1 }, { timeBucket: february, count: 1 }]);
    expect(favorites.assetsFor(march).map((entry) => entry.id)).toEqual(["fav-2"]);

    favorites.removeAsset("fav-3");
    expect(favorites.buckets).toEqual([{ timeBucket: march, count: 1 }]);
    expect(favorites.assetsFor(february)).toEqual([]);

    favorites.removeAsset("fav-2");
    expect(favorites.buckets).toEqual([]);
    expect(favorites.assetsFor(march)).toEqual([]);
  });

  it("reports permissionDenied for its own permission code and resets it after a successful refresh", async () => {
    const getBuckets = vi.fn()
      .mockRejectedValueOnce(ARCHIVE_PERMISSION_DENIED)
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    const archive = createFilteredTimelineController(adapterOf(getBuckets), "archive");

    await archive.refresh();
    expect(archive.permissionDenied).toBe(true);
    expect(archive.hasInitialLoadError).toBe(true);

    await archive.refresh();
    expect(archive.permissionDenied).toBe(false);
  });

  it("never reports another Filtered Timeline's permission code as its own", async () => {
    const trash = createFilteredTimelineController(adapterOf(vi.fn().mockRejectedValue(ARCHIVE_PERMISSION_DENIED)), "trash");

    await trash.refresh();

    expect(trash.hasInitialLoadError).toBe(true);
    expect(trash.permissionDenied).toBe(false);
  });

  it("clears permissionDenied on clear()", async () => {
    const archive = createFilteredTimelineController(adapterOf(vi.fn().mockRejectedValue(ARCHIVE_PERMISSION_DENIED)), "archive");
    await archive.refresh();
    expect(archive.permissionDenied).toBe(true);

    archive.clear();
    expect(archive.permissionDenied).toBe(false);
  });

  it("ignores a permission denial that rejects after clear() so the next account does not inherit it", async () => {
    const inFlight = deferred<never>();
    const archive = createFilteredTimelineController(adapterOf(() => inFlight.promise), "archive");

    const refresh = archive.refresh();
    archive.clear();
    inFlight.reject(ARCHIVE_PERMISSION_DENIED);
    await refresh;

    expect(archive.permissionDenied).toBe(false);
  });

  it("ignores a bucket-assets denial that rejects after clear()", async () => {
    const inFlight = deferred<never>();
    const archive = createFilteredTimelineController(adapterOf(
      vi.fn().mockResolvedValue([{ timeBucket: march, count: 1 }]),
      () => inFlight.promise,
    ), "archive");
    await archive.refresh();

    const activation = archive.activateBucket(march).catch(() => {});
    archive.clear();
    inFlight.reject(ARCHIVE_PERMISSION_DENIED);
    await activation;

    expect(archive.permissionDenied).toBe(false);
  });
});
