import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { ContextSearchPage, SearchFeatures, SearchSupport, SearchResults } from "../contracts";
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

function contextPage(ids: string[], nextPage: number | null): ContextSearchPage {
  return { assets: ids.map(asset), nextPage };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function setupDefaultMode(overrides: Partial<Parameters<typeof createSearchController>[0]> = {}) {
  const adapter = {
    getSearchSupport: vi.fn().mockResolvedValue(SUPPORTED),
    getSearchFeatures: vi.fn().mockResolvedValue(FEATURES),
    searchAssets: vi.fn().mockResolvedValue(results(["a", "b"])),
    searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b"], null)),
    getSearchSuggestions: vi.fn().mockResolvedValue(["Canon", "FUJIFILM"]),
    ...overrides,
  };
  return { adapter, search: createSearchController(adapter) };
}

/** A Search on the Filename mode, as most tests below expect; `setupDefaultMode` keeps the default Context mode. */
function setup(overrides: Partial<Parameters<typeof createSearchController>[0]> = {}) {
  const made = setupDefaultMode(overrides);
  made.search.mode = "filename";
  return made;
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
    expect(search.mode).toBe("context");
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

describe("Search controller: Search Filters", () => {
  // "Local day" depends on the time zone, so the boundaries are checked in a fixed one: Berlin is UTC+1 in
  // March and UTC+2 after the clock change on 30 March 2025.
  let originalZone: string | undefined;
  beforeAll(() => {
    originalZone = process.env.TZ;
    process.env.TZ = "Europe/Berlin";
  });
  afterAll(() => {
    if (originalZone === undefined) delete process.env.TZ;
    else process.env.TZ = originalZone;
  });

  it("runs a Search with filters and no text", async () => {
    const { adapter, search } = setup();
    search.filters.mediaType = "video";

    expect(search.canSubmit).toBe(true);
    expect(await search.submit()).toBe(true);

    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "", mode: "filename", filters: { mediaType: "video" } });
    expect(search.resultCount).toBe(2);
  });

  it("turns the chosen local days into the start of the first day and the end of the last day, inclusive", async () => {
    const { adapter, search } = setup();
    search.filters.date = { from: "2025-03-01", to: "2025-03-31" };

    await search.submit();

    expect(adapter.searchAssets).toHaveBeenCalledWith({
      text: "",
      mode: "filename",
      filters: { date: { from: "2025-02-28T23:00:00.000Z", to: "2025-03-31T21:59:59.999Z" } },
    });
  });

  it("includes the whole last day when only a single day is chosen", async () => {
    const { adapter, search } = setup();
    search.filters.date = { from: "2025-06-15", to: "2025-06-15" };

    await search.submit();

    expect(vi.mocked(adapter.searchAssets).mock.calls[0][0].filters.date).toEqual({
      from: "2025-06-14T22:00:00.000Z",
      to: "2025-06-15T21:59:59.999Z",
    });
  });

  it("accepts either date bound alone", async () => {
    const { adapter, search } = setup();
    search.filters.date = { from: "2025-03-01" };
    await search.submit();
    expect(vi.mocked(adapter.searchAssets).mock.calls[0][0].filters).toEqual({ date: { from: "2025-02-28T23:00:00.000Z" } });

    search.filters.date = { to: "2025-03-01" };
    await search.submit();
    expect(vi.mocked(adapter.searchAssets).mock.calls[1][0].filters).toEqual({ date: { to: "2025-03-01T22:59:59.999Z" } });
  });

  it("rejects a range whose start is after its end with a message, without searching or swapping", async () => {
    const { adapter, search } = setup();
    search.filters.date = { from: "2025-03-31", to: "2025-03-01" };

    expect(search.filterErrors.date).toBe("search_filter_date_invalid_range");
    expect(search.canSubmit).toBe(false);
    expect(await search.submit()).toBe(false);

    expect(adapter.getSearchSupport).not.toHaveBeenCalled();
    expect(adapter.searchAssets).not.toHaveBeenCalled();
    expect(search.filters.date).toEqual({ from: "2025-03-31", to: "2025-03-01" });

    search.filters.date = { from: "2025-03-01", to: "2025-03-01" };
    expect(search.filterErrors.date).toBeUndefined();
    expect(await search.submit()).toBe(true);
  });

  it("does not search with an invalid range even when there is a text", async () => {
    const { adapter, search } = setup();
    search.text = "IMG";
    search.filters.date = { from: "2025-03-31", to: "2025-03-01" };

    expect(await search.submit()).toBe(false);
    expect(adapter.searchAssets).not.toHaveBeenCalled();
  });

  it("treats a cleared date field, an unset media type and an empty range as no filter", async () => {
    const { adapter, search } = setup();
    search.filters.date = { from: "", to: "" };
    search.filters.mediaType = undefined;

    expect(search.canSubmit).toBe(false);
    expect(await search.submit()).toBe(false);
    expect(adapter.searchAssets).not.toHaveBeenCalled();
  });

  it("changes and clears each filter on its own", async () => {
    const { adapter, search } = setup();
    search.filters.mediaType = "image";
    search.filters.date = { from: "2025-03-01" };
    await search.submit();

    search.filters.mediaType = "video";
    await search.submit();
    expect(vi.mocked(adapter.searchAssets).mock.calls[1][0].filters).toEqual({ mediaType: "video", date: { from: "2025-02-28T23:00:00.000Z" } });

    search.filters.date = undefined;
    await search.submit();
    expect(vi.mocked(adapter.searchAssets).mock.calls[2][0].filters).toEqual({ mediaType: "video" });

    search.filters.mediaType = undefined;
    expect(search.canSubmit).toBe(false);
  });

  it("combines the text with every set filter", async () => {
    const { adapter, search } = setup();
    search.text = "IMG";
    search.filters.mediaType = "image";
    search.filters.date = { to: "2025-03-01" };

    await search.submit();

    expect(adapter.searchAssets).toHaveBeenCalledWith({
      text: "IMG",
      mode: "filename",
      filters: { mediaType: "image", date: { to: "2025-03-01T22:59:59.999Z" } },
    });
  });

  it("keeps filters for the session and repeats them on Retry", async () => {
    const searchAssets = vi.fn().mockRejectedValueOnce("search_unavailable").mockResolvedValue(results(["a"]));
    const { search } = setup({ searchAssets });
    search.filters.mediaType = "video";
    await search.submit();

    await search.refresh();

    expect(searchAssets.mock.calls[1]).toEqual(searchAssets.mock.calls[0]);
    expect(search.filters.mediaType).toBe("video");
  });

  it("resets the filters, and what the Search shows, on clear", async () => {
    const { search } = setup();
    search.text = "IMG";
    search.mode = "filename";
    search.filters.mediaType = "video";
    search.filters.date = { from: "2025-03-01" };
    await search.submit();

    search.clear();

    expect(search.text).toBe("");
    expect(search.mode).toBe("context");
    expect(search.filters).toEqual({});
    expect(search.canSubmit).toBe(false);
    expect(search.filterErrors).toEqual({});
  });
});

