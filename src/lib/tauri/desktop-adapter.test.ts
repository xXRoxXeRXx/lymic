import { describe, expect, it, vi } from "vitest";

const { invoke, confirm, open, save, getVersion, check, relaunch, listen, isPermissionGranted, sendNotification, openUrl } = vi.hoisted(() => ({
  invoke: vi.fn(),
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  getVersion: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
  listen: vi.fn(),
  isPermissionGranted: vi.fn(),
  sendNotification: vi.fn(),
  openUrl: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm, open, save }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted, sendNotification }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));

import type { TrashMoveResult } from "../contracts";
import {
  tauriDesktopAdapter,
  type AlbumAssignmentDesktopAdapter,
  type AlbumsDesktopAdapter,
  type AlbumCatalogueDesktopAdapter,
  type AlbumTimelineDesktopAdapter,
  type PlaceTimelineDesktopAdapter,
  type AuthDesktopAdapter,
  type FilteredTimelineDesktopAdapter,
  type TrashRestoreDesktopAdapter,
  type TrashMoveDesktopAdapter,
  type AssetVisibilityDesktopAdapter,
  type PreferencesDesktopAdapter,
  type SearchDesktopAdapter,
  type RemovableVolumeDesktopAdapter,
  type SyncDesktopAdapter,
  type TimelineDesktopAdapter,
  type UpdateDesktopAdapter,
  type VirtualLibraryDesktopAdapter,
  type WatchedFoldersDesktopAdapter,
} from "./desktop-adapter";

