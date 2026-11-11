import { describe, expect, it, vi } from "vitest";
import type { AssetDesktopAdapter, TimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController } from "./timeline-controller.svelte";

type TimelineAdapter = TimelineDesktopAdapter & Pick<AssetDesktopAdapter, "downloadAsset">;

describe("Photo Timeline controller", () => {
  it("loads month buckets in descending chronological order", async () => {
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      ]),
      getTimelineBucketAssets: vi.fn(),
      downloadAsset: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();

    expect(timeline.buckets).toEqual([
      { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    ]);
  });

  it("loads an activated month once and retains its asset details in memory", async () => {
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn(),
      getTimelineBucketAssets: vi.fn().mockResolvedValue([
        { id: "asset-1", width: 640, height: 480 },
      ]),
      downloadAsset: vi.fn(),
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
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn()
        .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 3 }])
        .mockRejectedValueOnce(new Error("Network request failed")),
      getTimelineBucketAssets: vi.fn(),
      downloadAsset: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 3 }]);
    expect(timeline.isOffline).toBe(true);
  });

  it("distinguishes an initial load failure from a successfully empty timeline", async () => {
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn().mockRejectedValue(new Error("Connection refused")),
      getTimelineBucketAssets: vi.fn(),
      downloadAsset: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();

    expect(timeline.buckets).toEqual([]);
    expect(timeline.hasInitialLoadError).toBe(true);
    expect(timeline.isOffline).toBe(true);
  });

  it("clears the offline state after a successful retry", async () => {
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn()
        .mockRejectedValueOnce(new Error("Connection refused"))
        .mockResolvedValueOnce([{ timeBucket: "2025-04-01T00:00:00.000Z", count: 1 }]),
      getTimelineBucketAssets: vi.fn(),
      downloadAsset: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.isOffline).toBe(false);
    expect(timeline.buckets).toEqual([{ timeBucket: "2025-04-01T00:00:00.000Z", count: 1 }]);
  });

  it("re-fetches stale bucket assets when re-activated after a timeline refresh", async () => {
    const adapter: TimelineAdapter = {
      getTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
      ]),
      getTimelineBucketAssets: vi.fn()
        .mockResolvedValueOnce([{ id: "asset-1", width: 640, height: 480 }])
        .mockResolvedValueOnce([
          { id: "asset-1", width: 640, height: 480 },
          { id: "asset-2", width: 800, height: 600 },
        ]),
      downloadAsset: vi.fn(),
    };
    const timeline = createTimelineController(adapter);

    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    await timeline.refresh();
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toHaveLength(1);

    await timeline.activateBucket("2025-03-01T00:00:00.000Z");
    expect(adapter.getTimelineBucketAssets).toHaveBeenCalledTimes(2);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z")).toHaveLength(2);
  });

});
