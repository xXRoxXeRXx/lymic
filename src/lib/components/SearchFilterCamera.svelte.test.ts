import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { SearchSuggestionsRequest } from "../contracts";
import { createSearchController } from "../controllers/search-controller.svelte";
import SearchPanel from "./SearchPanel.svelte";

afterEach(cleanup);

const MODELS: Record<string, string[]> = { Canon: ["EOS R5", "EOS R6"], FUJIFILM: ["X100V", "X-T5"] };

/** The Search panel over a real Search controller whose adapter answers suggestions like Immich. */
function setup(getSearchSuggestions = vi.fn(async (request: SearchSuggestionsRequest) => {
  if (request.type === "cameraMake") return Object.keys(MODELS);
  if (request.type === "cameraModel") return request.make ? MODELS[request.make] ?? [] : Object.values(MODELS).flat();
  return ["RF 50mm", "23mm F2"];
})) {
  const adapter = {
    getSearchSupport: vi.fn().mockResolvedValue({ supported: true, version: "3.4.1", requiredVersion: "3.2" }),
    getSearchFeatures: vi.fn().mockResolvedValue({ smartSearch: true, ocr: true }),
    searchAssets: vi.fn().mockResolvedValue({ buckets: [], assetsByBucket: {}, truncated: false }),
    searchContextPage: vi.fn().mockResolvedValue({ assets: [], nextPage: null }),
    getSearchSuggestions,
  };
  const search = createSearchController(adapter);
  const people = { people: [], isLoading: false, isRefreshing: false, isOffline: false, hasNextPage: false, error: null, refresh: vi.fn(), loadMore: vi.fn(), clear: vi.fn() };
  render(SearchPanel, { search, people, id: "search-panel" });
  return { adapter, search };
}

const make = () => screen.getByRole("combobox", { name: "Camera make" });
const model = () => screen.getByRole("combobox", { name: "Camera model" });
const lens = () => screen.getByRole("combobox", { name: "Lens model" });

async function choose(box: HTMLElement, option: string) {
  await fireEvent.focus(box);
  await fireEvent.click(await screen.findByRole("option", { name: option }));
}