describe("tauriDesktopAdapter", () => {
  const authAdapter: AuthDesktopAdapter = tauriDesktopAdapter;
  const syncAdapter: SyncDesktopAdapter = tauriDesktopAdapter;
  const foldersAdapter: WatchedFoldersDesktopAdapter = tauriDesktopAdapter;
  const albumsAdapter: AlbumsDesktopAdapter = tauriDesktopAdapter;
  const albumAssignmentAdapter: AlbumAssignmentDesktopAdapter = tauriDesktopAdapter;
  const albumCatalogueAdapter: AlbumCatalogueDesktopAdapter = tauriDesktopAdapter;
  const timelineAdapter: TimelineDesktopAdapter = tauriDesktopAdapter;
  const filteredTimelineAdapter: FilteredTimelineDesktopAdapter = tauriDesktopAdapter;
  const trashRestoreAdapter: TrashRestoreDesktopAdapter = tauriDesktopAdapter;
  const trashMoveAdapter: TrashMoveDesktopAdapter = tauriDesktopAdapter;
  const assetVisibilityAdapter: AssetVisibilityDesktopAdapter = tauriDesktopAdapter;
  const albumTimelineAdapter: AlbumTimelineDesktopAdapter = tauriDesktopAdapter;
  const placeTimelineAdapter: PlaceTimelineDesktopAdapter = tauriDesktopAdapter;
  const preferencesAdapter: PreferencesDesktopAdapter = tauriDesktopAdapter;
  const virtualLibraryAdapter: VirtualLibraryDesktopAdapter = tauriDesktopAdapter;
  const updateAdapter: UpdateDesktopAdapter = tauriDesktopAdapter;
  const removableVolumeAdapter: RemovableVolumeDesktopAdapter = tauriDesktopAdapter;

  it("delegates every desktop IPC operation through its domain interfaces with backend command and payload", async () => {
    invoke.mockResolvedValue(undefined);
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    check.mockResolvedValue({ version: "1.1.0", body: "Release notes", downloadAndInstall });

    await syncAdapter.startSync();
    await syncAdapter.pauseSync();
    await syncAdapter.resumeSync();
    await syncAdapter.getSyncStatus();
    await syncAdapter.getFailedSyncs();
    await syncAdapter.retryFailedSyncs();
    await preferencesAdapter.getUploadParallelism();
    await preferencesAdapter.setUploadParallelism(4);
    await foldersAdapter.addFolder("C:/Photos", "album-1");
    await foldersAdapter.acceptRemovableMediaFolder("volume-1", "E:/DCIM", null);
    await foldersAdapter.getFolders();
    await foldersAdapter.updateFolderExclusionPatterns(7, ["Screenshots/**"]);
    await foldersAdapter.removeFolder(7);
    await foldersAdapter.removeFolder(7, true);
    await authAdapter.getAuthStatus();
    await authAdapter.getServerUrl();
    await authAdapter.getCurrentUser();
    await authAdapter.login({
      serverUrl: "https://immich.example",
      apiKey: "api-key",
      allowLocalHttp: false,
      cloudflareClientId: null,
      cloudflareClientSecret: null,
    });
    await authAdapter.logout();
    await authAdapter.confirm("Log out?", { title: "Log out", kind: "warning" });
    await albumsAdapter.getAlbums();
    await albumCatalogueAdapter.getAlbumCatalogue();
    await albumsAdapter.createAlbum("Summer");
    await albumAssignmentAdapter.addAssetsToAlbum(["asset-1", "asset-2"], "album-1");
    await timelineAdapter.getTimelineBuckets();
    await timelineAdapter.getTimelineBucketAssets("2025-03-01T00:00:00.000Z");
    await filteredTimelineAdapter.getFilteredTimelineBuckets("favorites");
    await filteredTimelineAdapter.getFilteredTimelineBucketAssets("favorites", "2025-03-01T00:00:00.000Z");
    await filteredTimelineAdapter.getFilteredTimelineBuckets("archive");
    await filteredTimelineAdapter.getFilteredTimelineBucketAssets("archive", "2025-03-01T00:00:00.000Z");
    await filteredTimelineAdapter.getFilteredTimelineBuckets("trash");
    await filteredTimelineAdapter.getFilteredTimelineBucketAssets("trash", "2025-03-01T00:00:00.000Z");
    await trashRestoreAdapter.restoreTrashedAssets(["trash-1", "trash-2"]);
    await trashMoveAdapter.trashAssets(["asset-1", "asset-2"]);
    await assetVisibilityAdapter.setAssetsVisibility(["archived-1", "archived-2"], "timeline");
    await tauriDesktopAdapter.getAssetDetails("asset-1");
    await tauriDesktopAdapter.setAssetFavorite("asset-1", true);
    await tauriDesktopAdapter.chooseDownloadDestination("IMG_0001.CR3");
    await tauriDesktopAdapter.downloadAsset("asset-1", "C:/Downloads/IMG_0001.CR3");
    await tauriDesktopAdapter.downloadArchive(["asset-1", "asset-2"]);
    await albumTimelineAdapter.getAlbumTimeline("album-1");
    await placeTimelineAdapter.getPlaceTimeline("Paris");
    await tauriDesktopAdapter.getPersonTimelineBuckets("person-1");
    await tauriDesktopAdapter.getPersonTimelineBucketAssets("person-1", "2025-03-01T00:00:00.000Z");
    await tauriDesktopAdapter.getTagTimelineBuckets("tag-1");
    await tauriDesktopAdapter.getTagTimelineBucketAssets("tag-1", "2025-03-01T00:00:00.000Z");
    await preferencesAdapter.getLocaleCatalog();
    await preferencesAdapter.updateLocale("de");
    await preferencesAdapter.getDatabaseRecoveryNotice();
    await preferencesAdapter.isAutostartEnabled();
    await preferencesAdapter.setAutostartEnabled(true);
    await preferencesAdapter.setAutostartEnabled(false);
    await updateAdapter.recordWebviewStartupPhaseProbe("update_check_started");
    await updateAdapter.getReleaseChannel();
    await updateAdapter.getAppVersion();
    await updateAdapter.checkForUpdate();
    await updateAdapter.downloadAndInstallUpdate(vi.fn());
    await updateAdapter.relaunchApp();
    await updateAdapter.openExternalUrl("https://example.com");
    await foldersAdapter.openFolderDialog({ directory: true, multiple: false, title: "Select folder" });
    await removableVolumeAdapter.isDesktopNotificationPermissionGranted();
    removableVolumeAdapter.sendDesktopNotification({ title: "Card detected", body: "EOS" });
    await removableVolumeAdapter.subscribeToRemovableVolumeDetected(vi.fn());
    await removableVolumeAdapter.subscribeToRemovableVolumeRemoved(vi.fn());
    await removableVolumeAdapter.subscribeToRemovableVolumeRemounted(vi.fn());

    expect(invoke.mock.calls).toEqual([
      ["start_sync"],
      ["pause_sync"],
      ["resume_sync"],
      ["get_sync_status"],
      ["get_failed_syncs"],
      ["retry_failed_syncs"],
      ["get_upload_parallelism"],
      ["set_upload_parallelism", { uploadParallelism: 4 }],
      ["add_folder", { path: "C:/Photos", targetAlbumId: "album-1" }],
      ["accept_removable_media_folder", { volumeId: "volume-1", path: "E:/DCIM", targetAlbumId: null }],
      ["get_folders"],
      ["update_folder_exclusion_patterns", { id: 7, exclusionPatterns: ["Screenshots/**"] }],
      ["remove_folder", { id: 7 }],
      ["remove_folder", { id: 7, confirmCancellation: true }],
      ["get_auth_status"],
      ["get_server_url"],
      ["get_current_user"],
      ["login", {
        serverUrl: "https://immich.example",
        apiKey: "api-key",
        allowLocalHttp: false,
        cloudflareClientId: null,
        cloudflareClientSecret: null,
      }],
      ["logout"],
      ["get_albums"],
      ["get_album_catalogue"],
      ["create_album", { name: "Summer" }],
      ["add_assets_to_album", { assetIds: ["asset-1", "asset-2"], albumId: "album-1" }],
      ["get_timeline_buckets"],
      ["get_timeline_bucket_assets", { timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["get_filtered_timeline_buckets", { timeline: "favorites" }],
      ["get_filtered_timeline_bucket_assets", { timeline: "favorites", timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["get_filtered_timeline_buckets", { timeline: "archive" }],
      ["get_filtered_timeline_bucket_assets", { timeline: "archive", timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["get_filtered_timeline_buckets", { timeline: "trash" }],
      ["get_filtered_timeline_bucket_assets", { timeline: "trash", timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["restore_trashed_assets", { assetIds: ["trash-1", "trash-2"] }],
      ["trash_assets", { assetIds: ["asset-1", "asset-2"] }],
      ["set_assets_visibility", { assetIds: ["archived-1", "archived-2"], visibility: "timeline" }],
      ["get_asset_details", { assetId: "asset-1" }],
      ["set_asset_favorite", { assetId: "asset-1", isFavorite: true }],
      ["download_asset", { assetId: "asset-1", destinationPath: "C:/Downloads/IMG_0001.CR3" }],
      ["download_archive", { assetIds: ["asset-1", "asset-2"] }],
      ["get_album_timeline", { albumId: "album-1" }],
      ["get_place_timeline", { city: "Paris" }],
      ["get_person_timeline_buckets", { personId: "person-1" }],
      ["get_person_timeline_bucket_assets", { personId: "person-1", timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["get_tag_timeline_buckets", { tagId: "tag-1" }],
      ["get_tag_timeline_bucket_assets", { tagId: "tag-1", timeBucket: "2025-03-01T00:00:00.000Z" }],
      ["get_locale_catalog"],
      ["update_locale", { locale: "de" }],
      ["get_database_recovery_notice"],
      ["get_autostart_enabled"],
      ["set_autostart_enabled", { enabled: true }],
      ["set_autostart_enabled", { enabled: false }],
      ["record_webview_startup_phase_probe", { phase: "update_check_started" }],
      ["get_release_channel"],
    ]);
    expect(confirm).toHaveBeenCalledWith("Log out?", { title: "Log out", kind: "warning" });
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, title: "Select folder" });
    expect(save).toHaveBeenCalledWith({ defaultPath: "IMG_0001.CR3" });
    expect(getVersion).toHaveBeenCalledOnce();
    expect(check).toHaveBeenCalledOnce();
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(relaunch).toHaveBeenCalledOnce();
    expect(openUrl).toHaveBeenCalledWith("https://example.com");
    expect(isPermissionGranted).toHaveBeenCalledOnce();
    expect(sendNotification).toHaveBeenCalledWith({ title: "Card detected", body: "EOS" });
    expect(listen.mock.calls).toEqual([
      ["removable-volume-detected", expect.any(Function)],
      ["removable-volume-removed", expect.any(Function)],
      ["removable-volume-remounted", expect.any(Function)],
    ]);
  });

  it("reads and persists the Virtual Library settings and forwards the backend's change announcements", async () => {
    const settings = { available: true, enabled: true, root_folder: "C:\\Users\\Me\\Pictures\\Lymic" };
    invoke.mockResolvedValueOnce(settings).mockResolvedValueOnce(settings).mockResolvedValueOnce(settings);
    listen.mockImplementationOnce(async (_event: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: settings });
      return () => {};
    });
    const listener = vi.fn();

    await expect(virtualLibraryAdapter.getVirtualLibrarySettings()).resolves.toEqual(settings);
    await expect(virtualLibraryAdapter.setVirtualLibraryEnabled(true)).resolves.toEqual(settings);
    await expect(virtualLibraryAdapter.setVirtualLibraryRootFolder("D:\\Photos")).resolves.toEqual(settings);
    await virtualLibraryAdapter.subscribeToVirtualLibrarySettings(listener);

    expect(invoke).toHaveBeenCalledWith("get_virtual_library_settings");
    expect(invoke).toHaveBeenCalledWith("set_virtual_library_enabled", { enabled: true });
    expect(invoke).toHaveBeenCalledWith("set_virtual_library_root_folder", { path: "D:\\Photos" });
    expect(listen).toHaveBeenCalledWith("virtual-library-settings-changed", expect.any(Function));
    expect(listener).toHaveBeenCalledWith(settings);
  });

  it("returns the partial result of Move to Trash with its failure code as the backend reports it", async () => {
    const partial: TrashMoveResult = { movedAssetIds: ["asset-1"], failureCode: "trash_move_permission_denied:asset.delete" };
    invoke.mockClear();
    invoke.mockResolvedValueOnce(partial);

    const result = await trashMoveAdapter.trashAssets(["asset-1", "asset-2"]);

    expect(invoke).toHaveBeenCalledWith("trash_assets", { assetIds: ["asset-1", "asset-2"] });
    expect(result).toEqual({ movedAssetIds: ["asset-1"], failureCode: "trash_move_permission_denied:asset.delete" });
  });

  it("invokes the Search commands with the request as one argument", async () => {
    const searchAdapter: SearchDesktopAdapter = tauriDesktopAdapter;
    invoke.mockClear();

    await searchAdapter.getSearchSupport();
    await searchAdapter.getSearchFeatures();
    await searchAdapter.searchAssets({ text: "IMG", mode: "filename", filters: {} });
    await searchAdapter.searchAssets({ text: "scan", mode: "folderPath", filters: {} });
    await searchAdapter.searchContextPage({ text: "dog", mode: "context", filters: {} }, 2);
    await searchAdapter.getSearchSuggestions({ type: "cameraMake" });
    await searchAdapter.getSearchSuggestions({ type: "cameraModel", make: "Canon" });

    expect(invoke.mock.calls).toEqual([
      ["get_search_support"],
      ["get_search_features"],
      ["search_assets", { request: { text: "IMG", mode: "filename", filters: {} } }],
      ["search_assets", { request: { text: "scan", mode: "folderPath", filters: {} } }],
      ["search_context_page", { request: { text: "dog", mode: "context", filters: {} }, page: 2 }],
      ["get_search_suggestions", { request: { type: "cameraMake" } }],
      ["get_search_suggestions", { request: { type: "cameraModel", make: "Canon" } }],
    ]);
  });
});
