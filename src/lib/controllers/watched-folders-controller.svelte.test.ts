import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WatchedFoldersDesktopAdapter } from "../tauri/desktop-adapter";
import { createWatchedFoldersController } from "./watched-folders-controller.svelte";

const dialog = vi.hoisted(() => ({ confirm: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

function createFoldersAdapter(
  overrides: Partial<WatchedFoldersDesktopAdapter> = {},
): WatchedFoldersDesktopAdapter {
  return {
    addFolder: vi.fn().mockResolvedValue(1),
    acceptRemovableMediaFolder: vi.fn().mockResolvedValue(1),
    getFolders: vi.fn().mockResolvedValue([]),
    updateFolderExclusionPatterns: vi.fn().mockResolvedValue(undefined),
    removeFolder: vi.fn().mockResolvedValue({ requiresSyncCancellation: false }),
    ...overrides,
  };
}

function createController(
  adapter: WatchedFoldersDesktopAdapter,
  overrides = {},
) {
  return createWatchedFoldersController({
    adapter,
    reportError: vi.fn(),
    t: (key: string) => key,
    ...overrides,
  });
}

beforeEach(() => {
  dialog.confirm.mockClear();
  dialog.open.mockClear();
});

describe("watched folders controller", () => {
  it("clears loaded Watched Folders and a pending Target Album dialog", async () => {
    const controller = createWatchedFoldersController({
      adapter: createFoldersAdapter({ getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/Photos", recursive: true, target_album_id: null }]) }),
      reportError: vi.fn(),
      t: (key) => key,
    });

    await controller.refresh();
    controller.clear();

    expect(controller.folders.watchedFolders).toEqual([]);
    expect(controller.targetAlbumDialog.isOpen).toBe(false);
  });

  it("ignores a refresh response that arrives after user state is cleared", async () => {
    let resolveFolders: (folders: Awaited<ReturnType<WatchedFoldersDesktopAdapter["getFolders"]>>) => void;
    const pendingFolders = new Promise<Awaited<ReturnType<WatchedFoldersDesktopAdapter["getFolders"]>>>((resolve) => { resolveFolders = resolve; });
    const controller = createWatchedFoldersController({
      adapter: createFoldersAdapter({ getFolders: vi.fn().mockReturnValue(pendingFolders) }),
      reportError: vi.fn(),
      t: (key) => key,
    });

    const refresh = controller.refresh();
    controller.clear();
    resolveFolders!([{ id: 1, path: "C:/Previous account", recursive: true, target_album_id: null }]);
    await refresh;

    expect(controller.folders.watchedFolders).toEqual([]);
  });

  it("does not report an error from a refresh that predates cleared user state", async () => {
    let rejectFolders: (error: Error) => void;
    const pendingFolders = new Promise<Awaited<ReturnType<WatchedFoldersDesktopAdapter["getFolders"]>>>((_resolve, reject) => { rejectFolders = reject; });
    const reportError = vi.fn();
    const controller = createWatchedFoldersController({
      adapter: createFoldersAdapter({ getFolders: vi.fn().mockReturnValue(pendingFolders) }),
      reportError,
      t: (key) => key,
    });

    const refresh = controller.refresh();
    controller.clear();
    rejectFolders!(new Error("Previous account unavailable"));
    await refresh;

    expect(reportError).not.toHaveBeenCalled();
  });

  it("refreshes watched folders and exposes folder view state", async () => {
    const adapter = createFoldersAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }]),
    });
    const controller = createController(adapter);

    expect(controller.hasFolders).toBe(false);
    await controller.refresh();

    expect(controller.hasFolders).toBe(true);
    expect(controller.folders.watchedFolders).toEqual([
      { id: 1, path: "C:/photos", recursive: true, target_album_id: null },
    ]);
  });

  it("opens the target album dialog when a folder is selected and cancels cleanly", async () => {
    const adapter = createFoldersAdapter();
    dialog.open.mockResolvedValue("C:/media/photos");
    const controller = createController(adapter);

    await controller.folders.onAddFolder();

    expect(controller.targetAlbumDialog.isOpen).toBe(true);
    expect(controller.targetAlbumDialog.folderPath).toBe("C:/media/photos");
    expect(adapter.addFolder).not.toHaveBeenCalled();

    controller.targetAlbumDialog.cancel();

    expect(controller.targetAlbumDialog.isOpen).toBe(false);
    expect(controller.targetAlbumDialog.folderPath).toBeNull();
  });

  it("adds a folder with its target album and refreshes the folder list", async () => {
    const adapter = createFoldersAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/media/photos", recursive: true, target_album_id: "alb-99" }]),
    });
    dialog.open.mockResolvedValue("C:/media/photos");
    const controller = createController(adapter);

    await controller.folders.onAddFolder();
    await controller.targetAlbumDialog.confirm("alb-99");

    expect(adapter.addFolder).toHaveBeenCalledWith("C:/media/photos", "alb-99");
    expect(controller.targetAlbumDialog.isOpen).toBe(false);
    expect(controller.folders.watchedFolders).toEqual([
      { id: 1, path: "C:/media/photos", recursive: true, target_album_id: "alb-99" },
    ]);
  });

  it("adds a removable folder with its volume and target album", async () => {
    const adapter = createFoldersAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "E:/DCIM", recursive: true, target_album_id: "alb-vacation" }]),
    });
    dialog.open.mockResolvedValue("E:/DCIM");
    const onRemovableVolumeAccepted = vi.fn();
    const controller = createController(adapter, { onRemovableVolumeAccepted });

    await controller.acceptRemovableFolder({
      volumeId: "vol-sd-1",
      mountPath: "E:\\",
      displayName: "EOS_DIGITAL",
    });
    await controller.targetAlbumDialog.confirm("alb-vacation");

    expect(adapter.acceptRemovableMediaFolder).toHaveBeenCalledWith("vol-sd-1", "E:/DCIM", "alb-vacation");
    expect(onRemovableVolumeAccepted).toHaveBeenCalledWith("vol-sd-1");
  });

  it("reports adapter errors when adding a removable folder", async () => {
    const error = new Error("Path does not reside within removable volume");
    const adapter = createFoldersAdapter({
      acceptRemovableMediaFolder: vi.fn().mockRejectedValue(error),
    });
    dialog.open.mockResolvedValue("C:/Photos");
    const reportError = vi.fn();
    const controller = createController(adapter, { reportError });

    await controller.acceptRemovableFolder({
      volumeId: "vol-sd-1",
      mountPath: "E:\\",
      displayName: "EOS_DIGITAL",
    });
    await controller.targetAlbumDialog.confirm(null);

    expect(reportError).toHaveBeenCalledWith(error);
  });

  it("removes a folder after the required sync cancellation warning and notifies onFolderRemoved", async () => {
    const adapter = createFoldersAdapter({
      removeFolder: vi.fn().mockResolvedValue({ requiresSyncCancellation: true }),
    });
    dialog.confirm.mockResolvedValue(true);
    const onFolderRemoved = vi.fn();
    const controller = createController(adapter, { onFolderRemoved });

    await controller.folders.onRemoveFolder(4);

    expect(dialog.confirm).toHaveBeenCalledWith("remove_folder_sync_warning", {
      title: "remove_folder",
      kind: "warning",
    });
    expect(adapter.removeFolder).toHaveBeenNthCalledWith(1, 4);
    expect(adapter.removeFolder).toHaveBeenNthCalledWith(2, 4, true);
    expect(onFolderRemoved).toHaveBeenCalledOnce();
  });

  it("does not remove a folder after a declined confirmation", async () => {
    const adapter = createFoldersAdapter();
    dialog.confirm.mockResolvedValue(false);
    const onFolderRemoved = vi.fn();
    const controller = createController(adapter, { onFolderRemoved });

    await controller.folders.onRemoveFolder(4);

    expect(adapter.removeFolder).toHaveBeenCalledOnce();
    expect(adapter.removeFolder).toHaveBeenCalledWith(4);
    expect(onFolderRemoved).not.toHaveBeenCalled();
  });

  it("updates folder exclusion patterns and refreshes folders", async () => {
    const adapter = createFoldersAdapter({
      getFolders: vi.fn().mockResolvedValue([
        { id: 1, path: "C:/photos", recursive: true, target_album_id: null, exclusion_patterns: ["*.mp4"] },
      ]),
    });
    const controller = createController(adapter);

    await controller.folders.onUpdateFolderExclusionPatterns(1, ["*.mp4"]);

    expect(adapter.updateFolderExclusionPatterns).toHaveBeenCalledWith(1, ["*.mp4"]);
    expect(controller.folders.watchedFolders).toEqual([
      { id: 1, path: "C:/photos", recursive: true, target_album_id: null, exclusion_patterns: ["*.mp4"] },
    ]);
  });

  it("ignores folder updates after destroy", async () => {
    const adapter = createFoldersAdapter({
      getFolders: vi.fn().mockResolvedValue([{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }]),
    });
    const controller = createController(adapter);
    controller.destroy();

    await controller.refresh();

    expect(controller.hasFolders).toBe(false);
  });
});
