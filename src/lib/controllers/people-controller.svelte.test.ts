import { describe, expect, it, vi } from "vitest";
import { createPeopleController } from "./people-controller.svelte";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

describe("People controller", () => {
  it("loads pages in server order and retains cached people when refresh fails", async () => {
    const adapter: PeopleDesktopAdapter = {
      getPeople: vi.fn()
        .mockResolvedValueOnce({ people: [{ id: "one", name: "Ada", assetCount: 4, thumbnailPath: null }], hasNextPage: true })
        .mockResolvedValueOnce({ people: [{ id: "two", name: null, assetCount: 1, thumbnailPath: null }], hasNextPage: false })
        .mockRejectedValueOnce(new Error("offline")),
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
});
