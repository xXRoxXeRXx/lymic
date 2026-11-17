import { describe, expect, it, vi } from "vitest";
import type { Place } from "../contracts";
import { createSearchCities } from "./search-cities.svelte";

const place = (city: string, id = `asset-${city}`): Place => ({ city, thumbnailAssetId: id });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function setup(getPlaces = vi.fn().mockResolvedValue([place("Paris"), place("Berlin")])) {
  return { getPlaces, cities: createSearchCities({ getPlaces }) };
}

describe("Search city listing", () => {
  it("is not read until it is requested", () => {
    const { cities, getPlaces } = setup();

    expect(cities.state).toEqual({ status: "idle", items: [], failure: null });
    expect(getPlaces).not.toHaveBeenCalled();
  });

  it("reads the city listing of the Explore Places and offers the names sorted, exactly as returned", async () => {
    const { cities, getPlaces } = setup(vi.fn().mockResolvedValue([place("Paris"), place(" Rio "), place("Zürich"), place("Berlin"), place("Paris", "other")]));

    const loading = cities.load();
    expect(cities.state.status).toBe("loading");
    await loading;

    expect(getPlaces).toHaveBeenCalledTimes(1);
    expect(cities.state).toEqual({ status: "ready", items: [" Rio ", "Berlin", "Paris", "Zürich"], failure: null });
  });

  it("reads once and keeps the listing, including for loads that start while it is in flight", async () => {
    const inFlight = deferred<Place[]>();
    const { cities, getPlaces } = setup(vi.fn().mockReturnValue(inFlight.promise));

    const first = cities.load();
    const second = cities.load();
    inFlight.resolve([place("Paris")]);
    await Promise.all([first, second]);
    await cities.load();

    expect(getPlaces).toHaveBeenCalledTimes(1);
  });

  it("records a failure and reads again on the next load", async () => {
    const getPlaces = vi.fn().mockRejectedValueOnce("places_unavailable").mockResolvedValue([place("Paris")]);
    const { cities } = setup(getPlaces);

    await cities.load();
    expect(cities.state).toEqual({ status: "error", items: [], failure: "unavailable" });

    await cities.load();
    expect(getPlaces).toHaveBeenCalledTimes(2);
    expect(cities.state).toEqual({ status: "ready", items: ["Paris"], failure: null });
  });

  it("tells a missing API key permission apart from other failures", async () => {
    const denied = setup(vi.fn().mockRejectedValue("places_permission_denied:asset.read"));
    await denied.cities.load();
    expect(denied.cities.state.failure).toBe("permission_denied");

    const broken = setup(vi.fn().mockRejectedValue("places_invalid_response"));
    await broken.cities.load();
    expect(broken.cities.state.failure).toBe("error");
  });

  it("never rejects, so a failure cannot break the rest of Search", async () => {
    const { cities } = setup(vi.fn().mockRejectedValue(new Error("boom")));

    await expect(cities.load()).resolves.toBeUndefined();
  });

  it("forgets everything on clear and ignores an answer that arrives later", async () => {
    const inFlight = deferred<Place[]>();
    const { cities, getPlaces } = setup(vi.fn().mockReturnValue(inFlight.promise));

    const loading = cities.load();
    cities.clear();
    inFlight.resolve([place("Paris")]);
    await loading;

    expect(cities.state.status).toBe("idle");
    getPlaces.mockResolvedValue([place("Oslo")]);
    await cities.load();
    expect(cities.state.items).toEqual(["Oslo"]);
  });
});
