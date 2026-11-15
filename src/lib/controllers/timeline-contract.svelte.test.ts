import { describe, expect, it } from "vitest";
import type { TimelineAsset, TimelineBucket } from "../contracts";
import { createAlbumTimelineController } from "./album-timeline-controller.svelte";
import { createPlaceTimelineController } from "./place-timeline-controller.svelte";
import { createArchiveTimelineController } from "./archive-timeline-controller.svelte";
import { createFavoritesTimelineController } from "./favorites-timeline-controller.svelte";
import { createPersonTimelineController } from "./person-timeline-controller.svelte";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const february = "2025-02-01T00:00:00.000Z";

function asset(id: string): TimelineAsset {
  return { id, width: 800, height: 600, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true };
}

const assetsByBucket: Record<string, TimelineAsset[]> = {
  [march]: [asset("a"), asset("b"), asset("c")],
  [february]: [asset("d")],
};
const buckets: TimelineBucket[] = Object.entries(assetsByBucket).map(([timeBucket, assets]) => ({ timeBucket, count: assets.length }));
const bucketSource = {
  getBuckets: async () => buckets,
  getBucketAssets: async (timeBucket: string) => assetsByBucket[timeBucket],
};

// Every timeline kind must satisfy the same contract; the annotation makes the compiler enforce it.
const timelines: Array<[string, () => TimelineController]> = [
  ["Photo Timeline", () => createTimelineController({
    getTimelineBuckets: bucketSource.getBuckets,
    getTimelineBucketAssets: bucketSource.getBucketAssets,
  })],
  ["Favorites Timeline", () => createFavoritesTimelineController({
    getFavoritesTimelineBuckets: bucketSource.getBuckets,
    getFavoritesTimelineBucketAssets: bucketSource.getBucketAssets,
  })],
  ["Archive Timeline", () => createArchiveTimelineController({
    getArchiveTimelineBuckets: bucketSource.getBuckets,
    getArchiveTimelineBucketAssets: bucketSource.getBucketAssets,
  })],
  ["Album Timeline", () => createAlbumTimelineController({
    getAlbumTimeline: async () => ({ buckets, assetsByBucket }),
  }, "album-1")],
  ["Place Timeline", () => createPlaceTimelineController({
    getPlaceTimeline: async () => ({ buckets, assetsByBucket }),
  }, "Paris")],
  ["Person Timeline", () => createPersonTimelineController({
    getPersonTimelineBuckets: bucketSource.getBuckets,
    getPersonTimelineBucketAssets: (_personId, timeBucket) => bucketSource.getBucketAssets(timeBucket),
  }, "person-1")],
];

for (const [name, create] of timelines) describe(`${name} removal and navigation contract`, () => {
  async function loaded() {
    const timeline = create();
    await timeline.refresh();
    for (const { timeBucket } of buckets) await timeline.activateBucket(timeBucket);
    return timeline;
  }

  it("locates an asset and removes it from its bucket", async () => {
    const timeline = await loaded();

    expect(timeline.locateAsset("b")).toEqual({ timeBucket: march, index: 1 });
    timeline.removeAsset("b");

    expect(timeline.locateAsset("b")).toBeNull();
    expect(timeline.assetsFor(march).map((candidate) => candidate.id)).toEqual(["a", "c"]);
  });

  it("steps to the neighbouring asset, crossing bucket boundaries", async () => {
    const timeline = await loaded();

    expect((await timeline.stepAsset("b", 1))?.id).toBe("c");
    expect((await timeline.stepAsset("c", 1))?.id).toBe("d");
    expect((await timeline.stepAsset("a", -1))?.id).toBe("d");
  });

  it("steps from the former position after the current asset was removed", async () => {
    const timeline = await loaded();
    const former = timeline.locateAsset("c");

    timeline.removeAsset("c");

    expect((await timeline.stepAsset("c", 1, former))?.id).toBe("d");
    expect((await timeline.stepAsset("c", -1, former))?.id).toBe("b");
  });

  it("steps from the former position, wrapping around, after removal empties the last bucket", async () => {
    const timeline = await loaded();
    const former = timeline.locateAsset("d");

    timeline.removeAsset("d");

    expect(timeline.buckets.map((bucket) => bucket.timeBucket)).toEqual([march]);
    expect((await timeline.stepAsset("d", 1, former))?.id).toBe("a");
    expect((await timeline.stepAsset("d", -1, former))?.id).toBe("c");
  });

  it("ignores removal of an asset that is not loaded", async () => {
    const timeline = await loaded();

    timeline.removeAsset("missing");

    expect(timeline.buckets).toEqual(buckets);
    expect(timeline.assetsFor(march).map((candidate) => candidate.id)).toEqual(["a", "b", "c"]);
  });
});
