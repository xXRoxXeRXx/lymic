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
  it("renders a shared album cover and selects the album through the credential-safe thumbnail protocol", async () => {
    const onSelect = vi.fn();
    render(AlbumCatalogueView, { catalogue: catalogueState(), onSelect });
    expect(screen.getByRole("img", { name: "Shared memories" })).toHaveAttribute("src", "lymic-thumb://asset/cover-1");
    expect(screen.getByText("12 assets")).toBeInTheDocument();
    expect(screen.getByLabelText("Shared")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: /Shared memories/ }));
    expect(onSelect).toHaveBeenCalledWith(catalogueState().albums[0]);
  });

  it("uses Tauri convertFileSrc when resolving album cover URLs in WebView2", () => {
    const globals = globalThis as typeof globalThis & {
      __TAURI_INTERNALS__?: { convertFileSrc?: (filePath: string, protocol?: string) => string };
    };
    const previous = globals.__TAURI_INTERNALS__;
    globals.__TAURI_INTERNALS__ = {
      convertFileSrc: (_filePath, protocol) => `http://${protocol}.localhost/`,
    };
    try {
      render(AlbumCatalogueView, { catalogue: catalogueState(), onSelect: vi.fn() });
      expect(screen.getByRole("img", { name: "Shared memories" })).toHaveAttribute(
        "src",
        "http://lymic-thumb.localhost/asset/cover-1",
      );
    } finally {
      globals.__TAURI_INTERNALS__ = previous;
    }
  });

  it("offers retry after an initial error", async () => {
    const refresh = vi.fn();
    render(AlbumCatalogueView, { catalogue: { ...catalogueState(), albums: [], error: "error", refresh }, onSelect: vi.fn() });
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("renders missing covers and an empty catalogue without attempting a thumbnail request", () => {
    const { rerender } = render(AlbumCatalogueView, {
      catalogue: {
        ...catalogueState(),
        albums: [{ id: "empty", albumName: "Empty album", albumThumbnailAssetId: null, assetCount: 0, isShared: false, order: null }],
      },
      onSelect: vi.fn(),
    });

    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("Empty album")).toBeInTheDocument();
    expect(screen.getByText("0 assets")).toBeInTheDocument();

    rerender({ catalogue: { ...catalogueState(), albums: [] }, onSelect: vi.fn() });
    expect(screen.getByText("No accessible albums yet.")).toBeInTheDocument();
  });
});