describe("camera filters", () => {
  it("runs a Search with the chosen camera and no text", async () => {
    const { adapter, search } = setup();
    search.filters.camera = { make: "FUJIFILM", model: "X100V", lensModel: "23mm F2" };

    expect(search.canSubmit).toBe(true);
    expect(await search.submit()).toBe(true);

    expect(adapter.searchAssets).toHaveBeenCalledWith({
      text: "",
      mode: "filename",
      filters: { camera: { make: "FUJIFILM", model: "X100V", lensModel: "23mm F2" } },
    });
  });

  it("combines the camera with the text and the other filters", async () => {
    const { adapter, search } = setup();
    search.text = "IMG";
    search.filters.mediaType = "image";
    search.filters.camera = { make: "Canon" };
    await search.submit();

    expect(vi.mocked(adapter.searchAssets).mock.calls[0][0]).toEqual({
      text: "IMG",
      mode: "filename",
      filters: { mediaType: "image", camera: { make: "Canon" } },
    });
  });

  it("is no filter when no part is chosen", () => {
    const { search } = setup();
    search.filters.camera = {};

    expect(search.canSubmit).toBe(false);
  });

  it("offers the suggestions of the adapter and forgets them, with the camera, on clear", async () => {
    const { adapter, search } = setup();
    search.filters.camera = { make: "Canon" };
    await search.suggestions.load({ type: "cameraMake" });
    expect(search.suggestions.stateFor({ type: "cameraMake" }).items).toEqual(["Canon", "FUJIFILM"]);

    search.clear();

    expect(search.filters.camera).toBeUndefined();
    expect(search.suggestions.stateFor({ type: "cameraMake" }).status).toBe("idle");
    await search.suggestions.load({ type: "cameraMake" });
    expect(adapter.getSearchSuggestions).toHaveBeenCalledTimes(2);
  });

  it("keeps Search working when the suggestions fail", async () => {
    const { search } = setup({ getSearchSuggestions: vi.fn().mockRejectedValue("search_unavailable") });
    await search.suggestions.load({ type: "cameraMake" });
    search.text = "IMG";

    expect(search.suggestions.stateFor({ type: "cameraMake" }).failure).toBe("unavailable");
    expect(await search.submit()).toBe(true);
    expect(search.failure).toBeNull();
  });
});

