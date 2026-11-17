import { flushSync } from "svelte";
import { describe, expect, it, vi } from "vitest";
import { createRemoteCollection } from "./remote-collection.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("Remote collection", () => {
  it("shows the first load as loading and a later load as refreshing", async () => {
    const first = deferred<string[]>();
    const second = deferred<string[]>();
    const collection = createRemoteCollection({ load: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise) });

    const firstRefresh = collection.refresh();
    expect(collection.isLoading).toBe(true);
    expect(collection.isRefreshing).toBe(true);
    first.resolve(["a"]);
    await firstRefresh;
    expect(collection.items).toEqual(["a"]);
    expect(collection.isLoading).toBe(false);
    expect(collection.isRefreshing).toBe(false);

    const secondRefresh = collection.refresh();
    expect(collection.isLoading).toBe(false);
    expect(collection.isRefreshing).toBe(true);
    expect(collection.items).toEqual(["a"]);
    second.resolve(["b"]);
    await secondRefresh;
    expect(collection.items).toEqual(["b"]);
    expect(collection.isRefreshing).toBe(false);
  });

  it("ignores a response that arrives after clear", async () => {
    const pending = deferred<string[]>();
    const collection = createRemoteCollection({ load: vi.fn().mockReturnValue(pending.promise) });

    const refresh = collection.refresh();
    collection.clear();
    pending.resolve(["previous account"]);
    await refresh;

    expect(collection.items).toEqual([]);
    expect(collection.isLoading).toBe(false);
    expect(collection.isRefreshing).toBe(false);
    expect(collection.error).toBeNull();
  });

  it("ignores a failure that arrives after clear", async () => {
    const pending = deferred<string[]>();
    const collection = createRemoteCollection({ load: vi.fn().mockReturnValue(pending.promise) });

    const refresh = collection.refresh();
    collection.clear();
    pending.reject("boom");
    await refresh;

    expect(collection.error).toBeNull();
    expect(collection.isOffline).toBe(false);
  });

  it("ignores a response from a request that a clear and a newer refresh superseded", async () => {
    const older = deferred<string[]>();
    const load = vi.fn().mockReturnValueOnce(older.promise).mockResolvedValueOnce(["new"]);
    const collection = createRemoteCollection({ load });

    const olderRefresh = collection.refresh();
    collection.clear();
    await collection.refresh();
    older.resolve(["stale"]);
    await olderRefresh;

    expect(collection.items).toEqual(["new"]);
    expect(collection.isRefreshing).toBe(false);
  });

  it("shares one request between concurrent refreshes", async () => {
    const pending = deferred<string[]>();
    const load = vi.fn().mockReturnValue(pending.promise);
    const collection = createRemoteCollection({ load });

    const first = collection.refresh();
    const second = collection.refresh();
    pending.resolve(["a"]);
    await Promise.all([first, second]);

    expect(load).toHaveBeenCalledOnce();
    expect(collection.items).toEqual(["a"]);
  });

  it("starts a new request once the shared one has settled", async () => {
    const load = vi.fn().mockResolvedValueOnce(["a"]).mockResolvedValueOnce(["b"]);
    const collection = createRemoteCollection({ load });

    await collection.refresh();
    await collection.refresh();

    expect(load).toHaveBeenCalledTimes(2);
    expect(collection.items).toEqual(["b"]);
  });

  it("keeps cached items and reports offline when a refresh fails", async () => {
    const load = vi.fn().mockResolvedValueOnce(["a"]).mockRejectedValueOnce("offline");
    const collection = createRemoteCollection({ load });

    await collection.refresh();
    await collection.refresh();

    expect(collection.items).toEqual(["a"]);
    expect(collection.isOffline).toBe(true);
    expect(collection.error).toBeNull();
    expect(collection.isRefreshing).toBe(false);
  });

  it("reports the failure reason as an error when nothing is cached", async () => {
    const reason = new Error("down");
    const collection = createRemoteCollection({ load: vi.fn().mockRejectedValue(reason) });

    await collection.refresh();

    expect(collection.error).toEqual({ reason });
    expect(collection.isOffline).toBe(false);
    expect(collection.isLoading).toBe(false);
  });

  it("reports an error even when the failure reason is undefined", async () => {
    const collection = createRemoteCollection({ load: vi.fn().mockRejectedValue(undefined) });

    await collection.refresh();

    expect(collection.error).not.toBeNull();
  });

  it("recovers from an error and from offline on the next successful refresh", async () => {
    const load = vi.fn()
      .mockRejectedValueOnce("down")
      .mockResolvedValueOnce(["a"])
      .mockRejectedValueOnce("down")
      .mockResolvedValueOnce(["b"]);
    const collection = createRemoteCollection({ load });

    await collection.refresh();
    expect(collection.error).not.toBeNull();
    await collection.refresh();
    expect(collection.error).toBeNull();
    await collection.refresh();
    expect(collection.isOffline).toBe(true);
    await collection.refresh();
    expect(collection.isOffline).toBe(false);
    expect(collection.items).toEqual(["b"]);
  });

  it("clears items, error and offline state", async () => {
    const load = vi.fn().mockResolvedValueOnce(["a"]).mockRejectedValueOnce("down");
    const collection = createRemoteCollection({ load });

    await collection.refresh();
    await collection.refresh();
    collection.clear();

    expect(collection.items).toEqual([]);
    expect(collection.isOffline).toBe(false);
    expect(collection.error).toBeNull();
  });

  it("lets a refresh after clear load again while an older request is still in flight", async () => {
    const older = deferred<string[]>();
    const load = vi.fn().mockReturnValueOnce(older.promise).mockResolvedValueOnce(["new"]);
    const collection = createRemoteCollection({ load });

    void collection.refresh();
    collection.clear();
    await collection.refresh();

    expect(load).toHaveBeenCalledTimes(2);
    expect(collection.items).toEqual(["new"]);
  });

  it("does not subscribe an effect that calls refresh to the collection state", async () => {
    const load = vi.fn().mockResolvedValueOnce(["a"]).mockResolvedValueOnce(["b"]);
    const collection = createRemoteCollection({ load });
    let runs = 0;
    const stop = $effect.root(() => {
      $effect(() => {
        runs += 1;
        void collection.refresh();
      });
    });

    flushSync();
    await collection.refresh();
    flushSync();

    expect(runs).toBe(1);
    stop();
  });
  it("hands out the loaded items without wrapping them", async () => {
    const loaded = [{ id: "a" }];
    const collection = createRemoteCollection({ load: vi.fn().mockResolvedValue(loaded) });

    await collection.refresh();

    expect(collection.items).toBe(loaded);
  });

  describe("paging", () => {
    it("loads the next page, appends it and stops when there is no further page", async () => {
      const load = vi.fn()
        .mockResolvedValueOnce({ items: ["a"], hasNextPage: true })
        .mockResolvedValueOnce({ items: ["b"], hasNextPage: false });
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      expect(collection.hasNextPage).toBe(true);
      await collection.loadMore();
      expect(collection.items).toEqual(["a", "b"]);
      expect(collection.hasNextPage).toBe(false);
      await collection.loadMore();

      expect(load).toHaveBeenCalledTimes(2);
      expect(load).toHaveBeenNthCalledWith(1, 1);
      expect(load).toHaveBeenNthCalledWith(2, 2);
    });

    it("has no next page for a list that loads as one array", async () => {
      const load = vi.fn().mockResolvedValue(["a"]);
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      await collection.loadMore();

      expect(collection.hasNextPage).toBe(false);
      expect(load).toHaveBeenCalledOnce();
    });

    it("restarts at the first page and replaces the items on refresh", async () => {
      const load = vi.fn()
        .mockResolvedValueOnce({ items: ["a"], hasNextPage: true })
        .mockResolvedValueOnce({ items: ["b"], hasNextPage: true })
        .mockResolvedValueOnce({ items: ["a2"], hasNextPage: false });
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      await collection.loadMore();
      await collection.refresh();

      expect(load).toHaveBeenNthCalledWith(3, 1);
      expect(collection.items).toEqual(["a2"]);
      expect(collection.hasNextPage).toBe(false);
    });

    it("does not show loading or refreshing while a further page loads", async () => {
      const next = deferred<{ items: string[]; hasNextPage: boolean }>();
      const load = vi.fn().mockResolvedValueOnce({ items: ["a"], hasNextPage: true }).mockReturnValueOnce(next.promise);
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      const more = collection.loadMore();
      expect(collection.isLoading).toBe(false);
      expect(collection.isRefreshing).toBe(false);
      next.resolve({ items: ["b"], hasNextPage: false });
      await more;
    });

    it("shares one request between a further page and a concurrent loadMore", async () => {
      const next = deferred<{ items: string[]; hasNextPage: boolean }>();
      const load = vi.fn().mockResolvedValueOnce({ items: ["a"], hasNextPage: true }).mockReturnValueOnce(next.promise);
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      const first = collection.loadMore();
      const second = collection.loadMore();
      next.resolve({ items: ["b"], hasNextPage: false });
      await Promise.all([first, second]);

      expect(load).toHaveBeenCalledTimes(2);
      expect(collection.items).toEqual(["a", "b"]);
    });

    it("lets a refresh supersede a further page that is still loading", async () => {
      const next = deferred<{ items: string[]; hasNextPage: boolean }>();
      const load = vi.fn()
        .mockResolvedValueOnce({ items: ["a"], hasNextPage: true })
        .mockReturnValueOnce(next.promise)
        .mockResolvedValueOnce({ items: ["fresh"], hasNextPage: false });
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      const more = collection.loadMore();
      await collection.refresh();
      next.resolve({ items: ["late"], hasNextPage: true });
      await more;

      expect(collection.items).toEqual(["fresh"]);
      expect(collection.hasNextPage).toBe(false);
    });

    it("keeps the loaded pages and reports offline when a further page fails, then retries that page", async () => {
      const load = vi.fn()
        .mockResolvedValueOnce({ items: ["a"], hasNextPage: true })
        .mockRejectedValueOnce("down")
        .mockResolvedValueOnce({ items: ["b"], hasNextPage: false });
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      await collection.loadMore();
      expect(collection.items).toEqual(["a"]);
      expect(collection.isOffline).toBe(true);
      expect(collection.hasNextPage).toBe(true);
      await collection.loadMore();

      expect(load).toHaveBeenNthCalledWith(3, 2);
      expect(collection.items).toEqual(["a", "b"]);
      expect(collection.isOffline).toBe(false);
    });

    it("forgets the pages on clear", async () => {
      const load = vi.fn().mockResolvedValue({ items: ["a"], hasNextPage: true });
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      collection.clear();
      expect(collection.hasNextPage).toBe(false);
      await collection.refresh();

      expect(load).toHaveBeenLastCalledWith(1);
    });
  });

  describe("abandoning", () => {
    it("keeps the cached items and ignores a response that arrives after abandon", async () => {
      const older = deferred<string[]>();
      const load = vi.fn().mockResolvedValueOnce(["saved"]).mockReturnValueOnce(older.promise).mockResolvedValueOnce(["fresh"]);
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      const abandoned = collection.refresh();
      collection.abandon();
      older.resolve(["obsolete"]);
      await abandoned;

      expect(collection.items).toEqual(["saved"]);
      expect(collection.isRefreshing).toBe(false);
      expect(collection.isLoading).toBe(false);
      await collection.refresh();
      expect(collection.items).toEqual(["fresh"]);
    });

    it("ignores a failure that arrives after abandon", async () => {
      const older = deferred<string[]>();
      const collection = createRemoteCollection({ load: vi.fn().mockResolvedValueOnce(["saved"]).mockReturnValueOnce(older.promise) });

      await collection.refresh();
      const abandoned = collection.refresh();
      collection.abandon();
      older.reject("boom");
      await abandoned;

      expect(collection.isOffline).toBe(false);
      expect(collection.failure).toBeNull();
    });

    it("abandons a further page that is still loading", async () => {
      const next = deferred<{ items: string[]; hasNextPage: boolean }>();
      const load = vi.fn().mockResolvedValueOnce({ items: ["a"], hasNextPage: true }).mockReturnValueOnce(next.promise);
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      const more = collection.loadMore();
      collection.abandon();
      next.resolve({ items: ["b"], hasNextPage: false });
      await more;

      expect(collection.items).toEqual(["a"]);
      expect(collection.hasNextPage).toBe(true);
    });
  });

  describe("failure reason", () => {
    it("keeps the reason of a failed refresh while cached items stay visible", async () => {
      const reason = new Error("denied");
      const collection = createRemoteCollection({ load: vi.fn().mockResolvedValueOnce(["a"]).mockRejectedValueOnce(reason) });

      await collection.refresh();
      expect(collection.failure).toBeNull();
      await collection.refresh();

      expect(collection.error).toBeNull();
      expect(collection.failure).toEqual({ reason });
    });

    it("reports the reason as the error when nothing is cached", async () => {
      const reason = new Error("down");
      const collection = createRemoteCollection({ load: vi.fn().mockRejectedValue(reason) });

      await collection.refresh();

      expect(collection.failure).toEqual({ reason });
      expect(collection.error).toEqual({ reason });
    });

    it("drops the reason on the next successful refresh and on clear", async () => {
      const load = vi.fn().mockRejectedValueOnce("down").mockResolvedValueOnce(["a"]).mockRejectedValueOnce("down");
      const collection = createRemoteCollection({ load });

      await collection.refresh();
      await collection.refresh();
      expect(collection.failure).toBeNull();
      await collection.refresh();
      expect(collection.failure).not.toBeNull();
      collection.clear();
      expect(collection.failure).toBeNull();
    });
  });

  describe("has loaded", () => {
    it("is true once a load succeeded, even with an empty list, until clear", async () => {
      const load = vi.fn().mockRejectedValueOnce("down").mockResolvedValueOnce([]);
      const collection = createRemoteCollection<string>({ load });

      expect(collection.hasLoaded).toBe(false);
      await collection.refresh();
      expect(collection.hasLoaded).toBe(false);
      await collection.refresh();
      expect(collection.hasLoaded).toBe(true);
      collection.clear();
      expect(collection.hasLoaded).toBe(false);
    });
  });
});
