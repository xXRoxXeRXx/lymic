import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopAdapter } from "../tauri/desktop-adapter";
import { createUpdateController } from "./update-controller.svelte";

const tauri = vi.hoisted(() => ({ getVersion: vi.fn(), check: vi.fn(), relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: tauri.getVersion }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: tauri.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: tauri.relaunch }));

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

describe("update controller", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("records startup phases through the desktop adapter", async () => {
    const adapter = createDesktopAdapter();
    tauri.check.mockResolvedValue(null);
    tauri.getVersion.mockResolvedValue("1.0.0");
    const controller = createUpdateController(() => false, adapter);
    controller.startLifecycle();

    expect(controller.isUpdatesSupported).toBe(true);
    expect(controller.releaseChannel).toBe("github");

    controller.startup();

    await vi.waitFor(() => expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("update_check_complete"));
    expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("version_load_started");
    expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("version_load_complete");
  });

  it("does not install an available update while sync is active", async () => {
    const downloadAndInstall = vi.fn();
    tauri.check.mockResolvedValue({ version: "1.0.0", body: "", downloadAndInstall });
    const controller = createUpdateController(() => true, createDesktopAdapter());
    controller.startLifecycle();

    await controller.checkForUpdates();
    await controller.installUpdate();

    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(tauri.relaunch).not.toHaveBeenCalled();
  });

  it("in Store channel, does not contact updater endpoint and disables in-app update checks", async () => {
    const adapter = createDesktopAdapter({ getReleaseChannel: vi.fn().mockResolvedValue("store") });
    tauri.check.mockResolvedValue(null);
    tauri.getVersion.mockResolvedValue("1.0.0");
    const controller = createUpdateController(() => false, adapter, "store");
    controller.startLifecycle();

    expect(controller.isUpdatesSupported).toBe(false);
    expect(controller.releaseChannel).toBe("store");

    controller.startup();

    await vi.waitFor(() => expect(controller.appVersion).toBe("1.0.0"));
    expect(tauri.check).not.toHaveBeenCalled();
    expect(adapter.recordWebviewStartupPhaseProbe).not.toHaveBeenCalledWith("update_check_started");
    expect(adapter.recordWebviewStartupPhaseProbe).not.toHaveBeenCalledWith("update_check_complete");

    await controller.checkForUpdates();
    expect(tauri.check).not.toHaveBeenCalled();
    expect(controller.availableUpdate).toBeNull();
    expect(controller.isUpdateDialogOpen).toBe(false);

    await controller.installUpdate();
    expect(tauri.relaunch).not.toHaveBeenCalled();
  });
});