describe("Location filter", () => {
  it("runs a Search with the chosen country and state and no text", async () => {
    const { adapter, search } = setup();
    search.filters.location = { country: "Italy", state: "Tuscany" };

    expect(search.canSubmit).toBe(true);
    expect(await search.submit()).toBe(true);

    expect(adapter.searchAssets).toHaveBeenCalledWith({
      text: "",
      mode: "filename",
      filters: { location: { country: "Italy", state: "Tuscany" } },
    });
  });

  it("combines the Location with the text and the other filters", async () => {
    const { adapter, search } = setup();
    search.text = "dinner";
    search.filters.camera = { make: "Canon" };
    search.filters.location = { state: "Bavaria" };
    await search.submit();

    expect(vi.mocked(adapter.searchAssets).mock.calls[0][0]).toEqual({
      text: "dinner",
      mode: "filename",
      filters: { camera: { make: "Canon" }, location: { state: "Bavaria" } },
    });
  });

  it("is no filter when no part is chosen, and is reset by clear", () => {
    const { search } = setup();
    search.filters.location = {};
    expect(search.canSubmit).toBe(false);

    search.filters.location = { country: "Italy" };
    search.clear();

    expect(search.filters.location).toBeUndefined();
  });
});

describe("Search Mode", () => {
  it("starts on Context and sends the chosen mode with the Search", async () => {
    const { adapter, search } = setupDefaultMode();
    expect(search.mode).toBe("context");

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

    expect(search.mode).toBe("context");
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

  describe("display options and Rating", () => {
    it("sends no display option and no Rating by default, so Archived Assets are not asked for", async () => {
      const { adapter, search } = setup();
      search.text = "IMG";
      expect(search.filters.favorite).toBeUndefined();
      expect(search.filters.archived).toBeUndefined();
      expect(search.filters.unfiled).toBeUndefined();
      expect(search.filters.rating).toBeUndefined();

      await search.submit();

      expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "IMG", mode: "filename", filters: {} });
    });

    it("sends each display option on its own and runs a Search with no text", async () => {
      for (const option of ["favorite", "archived", "unfiled"] as const) {
        const { adapter, search } = setup();
        search.filters[option] = true;

        expect(search.canSubmit).toBe(true);
        await search.submit();

        expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "", mode: "filename", filters: { [option]: true } });
      }
    });

    it("treats a switched-off display option as no filter", async () => {
      const { adapter, search } = setup();
      search.filters.favorite = false;
      search.filters.archived = false;
      search.filters.unfiled = false;

      expect(search.canSubmit).toBe(false);
      expect(await search.submit()).toBe(false);
      expect(adapter.searchAssets).not.toHaveBeenCalled();

      search.text = "IMG";
      await search.submit();
      expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "IMG", mode: "filename", filters: {} });
    });

    it("sends exactly one Rating level", async () => {
      const { adapter, search } = setup();
      for (const stars of [1, 2, 3, 4, 5] as const) {
        search.filters.rating = stars;
        await search.submit();
        expect(adapter.searchAssets).toHaveBeenLastCalledWith({ text: "", mode: "filename", filters: { rating: stars } });
      }
    });

    it("selecting the Rating again clears it, and selecting another level replaces it", () => {
      const { search } = setup();

      search.toggleRating(4);
      expect(search.filters.rating).toBe(4);
      search.toggleRating(2);
      expect(search.filters.rating).toBe(2);
      search.toggleRating(2);
      expect(search.filters.rating).toBeUndefined();
      expect(search.canSubmit).toBe(false);
    });

    it("combines display options and Rating with the text and the other filters, and clear resets them", async () => {
      const { adapter, search } = setup();
      search.text = "IMG";
      search.filters.mediaType = "image";
      search.filters.favorite = true;
      search.filters.archived = true;
      search.filters.unfiled = true;
      search.toggleRating(5);

      await search.submit();

      expect(adapter.searchAssets).toHaveBeenCalledWith({
        text: "IMG",
        mode: "filename",
        filters: { mediaType: "image", favorite: true, archived: true, unfiled: true, rating: 5 },
      });

      search.clear();
      expect(search.filters).toEqual({});
    });
  });
});

