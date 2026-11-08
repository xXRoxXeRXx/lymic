import { describe, expect, it, vi } from "vitest";
import { createAlbumCatalogueController } from "./album-catalogue-controller.svelte";
import type { AlbumCatalogueDesktopAdapter } from "../tauri/desktop-adapter";

describe("Album catalogue controller", () => {
  it("keeps the last successful catalogue visible when a refresh is offline", async () => {
    const adapter: AlbumCatalogueDesktopAdapter = {
      getAlbumCatalogue: vi.fn()
        .mockResolvedValueOnce([
          { id: "shared", albumName: "Shared", albumThumbnailAssetId: "cover", assetCount: 12, isShared: true, order: "z" },
          { id: "empty", albumName: "Empty", albumThumbnailAssetId: null, assetCount: 0, isShared: false, order: "a" },
        ])
        .mockRejectedValueOnce("offline"),
    };
    const catalogue = createAlbumCatalogueController(adapter);

    await catalogue.refresh();
    await catalogue.refresh();

    expect(catalogue.albums.map((album) => album.id)).toEqual(["shared", "empty"]);
    expect(catalogue.isOffline).toBe(true);
    expect(catalogue.error).toBeNull();
  });

  it("exposes an error when the initial catalogue request fails", async () => {
    const catalogue = createAlbumCatalogueController({ getAlbumCatalogue: vi.fn().mockRejectedValue("offline") });
    await catalogue.refresh();
    expect(catalogue.error).toBe("error");
    expect(catalogue.isLoading).toBe(false);
  });
});
