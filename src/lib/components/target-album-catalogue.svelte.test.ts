import { describe, expect, it, vi } from "vitest";
import type { DesktopAdapter } from "../tauri/desktop-adapter";
import { createTargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

const album = (id: string, albumName: string) => ({ id, albumName });

function createDesktopAdapter(overrides: Partial<DesktopAdapter> = {}): DesktopAdapter {
  return {
    startSync: vi.fn(), pauseSync: vi.fn(), resumeSync: vi.fn(), getSyncStatus: vi.fn(), getFailedSyncs: vi.fn(), retryFailedSyncs: vi.fn(),
    getUploadParallelism: vi.fn(), setUploadParallelism: vi.fn(), addFolder: vi.fn(), acceptRemovableMediaFolder: vi.fn(), getFolders: vi.fn(), updateFolderExclusionPatterns: vi.fn(), removeFolder: vi.fn(),
    getAuthStatus: vi.fn(), getServerUrl: vi.fn(), getCurrentUser: vi.fn(), login: vi.fn(), logout: vi.fn(), getAlbums: vi.fn(), createAlbum: vi.fn(),
    getLocaleCatalog: vi.fn(), updateLocale: vi.fn(), getDatabaseRecoveryNotice: vi.fn(), recordWebviewStartupPhaseProbe: vi.fn(),
    getReleaseChannel: vi.fn().mockResolvedValue("github"),
    ...overrides,
  };
}

describe("Target Album catalogue", () => {
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
