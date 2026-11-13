import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import "../i18n";
import MapView from "./MapView.svelte";
import { createMapController } from "../controllers/map-controller.svelte";
import { createTargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

const mapHarness = vi.hoisted(() => ({ handlers: new Map<string, (...args: never[]) => void>(), features: [] as unknown[], clusterClick: undefined as ((event: unknown) => void) | undefined, leaves: vi.fn(), zoom: 22 }));
vi.mock("maplibre-gl", () => ({
  setWorkerUrl: vi.fn(),
  Map: class {
    addControl() {} remove() {} resize() {} isMoving() { return false; }
    on(event: string, handler: ((...args: never[]) => void) | string, layerHandler?: (...args: never[]) => void) { if (event === "click" && layerHandler) mapHarness.clusterClick = layerHandler as (event: unknown) => void; else if (typeof handler === "function") mapHarness.handlers.set(event, handler); }
    getSource() { return { getClusterExpansionZoom: async () => mapHarness.zoom, getClusterLeaves: mapHarness.leaves }; }
    getZoom() { return 21; }
    getLayer() { return {}; }
    queryRenderedFeatures() { return mapHarness.features; }
  },
  Marker: class {
    constructor(private options: { element: HTMLElement }) {}
    setLngLat() { return this; }
    addTo() { document.body.append(this.options.element); return this; }
    remove() { this.options.element.remove(); }
  },
  NavigationControl: class {},
}));

afterEach(cleanup);

it("shows permission errors from the desktop adapter separately from an empty library", async () => {
  const state = createMapController({ getMapMarkers: vi.fn().mockRejectedValue("map_permission_denied:map.read") });
  await state.refresh();
  const desktop = {
    getAssetDetails: vi.fn(), setAssetFavorite: vi.fn(), chooseDownloadDestination: vi.fn(),
    downloadAsset: vi.fn(), downloadArchive: vi.fn(), addAssetsToAlbum: vi.fn(), openExternalUrl: vi.fn(),
  };
  const albumCatalogue = createTargetAlbumCatalogue({ getAlbums: vi.fn(), createAlbum: vi.fn() });
  render(MapView, { mapState: state, active: true, desktop, albumCatalogue, serverUrl: "" });
  expect(await screen.findByRole("alert")).toHaveTextContent("map.read");
  expect(screen.queryByText("No assets with locations found.")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /refresh|reset/i })).not.toBeInTheDocument();
});


it("opens the selected video thumbnail using the shared Lightbox", async () => {
  const { fireEvent, waitFor } = await import("@testing-library/svelte");
  mapHarness.features = [{ properties: { id: "video-1" }, geometry: { type: "Point", coordinates: [13, 52] } }];
  const state = createMapController({ getMapMarkers: vi.fn() });
  const details = { assetType: "VIDEO", imageWidth: 10, imageHeight: 10, cameraMake: null, cameraModel: null, fileSize: null, isFavorite: false, originalFileName: "video.mp4", city: null, state: null, country: null, fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null };
  const desktop = { getAssetDetails: vi.fn().mockResolvedValue(details), setAssetFavorite: vi.fn(), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(), downloadArchive: vi.fn(), addAssetsToAlbum: vi.fn(), openExternalUrl: vi.fn() };
  const albumCatalogue = createTargetAlbumCatalogue({ getAlbums: vi.fn(), createAlbum: vi.fn() });
  render(MapView, { mapState: state, active: true, desktop, albumCatalogue, serverUrl: "" });
  mapHarness.handlers.get("sourcedata")?.();
  const button = await screen.findByRole("button", { name: "Open asset" });
  expect(button.querySelector("img")).toHaveAttribute("src", "lymic-thumb://asset/video-1");
  await fireEvent.click(button);
  await waitFor(() => expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/video-1"));
  expect(desktop.getAssetDetails).toHaveBeenCalledWith("video-1");
  mapHarness.features = [];
});

it("limits simultaneous visible thumbnail requests", async () => {
  mapHarness.features = Array.from({ length: 12 }, (_, index) => ({ properties: { id: `asset-${index}` }, geometry: { type: "Point", coordinates: [index, 0] } }));
  const state = createMapController({ getMapMarkers: vi.fn() });
  const desktop = { getAssetDetails: vi.fn(), setAssetFavorite: vi.fn(), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(), downloadArchive: vi.fn(), addAssetsToAlbum: vi.fn(), openExternalUrl: vi.fn() };
  const albumCatalogue = createTargetAlbumCatalogue({ getAlbums: vi.fn(), createAlbum: vi.fn() });
  render(MapView, { mapState: state, active: true, desktop, albumCatalogue, serverUrl: "" });
  mapHarness.handlers.get("sourcedata")?.();
  const images = screen.getAllByRole("button", { name: "Open asset" }).map((button) => button.querySelector("img")!);
  expect(images.filter((image) => image.hasAttribute("src"))).toHaveLength(8);
  images[0].dispatchEvent(new Event("load"));
  expect(images.filter((image) => image.hasAttribute("src"))).toHaveLength(9);
  mapHarness.features = [];
});


it("reaches the last member of a large location and navigates across result pages", async () => {
  const { fireEvent, waitFor } = await import("@testing-library/svelte");
  mapHarness.leaves.mockImplementation(async (_cluster: number, limit: number, offset: number) =>
    Array.from({ length: Math.min(limit, 300000 - offset) }, (_, i) => ({ properties: { id: `asset-${offset + i}` } })));
  const details = { assetType: "IMAGE", imageWidth: 10, imageHeight: 10, cameraMake: null, cameraModel: null, fileSize: null, isFavorite: false, originalFileName: null, city: null, state: null, country: null, fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null };
  const desktop = { getAssetDetails: vi.fn().mockResolvedValue(details), setAssetFavorite: vi.fn(), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(), downloadArchive: vi.fn(), addAssetsToAlbum: vi.fn(), openExternalUrl: vi.fn() };
  render(MapView, { mapState: createMapController({ getMapMarkers: vi.fn() }), active: true, desktop, albumCatalogue: createTargetAlbumCatalogue({ getAlbums: vi.fn(), createAlbum: vi.fn() }), serverUrl: "" });
  mapHarness.clusterClick?.({ features: [{ properties: { cluster_id: 1, point_count: 300000 }, geometry: { type: "Point", coordinates: [0, 0] } }] });
  await screen.findByText("1 / 3000");
  expect(screen.getAllByRole("button", { name: "Open asset" })).toHaveLength(100);
  await fireEvent.click(screen.getByRole("button", { name: "Last page" }));
  await screen.findByText("3000 / 3000");
  await fireEvent.click(screen.getAllByRole("button", { name: "Open asset" })[99]);
  await waitFor(() => expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/asset-299999"));
  await fireEvent.click(screen.getByRole("button", { name: "Previous asset" }));
  await waitFor(() => expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/asset-299998"));
  expect(mapHarness.leaves.mock.calls.every((call) => call[1] <= 100)).toBe(true);
});
