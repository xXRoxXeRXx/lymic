import { describe, expect, it } from "vitest";
import { buildTimelineScrubberIndex } from "./timeline-scrubber";

describe("Timeline Scrubber index", () => {
  it("groups loaded Timeline Buckets into distinct years and chronological months", () => {
    expect(buildTimelineScrubberIndex([
      { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
      { timeBucket: "2024-12-01T00:00:00.000Z", count: 3 },
      { timeBucket: "2025-01-01T00:00:00.000Z", count: 4 },
    ])).toEqual([
      { year: 2025, months: ["2025-03-01T00:00:00.000Z", "2025-01-01T00:00:00.000Z"] },
      { year: 2024, months: ["2024-12-01T00:00:00.000Z"] },
    ]);
  });
});
