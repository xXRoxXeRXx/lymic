import { describe, expect, it, vi } from "vitest";
import type { SearchSupport, SearchTimeline } from "../contracts";
import { createSearchController } from "./search-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const UNSUPPORTED: SearchSupport = { supported: false, version: "3.1.0", requiredVersion: "3.2" };

function asset(id: string) {
  return { id, width: 640, height: 480, type: "IMAGE" as const, takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false };
}

function results(ids: string[], truncated = false): SearchTimeline {
  return { buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(asset) }, truncated };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function setup(overrides: Partial<Parameters<typeof createSearchController>[0]> = {}) {
  const adapter = {
    getSearchSupport: vi.fn().mockResolvedValue(SUPPORTED),
    searchAssets: vi.fn().mockResolvedValue(results(["a", "b"])),
    ...overrides,
  };
  return { adapter, search: createSearchController(adapter) };
}

describe("Search controller", () => {
  it("runs a Filename Search with the trimmed text and shows the results chronologically", async () => {
    const { adapter, search } = setup();
    search.text = "  IMG_  ";

    expect(search.canSubmit).toBe(true);
    expect(await search.submit()).toBe(true);

    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "IMG_", mode: "filename", filters: {} });
    expect(search.buckets).toEqual([{ timeBucket: march, count: 2 }]);
    await search.activateBucket(march);
    expect(search.assetsFor(march).map((candidate) => candidate.id)).toEqual(["a", "b"]);
    expect(search.resultCount).toBe(2);
    expect(search.failure).toBeNull();
  });

  it("does nothing for an empty text", async () => {
    const { adapter, search } = setup();
    search.text = "   ";

    expect(search.canSubmit).toBe(false);
    expect(await search.submit()).toBe(false);

    expect(adapter.getSearchSupport).not.toHaveBeenCalled();
    expect(adapter.searchAssets).not.toHaveBeenCalled();
  });

  it("reads the server support once and reuses it for later searches", async () => {
    const { adapter, search } = setup();
    search.text = "a";
    await search.submit();
    search.text = "b";
    await search.submit();

    expect(adapter.getSearchSupport).toHaveBeenCalledTimes(1);
    expect(adapter.searchAssets).toHaveBeenCalledTimes(2);
  });

  it("reports an unsupported server without searching, and checks again on the next Search", async () => {
    const { adapter, search } = setup({ getSearchSupport: vi.fn().mockResolvedValueOnce(UNSUPPORTED).mockResolvedValue(SUPPORTED) });
    search.text = "a";

    await search.submit();

    expect(search.unsupported).toEqual({ version: "3.1.0", requiredVersion: "3.2" });
    expect(adapter.searchAssets).not.toHaveBeenCalled();
    expect(search.failure).toBeNull();

    await search.submit();

    expect(search.unsupported).toBeNull();
    expect(adapter.searchAssets).toHaveBeenCalledTimes(1);
  });

  it("flags a Search that was cut at the result limit and clears the flag for the next one", async () => {
    const { search } = setup({ searchAssets: vi.fn().mockResolvedValueOnce(results(["a"], true)).mockResolvedValue(results(["b"])) });
    search.text = "a";

    await search.submit();
    expect(search.truncated).toBe(true);

    await search.submit();
    expect(search.truncated).toBe(false);
  });

  it.each([
    ["search_authentication_required", "authentication_required"],
    ["search_permission_denied:asset.read", "permission_denied"],
    ["search_unavailable", "unavailable"],
    ["search_invalid_response", "error"],
  ] as const)("classifies the failure %s as %s", async (code, expected) => {
    const { search } = setup({ searchAssets: vi.fn().mockRejectedValue(code) });
    search.text = "a";

    await search.submit();

    expect(search.failure).toBe(expected);
    expect(search.hasInitialLoadError).toBe(true);
    expect(search.text).toBe("a");
  });

  it("classifies a failure to read the server version like a failed Search", async () => {
    const { search } = setup({ getSearchSupport: vi.fn().mockRejectedValue("search_unavailable") });
    search.text = "a";

    await search.submit();

    expect(search.failure).toBe("unavailable");
  });

  it("keeps the previous text and results visible when a later Search fails, and recovers on Retry", async () => {
    const searchAssets = vi.fn()
      .mockResolvedValueOnce(results(["old"]))
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValueOnce(results(["new"]));
    const { search } = setup({ searchAssets });
    search.text = "first";
    await search.submit();

    search.text = "second";
    await search.submit();

    expect(search.failure).toBe("unavailable");
    expect(search.isOffline).toBe(true);
    expect(search.hasInitialLoadError).toBe(false);
    expect(search.submittedText).toBe("first");
    expect(search.buckets).toEqual([{ timeBucket: march, count: 1 }]);
    await search.activateBucket(march);
    expect(search.assetsFor(march).map((candidate) => candidate.id)).toEqual(["old"]);

    await search.refresh();

    expect(searchAssets).toHaveBeenLastCalledWith({ text: "second", mode: "filename", filters: {} });
    expect(search.failure).toBeNull();
    expect(search.isOffline).toBe(false);
    expect(search.submittedText).toBe("second");
    await search.activateBucket(march);
    expect(search.assetsFor(march).map((candidate) => candidate.id)).toEqual(["new"]);
  });

  it("keeps text and mode in memory and resets everything on clear", async () => {
    const { search } = setup();
    search.text = "IMG";
    await search.submit();

    search.clear();

    expect(search.text).toBe("");
    expect(search.mode).toBe("filename");
    expect(search.submittedText).toBe("");
    expect(search.buckets).toEqual([]);
    expect(search.truncated).toBe(false);
    expect(search.unsupported).toBeNull();
    expect(search.failure).toBeNull();
  });

  it("ignores a Search that resolves after clear so the next account does not inherit it", async () => {
    const inFlight = deferred<SearchTimeline>();
    const { search } = setup({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
    search.text = "IMG";

    const running = search.submit();
    await vi.waitFor(() => expect(search.isLoading).toBe(true));
    search.clear();
    inFlight.resolve(results(["late"], true));
    await running;

    expect(search.buckets).toEqual([]);
    expect(search.truncated).toBe(false);
    expect(search.submittedText).toBe("");
  });

  it("ignores a failure that rejects after clear", async () => {
    const inFlight = deferred<SearchTimeline>();
    const { search } = setup({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
    search.text = "IMG";

    const running = search.submit();
    await vi.waitFor(() => expect(search.isLoading).toBe(true));
    search.clear();
    inFlight.reject("search_permission_denied:asset.read");
    await running;

    expect(search.failure).toBeNull();
  });
});
