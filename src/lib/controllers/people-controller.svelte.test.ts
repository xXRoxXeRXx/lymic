import { describe, expect, it, vi } from "vitest";
import { createPeopleController } from "./people-controller.svelte";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

describe("People controller", () => {
  it("loads pages in server order and retains cached people when refresh fails", async () => {
    const adapter: PeopleDesktopAdapter = {
      getPeople: vi.fn()
        .mockResolvedValueOnce({ people: [{ id: "one", name: "Ada", thumbnailPath: null }], hasNextPage: true })
        .mockResolvedValueOnce({ people: [{ id: "two", name: null, thumbnailPath: null }], hasNextPage: false })
        .mockRejectedValueOnce("recognized_people_permission_denied:people"),
      getPersonTimelineAssets: vi.fn(),
    };
    const people = createPeopleController(adapter);
    await people.refresh();
    await people.loadMore();
    await people.refresh();
    expect(people.people.map((person) => person.id)).toEqual(["one", "two"]);
    expect(people.isOffline).toBe(true);
    expect(adapter.getPeople).toHaveBeenNthCalledWith(1, 1, 50);
    expect(adapter.getPeople).toHaveBeenNthCalledWith(2, 2, 50);
  });

  it("shows the capability state when Tauri rejects a permission error string", async () => {
    const adapter: PeopleDesktopAdapter = {
      getPeople: vi.fn().mockRejectedValue("recognized_people_permission_denied:people"),
      getPersonTimelineAssets: vi.fn(),
    };

    const people = createPeopleController(adapter);
    await people.refresh();

    expect(people.error).toBe("unavailable");
  });
});
