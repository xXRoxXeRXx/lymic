import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import AlbumCatalogueView from "./AlbumCatalogueView.svelte";
import type { AlbumCatalogueViewState } from "../controllers/album-catalogue-controller.svelte";

afterEach(cleanup);

function catalogueState(): AlbumCatalogueViewState {
  return {
    albums: [{ id: "shared", albumName: "Shared memories", albumThumbnailAssetId: "cover-1", assetCount: 12, isShared: true, order: "z" }],
    isLoading: false, isRefreshing: false, isOffline: false, error: null, refresh: vi.fn(), clear: vi.fn(),
  };
}

describe("AlbumCatalogueView", () => {
  it("renders a shared album cover and asset count through the credential-safe thumbnail protocol", () => {
    render(AlbumCatalogueView, { catalogue: catalogueState() });
    expect(screen.getByRole("img", { name: "Shared memories" })).toHaveAttribute("src", "lymic-thumb://asset/cover-1");
    expect(screen.getByText("12 assets")).toBeInTheDocument();
    expect(screen.getByLabelText("Shared")).toBeInTheDocument();
  });

  it("offers retry after an initial error", async () => {
    const refresh = vi.fn();
    render(AlbumCatalogueView, { catalogue: { ...catalogueState(), albums: [], error: "error", refresh } });
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refresh).toHaveBeenCalledOnce();
  });
});
