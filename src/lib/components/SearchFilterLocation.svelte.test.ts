import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { SearchSuggestionsRequest } from "../contracts";
import { createSearchController } from "../controllers/search-controller.svelte";
import SearchPanel from "./SearchPanel.svelte";

afterEach(cleanup);

const STATES: Record<string, string[]> = { Italy: ["Tuscany", "Lazio"], Germany: ["Bavaria", "Hesse"] };

/** The Search panel over a real Search controller whose adapter answers suggestions like Immich. */
function setup(getSearchSuggestions = vi.fn(async (request: SearchSuggestionsRequest) => {
  if (request.type === "country") return Object.keys(STATES);
  if (request.type === "state") return request.country ? STATES[request.country] ?? [] : Object.values(STATES).flat();
  return [];
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

const country = () => screen.getByRole("combobox", { name: "Country" });
const state = () => screen.getByRole("combobox", { name: "State or region" });

async function choose(box: HTMLElement, option: string) {
  await fireEvent.focus(box);
  await fireEvent.click(await screen.findByRole("option", { name: option }));
}

describe("Location Search Filter", () => {
  it("offers country and state in one Location section", () => {
    setup();

    const group = screen.getByRole("group", { name: "Location" });
    expect(within(group).getAllByRole("combobox").map((box) => box.getAttribute("aria-label"))).toEqual(["Country", "State or region"]);
  });

  it("offers Immich's country suggestions and applies the one that is clicked", async () => {
    const { adapter, search } = setup();

    await fireEvent.focus(country());
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Italy", "Germany"]);
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "country" });
    await fireEvent.click(screen.getByRole("option", { name: "Germany" }));

    expect(search.filters.location).toEqual({ country: "Germany" });
    expect(search.canSubmit).toBe(true);
  });

  it("does not apply free text that is not a suggestion", async () => {
    const { search } = setup();
    await fireEvent.focus(country());
    await screen.findAllByRole("option");

    await fireEvent.input(country(), { target: { value: "Atlantis" } });
    await fireEvent.keyDown(country(), { key: "Enter" });
    await fireEvent.blur(country());

    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(search.filters.location).toBeUndefined();
  });

  it("limits the state suggestions to the chosen country", async () => {
    const { adapter, search } = setup();
    await choose(country(), "Italy");

    await fireEvent.focus(state());

    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "state", country: "Italy" });
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Tuscany", "Lazio"]);
    await fireEvent.click(screen.getByRole("option", { name: "Lazio" }));
    expect(search.filters.location).toEqual({ country: "Italy", state: "Lazio" });
  });

  it("suggests every state while no country is chosen, and a state alone is a valid Location", async () => {
    const { adapter, search } = setup();

    await fireEvent.focus(state());

    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Tuscany", "Lazio", "Bavaria", "Hesse"]);
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "state" });
    await fireEvent.click(screen.getByRole("option", { name: "Bavaria" }));
    expect(search.filters.location).toEqual({ state: "Bavaria" });
    expect(search.canSubmit).toBe(true);
  });

  it("drops the chosen state when another country is chosen, and narrows the next state list to it", async () => {
    const { adapter, search } = setup();
    await choose(country(), "Italy");
    await choose(state(), "Tuscany");
    expect(search.filters.location).toEqual({ country: "Italy", state: "Tuscany" });

    await fireEvent.click(screen.getByRole("button", { name: "Clear Country" }));
    expect(search.filters.location).toEqual({ state: "Tuscany" });
    await choose(country(), "Germany");

    expect(search.filters.location).toEqual({ country: "Germany" });
    await fireEvent.focus(state());
    expect(adapter.getSearchSuggestions).toHaveBeenCalledWith({ type: "state", country: "Germany" });
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["Bavaria", "Hesse"]);
  });

  it("clears each part on its own, leaving the filter unset when none is left", async () => {
    const { search } = setup();
    await choose(country(), "Italy");
    await choose(state(), "Lazio");

    await fireEvent.click(screen.getByRole("button", { name: "Clear State or region" }));
    expect(search.filters.location).toEqual({ country: "Italy" });
    await fireEvent.click(screen.getByRole("button", { name: "Clear Country" }));

    expect(search.filters.location).toBeUndefined();
    expect(search.canSubmit).toBe(false);
  });

  it("shows a local error with Retry when the suggestions fail, leaving the rest of the panel usable", async () => {
    const getSearchSuggestions = vi.fn()
      .mockRejectedValueOnce("search_unavailable")
      .mockResolvedValue(["Italy"]);
    const { search } = setup(getSearchSuggestions);

    await fireEvent.focus(country());

    const alert = await screen.findByRole("alert");
    expect(alert.closest("[role=group]")).toBe(screen.getByRole("group", { name: "Location" }));
    await fireEvent.click(screen.getByRole("radio", { name: "Videos" }));
    expect(search.filters.mediaType).toBe("video");
    await fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("option", { name: "Italy" })).toBeInTheDocument();
  });
});
