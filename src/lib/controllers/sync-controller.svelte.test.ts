import { afterEach, describe, expect, it, vi } from "vitest";
import { createSyncController } from "./sync-controller.svelte";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn(), open: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: tauri.confirm, open: tauri.open }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

afterEach(() => vi.useRealTimers());

describe("sync controller", () => {
  it("clears the progress reset timer on disposal", async () => {
    vi.useFakeTimers();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_folders") return Promise.resolve([{ id: 1, path: "C:/photos" }]);
      if (command === "get_sync_status") return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      if (command === "start_sync") return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      if (command === "get_failed_syncs") return Promise.resolve([]);
      return Promise.resolve();
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });
    controller.startLifecycle();
    await controller.hydrate();
    await controller.dashboard.onSyncAction();

    expect(controller.progress).toBe(100);
    controller.destroy();
    vi.advanceTimersByTime(2000);

    expect(controller.progress).toBe(100);
  });

  it("exposes dashboard state and intentions through one view-state interface", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_folders") return Promise.resolve([{ id: 1, path: "C:/photos" }]);
      if (command === "get_sync_status") return Promise.resolve({ status: "PAUSED", total: 4, succeeded: 1, failed: 0, currentPath: "C:/photos/two.jpg" });
      return Promise.resolve();
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });
    controller.startLifecycle();
    await controller.hydrate();

    expect(controller.dashboard).toMatchObject({
      syncStatus: "paused",
      processed: 1,
      syncTotal: 4,
      currentFile: "two.jpg",
      watchedFolders: [{ id: 1, path: "C:/photos" }],
      onSyncAction: expect.any(Function),
      onRetryFailedSyncs: expect.any(Function),
      onAddFolder: expect.any(Function),
      onRemoveFolder: expect.any(Function),
      onConfirmAddFolder: expect.any(Function),
      onCancelAddFolder: expect.any(Function),
    });
  });

  it("reflects synchronization event sequences through the dashboard interface", async () => {
    const listeners = new Map<string, (event: { payload: unknown }) => void>();
    tauri.listen.mockImplementation((event: string, handler: (event: { payload: unknown }) => void) => {
      listeners.set(event, handler);
      return Promise.resolve(() => {});
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });
    controller.startLifecycle();
    controller.startSubscriptions();

    listeners.get("sync-progress-snapshot")?.({
      payload: { status: "RUNNING", total: 3, succeeded: 1, failed: 0, currentPath: "C:/photos/one.jpg" },
    });
    expect(controller.dashboard).toMatchObject({ syncStatus: "syncing", processed: 1, syncTotal: 3, currentFile: "one.jpg" });

    listeners.get("sync-paused")?.({
      payload: { status: "PAUSED", total: 3, succeeded: 1, failed: 0, currentPath: "C:/photos/one.jpg" },
    });
    expect(controller.dashboard.syncStatus).toBe("paused");

    listeners.get("sync-resumed")?.({
      payload: { status: "RUNNING", total: 3, succeeded: 1, failed: 0, currentPath: "C:/photos/one.jpg" },
    });
    expect(controller.dashboard.syncStatus).toBe("syncing");
  });

  it("retries failed synchronization through the dashboard interface", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "retry_failed_syncs") return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      return Promise.resolve();
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });
    controller.startLifecycle();
    controller.dashboard.failedSyncs.push({ localPath: "C:/photos/failed.jpg", failureReason: "temporary failure" });

    await controller.dashboard.onRetryFailedSyncs();

    expect(tauri.invoke).toHaveBeenCalledWith("retry_failed_syncs");
    expect(controller.dashboard.syncStatus).toBe("idle");
  });

  it("uses the confirmed cancellation command only when required", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "remove_folder") return Promise.resolve({ requiresSyncCancellation: true });
      if (command === "get_folders" || command === "get_failed_syncs") return Promise.resolve([]);
      return Promise.resolve();
    });
    tauri.confirm.mockResolvedValue(true);
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });

    await controller.dashboard.onRemoveFolder(4);

    expect(tauri.invoke).toHaveBeenCalledWith("remove_folder", { id: 4 });
    expect(tauri.invoke).toHaveBeenCalledWith("remove_folder", { id: 4, confirmCancellation: true });
  });

  it("opens the target album modal when a folder is selected from native picker", async () => {
    tauri.open.mockResolvedValue("C:/media/photos");
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });

    await controller.dashboard.onAddFolder();

    expect(controller.isTargetAlbumModalOpen).toBe(true);
    expect(controller.pendingFolderPath).toBe("C:/media/photos");
    expect(tauri.invoke).not.toHaveBeenCalledWith("add_folder", expect.anything());

    controller.dashboard.onCancelAddFolder();
    expect(controller.isTargetAlbumModalOpen).toBe(false);
    expect(controller.pendingFolderPath).toBe(null);
  });

  it("persists target album ID and refreshes folders on confirmAddFolder", async () => {
    tauri.open.mockResolvedValue("C:/media/photos");
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "add_folder") return Promise.resolve(1);
      if (command === "get_folders") return Promise.resolve([{ id: 1, path: "C:/media/photos", target_album_id: "alb-99" }]);
      return Promise.resolve();
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });

    await controller.dashboard.onAddFolder();
    await controller.dashboard.onConfirmAddFolder("alb-99");

    expect(tauri.invoke).toHaveBeenCalledWith("add_folder", { path: "C:/media/photos", targetAlbumId: "alb-99" });
    expect(controller.isTargetAlbumModalOpen).toBe(false);
    expect(controller.pendingFolderPath).toBe(null);
    expect(controller.watchedFolders).toEqual([{ id: 1, path: "C:/media/photos", target_album_id: "alb-99" }]);
  });

  it("opens the target album modal with defaultPath for removable volume", async () => {
    tauri.open.mockResolvedValue("E:/DCIM");
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });

    await controller.acceptRemovableFolder({
      volumeId: "vol-sd-1",
      mountPath: "E:\\",
      displayName: "EOS_DIGITAL",
    });

    expect(tauri.open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      defaultPath: "E:\\",
      title: "select_folder",
    });
    expect(controller.isTargetAlbumModalOpen).toBe(true);
    expect(controller.pendingFolderPath).toBe("E:/DCIM");
  });

  it("invokes accept_removable_media_folder when confirming a removable folder", async () => {
    tauri.open.mockResolvedValue("E:/DCIM");
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "accept_removable_media_folder") return Promise.resolve(1);
      if (command === "get_folders") return Promise.resolve([{ id: 1, path: "E:/DCIM", target_album_id: "alb-vacation" }]);
      return Promise.resolve();
    });

    const onRemovableVolumeAccepted = vi.fn();
    const controller = createSyncController({
      reportError: vi.fn(),
      t: (key) => key,
      locale: () => "en",
      onRemovableVolumeAccepted,
    });

    await controller.acceptRemovableFolder({
      volumeId: "vol-sd-1",
      mountPath: "E:\\",
      displayName: "EOS_DIGITAL",
    });
    await controller.dashboard.onConfirmAddFolder("alb-vacation");

    expect(tauri.invoke).toHaveBeenCalledWith("accept_removable_media_folder", {
      volumeId: "vol-sd-1",
      path: "E:/DCIM",
      targetAlbumId: "alb-vacation",
    });
    expect(onRemovableVolumeAccepted).toHaveBeenCalledWith("vol-sd-1");
    expect(controller.isTargetAlbumModalOpen).toBe(false);
    expect(controller.pendingFolderPath).toBe(null);
    expect(controller.watchedFolders).toEqual([{ id: 1, path: "E:/DCIM", target_album_id: "alb-vacation" }]);
  });

  it("reports error when accept_removable_media_folder fails", async () => {
    tauri.open.mockResolvedValue("C:/Photos");
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "accept_removable_media_folder") {
        return Promise.reject(new Error("Path 'C:/Photos' does not reside within removable volume"));
      }
      return Promise.resolve();
    });

    const reportError = vi.fn();
    const controller = createSyncController({
      reportError,
      t: (key) => key,
      locale: () => "en",
    });

    await controller.acceptRemovableFolder({
      volumeId: "vol-sd-1",
      mountPath: "E:\\",
      displayName: "EOS_DIGITAL",
    });
    await controller.dashboard.onConfirmAddFolder(null);

    expect(reportError).toHaveBeenCalledWith(expect.any(Error));
  });
});

