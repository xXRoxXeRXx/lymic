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
  it("ignores a response that arrives after clear", async () => {
    let resolveLoad!: (albums: never[]) => void;
    const pending = new Promise<never[]>((resolve) => { resolveLoad = resolve; });
    const catalogue = createAlbumCatalogueController({ getAlbumCatalogue: vi.fn().mockReturnValue(pending) });

    const refresh = catalogue.refresh();
    catalogue.clear();
    resolveLoad([{ id: "previous", albumName: "Previous account", albumThumbnailAssetId: null, assetCount: 0, isShared: false, order: "a" }] as never[]);
    await refresh;

    expect(catalogue.albums).toEqual([]);
    expect(catalogue.isLoading).toBe(false);
    expect(catalogue.error).toBeNull();
  });

  it("keeps the newest catalogue when an older response arrives after a clear and a newer refresh", async () => {
    const entry = (id: string) => ({ id, albumName: id, albumThumbnailAssetId: null, assetCount: 0, isShared: false, order: "a" });
    let resolveOlder!: (albums: ReturnType<typeof entry>[]) => void;
    const older = new Promise<ReturnType<typeof entry>[]>((resolve) => { resolveOlder = resolve; });
    const catalogue = createAlbumCatalogueController({
      getAlbumCatalogue: vi.fn().mockReturnValueOnce(older).mockResolvedValueOnce([entry("new")]),
    });

    const olderRefresh = catalogue.refresh();
    catalogue.clear();
    await catalogue.refresh();
    resolveOlder([entry("stale")]);
    await olderRefresh;

    expect(catalogue.albums.map((album) => album.id)).toEqual(["new"]);
  });
});
