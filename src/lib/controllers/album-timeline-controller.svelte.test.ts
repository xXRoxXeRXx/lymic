import { describe, expect, it, vi } from "vitest";
import type { AlbumTimeline, TimelineAsset } from "../contracts";
import type { AlbumTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createAlbumTimelineController } from "./album-timeline-controller.svelte";

function makeAsset(id: string, takenAt = "2025-03-02T00:00:00.000Z"): TimelineAsset {
  return {
    id,
    width: 640,
    height: 480,
    type: "IMAGE",
    takenAt,
    cameraMake: null,
    cameraModel: null,
    fileSize: null,
    duration: null,
    isFavorite: false,
  };
}

interface DeferredAlbumTimeline {
  resolve(value: AlbumTimeline): void;
  reject(reason?: unknown): void;
}

function createDeferredAdapter(initial?: AlbumTimeline) {
  const requests: DeferredAlbumTimeline[] = [];
  const getAlbumTimeline = vi.fn<AlbumTimelineDesktopAdapter["getAlbumTimeline"]>();
  if (initial) {
    getAlbumTimeline.mockResolvedValueOnce(initial);
  }
  getAlbumTimeline.mockImplementation(
    () => new Promise<AlbumTimeline>((resolve, reject) => {
      requests.push({ resolve, reject });
    }),
  );
  return { adapter: { getAlbumTimeline }, requests };
}

describe("Album Timeline controller", () => {
  it("publishes the complete month index only after the full album result and activates cached months", async () => {
    const adapter: AlbumTimelineDesktopAdapter = {
      getAlbumTimeline: vi.fn().mockResolvedValue({
        buckets: [
          { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
          { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
        ],
        assetsByBucket: {
          "2025-03-01T00:00:00.000Z": [makeAsset("march", "2025-03-02T00:00:00.000Z")],
          "2025-02-01T00:00:00.000Z": [makeAsset("february", "2025-02-02T00:00:00.000Z")],
        },
      }),
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
    const march = "2025-03-01T00:00:00.000Z";
    const feb = "2025-02-01T00:00:00.000Z";
    const adapter: AlbumTimelineDesktopAdapter = {
      getAlbumTimeline: vi.fn()
        .mockResolvedValueOnce({
          buckets: [
            { timeBucket: march, count: 1 },
            { timeBucket: feb, count: 1 },
          ],
          assetsByBucket: {
            [march]: [makeAsset("march-1", "2025-03-02T00:00:00.000Z")],
            [feb]: [makeAsset("feb-1", "2025-02-02T00:00:00.000Z")],
          },
        })
        .mockRejectedValueOnce(new Error("offline")),
    };
    const timeline = createAlbumTimelineController(adapter, "album-1");

    await timeline.refresh();
    await timeline.activateBucket(march);
    await timeline.refresh();

    expect(timeline.buckets).toEqual([
      { timeBucket: march, count: 1 },
      { timeBucket: feb, count: 1 },
    ]);
    expect(timeline.isOffline).toBe(true);
    expect(timeline.assetsFor(march).map((item) => item.id)).toEqual(["march-1"]);

    await timeline.activateBucket(feb);
    expect(timeline.assetsFor(feb).map((item) => item.id)).toEqual(["feb-1"]);
  });

  it("keeps buckets and activated assets from the latest accepted refresh when overlapping refreshes finish out of order", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const { adapter, requests } = createDeferredAdapter();
    const timeline = createAlbumTimelineController(adapter, "album-1");

    const firstRefresh = timeline.refresh();
    const secondRefresh = timeline.refresh();

    requests[1].resolve({
      buckets: [{ timeBucket: month, count: 1 }],
      assetsByBucket: { [month]: [makeAsset("newer")] },
    });
    await secondRefresh;

    requests[0].resolve({
      buckets: [{ timeBucket: "2025-01-01T00:00:00.000Z", count: 1 }],
      assetsByBucket: { [month]: [makeAsset("older")], "2025-01-01T00:00:00.000Z": [makeAsset("older-jan", "2025-01-02T00:00:00.000Z")] },
    });
    await firstRefresh;

    await timeline.activateBucket(month);

    expect(timeline.buckets).toEqual([{ timeBucket: month, count: 1 }]);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["newer"]);
  });

  it("immediately updates previously activated buckets to the latest accepted refresh without waiting for re-activation", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const { adapter, requests } = createDeferredAdapter({
      buckets: [{ timeBucket: month, count: 1 }],
      assetsByBucket: { [month]: [makeAsset("initial")] },
    });
    const timeline = createAlbumTimelineController(adapter, "album-1");

    await timeline.refresh();
    await timeline.activateBucket(month);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["initial"]);

    const staleRefresh = timeline.refresh();
    const latestRefresh = timeline.refresh();

    requests[1].resolve({
      buckets: [{ timeBucket: month, count: 1 }],
      assetsByBucket: { [month]: [makeAsset("latest")] },
    });
    await latestRefresh;

    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["latest"]);

    requests[0].resolve({
      buckets: [{ timeBucket: "2025-01-01T00:00:00.000Z", count: 1 }],
      assetsByBucket: {
        [month]: [makeAsset("stale")],
        "2025-01-01T00:00:00.000Z": [makeAsset("stale-jan", "2025-01-02T00:00:00.000Z")],
      },
    });
    await staleRefresh;

    expect(timeline.buckets).toEqual([{ timeBucket: month, count: 1 }]);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["latest"]);
  });

  it("retains the prior timeline when a newer overlapping refresh fails and recovers on retry", async () => {
    const month = "2025-03-01T00:00:00.000Z";
    const { adapter, requests } = createDeferredAdapter({
      buckets: [{ timeBucket: month, count: 1 }],
      assetsByBucket: { [month]: [makeAsset("initial")] },
    });
    const timeline = createAlbumTimelineController(adapter, "album-1");

    await timeline.refresh();
    await timeline.activateBucket(month);

    const staleRefresh = timeline.refresh();
    const failingLatestRefresh = timeline.refresh();

    requests[1].reject(new Error("offline"));
    await failingLatestRefresh;

    requests[0].resolve({
      buckets: [{ timeBucket: "2025-01-01T00:00:00.000Z", count: 1 }],
      assetsByBucket: {
        [month]: [makeAsset("stale")],
        "2025-01-01T00:00:00.000Z": [makeAsset("stale-jan", "2025-01-02T00:00:00.000Z")],
      },
    });
    await staleRefresh;

    expect(timeline.isOffline).toBe(true);
    expect(timeline.buckets).toEqual([{ timeBucket: month, count: 1 }]);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["initial"]);

    const recoveredRefresh = timeline.refresh();
    requests[2].resolve({
      buckets: [{ timeBucket: month, count: 1 }],
      assetsByBucket: { [month]: [makeAsset("recovered")] },
    });
    await recoveredRefresh;

    expect(timeline.isOffline).toBe(false);
    expect(timeline.assetsFor(month).map((item) => item.id)).toEqual(["recovered"]);
  });
});
