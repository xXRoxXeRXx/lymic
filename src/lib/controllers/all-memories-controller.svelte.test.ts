import { describe, expect, it, vi } from "vitest";
import type { MemoriesPage, Memory } from "../contracts";
import { ALL_MEMORIES_PAGE_SIZE, createAllMemoriesController } from "./all-memories-controller.svelte";

function memory(id: string): Memory {
  return { id, kind: "onThisDay", memoryAt: "2020-11-15T00:00:00.000Z", year: 2020, personName: null, coverAssetId: `${id}-cover`, timeline: { buckets: [], assetsByBucket: {} } };
}

describe("All Memories controller", () => {
  it("appends pages in order and stops requesting once Immich has no more", async () => {
    const getMemoriesPage = vi.fn()
      .mockResolvedValueOnce({ memories: [memory("a"), memory("b")], hasNextPage: true })
      .mockResolvedValueOnce({ memories: [memory("c")], hasNextPage: false });
    const all = createAllMemoriesController({ getMemoriesPage });

    await all.refresh();
    await all.loadMore();
    await all.loadMore();

    expect(getMemoriesPage.mock.calls).toEqual([[1, ALL_MEMORIES_PAGE_SIZE], [2, ALL_MEMORIES_PAGE_SIZE]]);
    expect(all.memories.map((item) => item.id)).toEqual(["a", "b", "c"]);
    expect(all.hasNextPage).toBe(false);
  });

  it("requests a page once while it is in flight and ignores Memories it already shows", async () => {
    let resolveSecond!: (page: MemoriesPage) => void;
    const getMemoriesPage = vi.fn()
      .mockResolvedValueOnce({ memories: [memory("a")], hasNextPage: true })
      .mockReturnValueOnce(new Promise<MemoriesPage>((resolve) => { resolveSecond = resolve; }));
    const all = createAllMemoriesController({ getMemoriesPage });
    await all.refresh();

    const first = all.loadMore();
    const second = all.loadMore();
    resolveSecond({ memories: [memory("a"), memory("b")], hasNextPage: false });
    await Promise.all([first, second]);

    expect(getMemoriesPage).toHaveBeenCalledTimes(2);
    expect(all.memories.map((item) => item.id)).toEqual(["a", "b"]);
  });

  it("keeps earlier pages when a further page fails and retries that same page", async () => {
    const getMemoriesPage = vi.fn()
      .mockResolvedValueOnce({ memories: [memory("a")], hasNextPage: true })
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce({ memories: [memory("b")], hasNextPage: false });
    const all = createAllMemoriesController({ getMemoriesPage });
    await all.refresh();

    await all.loadMore();
    expect(all.loadMoreFailed).toBe(true);
    expect(all.error).toBeNull();
    expect(all.memories.map((item) => item.id)).toEqual(["a"]);

    await all.loadMore();
    expect(getMemoriesPage.mock.calls[2]).toEqual([2, ALL_MEMORIES_PAGE_SIZE]);
    expect(all.loadMoreFailed).toBe(false);
    expect(all.memories.map((item) => item.id)).toEqual(["a", "b"]);
  });

  it("discards a late response after clear", async () => {
    let resolve!: (page: MemoriesPage) => void;
    const all = createAllMemoriesController({ getMemoriesPage: () => new Promise<MemoriesPage>((done) => { resolve = done; }) });
    const loading = all.refresh();
    all.clear();
    resolve({ memories: [memory("a")], hasNextPage: true });
    await loading;

    expect(all.memories).toEqual([]);
    expect(all.hasNextPage).toBe(false);
  });
});
