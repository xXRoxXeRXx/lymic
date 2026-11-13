import { describe, expect, it, vi } from "vitest";
import { createMapController } from "./map-controller.svelte";

describe("Map controller", () => {
  it("retains a successful snapshot when a subsequent entry fails", async () => {
    const adapter = { getMapMarkers: vi.fn().mockResolvedValueOnce([{ id: "one", lat: 0, lon: 0 }]).mockRejectedValueOnce("map_unavailable") };
    const map = createMapController(adapter);
    await map.refresh();
    await map.refresh();
    expect(map.markers).toEqual([{ id: "one", lat: 0, lon: 0 }]);
    expect(map.stale).toBe(true);
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

it("ignores a pending snapshot after leaving until the next entry succeeds", async () => {
  let resolveOld!: (value: { id: string; lat: number; lon: number }[]) => void;
  const adapter = { getMapMarkers: vi.fn().mockResolvedValueOnce([{ id: "saved", lat: 0, lon: 0 }]).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValueOnce([{ id: "fresh", lat: 1, lon: 2 }]) };
  const map = createMapController(adapter);
  await map.refresh();
  const old = map.refresh();
  map.leave();
  resolveOld([{ id: "obsolete", lat: 3, lon: 4 }]);
  await old;
  expect(map.markers.map((marker) => marker.id)).toEqual(["saved"]);
  expect(map.loading).toBe(false);
  await map.refresh();
  expect(map.markers.map((marker) => marker.id)).toEqual(["fresh"]);
});
