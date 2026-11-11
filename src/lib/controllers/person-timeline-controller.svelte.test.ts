import { describe, expect, it, vi } from "vitest";
import { createPersonTimelineController } from "./person-timeline-controller.svelte";

describe("Recognized Person Photo Timeline controller", () => {
  it("uses the regular timeline bucket contract with a person filter", async () => {
    const adapter = {
      getPersonTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
      ]),
      getPersonTimelineBucketAssets: vi.fn().mockResolvedValue([
        { id: "march", width: 4032, height: 3024, type: "IMAGE", takenAt: "2025-03-10T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null },
      ]),
      setAssetFavorite: vi.fn(),
    };
    const timeline = createPersonTimelineController(adapter, "person-1");

    await timeline.refresh();
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");

    expect(adapter.getPersonTimelineBuckets).toHaveBeenCalledWith("person-1");
    expect(adapter.getPersonTimelineBucketAssets).toHaveBeenCalledWith("person-1", "2025-03-01T00:00:00.000Z");
    expect(timeline.buckets).toHaveLength(2);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["march"]);
  });

  it("retains cached person assets and reports offline when refreshing buckets fails", async () => {
    const adapter = {
      getPersonTimelineBuckets: vi.fn().mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("network")),
      getPersonTimelineBucketAssets: vi.fn(),
      setAssetFavorite: vi.fn(),
    };
    const timeline = createPersonTimelineController(adapter, "person-1");

    await timeline.refresh();
    await timeline.refresh();

    expect(timeline.isOffline).toBe(true);
  });
});
