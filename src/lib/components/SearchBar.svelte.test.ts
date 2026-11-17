import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { SearchFeatures, SearchSupport } from "../contracts";
import { createPeopleController } from "../controllers/people-controller.svelte";
import { createSearchController } from "../controllers/search-controller.svelte";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import SearchBar from "./SearchBar.svelte";

afterEach(cleanup);

const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const UNSUPPORTED: SearchSupport = { supported: false, version: "3.1.0", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };

function setup(overrides: { getSearchSupport?: () => Promise<SearchSupport>; searchAssets?: () => Promise<never> } = {}) {
  const people = createPeopleController({ getPeople: vi.fn().mockResolvedValue({ people: [], hasNextPage: false }) } as unknown as PeopleDesktopAdapter);
  const search = createSearchController({
    getSearchSupport: vi.fn().mockResolvedValue(SUPPORTED),
    getSearchFeatures: vi.fn().mockResolvedValue(FEATURES),
    searchAssets: vi.fn(),
    searchContextPage: vi.fn(),
    getSearchSuggestions: vi.fn(),
    getPlaces: vi.fn(),
    ...overrides,
  });
  search.mode = "filename";
  render(SearchBar, { search, people, onSubmit: vi.fn(), onClear: vi.fn(), onOpenArchive: vi.fn() });
  return search;
}

describe("Search bar Clear control", () => {
  it("is absent for an untouched Search", () => {
    setup();

    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
  });

  it("stays while the unsupported server notice is showing, even with the text and filters emptied by hand", async () => {
    const search = setup({ getSearchSupport: () => Promise.resolve(UNSUPPORTED) });
    search.text = "dog";
    await search.submit();
    expect(search.unsupported).not.toBeNull();

    search.text = "";
    await Promise.resolve();

    expect(await screen.findByRole("button", { name: "Clear search" })).toBeInTheDocument();
  });

  it("stays while a first Search that failed is showing, even with the text emptied by hand", async () => {
    const search = setup({ searchAssets: () => Promise.reject("search_unavailable") });
    search.text = "dog";
    await search.submit();
    expect(search.failure).toBe("unavailable");
    expect(search.revision).toBe(0);

    search.text = "";
    await Promise.resolve();

    expect(await screen.findByRole("button", { name: "Clear search" })).toBeInTheDocument();
  });
});
