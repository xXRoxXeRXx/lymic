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
    };
    const timeline = createTimelineController(adapter);

    await timeline.refresh();

    expect(timeline.buckets).toEqual([
      { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
    ]);
  });
});
