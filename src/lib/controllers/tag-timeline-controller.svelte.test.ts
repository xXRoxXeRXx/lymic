import { describe, expect, it, vi } from "vitest";
import { createTagTimelineController } from "./tag-timeline-controller.svelte";

describe("Tag Timeline controller", () => {
  it("uses the regular timeline bucket contract with a Tag filter", async () => {
    const adapter = {
      getTagTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
      ]),
      getTagTimelineBucketAssets: vi.fn().mockResolvedValue([
        { id: "march", width: 4032, height: 3024, type: "IMAGE", takenAt: "2025-03-10T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null },
      ]),
    };
    const timeline = createTagTimelineController(adapter, "tag-1");

    await timeline.refresh();
    expect(adapter.getTagTimelineBucketAssets).not.toHaveBeenCalled();
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");

    expect(adapter.getTagTimelineBuckets).toHaveBeenCalledWith("tag-1");
    expect(adapter.getTagTimelineBucketAssets).toHaveBeenCalledWith("tag-1", "2025-03-01T00:00:00.000Z");
    expect(timeline.buckets).toHaveLength(2);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["march"]);
  });
});
