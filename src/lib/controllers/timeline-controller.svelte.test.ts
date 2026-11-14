import { describe, expect, it, vi } from "vitest";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController } from "./timeline-controller.svelte";

describe("Photo Timeline controller", () => {
  it("re-fetches the index if a removal is confirmed during refresh", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    let resolveRefresh!: (buckets: Array<{ timeBucket: string; count: number }>) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValueOnce([{ timeBucket: month, count: 1 }])
        .mockImplementationOnce(() => new Promise((resolve) => { resolveRefresh = resolve; }))
        .mockResolvedValueOnce([]),
      getTimelineBucketAssets: vi.fn().mockResolvedValue([{ id: "a", width: 1, height: 1, type: "IMAGE", takenAt: null, fileSize: null, duration: null, isFavorite: true }]),
    };
    const timeline = createTimelineController(adapter);
    await timeline.refresh();
    await timeline.activateBucket(month);
    const pending = timeline.refresh();
    timeline.removeAsset("a");
    resolveRefresh([{ timeBucket: month, count: 1 }]);
    await pending;
    expect(adapter.getTimelineBuckets).toHaveBeenCalledTimes(3);
    expect(timeline.buckets).toEqual([]);
  });
  it("does not resurrect a confirmed removal when an older bucket request finishes", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 1, height: 1, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let resolveBucket!: (assets: ReturnType<typeof asset>[]) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: month, count: 2 }]),
      getTimelineBucketAssets: vi.fn().mockResolvedValueOnce([asset("a"), asset("b")])
        .mockImplementationOnce(() => new Promise((resolve) => { resolveBucket = resolve; })),
    };
    const timeline = createTimelineController(adapter);
    await timeline.refresh();
    await timeline.activateBucket(month);
    await timeline.refresh();
    const pending = timeline.activateBucket(month);
    timeline.removeAsset("a");
    resolveBucket([asset("a"), asset("b")]);
    await pending;
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["b"]);
  });

  it("retains the month when the refreshed count exceeds its cached assets", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 1, height: 1, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let resolveBucket!: (assets: ReturnType<typeof asset>[]) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValueOnce([{ timeBucket: month, count: 1 }]).mockResolvedValueOnce([{ timeBucket: month, count: 2 }]),
      getTimelineBucketAssets: vi.fn().mockResolvedValueOnce([asset("a")])
        .mockImplementationOnce(() => new Promise((resolve) => { resolveBucket = resolve; })),
    };
    const timeline = createTimelineController(adapter);
    await timeline.refresh();
    await timeline.activateBucket(month);
    await timeline.refresh();
    const pending = timeline.activateBucket(month);
    timeline.removeAsset("a");
    expect(timeline.buckets).toEqual([{ timeBucket: month, count: 1 }]);
    resolveBucket([asset("a"), asset("b")]);
    await pending;
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["b"]);
  });
  it("loads month buckets in descending chronological order", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      ]),
      getTimelineBucketAssets: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();

    expect(timeline.buckets).toEqual([
      { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    ]);
  });

  it("loads an activated month once and retains its asset details in memory", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn(),
      getTimelineBucketAssets: vi.fn().mockResolvedValue([
        { id: "asset-1", width: 640, height: 480 },
      ]),
    };
    const timeline = createTimelineController(adapter);

    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");

    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(1);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toEqual([
      { id: "asset-1", width: 640, height: 480 },
    ]);
  });

  it("keeps cached buckets and exposes an offline state when refreshing fails", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn()
        .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 3 }])
        .mockRejectedValueOnce(new Error("Network request failed")),
      getTimelineBucketAssets: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 3 }]);
    expect(timeline.isOffline).toBe(true);
  });

  it("distinguishes an initial load failure from a successfully empty timeline", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockRejectedValue(new Error("Connection refused")),
      getTimelineBucketAssets: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();

    expect(timeline.buckets).toEqual([]);
    expect(timeline.hasInitialLoadError).toBe(true);
    expect(timeline.isOffline).toBe(true);
  });

  it("clears the offline state after a successful retry", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn()
        .mockRejectedValueOnce(new Error("Connection refused"))
        .mockResolvedValueOnce([{ timeBucket: "2025-04-01T00:00:00.000Z", count: 1 }]),
      getTimelineBucketAssets: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.isOffline).toBe(false);
    expect(timeline.buckets).toEqual([{ timeBucket: "2025-04-01T00:00:00.000Z", count: 1 }]);
  });

  it("re-fetches stale bucket assets when re-activated after a timeline refresh", async () => {
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
      ]),
      getTimelineBucketAssets: vi.fn()
        .mockResolvedValueOnce([{ id: "asset-1", width: 640, height: 480 }])
        .mockResolvedValueOnce([
          { id: "asset-1", width: 640, height: 480 },
          { id: "asset-2", width: 800, height: 600 },
        ]),
    };
    const timeline = createTimelineController(adapter);

    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    await timeline.refresh();
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toHaveLength(1);

    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(2);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toHaveLength(2);
  });

  it("keeps previously loaded buckets visible during refresh and prunes disappeared buckets afterwards", async () => {
    let resolveSecondRefresh: ((buckets: Array<{ timeBucket: string; count: number }>) => void) | undefined;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn()
        .mockResolvedValueOnce([
          { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
          { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
        ])
        .mockImplementationOnce(() => new Promise((resolve) => { resolveSecondRefresh = resolve; })),
      getTimelineBucketAssets: vi.fn((timeBucket: string) =>
        Promise.resolve([{ id: `asset-${timeBucket}`, width: 640, height: 480, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true }]),
      ),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    await timeline.activateBucket("2025-02-01T00:00:00.000Z");

    const pendingRefresh = timeline.refresh();
    expect(timeline.isLoading).toBe(false);
    expect(timeline.isRefreshing).toBe(true);
    expect(timeline.buckets).toHaveLength(2);
    expect(timeline.assetsFor("2025-02-01T00:00:00.000Z")).toHaveLength(1);

    resolveSecondRefresh?.([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]);
    await pendingRefresh;

    expect(timeline.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]);
    expect(timeline.assetsFor("2025-02-01T00:00:00.000Z")).toEqual([]);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toHaveLength(1);
  });

  it("clears a prior initial load error while a subsequent refresh is loading", async () => {
    let resolveRetry: ((buckets: Array<{ timeBucket: string; count: number }>) => void) | undefined;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn()
        .mockRejectedValueOnce(new Error("offline"))
        .mockImplementationOnce(() => new Promise((resolve) => { resolveRetry = resolve; })),
      getTimelineBucketAssets: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    expect(timeline.hasInitialLoadError).toBe(true);

    const retry = timeline.refresh();
    expect(timeline.isLoading).toBe(true);
    expect(timeline.hasInitialLoadError).toBe(false);

    resolveRetry?.([]);
    await retry;
    expect(timeline.isLoading).toBe(false);
    expect(timeline.hasInitialLoadError).toBe(false);
  });

  it("locates loaded assets and steps across buckets including unloaded months", async () => {
    const march = "2025-03-01T00:00:00.000Z";
    const feb = "2025-02-01T00:00:00.000Z";
    const jan = "2025-01-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 800, height: 600, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: march, count: 2 },
        { timeBucket: feb, count: 1 },
        { timeBucket: jan, count: 1 },
      ]),
      getTimelineBucketAssets: vi.fn((timeBucket: string) => {
        if (timeBucket === march) return Promise.resolve([asset("m1"), asset("m2")]);
        if (timeBucket === feb) return Promise.resolve([asset("f1")]);
        return Promise.resolve([asset("j1")]);
      }),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.activateBucket(march);

    expect(timeline.locateAsset("m2")).toEqual({ timeBucket: march, index: 1 });
    expect(timeline.locateAsset("missing")).toBeNull();

    expect((await timeline.stepAsset("m1", 1))?.id).toBe("m2");
    expect((await timeline.stepAsset("m2", 1))?.id).toBe("f1");
    expect((await timeline.stepAsset("f1", 1))?.id).toBe("j1");

    const formerPos = timeline.locateAsset("f1");
    timeline.removeAsset("f1");
    expect(timeline.buckets.map((b) => b.timeBucket)).toEqual([march, jan]);
    expect((await timeline.stepAsset("f1", 1, formerPos))?.id).toBe("j1");
    expect((await timeline.stepAsset("f1", -1, formerPos))?.id).toBe("m2");
  });

  it("deduplicates concurrent activations of the same bucket within a generation", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let resolveBucket!: (assets: ReturnType<typeof asset>[]) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: month, count: 1 }]),
      getTimelineBucketAssets: vi.fn().mockImplementationOnce(
        () => new Promise((resolve) => { resolveBucket = resolve; }),
      ),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    const first = timeline.activateBucket(month);
    const second = timeline.activateBucket(month);
    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(1);

    resolveBucket([asset("a1")]);
    await Promise.all([first, second]);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["a1"]);
  });

  it("fetches current assets after a refresh even when an earlier request is still pending and ignores stale successes", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let resolveStale!: (assets: ReturnType<typeof asset>[]) => void;
    let resolveCurrent!: (assets: ReturnType<typeof asset>[]) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: month, count: 1 }]),
      getTimelineBucketAssets: vi.fn()
        .mockImplementationOnce(() => new Promise((resolve) => { resolveStale = resolve; }))
        .mockImplementationOnce(() => new Promise((resolve) => { resolveCurrent = resolve; })),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    const staleActivation = timeline.activateBucket(month);
    await timeline.refresh();

    const currentFirst = timeline.activateBucket(month);
    const currentSecond = timeline.activateBucket(month);
    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(2);

    resolveCurrent([asset("current")]);
    await Promise.all([currentFirst, currentSecond]);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["current"]);

    resolveStale([asset("stale")]);
    await staleActivation;
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["current"]);
  });

  it("does not let a stale bucket success overwrite offline state after a current bucket failure", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let resolveStale!: (assets: ReturnType<typeof asset>[]) => void;
    let rejectCurrent!: (reason?: unknown) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: month, count: 1 }]),
      getTimelineBucketAssets: vi.fn()
        .mockImplementationOnce(() => new Promise((resolve) => { resolveStale = resolve; }))
        .mockImplementationOnce(() => new Promise((_, reject) => { rejectCurrent = reject; })),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    const staleActivation = timeline.activateBucket(month);
    await timeline.refresh();

    const currentActivation = timeline.activateBucket(month);
    rejectCurrent(new Error("current failure"));
    await expect(currentActivation).rejects.toThrow("current failure");
    expect(timeline.isOffline).toBe(true);

    resolveStale([asset("stale")]);
    await staleActivation;
    expect(timeline.isOffline).toBe(true);
    expect(timeline.assetsFor(month)).toEqual([]);
  });

  it("ignores stale bucket failures after a refresh without overwriting current assets or offline state", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const asset = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE" as const, takenAt: null, fileSize: null, duration: null, isFavorite: true });
    let rejectStale!: (reason?: unknown) => void;
    let resolveCurrent!: (assets: ReturnType<typeof asset>[]) => void;
    const adapter: TimelineDesktopAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: month, count: 1 }]),
      getTimelineBucketAssets: vi.fn()
        .mockImplementationOnce(() => new Promise((_, reject) => { rejectStale = reject; }))
        .mockImplementationOnce(() => new Promise((resolve) => { resolveCurrent = resolve; })),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    const staleActivation = timeline.activateBucket(month);
    await timeline.refresh();

    const currentActivation = timeline.activateBucket(month);
    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(2);

    resolveCurrent([asset("current")]);
    await currentActivation;
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["current"]);
    expect(timeline.isOffline).toBe(false);

    rejectStale(new Error("stale failure"));
    await expect(staleActivation).rejects.toThrow("stale failure");

    expect(timeline.isOffline).toBe(false);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["current"]);
  });

});
