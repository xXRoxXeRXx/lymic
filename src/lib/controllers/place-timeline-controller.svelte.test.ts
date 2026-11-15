import { describe, expect, it, vi } from "vitest";
import { createPlaceTimelineController } from "./place-timeline-controller.svelte";

describe("Place Timeline controller", () => {
  it("requests the city once and serves activated months from the complete result", async () => {
    const adapter = {
      getPlaceTimeline: vi.fn().mockResolvedValue({
        buckets: [{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }],
        assetsByBucket: {
          "2025-03-01T00:00:00.000Z": [
            { id: "march", width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false },
          ],
        },
      }),
    };
    const timeline = createPlaceTimelineController(adapter, "Paris");

    await timeline.refresh();
    await timeline.activateBucket("2025-03-01T00:00:00.000Z");

    expect(adapter.getPlaceTimeline).toHaveBeenCalledTimes(1);
    expect(adapter.getPlaceTimeline).toHaveBeenCalledWith("Paris");
    expect(timeline.buckets).toHaveLength(1);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["march"]);
  });
});
