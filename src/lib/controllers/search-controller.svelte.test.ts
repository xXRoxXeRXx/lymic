import { describe, expect, it, vi } from "vitest";
import type { ContextSearchPage, SearchFeatures, SearchSupport, SearchTimeline } from "../contracts";
import { createSearchController } from "./search-controller.svelte";

const march = "2025-03-01T00:00:00.000Z";
const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };
const UNSUPPORTED: SearchSupport = { supported: false, version: "3.1.0", requiredVersion: "3.2" };

function asset(id: string) {
  return { id, width: 640, height: 480, type: "IMAGE" as const, takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false };
}

function results(ids: string[], truncated = false): SearchTimeline {
  return { buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(asset) }, truncated };
}

function contextPage(ids: string[], nextPage: number | null): ContextSearchPage {
  return { assets: ids.map(asset), nextPage };
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
    searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b"], null)),
    ...overrides,
  };
  return { adapter, search: createSearchController(adapter) };
}

/** A Search that has left the default Context Search Mode, as the chronological tests below expect. */
function setupFilename(overrides: Partial<Parameters<typeof createSearchController>[0]> = {}) {
  const made = setup(overrides);
  made.search.mode = "filename";
  return made;
}

describe("Search controller", () => {
  it("runs a Filename Search with the trimmed text and shows the results chronologically", async () => {
    const { adapter, search } = setupFilename();
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
    const { adapter, search } = setupFilename();
    search.text = "   ";

    expect(search.canSubmit).toBe(false);
    expect(await search.submit()).toBe(false);

    expect(adapter.getSearchSupport).not.toHaveBeenCalled();
    expect(adapter.searchAssets).not.toHaveBeenCalled();
  });

  it("reads the server support once and reuses it for later searches", async () => {
    const { adapter, search } = setupFilename();
    search.text = "a";
    await search.submit();
    search.text = "b";
    await search.submit();

    expect(adapter.getSearchSupport).toHaveBeenCalledTimes(1);
    expect(adapter.searchAssets).toHaveBeenCalledTimes(2);
  });

  it("reports an unsupported server without searching, and checks again on the next Search", async () => {
    const { adapter, search } = setupFilename({ getSearchSupport: vi.fn().mockResolvedValueOnce(UNSUPPORTED).mockResolvedValue(SUPPORTED) });
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
    const { search } = setupFilename({ searchAssets: vi.fn().mockResolvedValueOnce(results(["a"], true)).mockResolvedValue(results(["b"])) });
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
    const { search } = setupFilename({ searchAssets: vi.fn().mockRejectedValue(code) });
    search.text = "a";

    await search.submit();

    expect(search.failure).toBe(expected);
    expect(search.hasInitialLoadError).toBe(true);
    expect(search.text).toBe("a");
  });

  it("classifies a failure to read the server version like a failed Search", async () => {
    const { search } = setupFilename({ getSearchSupport: vi.fn().mockRejectedValue("search_unavailable") });
    search.text = "a";

    await search.submit();

    expect(search.failure).toBe("unavailable");
  });

  it("keeps the previous text and results visible when a later Search fails, and recovers on Retry", async () => {
    const searchAssets = vi.fn()
      .mockResolvedValueOnce(results(["old"]))
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValueOnce(results(["new"]));
    const { search } = setupFilename({ searchAssets });
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
    const { search } = setupFilename();
    search.text = "IMG";
    await search.submit();

    search.clear();

    expect(search.text).toBe("");
    expect(search.mode).toBe("context");
    expect(search.submittedText).toBe("");
    expect(search.buckets).toEqual([]);
    expect(search.truncated).toBe(false);
    expect(search.unsupported).toBeNull();
    expect(search.failure).toBeNull();
  });

  it("ignores a Search that resolves after clear so the next account does not inherit it", async () => {
    const inFlight = deferred<SearchTimeline>();
    const { adapter, search } = setupFilename({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
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
    const inFlight = deferred<SearchTimeline>();
    const { adapter, search } = setupFilename({ searchAssets: vi.fn().mockReturnValue(inFlight.promise) });
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
  it("starts on Context and sends the chosen mode with the Search", async () => {
    const { adapter, search } = setup();
    expect(search.mode).toBe("context");

    for (const mode of ["description", "folderPath", "ocr", "filename"] as const) {
      search.mode = mode;
      search.text = "term";
      await search.submit();
      expect(adapter.searchAssets).toHaveBeenLastCalledWith({ text: "term", mode, filters: {} });
    }
  });

  it("keeps the chosen mode for later Searches in the session", async () => {
    const { adapter, search } = setupFilename();
    search.mode = "folderPath";
    search.text = "a";
    await search.submit();
    search.text = "b";
    await search.submit();

    expect(search.mode).toBe("folderPath");
    expect(adapter.searchAssets).toHaveBeenLastCalledWith({ text: "b", mode: "folderPath", filters: {} });
  });

  it("resets the mode and the held features on clear", async () => {
    const { adapter, search } = setupFilename({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    await search.loadFeatures();
    search.mode = "description";
    expect(search.ocrAvailable).toBe(false);

    search.clear();

    expect(search.mode).toBe("context");
    expect(search.ocrAvailable).toBe(true);
    await search.loadFeatures();
    expect(adapter.getSearchFeatures).toHaveBeenCalledTimes(2);
  });
});

describe("Search server features", () => {
  it("reads the features once, before the first Search, and holds them in memory", async () => {
    const calls: string[] = [];
    const { adapter, search } = setupFilename({
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
    const { adapter, search } = setupFilename({ getSearchFeatures: vi.fn().mockReturnValue(inFlight.promise) });

    const first = search.loadFeatures();
    const second = search.loadFeatures();
    inFlight.resolve(FEATURES);
    await Promise.all([first, second]);

    expect(adapter.getSearchFeatures).toHaveBeenCalledTimes(1);
  });

  it("enables OCR while the server has it on, and while the features are not known yet", async () => {
    const { search } = setupFilename();
    expect(search.ocrAvailable).toBe(true);

    await search.loadFeatures();

    expect(search.ocrAvailable).toBe(true);
    search.mode = "ocr";
    expect(search.mode).toBe("ocr");
  });

  it("disables OCR when the server has it off, and refuses to select it", async () => {
    const { search } = setupFilename({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    await search.loadFeatures();

    expect(search.ocrAvailable).toBe(false);
    search.mode = "ocr";

    expect(search.mode).toBe("filename");
  });

  it("falls back to Filename when the features show OCR is off after it was chosen", async () => {
    const { adapter, search } = setupFilename({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: false }) });
    search.mode = "ocr";
    search.text = "invoice";

    await search.submit();

    expect(search.mode).toBe("filename");
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "invoice", mode: "filename", filters: {} });
  });

  it("does not block a Search when the features cannot be read, and reads them again next time", async () => {
    const getSearchFeatures = vi.fn().mockRejectedValueOnce("search_unavailable").mockResolvedValue({ smartSearch: true, ocr: false });
    const { adapter, search } = setupFilename({ getSearchFeatures });
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
    const { search } = setupFilename({
      getSearchSupport: vi.fn().mockResolvedValue(UNSUPPORTED),
      getSearchFeatures: vi.fn().mockRejectedValue("search_unavailable"),
    });
    search.text = "a";

    await search.submit();

    expect(search.unsupported).toEqual({ version: "3.1.0", requiredVersion: "3.2" });
  });

  it("ignores features that arrive after clear", async () => {
    const inFlight = deferred<SearchFeatures>();
    const { search } = setupFilename({ getSearchFeatures: vi.fn().mockReturnValue(inFlight.promise) });

    const loading = search.loadFeatures();
    search.clear();
    inFlight.resolve({ smartSearch: true, ocr: false });
    await loading;

    expect(search.ocrAvailable).toBe(true);
  });
});

describe("Context Search", () => {
  it("is the default Search Mode while the server has Smart Search on, and can be chosen", async () => {
    const { search } = setup();
    expect(search.mode).toBe("context");
    expect(search.smartSearchAvailable).toBe(true);

    await search.loadFeatures();
    search.mode = "description";
    search.mode = "context";

    expect(search.mode).toBe("context");
    expect(search.smartSearchAvailable).toBe(true);
  });

  it("falls back to Filename and disables Context when the server has Smart Search off", async () => {
    const { search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });

    await search.loadFeatures();

    expect(search.smartSearchAvailable).toBe(false);
    expect(search.mode).toBe("filename");
    search.mode = "context";
    expect(search.mode).toBe("filename");
  });

  it("does not override another mode the user chose when Smart Search turns out to be off", async () => {
    const { search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });
    search.mode = "folderPath";

    await search.loadFeatures();

    expect(search.mode).toBe("folderPath");
  });

  it("runs a Filename Search for the first Search when the features show Smart Search is off", async () => {
    const { adapter, search } = setup({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });
    search.text = "beach";

    await search.submit();

    expect(search.mode).toBe("filename");
    expect(search.presentation).toBe("chronological");
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "beach", mode: "filename", filters: {} });
  });

  it("shows a Context text as a flat, relevance-ordered result", async () => {
    const { adapter, search } = setup({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["best", "next", "last"], null)) });
    search.text = "  dog on a beach ";

    expect(await search.submit()).toBe(true);

    expect(adapter.searchContextPage).toHaveBeenCalledWith({ text: "dog on a beach", mode: "context", filters: {} }, 1);
    expect(adapter.searchAssets).not.toHaveBeenCalled();
    expect(search.presentation).toBe("relevance");
    expect(search.context.buckets).toHaveLength(1);
    const bucket = search.context.buckets[0].timeBucket;
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["best", "next", "last"]);
    expect(search.resultCount).toBe(3);
    expect(search.submittedText).toBe("dog on a beach");
    expect(search.truncated).toBe(false);
    expect(search.failure).toBeNull();
    expect(search.context.hasNextPage).toBe(false);
  });

  it("runs a Context Search without text as the chronological filters-only Search", async () => {
    const { adapter, search } = setup();
    // The first Search Filters arrive with the filter panel; any set filter is a criterion.
    (search.filters as Record<string, unknown>).example = "set";

    expect(await search.submit()).toBe(true);

    expect(search.mode).toBe("context");
    expect(search.presentation).toBe("chronological");
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "", mode: "context", filters: { example: "set" } });
  });

  it("loads further pages the server announces, keeping the order and skipping duplicates", async () => {
    const searchContextPage = vi.fn()
      .mockResolvedValueOnce(contextPage(["a", "b"], 2))
      .mockResolvedValueOnce(contextPage(["b", "c"], 3))
      .mockResolvedValueOnce(contextPage(["d"], null));
    const { search } = setup({ searchContextPage });
    search.text = "dog";
    await search.submit();
    expect(search.context.hasNextPage).toBe(true);
    const bucket = search.context.buckets[0].timeBucket;
    const generation = search.context.refreshGeneration;

    await search.context.loadMore();
    await search.context.loadMore();

    expect(searchContextPage.mock.calls.map(([, page]) => page)).toEqual([1, 2, 3]);
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["a", "b", "c", "d"]);
    expect(search.context.buckets[0].count).toBe(4);
    expect(search.context.hasNextPage).toBe(false);
    expect(search.resultCount).toBe(4);
    // Further pages are not a new result set: an open Lightbox and the selection survive them.
    expect(search.context.refreshGeneration).toBe(generation);
    await search.context.loadMore();
    expect(searchContextPage).toHaveBeenCalledTimes(3);
  });

  it("does not request the same further page twice at once", async () => {
    const inFlight = deferred<ContextSearchPage>();
    const searchContextPage = vi.fn().mockResolvedValueOnce(contextPage(["a"], 2)).mockReturnValueOnce(inFlight.promise);
    const { search } = setup({ searchContextPage });
    search.text = "dog";
    await search.submit();

    const first = search.context.loadMore();
    const second = search.context.loadMore();
    expect(search.context.isLoadingMore).toBe(true);
    inFlight.resolve(contextPage(["b"], null));
    await Promise.all([first, second]);

    expect(searchContextPage).toHaveBeenCalledTimes(2);
    expect(search.context.isLoadingMore).toBe(false);
  });

  it("keeps the pages already shown when a further page fails, and retries that page", async () => {
    const searchContextPage = vi.fn()
      .mockResolvedValueOnce(contextPage(["a", "b"], 2))
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValueOnce(contextPage(["c"], null));
    const { search } = setup({ searchContextPage });
    search.text = "dog";
    await search.submit();
    const bucket = search.context.buckets[0].timeBucket;

    await search.context.loadMore();

    expect(search.context.loadMoreFailed).toBe(true);
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["a", "b"]);
    expect(search.context.hasInitialLoadError).toBe(false);
    expect(search.failure).toBeNull();

    await search.context.loadMore();

    expect(search.context.loadMoreFailed).toBe(false);
    expect(searchContextPage.mock.calls.map(([, page]) => page)).toEqual([1, 2, 2]);
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["a", "b", "c"]);
  });

  it.each([
    ["search_authentication_required", "authentication_required"],
    ["search_permission_denied:asset.read", "permission_denied"],
    ["search_unavailable", "unavailable"],
    ["search_invalid_response", "error"],
    ["search_context_unavailable", "context_unavailable"],
  ] as const)("classifies the failure %s of the first page as %s", async (code, expected) => {
    const { search } = setup({ searchContextPage: vi.fn().mockRejectedValue(code) });
    search.text = "dog";

    await search.submit();

    expect(search.failure).toBe(expected);
    expect(search.context.hasInitialLoadError).toBe(true);
    // A rejection by the server never silently changes the Search Mode.
    expect(search.mode).toBe("context");
    expect(search.text).toBe("dog");
  });

  it("keeps the earlier results offline and recovers on Retry", async () => {
    const searchContextPage = vi.fn()
      .mockResolvedValueOnce(contextPage(["old"], null))
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValueOnce(contextPage(["new"], null));
    const { search } = setup({ searchContextPage });
    search.text = "dog";
    await search.submit();
    const bucket = search.context.buckets[0].timeBucket;

    search.text = "cat";
    await search.submit();

    expect(search.context.isOffline).toBe(true);
    expect(search.context.hasInitialLoadError).toBe(false);
    expect(search.failure).toBe("unavailable");
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["old"]);
    expect(search.submittedText).toBe("dog");

    await search.context.refresh();

    expect(search.context.isOffline).toBe(false);
    expect(search.failure).toBeNull();
    expect(searchContextPage).toHaveBeenLastCalledWith({ text: "cat", mode: "context", filters: {} }, 1);
    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["new"]);
  });

  it("pages the Search whose results are shown, not a newer one that failed", async () => {
    const searchContextPage = vi.fn()
      .mockResolvedValueOnce(contextPage(["a"], 2))
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValueOnce(contextPage(["b"], null));
    const { search } = setup({ searchContextPage });
    search.text = "dog";
    await search.submit();
    search.text = "cat";
    await search.submit();

    await search.context.loadMore();

    expect(searchContextPage).toHaveBeenLastCalledWith({ text: "dog", mode: "context", filters: {} }, 2);
  });

  it("reports an unsupported server for a Context Search without searching", async () => {
    const { adapter, search } = setup({ getSearchSupport: vi.fn().mockResolvedValue(UNSUPPORTED) });
    search.text = "dog";

    await search.submit();

    expect(search.unsupported).toEqual({ version: "3.1.0", requiredVersion: "3.2" });
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(search.context.buckets).toEqual([]);
  });

  it("replaces the results of the other presentation when the kind of Search changes", async () => {
    const { search } = setup({ searchAssets: vi.fn().mockResolvedValue(results(["chrono"], true)) });
    search.text = "dog";
    await search.submit();
    expect(search.presentation).toBe("relevance");

    search.mode = "filename";
    search.text = "IMG";
    await search.submit();

    expect(search.presentation).toBe("chronological");
    expect(search.context.buckets).toEqual([]);
    expect(search.truncated).toBe(true);

    search.mode = "context";
    search.text = "dog";
    await search.submit();

    expect(search.presentation).toBe("relevance");
    expect(search.buckets).toEqual([]);
    expect(search.truncated).toBe(false);
  });

  it("clears the Context Search Results, the text and the mode on clear", async () => {
    const { search } = setup();
    search.text = "dog";
    await search.submit();

    search.clear();

    expect(search.context.buckets).toEqual([]);
    expect(search.presentation).toBe("chronological");
    expect(search.text).toBe("");
    expect(search.mode).toBe("context");
    expect(search.submittedText).toBe("");
    expect(search.resultCount).toBe(0);
  });

  it("ignores a page that resolves after clear so the next account does not inherit it", async () => {
    const inFlight = deferred<ContextSearchPage>();
    const { adapter, search } = setup({ searchContextPage: vi.fn().mockReturnValue(inFlight.promise) });
    search.text = "dog";

    const running = search.submit();
    await vi.waitFor(() => expect(adapter.searchContextPage).toHaveBeenCalled());
    search.clear();
    inFlight.resolve(contextPage(["late"], 2));
    await running;

    expect(search.context.buckets).toEqual([]);
    expect(search.context.hasNextPage).toBe(false);
    expect(search.submittedText).toBe("");
  });

  it("steps through the loaded results in relevance order, wrapping at the ends", async () => {
    const { search } = setup({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b", "c"], null)) });
    search.text = "dog";
    await search.submit();

    expect((await search.context.stepAsset("a", 1))?.id).toBe("b");
    expect((await search.context.stepAsset("c", 1))?.id).toBe("a");
    expect((await search.context.stepAsset("a", -1))?.id).toBe("c");
  });

  it("removes an asset from the results and keeps the rest in order", async () => {
    const { search } = setup({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b", "c"], null)) });
    search.text = "dog";
    await search.submit();
    const bucket = search.context.buckets[0].timeBucket;
    expect(search.context.locateAsset("b")).toEqual({ timeBucket: bucket, index: 1 });

    search.context.removeAsset("b");

    expect(search.context.assetsFor(bucket).map((candidate) => candidate.id)).toEqual(["a", "c"]);
    expect(search.context.locateAsset("b")).toBeNull();
    expect(search.resultCount).toBe(2);
  });
});
