import { describe, expect, it, vi } from "vitest";
import { createPlacesController } from "./places-controller.svelte";
import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";

const paris = { city: "Paris", thumbnailAssetId: "asset-1" };
const berlin = { city: "Berlin", thumbnailAssetId: "asset-2" };

describe("Places controller", () => {
  it("loads every Place and reports loading until the first result arrives", async () => {
    let resolve!: (places: typeof paris[]) => void;
    const adapter: PlacesDesktopAdapter = { getPlaces: vi.fn(() => new Promise<typeof paris[]>((r) => { resolve = r; })) };
    const places = createPlacesController(adapter);

    const loading = places.refresh();
    expect(places.isLoading).toBe(true);
    resolve([paris, berlin]);
    await loading;

    expect(places.isLoading).toBe(false);
    expect(places.places).toEqual([paris, berlin]);
    expect(places.error).toBeNull();
    expect(places.isOffline).toBe(false);
  });

  it("keeps stale Places visible with an offline indication when a refresh fails", async () => {
    const adapter: PlacesDesktopAdapter = {
      getPlaces: vi.fn().mockResolvedValueOnce([paris]).mockRejectedValueOnce(new Error("offline")),
    };
    const places = createPlacesController(adapter);
    await places.refresh();
    await places.refresh();

    expect(places.places).toEqual([paris]);
    expect(places.isOffline).toBe(true);
    expect(places.error).toBeNull();
  });

  it("classifies a permission-denied rejection separately from other failures", async () => {
    const adapter: PlacesDesktopAdapter = {
      getPlaces: vi.fn()
        .mockRejectedValueOnce("places_permission_denied:configure an API key with asset.read permission")
        .mockRejectedValueOnce("places_unavailable")
        .mockRejectedValueOnce("places_authentication_required")
        .mockRejectedValueOnce(new Error("boom"))
        .mockResolvedValueOnce([paris]),
    };
    const places = createPlacesController(adapter);

    await places.refresh();
    expect(places.error).toBe("permission_denied");
    await places.refresh();
    expect(places.error).toBe("unavailable");
    await places.refresh();
    expect(places.error).toBe("unavailable");
    await places.refresh();
    expect(places.error).toBe("error");
    await places.refresh();
    expect(places.error).toBeNull();
  });

  it("ignores a response that arrives after the controller was cleared", async () => {
    let resolve!: (places: typeof paris[]) => void;
    const adapter: PlacesDesktopAdapter = { getPlaces: vi.fn(() => new Promise<typeof paris[]>((r) => { resolve = r; })) };
    const places = createPlacesController(adapter);

    const pending = places.refresh();
    places.clear();
    resolve([paris]);
    await pending;

    expect(places.places).toEqual([]);
    expect(places.isLoading).toBe(false);
  });
});
