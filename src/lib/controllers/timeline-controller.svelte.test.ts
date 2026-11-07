import { describe, expect, it, vi } from "vitest";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController } from "./timeline-controller.svelte";

describe("Photo Timeline controller", () => {
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
});
