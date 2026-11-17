import { describe, expect, it, vi } from "vitest";
import { createFilteredTimelineController } from "./filtered-timeline-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const february = "2025-02-01T00:00:00.000Z";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("Filtered Timeline controller", () => {
  it("loads the source's buckets newest first and the assets of an activated bucket", async () => {
    const source = {
      getBuckets: vi.fn().mockResolvedValue([{ timeBucket: february, count: 1 }, { timeBucket: march, count: 1 }]),
      getBucketAssets: vi.fn().mockResolvedValue([
        { id: "a", width: 800, height: 600, type: "IMAGE", takenAt: null, fileSize: null, duration: null, isFavorite: false },
      ]),
    };
    const timeline = createFilteredTimelineController(source);

    await timeline.refresh();
    await timeline.activateBucket(march);

    expect(timeline.buckets.map((bucket) => bucket.timeBucket)).toEqual([march, february]);
    expect(source.getBucketAssets).toHaveBeenCalledWith(march);
    expect(timeline.assetsFor(march).map((asset) => asset.id)).toEqual(["a"]);
  });

  it("reports permissionDenied for the source's permission code and resets it after a successful refresh", async () => {
    const getBuckets = vi.fn()
      .mockRejectedValueOnce("archive_permission_denied:asset.read")
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    const timeline = createFilteredTimelineController({ getBuckets, getBucketAssets: vi.fn(), permissionPrefix: "archive" });

    await timeline.refresh();
    expect(timeline.permissionDenied).toBe(true);
    expect(timeline.hasInitialLoadError).toBe(true);

    await timeline.refresh();
    expect(timeline.permissionDenied).toBe(false);
  });

  it("never reports permissionDenied for a source without a permission code", async () => {
    const timeline = createFilteredTimelineController({
      getBuckets: vi.fn().mockRejectedValue("trash_permission_denied:asset.read"),
      getBucketAssets: vi.fn(),
    });

    await timeline.refresh();

    expect(timeline.hasInitialLoadError).toBe(true);
    expect(timeline.permissionDenied).toBe(false);
  });

  it("ignores a permission denial that rejects after clear()", async () => {
    const inFlight = deferred<never>();
    const timeline = createFilteredTimelineController({
      getBuckets: () => inFlight.promise,
      getBucketAssets: vi.fn(),
      permissionPrefix: "archive",
    });

    const refresh = timeline.refresh();
    timeline.clear();
    inFlight.reject("archive_permission_denied:asset.read");
    await refresh;

    expect(timeline.permissionDenied).toBe(false);
  });
});
