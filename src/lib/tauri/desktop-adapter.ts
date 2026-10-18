import { invoke } from "@tauri-apps/api/core";
import type { Album, FailedSyncEntry, WatchedFolder } from "../components/types";
import type { LocaleCatalogEntry } from "../i18n/locale-catalog";
import type { RemoveFolderResult, SyncSnapshot, SyncSummary } from "../controllers/types";

export interface CurrentUser {
  displayName: string;
  profileImage: string | null;
}

export interface DatabaseRecoveryNotice {
  message: string;
  key: string;
  reason: string;
  backup: string;
  count: number;
}

export interface LoginRequest {
  serverUrl: string;
  apiKey: string;
  allowLocalHttp: boolean;
  cloudflareClientId: string | null;
  cloudflareClientSecret: string | null;
}

/** The complete frontend contract for Tauri commands registered by Lymic. */
export interface DesktopAdapter {
  startSync(): Promise<SyncSummary>;
  pauseSync(): Promise<SyncSnapshot>;
  resumeSync(): Promise<SyncSnapshot>;
  getSyncStatus(): Promise<SyncSnapshot>;
  getFailedSyncs(): Promise<FailedSyncEntry[]>;
  retryFailedSyncs(): Promise<SyncSummary>;
  getUploadParallelism(): Promise<number>;
  setUploadParallelism(uploadParallelism: number): Promise<void>;

  addFolder(path: string, targetAlbumId: string | null): Promise<number>;
  acceptRemovableMediaFolder(volumeId: string, path: string, targetAlbumId: string | null): Promise<number>;
  getFolders(): Promise<WatchedFolder[]>;
  removeFolder(id: number, confirmCancellation?: boolean): Promise<RemoveFolderResult>;

  getAuthStatus(): Promise<boolean>;
  getServerUrl(): Promise<string>;
  getCurrentUser(): Promise<CurrentUser>;
  login(request: LoginRequest): Promise<void>;
  logout(): Promise<void>;

  getAlbums(): Promise<Album[]>;
  createAlbum(name: string): Promise<Album>;

  getLocaleCatalog(): Promise<LocaleCatalogEntry[]>;
  updateLocale(locale: string): Promise<void>;
  getDatabaseRecoveryNotice(): Promise<DatabaseRecoveryNotice | null>;
  recordWebviewStartupPhaseProbe(phase: string): Promise<void>;
}

export const tauriDesktopAdapter: DesktopAdapter = {
  startSync: () => invoke<SyncSummary>("start_sync"),
  pauseSync: () => invoke<SyncSnapshot>("pause_sync"),
  resumeSync: () => invoke<SyncSnapshot>("resume_sync"),
  getSyncStatus: () => invoke<SyncSnapshot>("get_sync_status"),
  getFailedSyncs: () => invoke<FailedSyncEntry[]>("get_failed_syncs"),
  retryFailedSyncs: () => invoke<SyncSummary>("retry_failed_syncs"),
  getUploadParallelism: () => invoke<number>("get_upload_parallelism"),
  setUploadParallelism: (uploadParallelism) => invoke<void>("set_upload_parallelism", { uploadParallelism }),

  addFolder: (path, targetAlbumId) => invoke<number>("add_folder", { path, targetAlbumId }),
  acceptRemovableMediaFolder: (volumeId, path, targetAlbumId) => invoke<number>("accept_removable_media_folder", { volumeId, path, targetAlbumId }),
  getFolders: () => invoke<WatchedFolder[]>("get_folders"),
  removeFolder: (id, confirmCancellation) => invoke<RemoveFolderResult>(
    "remove_folder",
    confirmCancellation === undefined ? { id } : { id, confirmCancellation },
  ),

  getAuthStatus: () => invoke<boolean>("get_auth_status"),
  getServerUrl: () => invoke<string>("get_server_url"),
  getCurrentUser: () => invoke<CurrentUser>("get_current_user"),
  login: (request) => invoke<void>("login", {
    serverUrl: request.serverUrl,
    apiKey: request.apiKey,
    allowLocalHttp: request.allowLocalHttp,
    cloudflareClientId: request.cloudflareClientId,
    cloudflareClientSecret: request.cloudflareClientSecret,
  }),
  logout: () => invoke<void>("logout"),

  getAlbums: () => invoke<Album[]>("get_albums"),
  createAlbum: (name) => invoke<Album>("create_album", { name }),

  getLocaleCatalog: () => invoke<LocaleCatalogEntry[]>("get_locale_catalog"),
  updateLocale: (locale) => invoke<void>("update_locale", { locale }),
  getDatabaseRecoveryNotice: () => invoke<DatabaseRecoveryNotice | null>("get_database_recovery_notice"),
  recordWebviewStartupPhaseProbe: (phase) => invoke<void>("record_webview_startup_phase_probe", { phase }),
};
