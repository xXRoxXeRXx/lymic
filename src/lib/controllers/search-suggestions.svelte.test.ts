import { describe, expect, it, vi } from "vitest";
import type { SearchSuggestionsRequest } from "../contracts";
import { createSearchSuggestions } from "./search-suggestions.svelte";

const MAKES: SearchSuggestionsRequest = { type: "cameraMake" };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function setup(getSearchSuggestions = vi.fn().mockResolvedValue(["Canon", "FUJIFILM"])) {
  return { getSearchSuggestions, suggestions: createSearchSuggestions({ getSearchSuggestions }) };
}

describe("Search suggestions", () => {
  it("has nothing to show before they are requested", () => {
    const { suggestions, getSearchSuggestions } = setup();

    expect(suggestions.stateFor(MAKES)).toEqual({ status: "idle", items: [], failure: null });
    expect(getSearchSuggestions).not.toHaveBeenCalled();
  });

  it("loads the suggestions Immich returns for a request", async () => {
    const { suggestions, getSearchSuggestions } = setup();

    const loading = suggestions.load(MAKES);
    expect(suggestions.stateFor(MAKES).status).toBe("loading");
    await loading;

    expect(getSearchSuggestions).toHaveBeenCalledWith({ type: "cameraMake" });
    expect(suggestions.stateFor(MAKES)).toEqual({ status: "ready", items: ["Canon", "FUJIFILM"], failure: null });
  });

  it("asks Immich once per request and keeps the answer", async () => {
    const { suggestions, getSearchSuggestions } = setup();

    await suggestions.load(MAKES);
    await suggestions.load({ type: "cameraMake" });

    expect(getSearchSuggestions).toHaveBeenCalledTimes(1);
  });

  it("shares one request between loads that start while it is in flight", async () => {
    const inFlight = deferred<string[]>();
    const { suggestions, getSearchSuggestions } = setup(vi.fn().mockReturnValue(inFlight.promise));

    const first = suggestions.load(MAKES);
    const second = suggestions.load(MAKES);
    inFlight.resolve(["Canon"]);
    await Promise.all([first, second]);

    expect(getSearchSuggestions).toHaveBeenCalledTimes(1);
  });

  it("keeps the suggestions of different narrowing apart", async () => {
    const getSearchSuggestions = vi.fn().mockImplementation(async (request: SearchSuggestionsRequest) =>
      request.make === "Canon" ? ["EOS R5"] : request.make === "FUJIFILM" ? ["X100V"] : ["EOS R5", "X100V"]);
    const { suggestions } = setup(getSearchSuggestions);
    const canon: SearchSuggestionsRequest = { type: "cameraModel", make: "Canon" };
    const fujifilm: SearchSuggestionsRequest = { type: "cameraModel", make: "FUJIFILM" };

    await suggestions.load(canon);
    await suggestions.load(fujifilm);
    await suggestions.load({ type: "cameraModel" });

    expect(getSearchSuggestions).toHaveBeenNthCalledWith(1, { type: "cameraModel", make: "Canon" });
    expect(getSearchSuggestions).toHaveBeenNthCalledWith(2, { type: "cameraModel", make: "FUJIFILM" });
    expect(suggestions.stateFor(canon).items).toEqual(["EOS R5"]);
    expect(suggestions.stateFor(fujifilm).items).toEqual(["X100V"]);
    expect(suggestions.stateFor({ type: "cameraModel" }).items).toEqual(["EOS R5", "X100V"]);
  });

  it("records a failure for that request only and asks again on the next load", async () => {
    const getSearchSuggestions = vi.fn()
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValue(["Canon"]);
    const { suggestions } = setup(getSearchSuggestions);

    await suggestions.load(MAKES);
    expect(suggestions.stateFor(MAKES)).toEqual({ status: "error", items: [], failure: "unavailable" });
    expect(suggestions.stateFor({ type: "cameraLensModel" }).status).toBe("idle");

    await suggestions.load(MAKES);
    expect(getSearchSuggestions).toHaveBeenCalledTimes(2);
    expect(suggestions.stateFor(MAKES)).toEqual({ status: "ready", items: ["Canon"], failure: null });
  });

  it("tells a missing API key permission apart from other failures", async () => {
    const { suggestions } = setup(vi.fn().mockRejectedValue("search_permission_denied:asset.read"));
    await suggestions.load(MAKES);
    expect(suggestions.stateFor(MAKES).failure).toBe("permission_denied");

    const other = setup(vi.fn().mockRejectedValue("search_invalid_response"));
    await other.suggestions.load(MAKES);
    expect(other.suggestions.stateFor(MAKES).failure).toBe("error");
  });

  it("never rejects, so a failure cannot break the rest of Search", async () => {
    const { suggestions } = setup(vi.fn().mockRejectedValue(new Error("boom")));

    await expect(suggestions.load(MAKES)).resolves.toBeUndefined();
  });

  it("forgets everything on clear and ignores answers that arrive later", async () => {
    const inFlight = deferred<string[]>();
    const { suggestions, getSearchSuggestions } = setup(vi.fn().mockReturnValue(inFlight.promise));

    const loading = suggestions.load(MAKES);
    suggestions.clear();
    inFlight.resolve(["Canon"]);
    await loading;

    expect(suggestions.stateFor(MAKES).status).toBe("idle");
    getSearchSuggestions.mockResolvedValue(["Sony"]);
    await suggestions.load(MAKES);
    expect(suggestions.stateFor(MAKES).items).toEqual(["Sony"]);
  });
});
