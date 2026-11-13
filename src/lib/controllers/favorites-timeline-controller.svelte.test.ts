import { describe, expect, it, vi } from "vitest";
import type { FavoritesTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createFavoritesTimelineController } from "./favorites-timeline-controller.svelte";

describe("Favorites Timeline controller", () => {
  it("loads favorite month buckets in descending order and fetches bucket assets on activation", async () => {
    const adapter: FavoritesTimelineDesktopAdapter = {
      getFavoritesTimelineBuckets: vi.fn().mockResolvedValue([
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
      ]),
      getFavoritesTimelineBucketAssets: vi.fn().mockResolvedValue([
        { id: "fav-photo", width: 800, height: 600, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", fileSize: null, duration: null, isFavorite: true },
        { id: "fav-video", width: 1920, height: 1080, type: "VIDEO", takenAt: "2025-03-02T12:00:00.000Z", fileSize: null, duration: 30, isFavorite: true },
      ]),
    };
    const favorites = createFavoritesTimelineController(adapter);

    await favorites.refresh();
    await favorites.activateBucket("2025-03-01T00:00:00.000Z");

    expect(adapter.getFavoritesTimelineBuckets).toHaveBeenCalledTimes(1);
    expect(adapter.getFavoritesTimelineBucketAssets).toHaveBeenCalledWith("2025-03-01T00:00:00.000Z");
    expect(favorites.buckets).toEqual([
      { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
      { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
    ]);
    expect(favorites.assetsFor("2025-03-01T00:00:00.000Z").map((asset) => asset.id)).toEqual(["fav-photo", "fav-video"]);
  });

  it("distinguishes initial load error from empty favorites and keeps cached buckets when re-entry refresh fails", async () => {
    const adapter: FavoritesTimelineDesktopAdapter = {
      getFavoritesTimelineBuckets: vi.fn()
        .mockRejectedValueOnce(new Error("offline"))
        .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }])
        .mockRejectedValueOnce(new Error("offline")),
      getFavoritesTimelineBucketAssets: vi.fn(),
    };
    const favorites = createFavoritesTimelineController(adapter);

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(true);
    expect(favorites.buckets).toEqual([]);

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(false);
    expect(favorites.isOffline).toBe(false);
    expect(favorites.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]);

    await favorites.refresh();
    expect(favorites.hasInitialLoadError).toBe(false);
    expect(favorites.isOffline).toBe(true);
    expect(favorites.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]);
  });
});

