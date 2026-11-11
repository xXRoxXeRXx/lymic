import { describe, expect, it, vi } from "vitest";
import type { AlbumTimelineDesktopAdapter, AssetDesktopAdapter } from "../tauri/desktop-adapter";
import { createAlbumTimelineController } from "./album-timeline-controller.svelte";

type AlbumTimelineAdapter = AlbumTimelineDesktopAdapter & Pick<AssetDesktopAdapter, "downloadAsset">;

describe("Album Timeline controller", () => {
  it("publishes the complete month index only after the full album result and activates cached months", async () => {
    const adapter: AlbumTimelineAdapter = {
      getAlbumTimeline: vi.fn().mockResolvedValue({
        buckets: [
          { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
          { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
        ],
        assetsByBucket: {
          "2025-03-01T00:00:00.000Z": [{ id: "march", width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null }],
          "2025-02-01T00:00:00.000Z": [{ id: "february", width: 640, height: 480, type: "IMAGE", takenAt: "2025-02-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null }],
        },
      }),
      downloadAsset: vi.fn(),
    };
    const timeline = createAlbumTimelineController(adapter, "album-1");

    expect(timeline.buckets).toEqual([]);
    await timeline.refresh();
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");

    expect(timeline.buckets).toHaveLength(2);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["march"]);
    expect(adapter.getAlbumTimeline).toHaveBeenCalledTimes(1);
  });

  it("retains a usable album timeline when its refresh goes offline", async () => {
    const adapter: AlbumTimelineAdapter = {
      getAlbumTimeline: vi.fn()
        .mockResolvedValueOnce({ buckets: [{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }], assetsByBucket: {} })
        .mockRejectedValueOnce(new Error("offline")),
      downloadAsset: vi.fn(),
    };
    const timeline = createAlbumTimelineController(adapter, "album-1");

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]);
    expect(timeline.isOffline).toBe(true);
  });
});
