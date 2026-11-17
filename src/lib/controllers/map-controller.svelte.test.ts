import { describe, expect, it, vi } from "vitest";
import { createMapController } from "./map-controller.svelte";

describe("Map controller", () => {
  it("retains a successful snapshot when a subsequent entry fails", async () => {
    const adapter = { getMapMarkers: vi.fn().mockResolvedValueOnce([{ id: "one", lat: 0, lon: 0 }]).mockRejectedValueOnce("map_unavailable") };
    const map = createMapController(adapter);
    await map.refresh();
    await map.refresh();
    expect(map.markers).toEqual([{ id: "one", lat: 0, lon: 0 }]);
    expect(map.isOffline).toBe(true);
  });
});

it("discards an old account response after clearing the session", async () => {
  let resolveOld!: (value: { id: string; lat: number; lon: number }[]) => void;
  const adapter = { getMapMarkers: vi.fn().mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValueOnce([{ id: "new", lat: 1, lon: 2 }]) };
  const map = createMapController(adapter);
  const old = map.refresh();
  map.clear();
  await map.refresh();
  resolveOld([{ id: "old", lat: 3, lon: 4 }]);
  await old;
  expect(map.markers.map((marker) => marker.id)).toEqual(["new"]);
});

it("ignores a pending snapshot after abandoning it until the next entry succeeds", async () => {
  let resolveOld!: (value: { id: string; lat: number; lon: number }[]) => void;
  const adapter = { getMapMarkers: vi.fn().mockResolvedValueOnce([{ id: "saved", lat: 0, lon: 0 }]).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValueOnce([{ id: "fresh", lat: 1, lon: 2 }]) };
  const map = createMapController(adapter);
  await map.refresh();
  const old = map.refresh();
  map.abandon();
  resolveOld([{ id: "obsolete", lat: 3, lon: 4 }]);
  await old;
  expect(map.markers.map((marker) => marker.id)).toEqual(["saved"]);
  expect(map.isLoading).toBe(false);
  await map.refresh();
  expect(map.markers.map((marker) => marker.id)).toEqual(["fresh"]);
});

it("tells a permission error from a server error when nothing is cached", async () => {
  const getMapMarkers = vi.fn()
    .mockRejectedValueOnce("map_permission_denied:map.read")
    .mockRejectedValueOnce("map_unavailable")
    .mockRejectedValueOnce(new Error("boom"));
  const map = createMapController({ getMapMarkers });
  await map.refresh();
  expect(map.error).toBe("permission_denied");
  await map.refresh();
  expect(map.error).toBe("unavailable");
  await map.refresh();
  expect(map.error).toBe("error");
  expect(map.hasLoaded).toBe(false);
});

it("keeps the failure kind while a stale snapshot stays visible", async () => {
  const adapter = { getMapMarkers: vi.fn().mockResolvedValueOnce([{ id: "one", lat: 0, lon: 0 }]).mockRejectedValueOnce("map_permission_denied:map.read") };
  const map = createMapController(adapter);
  await map.refresh();
  expect(map.hasLoaded).toBe(true);
  expect(map.error).toBeNull();
  await map.refresh();
  expect(map.error).toBe("permission_denied");
  expect(map.isOffline).toBe(true);
  expect(map.markers).toHaveLength(1);
});

it("reports an empty snapshot as loaded and resets it on clear", async () => {
  const map = createMapController({ getMapMarkers: vi.fn().mockResolvedValue([]) });
  await map.refresh();
  expect(map.hasLoaded).toBe(true);
  expect(map.markers).toEqual([]);
  map.clear();
  expect(map.hasLoaded).toBe(false);
});
