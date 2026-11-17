import { describe, expect, it, vi } from "vitest";
import type { AlbumsDesktopAdapter } from "../tauri/desktop-adapter";
import { createTargetAlbumCatalogue } from "./target-album-catalogue.svelte";

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

  it("clears loaded and pending Target Albums", async () => {
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({
      getAlbums: vi.fn()
        .mockResolvedValueOnce([album("previous", "Previous account")])
        .mockResolvedValueOnce([]),
      createAlbum: vi.fn().mockResolvedValue(album("pending", "Pending album")),
    }));

    await catalogue.refresh();
    await catalogue.create("Pending album");
    catalogue.clear();

    expect(catalogue.albums).toEqual([]);
    expect(catalogue.nameFor("previous")).toBeUndefined();
    expect(catalogue.nameFor("pending")).toBeUndefined();
  });

  it("keeps the newest refresh when a response from before a clear arrives later", async () => {
    let resolveFirst: (albums: ReturnType<typeof album>[]) => void;
    const first = new Promise<ReturnType<typeof album>[]>((resolve) => { resolveFirst = resolve; });
    const loadAlbums = vi.fn()
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce([album("new", "New catalogue")]);
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({ getAlbums: loadAlbums }));

    const olderRefresh = catalogue.refresh();
    catalogue.clear();
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
    expect(catalogue.error).toBeNull();
  });

  it("reports the failure when the first load fails with nothing cached", async () => {
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({
      getAlbums: vi.fn().mockRejectedValue(new Error("Network offline")),
    }));

    await catalogue.refresh();

    expect(catalogue.error).toBe("Network offline");
    expect(catalogue.isLoading).toBe(false);
  });

  it("adds a created Target Album in sorted order before the refresh returns it", async () => {
    const catalogue = createTargetAlbumCatalogue(createDesktopAdapter({
      getAlbums: vi.fn().mockResolvedValue([album("a", "Alpha"), album("z", "zulu")]),
      createAlbum: vi.fn().mockResolvedValue(album("m", "Mike")),
    }));

    await catalogue.refresh();
    await catalogue.create("Mike");

    expect(catalogue.albums.map((entry) => entry.albumName)).toEqual(["Alpha", "Mike", "zulu"]);
  });

  it("selects enriched album-list responses while the read-only catalogue remains a separate adapter method", async () => {
    const enrichedAlbum = {
      id: "family",
      albumName: "Family",
      albumThumbnailAssetId: "cover-1",
      assetCount: 12,
      isShared: true,
      order: "z",
    };
    const adapter = createDesktopAdapter({ getAlbums: vi.fn().mockResolvedValue([enrichedAlbum]) });
    const catalogue = createTargetAlbumCatalogue(adapter);

    await catalogue.refresh();

    expect(catalogue.nameFor("family")).toBe("Family");
    expect(catalogue.albums).toEqual([enrichedAlbum]);
    expect(adapter.getAlbums).toHaveBeenCalledOnce();
    expect("getAlbumCatalogue" in adapter).toBe(false);
  });
});
