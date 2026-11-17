import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import type { PeoplePage, SearchFeatures, SearchSupport } from "../contracts";
import { createPeopleController } from "../controllers/people-controller.svelte";
import { createSearchController } from "../controllers/search-controller.svelte";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import SearchFilterPerson from "./SearchFilterPerson.svelte";

afterEach(cleanup);

const SUPPORTED: SearchSupport = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
const FEATURES: SearchFeatures = { smartSearch: true, ocr: true };

function setup(getPeople: (page: number, size: number) => Promise<PeoplePage>) {
  const peopleAdapter = { getPeople: vi.fn(getPeople) };
  const people = createPeopleController(peopleAdapter as unknown as PeopleDesktopAdapter);
  const search = createSearchController({
    getSearchSupport: vi.fn().mockResolvedValue(SUPPORTED),
    getSearchFeatures: vi.fn().mockResolvedValue(FEATURES),
    searchAssets: vi.fn(),
    getSearchSuggestions: vi.fn(),
  });
  render(SearchFilterPerson, { search, people });
  return { people, peopleAdapter, search };
}

const person = (id: string, name: string | null) => ({ id, name, thumbnailPath: `/${id}.jpg` });

describe("Search Person filter", () => {
  it("lists the Recognized People from the people source, named before unnamed", async () => {
    setup(async () => ({ people: [person("u1", null), person("a", "Ada"), person("u2", null), person("b", "Bo")], hasNextPage: false }));

    await screen.findByRole("button", { name: "Ada" });
    const names = screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"));
    expect(names).toEqual(["Ada", "Bo", "Unnamed person", "Unnamed person"]);
  });

  it("shows each person with a face tile", async () => {
    setup(async () => ({ people: [person("a", "Ada")], hasNextPage: false }));

    expect(await screen.findByRole("img", { name: "Ada" })).toHaveAttribute("src", "lymic-thumb://person/a?v=%2Fa.jpg");
  });

  it("chooses several people, unnamed ones included, and unchooses them again", async () => {
    const { search } = setup(async () => ({ people: [person("a", "Ada"), person("u1", null)], hasNextPage: false }));

    const ada = await screen.findByRole("button", { name: "Ada" });
    const unnamed = screen.getByRole("button", { name: "Unnamed person" });
    expect(ada).toHaveAttribute("aria-pressed", "false");

    await fireEvent.click(ada);
    await fireEvent.click(unnamed);
    expect(search.filters.personIds).toEqual(["a", "u1"]);
    expect(ada).toHaveAttribute("aria-pressed", "true");

    await fireEvent.click(ada);
    expect(search.filters.personIds).toEqual(["u1"]);
    await fireEvent.click(unnamed);
    expect(search.filters.personIds).toBeUndefined();
  });

  it("loads more people on request", async () => {
    const { peopleAdapter } = setup(async (page) => page === 1
      ? { people: [person("a", "Ada")], hasNextPage: true }
      : { people: [person("b", "Bo")], hasNextPage: false });

    await fireEvent.click(await screen.findByRole("button", { name: "Load more" }));

    expect(await screen.findByRole("button", { name: "Bo" })).toBeInTheDocument();
    expect(peopleAdapter.getPeople).toHaveBeenLastCalledWith(2, 50);
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  });

  it("names person.read when the API key lacks it, offers no people, and does not block the Search", async () => {
    const { search } = setup(async () => {
      throw "recognized_people_permission_denied:people:configure an API key with person.read and asset.read permissions";
    });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("person.read");
    expect(screen.queryByRole("button", { name: /Unnamed|Ada/ })).not.toBeInTheDocument();
    search.text = "IMG";
    expect(search.canSubmit).toBe(true);
  });

  it("offers Retry when the people cannot be loaded", async () => {
    const getPeople = vi.fn()
      .mockRejectedValueOnce("recognized_people_unavailable:people")
      .mockResolvedValue({ people: [person("a", "Ada")], hasNextPage: false });
    setup(getPeople);

    await fireEvent.click(await screen.findByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("button", { name: "Ada" })).toBeInTheDocument();
  });
});
