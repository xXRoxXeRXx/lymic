import { describe, expect, it, vi } from "vitest";
import type { Memory } from "../contracts";
import type { MemoriesDesktopAdapter } from "../tauri/desktop-adapter";
import { createMemoriesController, MEMORIES_SECTION_SIZE } from "./memories-controller.svelte";

function memory(id: string): Memory {
  return { id, kind: "onThisDay", memoryAt: "2020-11-15T00:00:00.000Z", year: 2020, personName: null, coverAssetId: `${id}-cover`, timeline: { buckets: [], assetsByBucket: {} } };
}

describe("Memories controller", () => {
  it("loads the newest twelve Memories", async () => {
    const adapter: MemoriesDesktopAdapter = { getMemories: vi.fn().mockResolvedValue([memory("m-1")]) };
    const memories = createMemoriesController(adapter);

    await memories.refresh();

    expect(adapter.getMemories).toHaveBeenCalledWith(12);
    expect(MEMORIES_SECTION_SIZE).toBe(12);
    expect(memories.memories.map((item) => item.id)).toEqual(["m-1"]);
    expect(memories.error).toBeNull();
    expect(memories.isLoading).toBe(false);
  });

  it("reports a permission failure naming the permission as unavailable and any other failure as an error", async () => {
    const getMemories = vi.fn()
      .mockRejectedValueOnce("memories_permission_denied:configure an API key with memory.read and asset.read permissions")
      .mockRejectedValueOnce("memories_unavailable")
      .mockRejectedValueOnce(new Error("boom"));
    const memories = createMemoriesController({ getMemories });

    await memories.refresh();
    expect(memories.error).toBe("unavailable");
    await memories.refresh();
    expect(memories.error).toBe("error");
    await memories.refresh();
    expect(memories.error).toBe("error");
    expect(memories.memories).toEqual([]);
  });

  it("keeps cached Memories with an offline indication when a refresh fails, and recovers", async () => {
    const getMemories = vi.fn()
      .mockResolvedValueOnce([memory("m-1")])
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce([memory("m-2")]);
    const memories = createMemoriesController({ getMemories });

    await memories.refresh();
    await memories.refresh();
    expect(memories.isOffline).toBe(true);
    expect(memories.error).toBeNull();
    expect(memories.memories.map((item) => item.id)).toEqual(["m-1"]);

    await memories.refresh();
    expect(memories.isOffline).toBe(false);
    expect(memories.memories.map((item) => item.id)).toEqual(["m-2"]);
  });

  it("ignores a response that arrives after the controller was cleared", async () => {
    let resolve!: (value: Memory[]) => void;
    const memories = createMemoriesController({ getMemories: () => new Promise<Memory[]>((done) => { resolve = done; }) });

    const pending = memories.refresh();
    memories.clear();
    resolve([memory("stale")]);
    await pending;

    expect(memories.memories).toEqual([]);
    expect(memories.isLoading).toBe(false);
  });

  it("opens a Memory Timeline over exactly the Memory's grouped assets", async () => {
    const { createMemoryTimelineController } = await import("./memory-timeline-controller.svelte");
    const asset = { id: "a-1", width: 1, height: 1, type: "IMAGE" as const, takenAt: "2020-11-15T10:00:00.000Z", fileSize: null, duration: null, isFavorite: false };
    const timeline = createMemoryTimelineController({ ...memory("m-1"), timeline: { buckets: [{ timeBucket: "2020-11-01T00:00:00.000Z", count: 1 }], assetsByBucket: { "2020-11-01T00:00:00.000Z": [asset] } } });

    await timeline.refresh();
    await timeline.activateBucket("2020-11-01T00:00:00.000Z");

    expect(timeline.buckets).toEqual([{ timeBucket: "2020-11-01T00:00:00.000Z", count: 1 }]);
    expect(timeline.assetsFor("2020-11-01T00:00:00.000Z")).toEqual([asset]);
  });
});
