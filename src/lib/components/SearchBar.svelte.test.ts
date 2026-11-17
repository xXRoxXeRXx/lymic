import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { SearchFeatures, SearchSupport } from "../contracts";
import { createPeopleController } from "../controllers/people-controller.svelte";
import { createSearchController } from "../controllers/search-controller.svelte";
import { createTagsController } from "../controllers/tags-controller.svelte";
import type { PeopleDesktopAdapter, TagsDesktopAdapter } from "../tauri/desktop-adapter";
import SearchBar from "./SearchBar.svelte";

afterEach(cleanup);

const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const UNSUPPORTED: SearchSupport = { supported: false, version: "3.1.0", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };

function setup(overrides: { getSearchSupport?: () => Promise<SearchSupport>; searchAssets?: () => Promise<never> } = {}) {
  const people = createPeopleController({ getPeople: vi.fn().mockResolvedValue({ people: [], hasNextPage: false }) } as unknown as PeopleDesktopAdapter);
  const tags = createTagsController({ getTags: vi.fn().mockResolvedValue([]) } as unknown as TagsDesktopAdapter);
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
  render(SearchBar, { search, people, tags, onSubmit: vi.fn(), onClear: vi.fn(), onOpenArchive: vi.fn() });
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

describe("Search bar Filter Chips", () => {
  const optionsButton = () => screen.getByRole("button", { name: "Search options" });

  it("shows no chip line while no Search Filter is set", () => {
    setup();

    expect(screen.queryByRole("group", { name: "Active search filters" })).not.toBeInTheDocument();
  });

  it("shows the active Search Filters as chips while the panel is closed, and removes one with its X", async () => {
    const search = setup();
    search.filters.rating = 4;
    search.filters.favorite = true;

    const line = await screen.findByRole("group", { name: "Active search filters" });
    expect(line).toHaveTextContent("Rating: 4 ★");
    expect(line).toHaveTextContent("Display: Favorites");

    await fireEvent.click(screen.getByRole("button", { name: "Remove Rating filter" }));

    expect(search.filters.rating).toBeUndefined();
    expect(search.filters.favorite).toBe(true);
  });

  it("opens the panel at the editor of the chip that was clicked", async () => {
    const search = setup();
    search.filters.rating = 2;

    await fireEvent.click(await screen.findByRole("button", { name: "Rating: 2 ★" }));

    expect(optionsButton()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: "Rating: 2 ★" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Active search filters" })).not.toBeInTheDocument();
  });

  it("opens the panel with every editor closed and only one editor open at a time", async () => {
    setup();

    await fireEvent.click(optionsButton());
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search mode: Filename" })).toHaveAttribute("aria-expanded", "false");

    await fireEvent.click(screen.getByRole("button", { name: "Rating" }));
    expect(screen.getByRole("button", { name: "Rating" })).toHaveAttribute("aria-expanded", "true");

    await fireEvent.click(screen.getByRole("button", { name: "Media type" }));
    expect(screen.getAllByRole("region")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Rating" })).toHaveAttribute("aria-expanded", "false");

    await fireEvent.click(screen.getByRole("button", { name: "Media type" }));
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
  });

  it("closes the open editor with the first Escape and the panel with the second", async () => {
    setup();
    await fireEvent.click(optionsButton());
    await fireEvent.click(screen.getByRole("button", { name: "Rating" }));

    await fireEvent.keyDown(screen.getByRole("button", { name: "Rating" }), { key: "Escape" });
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(optionsButton()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Rating" })).toHaveFocus();

    await fireEvent.keyDown(screen.getByRole("button", { name: "Rating" }), { key: "Escape" });
    expect(optionsButton()).toHaveAttribute("aria-expanded", "false");
    expect(optionsButton()).toHaveFocus();
  });

  it("opens the editor of a filter that blocks the Search, and marks its chip", async () => {
    const search = setup();
    search.filters.date = { from: "2024-05-02", to: "2024-05-01" };

    await fireEvent.click(screen.getByRole("button", { name: "Search" }));

    expect(await screen.findByRole("region", { name: /^Date/ })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("The start date is after the end date");
  });

  it("closes the panel when the Search runs", async () => {
    const search = setup();
    search.text = "dog";
    await fireEvent.click(optionsButton());

    await fireEvent.click(screen.getByRole("button", { name: "Search" }));

    expect(optionsButton()).toHaveAttribute("aria-expanded", "false");
  });
});
