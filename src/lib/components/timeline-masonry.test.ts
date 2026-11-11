import { describe, expect, it } from "vitest";
import type { TimelineAsset } from "../contracts";
import { calculateMasonryLayout } from "./timeline-masonry";

const assets: TimelineAsset[] = [
  { id: "wide", width: 1000, height: 500, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false },
  { id: "square", width: 1000, height: 1000, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false },
  { id: "portrait", width: 500, height: 1000, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false },
  { id: "landscape", width: 2000, height: 1000, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false },
];

describe("Photo Timeline masonry layout", () => {
  it("distributes assets into the shortest column using their aspect ratios", () => {
    const layout = calculateMasonryLayout(assets, 616);

    expect(layout.columnCount).toBe(3);
    expect(layout.columns.map((column) => column.map((item) => item.asset.id))).toEqual([
      ["wide", "landscape"],
      ["square"],
      ["portrait"],
    ]);
    expect(layout.columns[0].map((item) => item.height)).toEqual([100, 100]);
    expect(layout.height).toBe(400);
  });

  it("recalculates columns responsively while retaining a minimum usable tile width", () => {
    expect(calculateMasonryLayout(assets, 616).columnCount).toBe(3);
    expect(calculateMasonryLayout(assets, 1_656).columnCount).toBe(8);
    expect(calculateMasonryLayout(assets, 2_072).columnCount).toBe(10);
  });

  it("uses a stable square fallback for incomplete dimensions", () => {
    const layout = calculateMasonryLayout([{ id: "unknown", width: 0, height: 0, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false }], 616);

    expect(layout.columns[0][0].height).toBe(200);
    expect(layout.height).toBe(200);
  });
});