describe("Context Search", () => {
  it("is the default Search Mode while the server has Smart Search on, and can be chosen", async () => {
    const { search } = setupDefaultMode();
    expect(search.mode).toBe("context");
    expect(search.smartSearchAvailable).toBe(true);

    await search.loadFeatures();
    search.mode = "description";
    search.mode = "context";

    expect(search.mode).toBe("context");
    expect(search.smartSearchAvailable).toBe(true);
  });

  it("falls back to Filename and disables Context when the server has Smart Search off", async () => {
    const { search } = setupDefaultMode({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });

    await search.loadFeatures();

    expect(search.smartSearchAvailable).toBe(false);
    expect(search.mode).toBe("filename");
    search.mode = "context";
    expect(search.mode).toBe("filename");
  });

  it("does not override another mode the user chose when Smart Search turns out to be off", async () => {
    const { search } = setupDefaultMode({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });
    search.mode = "folderPath";

    await search.loadFeatures();

    expect(search.mode).toBe("folderPath");
  });

  it("runs a Filename Search for the first Search when the features show Smart Search is off", async () => {
    const { adapter, search } = setupDefaultMode({ getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: false, ocr: true }) });
    search.text = "beach";

    await search.submit();

    expect(search.mode).toBe("filename");
    expect(search.presentation).toBe("chronological");
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "beach", mode: "filename", filters: {} });
  });

  it("shows a Context text as a flat, relevance-ordered result", async () => {
    const { adapter, search } = setupDefaultMode({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["best", "next", "last"], null)) });
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
    const { adapter, search } = setupDefaultMode();
    search.filters.mediaType = "video";

    expect(await search.submit()).toBe(true);

    expect(search.mode).toBe("context");
    expect(search.presentation).toBe("chronological");
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "", mode: "context", filters: { mediaType: "video" } });
  });

  it("loads further pages the server announces, keeping the order and skipping duplicates", async () => {
    const searchContextPage = vi.fn()
      .mockResolvedValueOnce(contextPage(["a", "b"], 2))
      .mockResolvedValueOnce(contextPage(["b", "c"], 3))
      .mockResolvedValueOnce(contextPage(["d"], null));
    const { search } = setupDefaultMode({ searchContextPage });
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
    const { search } = setupDefaultMode({ searchContextPage });
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
    const { search } = setupDefaultMode({ searchContextPage });
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
    const { search } = setupDefaultMode({ searchContextPage: vi.fn().mockRejectedValue(code) });
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
    const { search } = setupDefaultMode({ searchContextPage });
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
    const { search } = setupDefaultMode({ searchContextPage });
    search.text = "dog";
    await search.submit();
    search.text = "cat";
    await search.submit();

    await search.context.loadMore();

    expect(searchContextPage).toHaveBeenLastCalledWith({ text: "dog", mode: "context", filters: {} }, 2);
  });

  it("reports an unsupported server for a Context Search without searching", async () => {
    const { adapter, search } = setupDefaultMode({ getSearchSupport: vi.fn().mockResolvedValue(UNSUPPORTED) });
    search.text = "dog";

    await search.submit();

    expect(search.unsupported).toEqual({ version: "3.1.0", requiredVersion: "3.2" });
    expect(adapter.searchContextPage).not.toHaveBeenCalled();
    expect(search.context.buckets).toEqual([]);
  });

  it("replaces the results of the other presentation when the kind of Search changes", async () => {
    const { search } = setupDefaultMode({ searchAssets: vi.fn().mockResolvedValue(results(["chrono"], true)) });
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
    const { search } = setupDefaultMode();
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
    const { adapter, search } = setupDefaultMode({ searchContextPage: vi.fn().mockReturnValue(inFlight.promise) });
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
    const { search } = setupDefaultMode({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b", "c"], null)) });
    search.text = "dog";
    await search.submit();

    expect((await search.context.stepAsset("a", 1))?.id).toBe("b");
    expect((await search.context.stepAsset("c", 1))?.id).toBe("a");
    expect((await search.context.stepAsset("a", -1))?.id).toBe("c");
  });

  it("removes an asset from the results and keeps the rest in order", async () => {
    const { search } = setupDefaultMode({ searchContextPage: vi.fn().mockResolvedValue(contextPage(["a", "b", "c"], null)) });
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

describe("Search controller Person filter", () => {
  it("searches the chosen Recognized People alone, without text, as a filter", async () => {
    const { adapter, search } = setup();
    search.filters.personIds = ["person-1", "person-2"];

    expect(search.canSubmit).toBe(true);
    expect(await search.submit()).toBe(true);

    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "", mode: "filename", filters: { personIds: ["person-1", "person-2"] } });
  });

  it("treats no chosen person as no filter", async () => {
    const { adapter, search } = setup();
    search.filters.personIds = [];

    expect(search.canSubmit).toBe(false);
    expect(await search.submit()).toBe(false);
    expect(adapter.searchAssets).not.toHaveBeenCalled();
  });

  it("combines the people with the text and the other filters", async () => {
    const { adapter, search } = setup();
    search.text = "IMG";
    search.filters.personIds = ["person-1"];
    search.filters.mediaType = "image";

    await search.submit();

    expect(adapter.searchAssets).toHaveBeenCalledWith({ text: "IMG", mode: "filename", filters: { personIds: ["person-1"], mediaType: "image" } });
  });

  it("forgets the chosen people when the Search is cleared", () => {
    const { search } = setup();
    search.filters.personIds = ["person-1"];

    search.clear();

    expect(search.filters.personIds).toBeUndefined();
  });
});
