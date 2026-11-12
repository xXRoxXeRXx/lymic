import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, expect, it, vi } from "vitest";
import "../i18n";
import Lightbox from "./Lightbox.svelte";
import type { TimelineAsset } from "../contracts";

const photo: TimelineAsset = { id: "map-photo", width: 100, height: 80, type: "IMAGE", takenAt: null, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false };
const video: TimelineAsset = { ...photo, id: "map-video", type: "VIDEO" };
afterEach(cleanup);

it("opens one asset and delegates navigation to the caller's available results", async () => {
  const navigateAsset = vi.fn((delta: number) => delta === 1 ? video : null);
  const onClose = vi.fn();
  render(Lightbox, {
    asset: photo, navigateAsset, onClose, onFavoriteChange: vi.fn(), serverUrl: "",
    onOpenExternalUrl: vi.fn(), getAssetDetails: vi.fn().mockResolvedValue(null),
    setAssetFavorite: vi.fn(), chooseDownloadDestination: vi.fn(), downloadAsset: vi.fn(),
    addAssetsToAlbum: vi.fn(),
  });
  expect(screen.getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/map-photo");
  await fireEvent.click(screen.getByRole("button", { name: "Next asset" }));
  expect(navigateAsset).toHaveBeenCalledWith(1, photo);
  expect(screen.getByLabelText("Video preview")).toHaveAttribute("src", "lymic-asset://video/map-video");
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).toHaveBeenCalledOnce();
});
