import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  tauriDesktopAdapter,
  type AlbumsDesktopAdapter,
  type AuthDesktopAdapter,
  type PreferencesDesktopAdapter,
  type SyncDesktopAdapter,
  type UpdateDesktopAdapter,
  type WatchedFoldersDesktopAdapter,
} from "./desktop-adapter";

describe("tauriDesktopAdapter", () => {
  const authAdapter: AuthDesktopAdapter = tauriDesktopAdapter;
  const syncAdapter: SyncDesktopAdapter = tauriDesktopAdapter;
  const foldersAdapter: WatchedFoldersDesktopAdapter = tauriDesktopAdapter;
  const albumsAdapter: AlbumsDesktopAdapter = tauriDesktopAdapter;
  const preferencesAdapter: PreferencesDesktopAdapter = tauriDesktopAdapter;
  const updateAdapter: UpdateDesktopAdapter = tauriDesktopAdapter;

  it("delegates every desktop IPC operation through its domain interfaces with backend command and payload", async () => {
    invoke.mockResolvedValue(undefined);

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
    await albumsAdapter.getAlbums();
    await albumsAdapter.createAlbum("Summer");
    await preferencesAdapter.getLocaleCatalog();
    await preferencesAdapter.updateLocale("de");
    await preferencesAdapter.getDatabaseRecoveryNotice();
    await updateAdapter.recordWebviewStartupPhaseProbe("on_mount_started");
    await updateAdapter.getReleaseChannel();

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
      ["create_album", { name: "Summer" }],
      ["get_locale_catalog"],
      ["update_locale", { locale: "de" }],
      ["get_database_recovery_notice"],
      ["record_webview_startup_phase_probe", { phase: "on_mount_started" }],
      ["get_release_channel"],
    ]);
  });
});
