import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopAdapter } from "../tauri/desktop-adapter";
import { createSyncController } from "./sync-controller.svelte";

const dialog = vi.hoisted(() => ({ confirm: vi.fn(), open: vi.fn() }));
const events = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("@tauri-apps/api/event", () => events);

function createDesktopAdapter(overrides: Partial<DesktopAdapter> = {}): DesktopAdapter {
  return {
    startSync: vi.fn().mockResolvedValue({ processed: 1, uploaded: 1, failed: 0 }),
    pauseSync: vi.fn().mockResolvedValue({ status: "PAUSED", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    resumeSync: vi.fn().mockResolvedValue({ status: "RUNNING", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getSyncStatus: vi.fn().mockResolvedValue({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getFailedSyncs: vi.fn().mockResolvedValue([]),
    retryFailedSyncs: vi.fn().mockResolvedValue({ processed: 1, uploaded: 1, failed: 0 }),
    getUploadParallelism: vi.fn(),
    setUploadParallelism: vi.fn(),
    addFolder: vi.fn().mockResolvedValue(1),
    acceptRemovableMediaFolder: vi.fn().mockResolvedValue(1),
    getFolders: vi.fn().mockResolvedValue([]),
    removeFolder: vi.fn().mockResolvedValue({ requiresSyncCancellation: false }),
    getAuthStatus: vi.fn(),
    getServerUrl: vi.fn(),
    getCurrentUser: vi.fn(),
    login: vi.fn(),
    logout: vi.fn(),
    getAlbums: vi.fn(),
    createAlbum: vi.fn(),
    getLocaleCatalog: vi.fn(),
    updateLocale: vi.fn(),
    getDatabaseRecoveryNotice: vi.fn(),
    recordWebviewStartupPhaseProbe: vi.fn(),
    ...overrides,
  };
}

function createController(adapter: DesktopAdapter, overrides = {}) {
  return createSyncController({
    adapter,
    reportError: vi.fn(),
    t: (key: string) => key,
    locale: () => "en",
    ...overrides,
  });
}

beforeEach(() => {
  dialog.confirm.mockClear();
  dialog.open.mockClear();
  events.listen.mockClear();
});

afterEach(() => vi.useRealTimers());

describe("sync controller", () => {
  it("clears the progress reset timer on disposal", async () => {
    vi.useFakeTimers();
    const adapter = createDesktopAdapter({ getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/photos" }]) });
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();
    await controller.dashboard.onSyncAction();

    expect(controller.progress).toBe(100);
    controller.destroy();
    vi.advanceTimersByTime(2000);
    expect(controller.progress).toBe(100);
  });

  it("exposes dashboard state and intentions through one view-state interface", async () => {
    const adapter = createDesktopAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/photos" }]),
      getSyncStatus: vi.fn().mockResolvedValue({ status: "PAUSED", total: 4, succeeded: 1, failed: 0, currentPath: "C:/photos/two.jpg" }),
    });
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();

    expect(controller.dashboard).toMatchObject({ syncStatus: "paused", processed: 1, syncTotal: 4, currentFile: "two.jpg", watchedFolders: [{ id: 1, path: "C:/photos" }] });
  });

  it("pauses and resumes synchronization through the adapter", async () => {
    const adapter = createDesktopAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/photos" }]),
      getSyncStatus: vi.fn().mockResolvedValue({ status: "RUNNING", total: 4, succeeded: 1, failed: 0, currentPath: null }),
      pauseSync: vi.fn().mockResolvedValue({ status: "PAUSED", total: 4, succeeded: 1, failed: 0, currentPath: null }),
      resumeSync: vi.fn().mockResolvedValue({ status: "RUNNING", total: 4, succeeded: 1, failed: 0, currentPath: null }),
    });
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();

    await controller.syncAction();
    await controller.syncAction();

    expect(adapter.pauseSync).toHaveBeenCalledOnce();
    expect(adapter.resumeSync).toHaveBeenCalledOnce();
    expect(controller.syncStatus).toBe("syncing");
  });

  it("retries failed synchronization through the adapter", async () => {
    const adapter = createDesktopAdapter();
    const controller = createController(adapter);
    controller.startLifecycle();
    controller.dashboard.failedSyncs.push({ localPath: "C:/photos/failed.jpg", failureReason: "temporary failure" });

    await controller.dashboard.onRetryFailedSyncs();

    expect(adapter.retryFailedSyncs).toHaveBeenCalledOnce();
    expect(controller.dashboard.syncStatus).toBe("idle");
  });

  it("removes a folder after the required sync cancellation warning", async () => {
    const adapter = createDesktopAdapter({ removeFolder: vi.fn().mockResolvedValue({ requiresSyncCancellation: true }) });
    dialog.confirm.mockResolvedValue(true);
    const controller = createController(adapter);

    await controller.dashboard.onRemoveFolder(4);

    expect(dialog.confirm).toHaveBeenCalledWith("remove_folder_sync_warning", { title: "remove_folder", kind: "warning" });
    expect(adapter.removeFolder).toHaveBeenNthCalledWith(1, 4);
    expect(adapter.removeFolder).toHaveBeenNthCalledWith(2, 4, true);
  });

  it("does not remove a folder after a declined confirmation", async () => {
    const adapter = createDesktopAdapter();
    dialog.confirm.mockResolvedValue(false);
    const controller = createController(adapter);

    await controller.dashboard.onRemoveFolder(4);

    expect(adapter.removeFolder).toHaveBeenCalledOnce();
    expect(adapter.removeFolder).toHaveBeenCalledWith(4);
  });

  it("opens the target album modal when a folder is selected", async () => {
    const adapter = createDesktopAdapter();
    dialog.open.mockResolvedValue("C:/media/photos");
    const controller = createController(adapter);

    await controller.dashboard.onAddFolder();

    expect(controller.isTargetAlbumModalOpen).toBe(true);
    expect(controller.pendingFolderPath).toBe("C:/media/photos");
    expect(adapter.addFolder).not.toHaveBeenCalled();
  });

  it("adds a folder with its target album and refreshes the folder list", async () => {
    const adapter = createDesktopAdapter({ getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/media/photos", target_album_id: "alb-99" }]) });
    dialog.open.mockResolvedValue("C:/media/photos");
    const controller = createController(adapter);

    await controller.dashboard.onAddFolder();
    await controller.dashboard.onConfirmAddFolder("alb-99");

    expect(adapter.addFolder).toHaveBeenCalledWith("C:/media/photos", "alb-99");
    expect(controller.watchedFolders).toEqual([{ id: 1, path: "C:/media/photos", target_album_id: "alb-99" }]);
  });

  it("adds a removable folder with its volume and target album", async () => {
    const adapter = createDesktopAdapter({ getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "E:/DCIM", target_album_id: "alb-vacation" }]) });
    dialog.open.mockResolvedValue("E:/DCIM");
    const onRemovableVolumeAccepted = vi.fn();
    const controller = createController(adapter, { onRemovableVolumeAccepted });

    await controller.acceptRemovableFolder({ volumeId: "vol-sd-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" });
    await controller.dashboard.onConfirmAddFolder("alb-vacation");

    expect(adapter.acceptRemovableMediaFolder).toHaveBeenCalledWith("vol-sd-1", "E:/DCIM", "alb-vacation");
    expect(onRemovableVolumeAccepted).toHaveBeenCalledWith("vol-sd-1");
  });

  it("reports adapter errors when adding a removable folder", async () => {
    const error = new Error("Path does not reside within removable volume");
    const adapter = createDesktopAdapter({ acceptRemovableMediaFolder: vi.fn().mockRejectedValue(error) });
    dialog.open.mockResolvedValue("C:/Photos");
    const reportError = vi.fn();
    const controller = createController(adapter, { reportError });

    await controller.acceptRemovableFolder({ volumeId: "vol-sd-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" });
    await controller.dashboard.onConfirmAddFolder(null);

    expect(reportError).toHaveBeenCalledWith(error);
  });
});
