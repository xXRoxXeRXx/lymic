import { describe, expect, it, vi } from "vitest";

const { invoke, confirm, open, disable, enable, isEnabled, getVersion, check, relaunch, listen, isPermissionGranted, sendNotification, openUrl } = vi.hoisted(() => ({
  invoke: vi.fn(),
  confirm: vi.fn(),
  open: vi.fn(),
  disable: vi.fn(),
  enable: vi.fn(),
  isEnabled: vi.fn(),
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
vi.mock("@tauri-apps/plugin-autostart", () => ({ disable, enable, isEnabled }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm, open }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted, sendNotification }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));

import {
  tauriDesktopAdapter,
  type AlbumsDesktopAdapter,
  type AuthDesktopAdapter,
  type PreferencesDesktopAdapter,
  type RemovableVolumeDesktopAdapter,
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
    await albumsAdapter.createAlbum("Summer");
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
      ["create_album", { name: "Summer" }],
      ["get_locale_catalog"],
      ["update_locale", { locale: "de" }],
      ["get_database_recovery_notice"],
      ["record_webview_startup_phase_probe", { phase: "update_check_started" }],
      ["get_release_channel"],
    ]);
    expect(confirm).toHaveBeenCalledWith("Log out?", { title: "Log out", kind: "warning" });
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, title: "Select folder" });
    expect(isEnabled).toHaveBeenCalledOnce();
    expect(enable).toHaveBeenCalledOnce();
    expect(disable).toHaveBeenCalledOnce();
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
});
