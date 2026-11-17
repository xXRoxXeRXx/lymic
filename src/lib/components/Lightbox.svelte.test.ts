import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import "../i18n";
import Lightbox from "./Lightbox.svelte";
import { createRemoteAssetActions } from "./remote-asset-actions.svelte";
import type { AssetDetails, TimelineAsset } from "../contracts";

const photo: TimelineAsset = { id: "map-photo", width: 100, height: 80, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false };
const video: TimelineAsset = { ...photo, id: "map-video", type: "VIDEO" };
afterEach(cleanup);

const photoDetails = { assetType: "IMAGE", takenAt: null, imageWidth: 100, imageHeight: 80, cameraMake: null, cameraModel: null, fileSize: null, isFavorite: false, originalFileName: "IMG_0001.jpg", city: null, state: null, country: null, fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null } as AssetDetails;

function lightboxProps(assetActions: ReturnType<typeof createRemoteAssetActions>) {
  return { asset: photo, assetActions, onClose: vi.fn(), onFavoriteChange: vi.fn(), serverUrl: "", onOpenExternalUrl: vi.fn(), setAssetFavorite: vi.fn(), addAssetsToAlbum: vi.fn() };
}

function assetActions() {
  return createRemoteAssetActions({
    getAssetDetails: vi.fn().mockResolvedValue(null), chooseDownloadDestination: vi.fn(),
    downloadAsset: vi.fn(), downloadArchive: vi.fn(),
  });
}

it("opens one asset and delegates navigation to the caller's available results", async () => {
  const navigateAsset = vi.fn((delta: number) => delta === 1 ? video : null);
  const onClose = vi.fn();
  render(Lightbox, {
    asset: photo, navigateAsset, onClose, onFavoriteChange: vi.fn(), serverUrl: "",
    onOpenExternalUrl: vi.fn(), assetActions: assetActions(),
    setAssetFavorite: vi.fn(), addAssetsToAlbum: vi.fn(),
  });
  expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/map-photo");
  await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));
  expect(navigateAsset).toHaveBeenCalledWith(1, photo);
  expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/map-video");
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).toHaveBeenCalledOnce();
});

it("waits for navigation into another page of a collection", async () => {
  render(Lightbox, {
    asset: photo, navigateAsset: async () => video, onClose: vi.fn(), onFavoriteChange: vi.fn(), serverUrl: "",
    onOpenExternalUrl: vi.fn(), assetActions: assetActions(),
    setAssetFavorite: vi.fn(), addAssetsToAlbum: vi.fn(),
  });
  await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));
  expect(await screen.findByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/map-video");
});

it("does not save a Download whose Lightbox was closed while the destination dialog was open", async () => {
  let chooseDestination!: (path: string | null) => void;
  const chooseDownloadDestination = vi.fn(() => new Promise<string | null>((resolve) => { chooseDestination = resolve; }));
  const downloadAsset = vi.fn(async () => {});
  const actions = createRemoteAssetActions({ getAssetDetails: vi.fn().mockResolvedValue(photoDetails), chooseDownloadDestination, downloadAsset, downloadArchive: vi.fn() });
  const { unmount } = render(Lightbox, lightboxProps(actions));
  const download = await screen.findByRole("button", { name: "Download original" });
  await waitFor(() => expect(download).toBeEnabled());
  await fireEvent.click(download);
  await waitFor(() => expect(chooseDownloadDestination).toHaveBeenCalledWith("IMG_0001.jpg"));

  unmount();
  chooseDestination("C:/Downloads/IMG_0001.jpg");
  await new Promise((resolve) => setTimeout(resolve));

  expect(downloadAsset).not.toHaveBeenCalled();
});

it("reports a failed Download and keeps the asset open", async () => {
  const actions = createRemoteAssetActions({
    getAssetDetails: vi.fn().mockResolvedValue(photoDetails), chooseDownloadDestination: vi.fn().mockResolvedValue("C:/Downloads/IMG_0001.jpg"),
    downloadAsset: vi.fn().mockRejectedValue(new Error("disk full")), downloadArchive: vi.fn(),
  });
  render(Lightbox, lightboxProps(actions));
  const download = await screen.findByRole("button", { name: "Download original" });
  await waitFor(() => expect(download).toBeEnabled());
  await fireEvent.click(download);

  expect(await screen.findByRole("alert")).toHaveTextContent("Could not download original");
});

it("loads details again after the shared details are invalidated, ignoring the request that was in flight", async () => {
  let resolveFirst!: (details: AssetDetails) => void;
  const getAssetDetails = vi.fn<(id: string) => Promise<AssetDetails | null>>()
    .mockReturnValueOnce(new Promise((resolve) => { resolveFirst = resolve; }))
    .mockResolvedValueOnce({ ...photoDetails, originalFileName: "fresh.jpg" });
  const actions = createRemoteAssetActions({ getAssetDetails, chooseDownloadDestination: vi.fn().mockResolvedValue(null), downloadAsset: vi.fn(), downloadArchive: vi.fn() });
  render(Lightbox, lightboxProps(actions));
  await waitFor(() => expect(getAssetDetails).toHaveBeenCalledTimes(1));

  actions.invalidate();
  await waitFor(() => expect(getAssetDetails).toHaveBeenCalledTimes(2));
  resolveFirst({ ...photoDetails, originalFileName: "stale.jpg" });
  await fireEvent.click(await screen.findByRole("button", { name: "Info" }));

  expect(await screen.findByText("fresh.jpg")).toBeInTheDocument();
  expect(screen.queryByText("stale.jpg")).not.toBeInTheDocument();
});

it("closes the Lightbox before it reports the selected Tag chip, and offers no chips without Tags", async () => {
  const calls: string[] = [];
  const rome = { id: "t-rome", name: "Rome", value: "Travel/Italy/Rome", color: null };
  const withTags = createRemoteAssetActions({ getAssetDetails: vi.fn().mockResolvedValue({ ...photoDetails, tags: [rome] }), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(), downloadArchive: vi.fn() });
  const view = render(Lightbox, { ...lightboxProps(withTags), onClose: () => calls.push("close"), onSelectTag: (tag: unknown) => calls.push(`select:${(tag as { id: string }).id}`) });
  await fireEvent.click(screen.getByRole("button", { name: "Info" }));

  await fireEvent.click(await screen.findByRole("button", { name: "Show photos tagged Travel/Italy/Rome" }));

  expect(calls).toEqual(["close", "select:t-rome"]);
  view.unmount();

  const withoutTags = createRemoteAssetActions({ getAssetDetails: vi.fn().mockResolvedValue(photoDetails), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(), downloadArchive: vi.fn() });
  render(Lightbox, { ...lightboxProps(withoutTags), onSelectTag: vi.fn() });
  await fireEvent.click(screen.getByRole("button", { name: "Info" }));
  await screen.findByText("IMG_0001.jpg");
  expect(screen.queryByRole("heading", { name: "Tags" })).not.toBeInTheDocument();
});
