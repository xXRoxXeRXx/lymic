import { describe, expect, it, vi } from "vitest";
import { createPeopleController } from "./people-controller.svelte";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

describe("People controller", () => {
  it("loads another page on demand and retains cached people when refresh fails", async () => {
    const adapter: PeopleDesktopAdapter = {
      getPeople: vi.fn()
        .mockResolvedValueOnce({ people: [{ id: "one", name: "Ada", thumbnailPath: null }], hasNextPage: true })
        .mockResolvedValueOnce({ people: [{ id: "two", name: null, thumbnailPath: null }], hasNextPage: false })
        .mockRejectedValueOnce("recognized_people_permission_denied:people"),
      getPersonTimelineBuckets: vi.fn(),
      getPersonTimelineBucketAssets: vi.fn(),
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
      getPersonTimelineBuckets: vi.fn(),
      getPersonTimelineBucketAssets: vi.fn(),
    };

    const people = createPeopleController(adapter);
    await people.refresh();

    expect(people.error).toBe("permission_denied");
  });

  it("maps authentication and unavailable failures to unavailable and anything else to an error", async () => {
    const getPeople = vi.fn()
      .mockRejectedValueOnce("recognized_people_authentication_required:people")
      .mockRejectedValueOnce("recognized_people_unavailable:people")
      .mockRejectedValueOnce(new Error("boom"));
    const people = createPeopleController({ getPeople, getPersonTimelineBuckets: vi.fn(), getPersonTimelineBucketAssets: vi.fn() });

    await people.refresh();
    expect(people.error).toBe("unavailable");
    await people.refresh();
    expect(people.error).toBe("unavailable");
    await people.refresh();
    expect(people.error).toBe("error");
  });

  it("stops loading further pages when Immich reports no next page", async () => {
    const getPeople = vi.fn()
      .mockResolvedValueOnce({ people: [{ id: "one", name: "Ada", thumbnailPath: null }], hasNextPage: true })
      .mockResolvedValueOnce({ people: [{ id: "two", name: null, thumbnailPath: null }], hasNextPage: false });
    const people = createPeopleController({ getPeople, getPersonTimelineBuckets: vi.fn(), getPersonTimelineBucketAssets: vi.fn() });

    await people.refresh();
    expect(people.hasNextPage).toBe(true);
    await people.loadMore();
    expect(people.hasNextPage).toBe(false);
    await people.loadMore();

    expect(getPeople).toHaveBeenCalledTimes(2);
  });

  it("keeps the unavailable state only while nothing is cached", async () => {
    const getPeople = vi.fn()
      .mockResolvedValueOnce({ people: [{ id: "one", name: "Ada", thumbnailPath: null }], hasNextPage: false })
      .mockRejectedValueOnce("recognized_people_unavailable:people");
    const people = createPeopleController({ getPeople, getPersonTimelineBuckets: vi.fn(), getPersonTimelineBucketAssets: vi.fn() });

    await people.refresh();
    await people.refresh();

    expect(people.error).toBeNull();
    expect(people.isOffline).toBe(true);
  });
});
