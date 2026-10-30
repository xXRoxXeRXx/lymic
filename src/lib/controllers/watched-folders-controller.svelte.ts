import { confirm, open } from "@tauri-apps/plugin-dialog";
import type { DetectedVolume, WatchedFolder } from "../contracts";
import type { WatchedFoldersDesktopAdapter } from "../tauri/desktop-adapter";

export interface WatchedFoldersViewState {
  readonly watchedFolders: WatchedFolder[];
  onAddFolder(): Promise<void>;
  onRemoveFolder(id: number): Promise<void>;
  onUpdateFolderExclusionPatterns(id: number, exclusionPatterns: string[]): Promise<void>;
}

export interface TargetAlbumDialogState {
  readonly isOpen: boolean;
  readonly folderPath: string | null;
  confirm(targetAlbumId: string | null): Promise<void>;
  cancel(): void;
}

export interface WatchedFoldersController {
  readonly folders: WatchedFoldersViewState;
  readonly targetAlbumDialog: TargetAlbumDialogState;
  readonly hasFolders: boolean;
  hydrate(): Promise<void>;
  refresh(): Promise<void>;
  acceptRemovableFolder(volume: DetectedVolume): Promise<void>;
  clearUserState(): void;
}

export interface WatchedFoldersControllerOptions {
  adapter: WatchedFoldersDesktopAdapter;
  reportError(error: unknown): void;
  t(key: string): string;
  onRemovableVolumeAccepted?(volumeId: string): void;
  onFolderRemoved?(): Promise<void> | void;
}

interface PendingFolderAddition {
  path: string;
  volume: DetectedVolume | null;
}

export function createWatchedFoldersController(
  options: WatchedFoldersControllerOptions,
): WatchedFoldersController {
  const { adapter } = options;

  let watchedFolders = $state<WatchedFolder[]>([]);
  let pendingAddition = $state<PendingFolderAddition | null>(null);

  async function refresh() {
    try {
      watchedFolders = await adapter.getFolders();
    } catch (error) {
      console.error("Failed to load folders", error);
      options.reportError(error);
    }
  }

  async function promptForFolder(volume: DetectedVolume | null, logMessage: string) {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        ...(volume ? { defaultPath: volume.mountPath } : {}),
        title: options.t("select_folder"),
      });
      if (typeof selected === "string") {
        pendingAddition = { path: selected, volume };
      }
    } catch (error) {
      console.error(logMessage, error);
      options.reportError(error);
    }
  }

  async function addFolder() {
    await promptForFolder(null, "Failed to add folder");
  }

  async function acceptRemovableFolder(volume: DetectedVolume) {
    await promptForFolder(volume, "Failed to select folder for removable volume");
  }

  async function confirmAddFolder(targetAlbumId: string | null) {
    if (!pendingAddition) return;
    const { path, volume } = pendingAddition;
    pendingAddition = null;
    try {
      if (volume) {
        await adapter.acceptRemovableMediaFolder(volume.volumeId, path, targetAlbumId);
        options.onRemovableVolumeAccepted?.(volume.volumeId);
      } else {
        await adapter.addFolder(path, targetAlbumId);
      }
      await refresh();
    } catch (error) {
      console.error("Failed to add folder", error);
      options.reportError(error);
    }
  }

  function cancelAddFolder() {
    pendingAddition = null;
  }

  async function removeFolder(id: number) {
    try {
      const result = await adapter.removeFolder(id);
      const warningMessage = result?.requiresSyncCancellation
        ? options.t("remove_folder_sync_warning")
        : options.t("remove_folder_confirmation");
      const confirmed = await confirm(warningMessage, {
        title: options.t("remove_folder"),
        kind: "warning",
      });
      if (!confirmed) return;
      await adapter.removeFolder(id, true);
      await refresh();
      await options.onFolderRemoved?.();
    } catch (error) {
      console.error("Failed to remove folder", error);
      options.reportError(error);
    }
  }

  async function updateFolderExclusionPatterns(id: number, exclusionPatterns: string[]) {
    try {
      await adapter.updateFolderExclusionPatterns(id, exclusionPatterns);
      await refresh();
    } catch (error) {
      options.reportError(error);
    }
  }

  async function hydrate() {
    await refresh();
  }

  return {
    get folders(): WatchedFoldersViewState {
      return {
        watchedFolders,
        onAddFolder: addFolder,
        onRemoveFolder: removeFolder,
        onUpdateFolderExclusionPatterns: updateFolderExclusionPatterns,
      };
    },
    get targetAlbumDialog(): TargetAlbumDialogState {
      return {
        isOpen: pendingAddition !== null,
        folderPath: pendingAddition?.path ?? null,
        confirm: confirmAddFolder,
        cancel: cancelAddFolder,
      };
    },
    get hasFolders() {
      return watchedFolders.length > 0;
    },
    hydrate,
    refresh,
    acceptRemovableFolder,
    clearUserState: cancelAddFolder,
  };
}
