import { describe, expect, it, vi } from "vitest";
import type { SearchFeatures, SearchSupport, SearchResults } from "../contracts";
import { createSearchController } from "./search-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };
const UNSUPPORTED: SearchSupport = { supported: false, version: "3.1.0", requiredVersion: "3.2" };

function asset(id: string) {
  return { id, width: 640, height: 480, type: "IMAGE" as const, takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false };
}

function results(ids: string[], truncated = false): SearchResults {
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
    getSearchFeatures: vi.fn().mockResolvedValue(FEATURES),
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
    const inFlight = deferred<SearchResults>();
    const { adapter, search } = setup({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
    search.text = "IMG";

    const running = search.submit();
    await vi.waitFor(() => expect(adapter.searchAssets).toHaveBeenCalled());
    search.clear();
    inFlight.resolve(results(["late"], true));
    await running;

    expect(search.buckets).toEqual([]);
    expect(search.truncated).toBe(false);
    expect(search.submittedText).toBe("");
  });

  it("ignores a failure that rejects after clear", async () => {
    const inFlight = deferred<SearchResults>();
    const { adapter, search } = setup({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
    search.text = "IMG";

    const running = search.submit();
    await vi.waitFor(() => expect(adapter.searchAssets).toHaveBeenCalled());
    search.clear();
    inFlight.reject("search_permission_denied:asset.read");
    await running;

    expect(search.failure).toBeNull();
  });
});

describe("Search Mode", () => {
  it("starts on Filename and sends the chosen mode with the Search", async () => {
    const { adapter, search } = setup();
    expect(search.mode).toBe("filename");

    for (const mode of ["description", "folderPath", "ocr", "filename"] as const) {
      search.mode = mode;
      search.text = "term";
      await search.submit();
      expect(adapter.searchAssets).toHaveBeenLastCalledWith({ text: "term", mode, filters: {} });
    }
  });

  it("keeps the chosen mode for later Searches in the session", async () => {
    const { adapter, search } = setup();
    search.mode = "folderPath";
    search.text = "a";
    await search.submit();
    search.text = "b";
    await search.submit();

    expect(search.mode).toBe("folderPath");
    expect(adapter.searchAssets).toHaveBeenLastCalledWith({ text: "b", mode: "folderPath", filters: {} });
  });

  it("resets the mode and the held features on clear", async () => {
    const { adapter, search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    await search.loadFeatures();
    search.mode = "description";
    expect(search.ocrAvailable).toBe(false);

    search.clear();

    expect(search.mode).toBe("filename");
    expect(search.ocrAvailable).toBe(true);
    await search.loadFeatures();
    expect(adapter.getSearchFeatures).toHaveBeenCalledTimes(2);
  });
});

describe("Search server features", () => {
  it("reads the features once, before the first Search, and holds them in memory", async () => {
    const calls: string[] = [];
    const { adapter, search } = setup({
      getSearchFeatures: vi.fn().mockImplementation(async () => { calls.push("features"); return FEATURES; }),
      searchAssets: vi.fn().mockImplementation(async () => { calls.push("search"); return results(["a"]); }),
    });
    search.text = "a";
    await search.submit();
    search.text = "b";
    await search.submit();
    await search.loadFeatures();

    expect(calls).toEqual(["features", "search", "search"]);
    expect(adapter.getSearchFeatures).toHaveBeenCalledTimes(1);
  });

  it("shares one read between concurrent callers", async () => {
    const inFlight = deferred<SearchFeatures>();
    const { adapter, search } = setup({ getSearchFeatures: vi.fn().mockReturnValue(inFlight.promise) });

    const first = search.loadFeatures();
    const second = search.loadFeatures();
    inFlight.resolve(FEATURES);
    await Promise.all([first, second]);

    expect(adapter.getSearchFeatures).toHaveBeenCalledTimes(1);
  });

  it("enables OCR while the server has it on, and while the features are not known yet", async () => {
    const { search } = setup();
    expect(search.ocrAvailable).toBe(true);

    await search.loadFeatures();

    expect(search.ocrAvailable).toBe(true);
    search.mode = "ocr";
    expect(search.mode).toBe("ocr");
  });

  it("disables OCR when the server has it off, and refuses to select it", async () => {
    const { search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    await search.loadFeatures();

    expect(search.ocrAvailable).toBe(false);
    search.mode = "ocr";

    expect(search.mode).toBe("filename");
  });

  it("falls back to Filename when the features show OCR is off after it was chosen", async () => {
    const { adapter, search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    search.mode = "ocr";
    search.text = "invoice";

    await search.submit();

    expect(search.mode).toBe("filename");
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "invoice", mode: "filename", filters: {} });
  });

  it("does not block a Search when the features cannot be read, and reads them again next time", async () => {
    const getSearchFeatures = vi.fn().mockRejectedValueOnce("search_unavailable").mockResolvedValue({ smartSearch: true, ocr: false });
    const { adapter, search } = setup({ getSearchFeatures });
    search.text = "IMG";

    await search.submit();

    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "IMG", mode: "filename", filters: {} });
    expect(search.failure).toBeNull();
    expect(search.featuresFailed).toBe(true);
    expect(search.ocrAvailable).toBe(true);

    await search.submit();

    expect(getSearchFeatures).toHaveBeenCalledTimes(2);
    expect(search.featuresFailed).toBe(false);
    expect(search.ocrAvailable).toBe(false);
  });

  it("still reports an unsupported server when the features cannot be read", async () => {
    const { search } = setup({
      getSearchSupport: vi.fn().mockResolvedValue(UNSUPPORTED),
      getSearchFeatures: vi.fn().mockRejectedValue("search_unavailable"),
    });
    search.text = "a";

    await search.submit();

    expect(search.unsupported).toEqual({ version: "3.1.0", requiredVersion: "3.2" });
  });

  it("ignores features that arrive after clear", async () => {
    const inFlight = deferred<SearchFeatures>();
    const { search } = setup({ getSearchFeatures: vi.fn().mockReturnValue(inFlight.promise) });

    const loading = search.loadFeatures();
    search.clear();
    inFlight.resolve({ smartSearch: true, ocr: false });
    await loading;

    expect(search.ocrAvailable).toBe(true);
  });
});
