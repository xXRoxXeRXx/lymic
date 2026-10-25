import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  tauriDesktopAdapter,
  type DesktopAdapter,
} from "./desktop-adapter";

describe("tauriDesktopAdapter", () => {
  const adapter: DesktopAdapter = tauriDesktopAdapter;

  it("delegates every desktop IPC operation with its backend command and payload", async () => {
    invoke.mockResolvedValue(undefined);

    await adapter.startSync();
    await adapter.pauseSync();
    await adapter.resumeSync();
    await adapter.getSyncStatus();
    await adapter.getFailedSyncs();
    await adapter.retryFailedSyncs();
    await adapter.getUploadParallelism();
    await adapter.setUploadParallelism(4);
    await adapter.addFolder("C:/Photos", "album-1");
    await adapter.acceptRemovableMediaFolder("volume-1", "E:/DCIM", null);
    await adapter.getFolders();
    await adapter.updateFolderExclusionPatterns(7, ["Screenshots/**"]);
    await adapter.removeFolder(7);
    await adapter.removeFolder(7, true);
    await adapter.getAuthStatus();
    await adapter.getServerUrl();
    await adapter.getCurrentUser();
    await adapter.login({
      serverUrl: "https://immich.example",
      apiKey: "api-key",
      allowLocalHttp: false,
      cloudflareClientId: null,
      cloudflareClientSecret: null,
    });
    await adapter.logout();
    await adapter.getAlbums();
    await adapter.createAlbum("Summer");
    await adapter.getLocaleCatalog();
    await adapter.updateLocale("de");
    await adapter.getDatabaseRecoveryNotice();
    await adapter.recordWebviewStartupPhaseProbe("on_mount_started");
    await adapter.getReleaseChannel();

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