describe("camera Search Filters", () => {
  it("offers make, model and lens model in one Camera section", () => {
    setup();

    const group = screen.getByRole("group", { name: "Camera" });
    expect(within(group).getAllByRole("combobox").map((box) => box.getAttribute("aria-label"))).toEqual(["Camera make", "Camera model", "Lens model"]);
  });

  it("offers Immich's suggestions for the make and applies the one that is clicked", async () => {
    const { adapter, search } = setup();

    await fireEvent.focus(make());
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Canon", "FUJIFILM"]);
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "cameraMake" });
    await fireEvent.click(screen.getByRole("option", { name: "FUJIFILM" }));

    expect(search.filters.camera).toEqual({ make: "FUJIFILM" });
    expect(screen.queryByRole("combobox", { name: "Camera make" })).not.toBeInTheDocument();
    expect(screen.getByText("FUJIFILM")).toBeInTheDocument();
  });

  it("narrows the suggestions as the user types, matching anywhere and ignoring case", async () => {
    setup();
    await fireEvent.focus(make());
    await screen.findAllByRole("option");

    await fireEvent.input(make(), { target: { value: "ujif" } });

    expect(screen.getAllByRole("option").map((option) => option.textContent?.trim())).toEqual(["FUJIFILM"]);
  });

  it("does not apply free text that is not a suggestion, with Enter or by leaving the field", async () => {
    const { search } = setup();
    await fireEvent.focus(make());
    await screen.findAllByRole("option");

    await fireEvent.input(make(), { target: { value: "Leica" } });
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByText("No matching suggestion")).toBeInTheDocument();
    await fireEvent.keyDown(make(), { key: "Enter" });
    await fireEvent.blur(make());

    expect(search.filters.camera).toBeUndefined();
    expect(make()).toHaveValue("");
  });

  it("applies the suggestion chosen with the arrow keys and Enter", async () => {
    const { search } = setup();
    await fireEvent.focus(make());
    await screen.findAllByRole("option");

    await fireEvent.keyDown(make(), { key: "ArrowDown" });
    await fireEvent.keyDown(make(), { key: "ArrowDown" });
    expect(make()).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "FUJIFILM" }).id);
    await fireEvent.keyDown(make(), { key: "Enter" });

    expect(search.filters.camera).toEqual({ make: "FUJIFILM" });
  });

  it("does not choose anything with Enter before an option is highlighted, and Escape closes the list", async () => {
    const { search } = setup();
    await fireEvent.focus(make());
    await screen.findAllByRole("option");

    await fireEvent.keyDown(make(), { key: "Enter" });
    expect(search.filters.camera).toBeUndefined();
    await fireEvent.keyDown(make(), { key: "Escape" });

    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("limits the model suggestions to the chosen make", async () => {
    const { adapter, search } = setup();
    await choose(make(), "Canon");

    await fireEvent.focus(model());

    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "cameraModel", make: "Canon" });
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["EOS R5", "EOS R6"]);
    await fireEvent.click(screen.getByRole("option", { name: "EOS R6" }));
    expect(search.filters.camera).toEqual({ make: "Canon", model: "EOS R6" });
  });

  it("suggests every model while no make is chosen", async () => {
    const { adapter } = setup();

    await fireEvent.focus(model());

    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["EOS R5", "EOS R6", "X100V", "X-T5"]);
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "cameraModel" });
  });

  it("drops the chosen model when the make is cleared and another one is chosen", async () => {
    const { search } = setup();
    await choose(make(), "Canon");
    await choose(model(), "EOS R5");
    expect(search.filters.camera).toEqual({ make: "Canon", model: "EOS R5" });

    await fireEvent.click(screen.getByRole("button", { name: "Clear Camera make" }));
    expect(search.filters.camera).toEqual({ model: "EOS R5" });
    await choose(make(), "FUJIFILM");

    expect(search.filters.camera).toEqual({ make: "FUJIFILM" });
  });

  it("offers lens model suggestions and clears each part on its own", async () => {
    const { adapter, search } = setup();
    await choose(lens(), "RF 50mm");
    await choose(make(), "Canon");
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "cameraLensModel" });
    expect(search.filters.camera).toEqual({ lensModel: "RF 50mm", make: "Canon" });

    await fireEvent.click(screen.getByRole("button", { name: "Clear Lens model" }));

    expect(search.filters.camera).toEqual({ make: "Canon" });
    await fireEvent.click(screen.getByRole("button", { name: "Clear Camera make" }));
    expect(search.filters.camera).toBeUndefined();
  });

  it("shows a local error with Retry when the suggestions fail, leaving the rest of the panel usable", async () => {
    const getSearchSuggestions = vi.fn()
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValue(["Canon"]);
    const { search } = setup(getSearchSuggestions);

    await fireEvent.focus(make());

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Suggestions could not be loaded.");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(alert.closest("[role=group]")).toBe(screen.getByRole("group", { name: "Camera" }));
    await fireEvent.click(screen.getByRole("radio", { name: "Videos" }));
    expect(search.filters.mediaType).toBe("video");
    expect(search.filters.camera).toBeUndefined();

    await fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("option", { name: "Canon" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("names the missing asset.read permission when the API key may not read suggestions", async () => {
    setup(vi.fn().mockRejectedValue("search_permission_denied:asset.read"));

    await fireEvent.focus(make());

    expect(await screen.findByRole("alert")).toHaveTextContent("asset.read");
  });

  it("says when there is nothing to suggest", async () => {
    setup(vi.fn().mockResolvedValue([]));

    await fireEvent.focus(lens());

    await waitFor(() => expect(screen.getByText("Immich has no suggestions for this filter.")).toBeInTheDocument());
  });
});
