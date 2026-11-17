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
});
