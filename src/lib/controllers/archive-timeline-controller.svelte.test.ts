import { describe, expect, it, vi } from "vitest";
import { createArchiveTimelineController } from "./archive-timeline-controller.svelte";

const PERMISSION_DENIED = "archive_permission_denied:asset.read";
const march = "2025-03-01T00:00:00.000Z";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("Archive Timeline controller", () => {
  it("reports permissionDenied for a 403 and resets it after a successful refresh", async () => {
    const getArchiveTimelineBuckets = vi.fn()
      .mockRejectedValueOnce(PERMISSION_DENIED)
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    const archive = createArchiveTimelineController({ getArchiveTimelineBuckets, getArchiveTimelineBucketAssets: vi.fn() });

    await archive.refresh();
    expect(archive.permissionDenied).toBe(true);

    await archive.refresh();
    expect(archive.permissionDenied).toBe(false);
  });

  it("clears permissionDenied on clear()", async () => {
    const archive = createArchiveTimelineController({
      getArchiveTimelineBuckets: vi.fn().mockRejectedValue(PERMISSION_DENIED),
      getArchiveTimelineBucketAssets: vi.fn(),
    });
    await archive.refresh();
    expect(archive.permissionDenied).toBe(true);

    archive.clear();
    expect(archive.permissionDenied).toBe(false);
  });

  it("ignores a permission denial that rejects after clear() so the next account does not inherit it", async () => {
    const inFlight = deferred<never>();
    const archive = createArchiveTimelineController({
      getArchiveTimelineBuckets: () => inFlight.promise,
      getArchiveTimelineBucketAssets: vi.fn(),
    });

    const refresh = archive.refresh();
    archive.clear();
    inFlight.reject(PERMISSION_DENIED);
    await refresh;

    expect(archive.permissionDenied).toBe(false);
  });

  it("ignores a bucket-assets denial that rejects after clear()", async () => {
    const inFlight = deferred<never>();
    const archive = createArchiveTimelineController({
      getArchiveTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: march, count: 1 }]),
      getArchiveTimelineBucketAssets: () => inFlight.promise,
    });
    await archive.refresh();

    const activation = archive.activateBucket(march).catch(() => {});
    archive.clear();
    inFlight.reject(PERMISSION_DENIED);
    await activation;

    expect(archive.permissionDenied).toBe(false);
  });
});
