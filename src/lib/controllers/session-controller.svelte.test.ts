import { describe, expect, it, vi } from "vitest";
import { createSessionController, type SessionDesktopAdapter } from "./session-controller.svelte";

function setup(overrides: Partial<SessionDesktopAdapter> = {}) {
  const adapter: SessionDesktopAdapter = {
    getAuthStatus: vi.fn().mockResolvedValue(false),
    getServerUrl: vi.fn().mockResolvedValue("https://example.com"),
    getCurrentUser: vi.fn().mockResolvedValue({ displayName: "Alice", profileImage: null }),
    login: vi.fn().mockResolvedValue(undefined),
    logout: vi.fn().mockResolvedValue(undefined),
    confirm: vi.fn().mockResolvedValue(true),
    startSync: vi.fn().mockResolvedValue({ processed: 0, uploaded: 0, failed: 0 }),
    pauseSync: vi.fn().mockResolvedValue({ status: "PAUSED", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    resumeSync: vi.fn().mockResolvedValue({ status: "RUNNING", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getSyncStatus: vi.fn().mockResolvedValue({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getFailedSyncs: vi.fn().mockResolvedValue([]),
    retryFailedSyncs: vi.fn().mockResolvedValue({ processed: 0, uploaded: 0, failed: 0 }),
    addFolder: vi.fn().mockResolvedValue(1),
    acceptRemovableMediaFolder: vi.fn().mockResolvedValue(1),
    getFolders: vi.fn().mockResolvedValue([]),
    updateFolderExclusionPatterns: vi.fn().mockResolvedValue(undefined),
    removeFolder: vi.fn().mockResolvedValue({ requiresSyncCancellation: false }),
    openFolderDialog: vi.fn().mockResolvedValue(null),
    getAlbums: vi.fn().mockResolvedValue([]),
    createAlbum: vi.fn().mockResolvedValue({ id: "album-1", albumName: "Album" }),
    getAlbumCatalogue: vi.fn().mockResolvedValue([]),
    getTimelineBuckets: vi.fn().mockResolvedValue([]),
    getTimelineBucketAssets: vi.fn().mockResolvedValue([]),
    getFavoritesTimelineBuckets: vi.fn().mockResolvedValue([]),
    getFavoritesTimelineBucketAssets: vi.fn().mockResolvedValue([]),
    getMapMarkers: vi.fn().mockResolvedValue([]),
    getPeople: vi.fn().mockResolvedValue({ people: [], hasNextPage: false }),
    getPersonTimelineBuckets: vi.fn().mockResolvedValue([]),
    getPersonTimelineBucketAssets: vi.fn().mockResolvedValue([]),
    subscribeToRemovableVolumeDetected: vi.fn().mockResolvedValue(() => {}),
    subscribeToRemovableVolumeRemoved: vi.fn().mockResolvedValue(() => {}),
    subscribeToRemovableVolumeRemounted: vi.fn().mockResolvedValue(() => {}),
    isDesktopNotificationPermissionGranted: vi.fn().mockResolvedValue(false),
    sendDesktopNotification: vi.fn(),
    ...overrides,
  };
  const resetView = vi.fn();
  const session = createSessionController({
    adapter,
    t: (key) => key,
    locale: () => "en",
    resetView,
  });
  session.serverUrl = "https://example.com";
  session.apiKey = "key";
  return { session, adapter, resetView };
}

describe("authenticated session", () => {
  it("initializes and clears session-owned controllers across login, account switch, and logout", async () => {
    const { session, resetView } = setup({
      getTimelineBuckets: vi.fn()
        .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 2 }])
        .mockResolvedValueOnce([{ timeBucket: "2024-01-01T00:00:00.000Z", count: 1 }]),
      getFolders: vi.fn()
        .mockResolvedValueOnce([{ id: 1, path: "C:/Alice", recursive: true, target_album_id: null }])
        .mockResolvedValueOnce([{ id: 2, path: "C:/Bob", recursive: true, target_album_id: null }]),
      getAlbums: vi.fn().mockResolvedValue([{ id: "album-1", albumName: "Trips" }]),
      getAlbumCatalogue: vi.fn().mockResolvedValue([{ id: "album-1", albumName: "Trips", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null }]),
      getFavoritesTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]),
      getPeople: vi.fn().mockResolvedValue({ people: [{ id: "person-1", name: "Alice", thumbnailPath: "" }], hasNextPage: false }),
      getMapMarkers: vi.fn().mockResolvedValue([{ id: "asset-1", lat: 48.8, lon: 2.3 }]),
      getCurrentUser: vi.fn()
        .mockResolvedValueOnce({ displayName: "Alice", profileImage: null })
        .mockResolvedValueOnce({ displayName: "Bob", profileImage: null }),
    });

    session.allowLocalHttp = true;
    session.cloudflareClientId = "cf-id";
    session.cloudflareClientSecret = "cf-secret";
    await session.login();

    expect(session.allowLocalHttp).toBe(false);
    expect(session.cloudflareClientId).toBe("");
    expect(session.cloudflareClientSecret).toBe("");
    expect(session.timeline.buckets).toEqual([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 2 }]);
    expect(session.watchedFolders.folders.watchedFolders).toEqual([{ id: 1, path: "C:/Alice", recursive: true, target_album_id: null }]);

    await Promise.all([
      session.targetAlbumCatalogue.refresh(),
      session.albumCatalogue.refresh(),
      session.favorites.refresh(),
      session.people.refresh(),
      session.map.refresh(),
    ]);
    session.feedback.report("Stale error");
    expect(session.targetAlbumCatalogue.albums).toHaveLength(1);
    expect(session.albumCatalogue.albums).toHaveLength(1);
    expect(session.favorites.buckets).toHaveLength(1);
    expect(session.people.people).toHaveLength(1);
    expect(session.map.markers).toHaveLength(1);
    expect(session.feedback.error).toBe("Stale error");

    session.apiKey = "bob-key";
    await session.login();

    expect(resetView).toHaveBeenCalledTimes(2);
    expect(session.currentUserName).toBe("Bob");
    expect(session.timeline.buckets).toEqual([{ timeBucket: "2024-01-01T00:00:00.000Z", count: 1 }]);
    expect(session.watchedFolders.folders.watchedFolders).toEqual([{ id: 2, path: "C:/Bob", recursive: true, target_album_id: null }]);
    expect(session.targetAlbumCatalogue.albums).toEqual([]);
    expect(session.albumCatalogue.albums).toEqual([]);
    expect(session.favorites.buckets).toEqual([]);
    expect(session.people.people).toEqual([]);
    expect(session.map.markers).toEqual([]);
    expect(session.feedback.error).toBe("");

    await session.logout();

    expect(resetView).toHaveBeenCalledTimes(3);
    expect(session.isAuthenticated).toBe(false);
    expect(session.currentUserName).toBe("");
    expect(session.timeline.buckets).toEqual([]);
    expect(session.watchedFolders.folders.watchedFolders).toEqual([]);
  });

  it("does not restore a previous profile when its request completes after logout", async () => {
    let resolveUser!: (value: { displayName: string; profileImage: null }) => void;
    const { session } = setup({ getCurrentUser: () => new Promise((resolve) => { resolveUser = resolve; }) });
    const login = session.login();
    await vi.waitFor(() => expect(resolveUser).toBeDefined());
    await session.logout();
    resolveUser({ displayName: "Alice", profileImage: null });
    await login;
    expect(session.isAuthenticated).toBe(false);
    expect(session.currentUserName).toBe("");
  });

  it("discards pending requests from a previous session across session-owned controllers", async () => {
    let resolveFailures!: (value: { localPath: string; failureReason: string }[]) => void;
    let resolveBuckets!: (value: { timeBucket: string; count: number }[]) => void;
    let resolveFolders!: (value: { id: number; path: string; recursive: boolean; target_album_id: string | null }[]) => void;
    let resolveAlbums!: (value: { id: string; albumName: string }[]) => void;
    let resolvePeople!: (value: { people: { id: string; name: string; thumbnailPath: string }[]; hasNextPage: boolean }) => void;
    let resolveMarkers!: (value: { id: string; lat: number; lon: number }[]) => void;

    const { session } = setup({
      getFailedSyncs: vi.fn()
        .mockImplementationOnce(() => new Promise((resolve) => { resolveFailures = resolve; }))
        .mockResolvedValue([]),
      getTimelineBuckets: vi.fn()
        .mockImplementationOnce(() => new Promise((resolve) => { resolveBuckets = resolve; }))
        .mockResolvedValue([]),
      getFolders: vi.fn()
        .mockImplementationOnce(() => new Promise((resolve) => { resolveFolders = resolve; }))
        .mockResolvedValue([]),
      getAlbums: () => new Promise((resolve) => { resolveAlbums = resolve; }),
      getPeople: () => new Promise((resolve) => { resolvePeople = resolve; }),
      getMapMarkers: () => new Promise((resolve) => { resolveMarkers = resolve; }),
    });

    const login = session.login();
    const pendingAlbums = session.targetAlbumCatalogue.refresh();
    const pendingPeople = session.people.refresh();
    const pendingMap = session.map.refresh();
    await vi.waitFor(() => {
      expect(resolveFailures).toBeDefined();
      expect(resolveBuckets).toBeDefined();
      expect(resolveFolders).toBeDefined();
      expect(resolveAlbums).toBeDefined();
      expect(resolvePeople).toBeDefined();
      expect(resolveMarkers).toBeDefined();
    });

    await session.logout();

    resolveFailures([{ localPath: "old.jpg", failureReason: "failed" }]);
    resolveBuckets([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 5 }]);
    resolveFolders([{ id: 1, path: "C:/Stale", recursive: true, target_album_id: null }]);
    resolveAlbums([{ id: "stale", albumName: "Stale Album" }]);
    resolvePeople({ people: [{ id: "stale", name: "Stale Person", thumbnailPath: "" }], hasNextPage: false });
    resolveMarkers([{ id: "stale", lat: 1, lon: 2 }]);
    await Promise.all([login, pendingAlbums, pendingPeople, pendingMap]);

    expect(session.sync.status.failedSyncs).toEqual([]);
    expect(session.sync.status.syncStatus).toBe("idle");
    expect(session.timeline.buckets).toEqual([]);
    expect(session.watchedFolders.folders.watchedFolders).toEqual([]);
    expect(session.targetAlbumCatalogue.albums).toEqual([]);
    expect(session.people.people).toEqual([]);
    expect(session.map.markers).toEqual([]);
  });

  it("clears state when hydration discovers the session has ended", async () => {
    const { session, resetView } = setup({
      getAuthStatus: vi.fn().mockResolvedValue(false),
      getTimelineBuckets: vi.fn().mockResolvedValue([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }]),
    });
    await session.login();
    expect(session.timeline.buckets).toHaveLength(1);

    await session.hydrate();
    expect(session.isAuthenticated).toBe(false);
    expect(session.currentUserName).toBe("");
    expect(session.timeline.buckets).toEqual([]);
    expect(resetView).toHaveBeenCalledTimes(2);
  });

  it("waits for an in-flight logout before hydrating session status", async () => {
    let authenticated = true;
    let resolveLogout!: () => void;
    const { session } = setup({
      getAuthStatus: vi.fn(async () => authenticated),
      logout: vi.fn(() => new Promise<void>((resolve) => {
        resolveLogout = () => {
          authenticated = false;
          resolve();
        };
      })),
    });

    await session.login();
    const pendingLogout = session.logout();
    await vi.waitFor(() => expect(resolveLogout).toBeDefined());

    const pendingHydrate = session.hydrate();
    resolveLogout();
    await Promise.all([pendingLogout, pendingHydrate]);

    expect(session.isAuthenticated).toBe(false);
  });

  it("ignores a previous account profile after signing in to another account", async () => {
    let resolveFirst!: (value: { displayName: string; profileImage: null }) => void;
    const getCurrentUser = vi.fn()
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ displayName: "Bob", profileImage: null });
    const { session } = setup({ getCurrentUser });
    const first = session.login();
    await vi.waitFor(() => expect(resolveFirst).toBeDefined());
    session.apiKey = "other key";
    await session.login();
    resolveFirst({ displayName: "Alice", profileImage: null });
    await first;
    expect(session.currentUserName).toBe("Bob");
  });
});
