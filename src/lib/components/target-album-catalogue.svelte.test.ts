import { describe, expect, it, vi } from "vitest";
import type { AlbumsDesktopAdapter } from "../tauri/desktop-adapter";
import { createTargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

const album = (id: string, albumName: string) => ({ id, albumName });

function createDesktopAdapter(overrides: Partial<AlbumsDesktopAdapter> = {}): AlbumsDesktopAdapter {
  return {
    getAlbums: vi.fn(),
    createAlbum: vi.fn(),
    ...overrides,
  };
}

describe("Target Album catalogue", () => {
  it("clears cached albums and ignores an in-flight refresh", async () => {
    let resolveAlbums: (albums: ReturnType<typeof album>[]) => void;
    const pendingAlbums = new Promise<ReturnType<typeof album>[]>((resolve) => { resolveAlbums = resolve; });
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({ getAlbums: vi.fn().mockReturnValue(pendingAlbums) }));

    const refresh = catalogue.refresh();
    catalogue.clear();
    resolveAlbums!([album("previous-account", "Previous account")]);
    await refresh;

    expect(catalogue.albums).toEqual([]);
    expect(catalogue.error).toBeNull();
    expect(catalogue.hasRequested).toBe(false);
    expect(catalogue.isLoading).toBe(false);
  });

  it("keeps the newest refresh when an older response arrives later", async () => {
    let resolveFirst: (albums: ReturnType<typeof album>[]) => void;
    const first = new Promise<ReturnType<typeof album>[]>((resolve) => { resolveFirst = resolve; });
    const loadAlbums = vi.fn()
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce([album("new", "New catalogue")]);
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({ getAlbums: loadAlbums }));

    const olderRefresh = catalogue.refresh();
    await catalogue.refresh();
    resolveFirst!([album("old", "Stale catalogue")]);
    await olderRefresh;

    expect(catalogue.nameFor("new")).toBe("New catalogue");
    expect(catalogue.nameFor("old")).toBeUndefined();
  });

  it("refreshes after creation and makes the created Target Album resolvable", async () => {
    const loadAlbums = vi.fn()
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);
    const createAlbum = vi.fn().mockResolvedValue(album("roadtrip", "Roadtrip"));
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({ getAlbums: loadAlbums, createAlbum }));

    await catalogue.refresh();
    await catalogue.create("Roadtrip");

    expect(createAlbum).toHaveBeenCalledWith("Roadtrip");
    expect(loadAlbums).toHaveBeenCalledTimes(2);
    expect(catalogue.nameFor("roadtrip")).toBe("Roadtrip");
  });

  it("retains known names when a temporary refresh fails and leaves unavailable albums unresolved", async () => {
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({
      getAlbums: vi.fn()
        .mockResolvedValueOnce([album("known", "Known album")])
        .mockRejectedValueOnce(new Error("Network offline")),
      createAlbum: vi.fn(),
    }));

    await catalogue.refresh();
    await catalogue.refresh();

    expect(catalogue.nameFor("known")).toBe("Known album");
    expect(catalogue.nameFor("unavailable")).toBeUndefined();
    expect(catalogue.error).toBe("Network offline");
  });
});
