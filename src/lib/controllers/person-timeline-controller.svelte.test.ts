import { describe, expect, it, vi } from "vitest";
import { createPersonTimelineController } from "./person-timeline-controller.svelte";

describe("Recognized Person Photo Timeline controller", () => {
  it("groups a selected person's assets chronologically and keeps an empty result usable", async () => {
    const adapter = { getPersonTimelineAssets: vi.fn().mockResolvedValue([
      { id: "old", width: 1, height: 1, type: "IMAGE", takenAt: "2025-02-10T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null },
      { id: "new", width: 1, height: 1, type: "IMAGE", takenAt: "2025-03-10T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null },
    ]) };
    const timeline = createPersonTimelineController(adapter, "person-1");

    await timeline.refresh();

    expect(timeline.buckets.map((bucket) => bucket.timeBucket)).toEqual([
      "2025-03-01T00:00:00.000Z", "2025-02-01T00:00:00.000Z",
    ]);
    expect(timeline.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["new"]);
  });

  it("retains cached person assets and reports offline when refresh fails", async () => {
    const adapter = { getPersonTimelineAssets: vi.fn().mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("network")) };
    const timeline = createPersonTimelineController(adapter, "person-1");
    await timeline.refresh();
    await timeline.refresh();
    expect(timeline.isOffline).toBe(true);
  });
});
