import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { SearchFeatures, SearchSupport, Tag } from "../contracts";
import { createSearchController } from "../controllers/search-controller.svelte";
import { createTagsController } from "../controllers/tags-controller.svelte";
import type { TagsDesktopAdapter } from "../tauri/desktop-adapter";
import SearchFilterTag from "./SearchFilterTag.svelte";

afterEach(cleanup);

const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };

const tag = (id: string, name: string, parentId: string | null = null, color: string | null = null): Tag => ({
  id,
  name,
  value: name,
  parentId,
  color,
});

const TAGS = [tag("travel", "Travel", null, "#ff0000"), tag("italy", "Italy", "travel"), tag("rome", "Rome", "italy"), tag("family", "Family")];

function setup(getTags: () => Promise<Tag[]> = async () => TAGS) {
  const adapter = { getTags: vi.fn(getTags) };
  const tags = createTagsController(adapter as unknown as TagsDesktopAdapter);
  const search = createSearchController({
    getSearchSupport: vi.fn().mockResolvedValue(SUPPORTED),
    getSearchFeatures: vi.fn().mockResolvedValue(FEATURES),
    searchAssets: vi.fn(),
    searchContextPage: vi.fn(),
    getSearchSuggestions: vi.fn(),
    getPlaces: vi.fn(),
  });
  render(SearchFilterTag, { search, tags });
  return { adapter, search, tags };
}

describe("Search Tag filter", () => {
  it("shows the Tags as a hierarchy, the top level expanded and deeper levels collapsed", async () => {
    setup();

    const tree = await screen.findByRole("tree", { name: "Tags" });
    expect(tree).toHaveAttribute("aria-multiselectable", "true");
    const travel = within(tree).getByRole("treeitem", { name: "Travel" });
    expect(travel).toHaveAttribute("aria-level", "1");
    expect(travel).toHaveAttribute("aria-expanded", "true");
    const italy = within(tree).getByRole("treeitem", { name: "Italy" });
    expect(italy).toHaveAttribute("aria-level", "2");
    expect(italy).toHaveAttribute("aria-expanded", "false");
    expect(within(tree).queryByRole("treeitem", { name: "Rome" })).not.toBeInTheDocument();

    await fireEvent.click(within(italy).getByTestId("tag-toggle"));

    expect(within(tree).getByRole("treeitem", { name: "Rome" })).toHaveAttribute("aria-level", "3");
  });

  it("chooses several Tags and unchooses them again, children not included", async () => {
    const { search } = setup();

    const travel = await screen.findByRole("treeitem", { name: "Travel" });
    const family = screen.getByRole("treeitem", { name: "Family" });
    expect(travel).toHaveAttribute("aria-selected", "false");

    await fireEvent.click(travel);
    await fireEvent.click(family);
    expect(search.filters.tagIds).toEqual(["travel", "family"]);
    expect(travel).toHaveAttribute("aria-selected", "true");

    await fireEvent.click(travel);
    expect(search.filters.tagIds).toEqual(["family"]);
    await fireEvent.click(family);
    expect(search.filters.tagIds).toBeUndefined();
  });

  it("chooses a Tag with the keyboard and expands with the arrow keys", async () => {
    const { search } = setup();

    const travel = await screen.findByRole("treeitem", { name: "Travel" });
    const italy = screen.getByRole("treeitem", { name: "Italy" });
    italy.focus();
    await fireEvent.keyDown(italy, { key: "ArrowRight" });
    expect(await screen.findByRole("treeitem", { name: "Rome" })).toBeInTheDocument();

    travel.focus();

    await fireEvent.keyDown(travel, { key: " " });
    expect(search.filters.tagIds).toEqual(["travel"]);
  });

  it("names tag.read when the API key lacks it, offers no Tags, and does not block the Search", async () => {
    const { search } = setup(async () => {
      throw "tags_permission_denied:tag.read";
    });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("tag.read");
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
    search.text = "IMG";
    expect(search.canSubmit).toBe(true);
  });

  it("still shows the chosen Tags and lets them be cleared when the Tags cannot be loaded", async () => {
    const { search } = setup(async () => {
      throw "tags_permission_denied:tag.read";
    });
    search.filters.tagIds = ["travel", "family"];

    expect(await screen.findByRole("alert")).toHaveTextContent("tag.read");
    expect(screen.getByText("2 chosen")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Clear chosen tags" }));

    expect(search.filters.tagIds).toBeUndefined();
    expect(screen.queryByText("2 chosen")).not.toBeInTheDocument();
  });

  it("drops a chosen Tag that no longer exists once the Tags have loaded", async () => {
    const { search } = setup();
    search.filters.tagIds = ["travel", "deleted-in-immich"];

    await screen.findByRole("tree");

    await vi.waitFor(() => expect(search.filters.tagIds).toEqual(["travel"]));
  });

  it("clears the filter when none of the chosen Tags exists any more", async () => {
    const { search } = setup(async () => []);
    search.filters.tagIds = ["deleted-in-immich"];

    await screen.findByText(/No tags yet/);

    await vi.waitFor(() => expect(search.filters.tagIds).toBeUndefined());
  });

  it("keeps the chosen Tags while the Tags are offline", async () => {
    const getTags = vi.fn().mockResolvedValueOnce(TAGS).mockRejectedValue("tags_unavailable");
    const { search, tags } = setup(getTags);
    await screen.findByRole("tree");
    search.filters.tagIds = ["travel"];

    await tags.refresh();

    expect(await screen.findByText("Tags is offline. Showing cached tags.")).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(search.filters.tagIds).toEqual(["travel"]);
  });

  it("labels the tree with the filter heading", async () => {
    setup();

    const tree = await screen.findByRole("tree");

    const heading = document.getElementById("search-filter-tag-label");
    expect(tree).toHaveAttribute("aria-labelledby", "search-filter-tag-label");
    expect(tree).not.toHaveAttribute("aria-label");
    expect(heading).toHaveTextContent("Tags");
  });

  it("offers Retry when the Tags cannot be loaded", async () => {
    const getTags = vi.fn().mockRejectedValueOnce("tags_unavailable").mockResolvedValue(TAGS);
    setup(getTags);

    await fireEvent.click(await screen.findByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
  });

  it("says so, and loads the Tags once, when the Library has none", async () => {
    const { adapter } = setup(async () => []);

    expect(await screen.findByText(/No tags yet/)).toBeInTheDocument();
    expect(adapter.getTags).toHaveBeenCalledTimes(1);
  });

  it("does not read the Tags again when they are already loaded", async () => {
    const adapter = { getTags: vi.fn(async () => TAGS) };
    const tags = createTagsController(adapter as unknown as TagsDesktopAdapter);
    await tags.refresh();
    const search = createSearchController({
      getSearchSupport: vi.fn(), getSearchFeatures: vi.fn(), searchAssets: vi.fn(), searchContextPage: vi.fn(), getSearchSuggestions: vi.fn(), getPlaces: vi.fn(),
    });
    render(SearchFilterTag, { search, tags });

    expect(await screen.findByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(adapter.getTags).toHaveBeenCalledTimes(1);
  });
});
