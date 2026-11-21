import { describe, expect, it, vi } from "vitest";
import type { AssetDetails } from "../contracts";
import { createRemoteAssetActions, maxCachedDetails } from "./remote-asset-actions.svelte";

function details(id: string, overrides: Partial<AssetDetails> = {}): AssetDetails {
  return { originalFileName: `${id}.jpg`, isFavorite: false, ...overrides } as AssetDetails;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function createActions(options: {
  getAssetDetails?: (assetId: string) => Promise<AssetDetails | null>;
  chooseDownloadDestination?: (name: string) => Promise<string | null>;
  downloadAsset?: (assetId: string, destination: string) => Promise<void>;
  downloadArchive?: (assetIds: string[]) => Promise<void>;
} = {}) {
  return createRemoteAssetActions({
    getAssetDetails: options.getAssetDetails ?? (async (id) => details(id)),
    chooseDownloadDestination: options.chooseDownloadDestination ?? (async (name) => `/downloads/${name}`),
    downloadAsset: options.downloadAsset ?? (async () => {}),
    downloadArchive: options.downloadArchive ?? (async () => {}),
  });
}

describe("Remote Asset details", () => {
  it("requests details once for repeated and concurrent loads", async () => {
    const getAssetDetails = vi.fn(async (id: string) => details(id));
    const actions = createActions({ getAssetDetails });

    expect(actions.cachedDetails("a")).toBeUndefined();
    const [first, concurrent] = await Promise.all([actions.loadDetails("a"), actions.loadDetails("a")]);
    const again = await actions.loadDetails("a");

    expect(getAssetDetails).toHaveBeenCalledTimes(1);
    expect(first).toEqual({ details: details("a"), stale: false });
    expect(concurrent).toEqual(first);
    expect(again).toEqual(first);
    expect(actions.cachedDetails("a")).toEqual(details("a"));
  });

  it("caches a missing result and distinguishes it from an uncached asset", async () => {
    const getAssetDetails = vi.fn(async () => null);
    const actions = createActions({ getAssetDetails });

    await actions.loadDetails("gone");
    await actions.loadDetails("gone");

    expect(getAssetDetails).toHaveBeenCalledTimes(1);
    expect(actions.cachedDetails("gone")).toBeNull();
  });

  it("does not cache a failed request and lets the next load retry", async () => {
    const getAssetDetails = vi.fn<(id: string) => Promise<AssetDetails | null>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(details("a"));
    const actions = createActions({ getAssetDetails });

    await expect(actions.loadDetails("a")).rejects.toThrow("offline");
    expect(actions.cachedDetails("a")).toBeUndefined();
    expect((await actions.loadDetails("a")).details).toEqual(details("a"));
  });

  it("evicts the oldest entry beyond the cache limit", async () => {
    const actions = createActions();
    for (let index = 0; index <= maxCachedDetails; index += 1) await actions.loadDetails(`asset-${index}`);

    expect(actions.cachedDetails("asset-0")).toBeUndefined();
    expect(actions.cachedDetails("asset-1")).toBeDefined();
    expect(actions.cachedDetails(`asset-${maxCachedDetails}`)).toBeDefined();
  });

  it("drops results that resolve after invalidate and reports them as stale", async () => {
    const request = deferred<AssetDetails | null>();
    const getAssetDetails = vi.fn(() => request.promise);
    const actions = createActions({ getAssetDetails });

    const generation = actions.generation;
    const loading = actions.loadDetails("a");
    actions.invalidate();
    request.resolve(details("a"));

    expect((await loading).stale).toBe(true);
    expect(actions.cachedDetails("a")).toBeUndefined();
    expect(actions.generation).toBe(generation + 1);
  });

  it("reports a failure after invalidate as stale instead of rejecting", async () => {
    const request = deferred<AssetDetails | null>();
    const actions = createActions({ getAssetDetails: () => request.promise });

    const loading = actions.loadDetails("a");
    actions.invalidate();
    request.reject(new Error("offline"));

    expect((await loading).stale).toBe(true);
  });

  it("starts a fresh request after invalidate rather than joining the stale one", async () => {
    const stale = deferred<AssetDetails | null>();
    const getAssetDetails = vi.fn<(id: string) => Promise<AssetDetails | null>>()
      .mockReturnValueOnce(stale.promise)
      .mockResolvedValueOnce(details("a", { originalFileName: "fresh.jpg" }));
    const actions = createActions({ getAssetDetails });

    const first = actions.loadDetails("a");
    actions.invalidate();
    const second = await actions.loadDetails("a");
    stale.resolve(details("a", { originalFileName: "stale.jpg" }));

    expect(second.details?.originalFileName).toBe("fresh.jpg");
    expect((await first).stale).toBe(true);
    expect(actions.cachedDetails("a")?.originalFileName).toBe("fresh.jpg");
  });

  it("clears the cache on invalidate", async () => {
    const actions = createActions();
    await actions.loadDetails("a");
    actions.invalidate();
    expect(actions.cachedDetails("a")).toBeUndefined();
  });
});

describe("Remote Asset favorite status", () => {
  it("patches cached details and overrides the asset's favorite status", async () => {
    const actions = createActions();
    await actions.loadDetails("a");

    actions.reflectFavorite("a", true);

    expect(actions.cachedDetails("a")?.isFavorite).toBe(true);
    expect(actions.isFavorite({ id: "a", isFavorite: false })).toBe(true);
    expect(actions.isFavorite({ id: "other", isFavorite: false })).toBe(false);
  });

  it("applies a favorite change made while details were loading", async () => {
    const request = deferred<AssetDetails | null>();
    const actions = createActions({ getAssetDetails: () => request.promise });

    const loading = actions.loadDetails("a");
    actions.reflectFavorite("a", true);
    request.resolve(details("a", { isFavorite: false }));

    expect((await loading).details?.isFavorite).toBe(true);
    expect(actions.cachedDetails("a")?.isFavorite).toBe(true);
  });

  it("reflects a visibility change in cached and later-loaded details until the next invalidate", async () => {
    const request = deferred<AssetDetails | null>();
    const actions = createActions({ getAssetDetails: () => request.promise });
    const loading = actions.loadDetails("a");

    actions.reflectVisibility("a", "archive");
    request.resolve(details("a", { visibility: "timeline" }));

    expect((await loading).details?.visibility).toBe("archive");
    actions.reflectVisibility("a", "timeline");
    expect(actions.cachedDetails("a")?.visibility).toBe("timeline");

    actions.invalidate();
    expect(actions.cachedDetails("a")).toBeUndefined();
  });

  it("forgets overrides on invalidate except for assets still being updated", async () => {
    const actions = createActions();
    actions.reflectFavorite("done", true);
    actions.reflectFavorite("updating", true);

    actions.invalidate(["updating"]);

    expect(actions.isFavorite({ id: "done", isFavorite: false })).toBe(false);
    expect(actions.isFavorite({ id: "updating", isFavorite: false })).toBe(true);
    expect((await actions.loadDetails("updating")).details?.isFavorite).toBe(true);
  });
});

describe("Remote Asset Download", () => {
  it("saves one asset's original file to the chosen destination", async () => {
    const chooseDownloadDestination = vi.fn(async (name: string) => `/downloads/${name}`);
    const downloadAsset = vi.fn(async () => {});
    const downloadArchive = vi.fn(async () => {});
    const actions = createActions({ chooseDownloadDestination, downloadAsset, downloadArchive });

    await expect(actions.download(["a"])).resolves.toBe("saved");

    expect(chooseDownloadDestination).toHaveBeenCalledWith("a.jpg");
    expect(downloadAsset).toHaveBeenCalledWith("a", "/downloads/a.jpg");
    expect(downloadArchive).not.toHaveBeenCalled();
  });

  it("uses cached details instead of requesting them again", async () => {
    const getAssetDetails = vi.fn(async (id: string) => details(id));
    const actions = createActions({ getAssetDetails });
    await actions.loadDetails("a");

    await actions.download(["a"]);

    expect(getAssetDetails).toHaveBeenCalledTimes(1);
  });

  it("downloads several assets as one archive without choosing a destination itself", async () => {
    const chooseDownloadDestination = vi.fn(async () => "/unused");
    const downloadAsset = vi.fn(async () => {});
    const downloadArchive = vi.fn(async () => {});
    const actions = createActions({ chooseDownloadDestination, downloadAsset, downloadArchive });

    await expect(actions.download(["a", "b"])).resolves.toBe("saved");

    expect(downloadArchive).toHaveBeenCalledWith(["a", "b"]);
    expect(chooseDownloadDestination).not.toHaveBeenCalled();
    expect(downloadAsset).not.toHaveBeenCalled();
  });

  it("reports a cancelled destination choice without downloading", async () => {
    const downloadAsset = vi.fn(async () => {});
    const actions = createActions({ chooseDownloadDestination: async () => null, downloadAsset });

    await expect(actions.download(["a"])).resolves.toBe("cancelled");
    expect(downloadAsset).not.toHaveBeenCalled();
  });

  it("fails when the original file name is unavailable", async () => {
    const chooseDownloadDestination = vi.fn(async () => "/unused");
    for (const missing of [null, details("a", { originalFileName: "" })]) {
      const actions = createActions({ getAssetDetails: async () => missing, chooseDownloadDestination });
      await expect(actions.download(["a"])).rejects.toThrow("Original file name is unavailable");
    }
    expect(chooseDownloadDestination).not.toHaveBeenCalled();
  });

  it("propagates details, single-asset and archive failures", async () => {
    await expect(createActions({ getAssetDetails: async () => { throw new Error("details"); } }).download(["a"]))
      .rejects.toThrow("details");
    await expect(createActions({ downloadAsset: async () => { throw new Error("save"); } }).download(["a"]))
      .rejects.toThrow("save");
    await expect(createActions({ downloadArchive: async () => { throw new Error("zip"); } }).download(["a", "b"]))
      .rejects.toThrow("zip");
  });

  it("retries details that were invalidated mid-request instead of failing the Download", async () => {
    const first = deferred<AssetDetails | null>();
    const getAssetDetails = vi.fn<(id: string) => Promise<AssetDetails | null>>()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(details("a"));
    const downloadAsset = vi.fn(async () => {});
    const actions = createActions({ getAssetDetails, downloadAsset });

    const downloading = actions.download(["a"]);
    actions.invalidate();
    first.reject(new Error("superseded"));

    await expect(downloading).resolves.toBe("saved");
    expect(downloadAsset).toHaveBeenCalledWith("a", "/downloads/a.jpg");
  });

  it("keeps retrying details while they are repeatedly invalidated mid-request", async () => {
    const first = deferred<AssetDetails | null>();
    const second = deferred<AssetDetails | null>();
    const getAssetDetails = vi.fn<(id: string) => Promise<AssetDetails | null>>()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockResolvedValueOnce(details("a"));
    const downloadAsset = vi.fn(async () => {});
    const actions = createActions({ getAssetDetails, downloadAsset });

    const downloading = actions.download(["a"]);
    actions.invalidate();
    first.reject(new Error("superseded"));
    await vi.waitFor(() => expect(getAssetDetails).toHaveBeenCalledTimes(2));
    actions.invalidate();
    second.reject(new Error("superseded again"));

    await expect(downloading).resolves.toBe("saved");
    expect(downloadAsset).toHaveBeenCalledWith("a", "/downloads/a.jpg");
  });

  it("stops with a stale result when the caller's context ends mid-download", async () => {
    const details$ = deferred<AssetDetails | null>();
    const downloadAsset = vi.fn(async () => {});
    const chooseDownloadDestination = vi.fn(async () => "/downloads/a.jpg");
    const actions = createActions({ getAssetDetails: () => details$.promise, downloadAsset, chooseDownloadDestination });
    let current = true;

    const downloading = actions.download(["a"], () => current);
    current = false;
    details$.resolve(details("a"));

    await expect(downloading).resolves.toBe("stale");
    expect(chooseDownloadDestination).not.toHaveBeenCalled();
    expect(downloadAsset).not.toHaveBeenCalled();
  });

  it("does not report an archive as saved when the caller's context ended during it", async () => {
    const archive = deferred<void>();
    const actions = createActions({ downloadArchive: () => archive.promise });
    let current = true;

    const downloading = actions.download(["a", "b"], () => current);
    current = false;
    archive.resolve();

    await expect(downloading).resolves.toBe("stale");
  });
});
