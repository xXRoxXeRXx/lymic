import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import { isPermissionGranted, sendNotification } from "@tauri-apps/plugin-notification";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import type {
  MapMarker,
  Album,
  AlbumAssignmentResult,
  AlbumCatalogueItem,
  AlbumTimeline,
  AssetDetails,
  CurrentUser,
  DatabaseRecoveryNotice,
  FailedSyncEntry,
  LoginRequest,
  ReleaseChannel,
  RemoveFolderResult,
  SyncSnapshot,
  SyncSummary,
  WebviewStartupPhase,
  DetectedVolume,
  WatchedFolder,
  TimelineBucket,
  TimelineAsset,
  PeoplePage,
} from "../contracts";
import type { LocaleCatalogEntry } from "../i18n/locale-catalog";

export interface AuthDesktopAdapter {
  getAuthStatus(): Promise<boolean>;
  getServerUrl(): Promise<string>;
  getCurrentUser(): Promise<CurrentUser>;
  login(request: LoginRequest): Promise<void>;
  logout(): Promise<void>;
  confirm(message: string, options: { title: string; kind: "warning" }): Promise<boolean>;
}

export interface SyncDesktopAdapter {
  startSync(): Promise<SyncSummary>;
  pauseSync(): Promise<SyncSnapshot>;
  resumeSync(): Promise<SyncSnapshot>;
  getSyncStatus(): Promise<SyncSnapshot>;
  getFailedSyncs(): Promise<FailedSyncEntry[]>;
  retryFailedSyncs(): Promise<SyncSummary>;
}

export interface WatchedFoldersDesktopAdapter {
  addFolder(path: string, targetAlbumId: string | null): Promise<number>;
  acceptRemovableMediaFolder(volumeId: string, path: string, targetAlbumId: string | null): Promise<number>;
  getFolders(): Promise<WatchedFolder[]>;
  updateFolderExclusionPatterns(id: number, exclusionPatterns: string[]): Promise<void>;
  removeFolder(id: number, confirmCancellation?: boolean): Promise<RemoveFolderResult>;
  openFolderDialog(options: { directory: true; multiple: false; defaultPath?: string; title: string }): Promise<string | string[] | null>;
  confirm(message: string, options: { title: string; kind: "warning" }): Promise<boolean>;
}

export interface AlbumsDesktopAdapter {
  getAlbums(): Promise<Album[]>;
  createAlbum(name: string): Promise<Album>;
}

export interface AlbumAssignmentDesktopAdapter {
  addAssetsToAlbum(assetIds: string[], albumId: string): Promise<AlbumAssignmentResult>;
}

export interface AlbumCatalogueDesktopAdapter {
  getAlbumCatalogue(): Promise<AlbumCatalogueItem[]>;
}

export interface MapDesktopAdapter {
  getMapMarkers(): Promise<MapMarker[]>;
}

export interface TimelineDesktopAdapter {
  getTimelineBuckets(): Promise<TimelineBucket[]>;
  getTimelineBucketAssets(timeBucket: string): Promise<TimelineAsset[]>;
}

export interface AssetDesktopAdapter {
  getAssetDetails(assetId: string): Promise<AssetDetails>;
  setAssetFavorite(assetId: string, isFavorite: boolean): Promise<void>;
  chooseDownloadDestination(originalFileName: string): Promise<string | null>;
  downloadAsset(assetId: string, destinationPath: string): Promise<void>;
  downloadArchive(assetIds: string[]): Promise<void>;
}

export interface AlbumTimelineDesktopAdapter {
  getAlbumTimeline(albumId: string): Promise<AlbumTimeline>;
}
export interface PeopleDesktopAdapter {
  getPeople(page: number, size: number): Promise<PeoplePage>;
  getPersonTimelineBuckets(personId: string): Promise<TimelineBucket[]>;
  getPersonTimelineBucketAssets(personId: string, timeBucket: string): Promise<TimelineAsset[]>;
}

export interface PreferencesDesktopAdapter {
  getUploadParallelism(): Promise<number>;
  setUploadParallelism(uploadParallelism: number): Promise<void>;
  getLocaleCatalog(): Promise<LocaleCatalogEntry[]>;
  updateLocale(locale: string): Promise<void>;
  getDatabaseRecoveryNotice(): Promise<DatabaseRecoveryNotice | null>;
  isAutostartEnabled(): Promise<boolean>;
  setAutostartEnabled(enabled: boolean): Promise<void>;
}

export interface UpdateDesktopAdapter {
  recordWebviewStartupPhaseProbe(phase: WebviewStartupPhase): Promise<void>;
  getReleaseChannel(): Promise<ReleaseChannel>;
  getAppVersion(): Promise<string>;
  checkForUpdate(): Promise<AvailableUpdate | null>;
  downloadAndInstallUpdate(onEvent: (event: UpdateDownloadEvent) => void): Promise<void>;
  relaunchApp(): Promise<void>;
  openExternalUrl(url: string): Promise<void>;
}

export interface AvailableUpdate {
  version: string;
  body?: string;
}

export type UpdateDownloadEvent = DownloadEvent;

export interface RemovableVolumeDesktopAdapter {
  subscribeToRemovableVolumeDetected(listener: (volume: DetectedVolume) => void): Promise<() => void>;
  subscribeToRemovableVolumeRemoved(listener: (volumeIdOrPath: string) => void): Promise<() => void>;
  subscribeToRemovableVolumeRemounted(listener: () => void): Promise<() => void>;
  isDesktopNotificationPermissionGranted(): Promise<boolean>;
  sendDesktopNotification(notification: { title: string; body: string }): void;
}

/** The complete frontend contract for Tauri commands registered by Lymic. */
export interface DesktopAdapter
  extends AuthDesktopAdapter,
    SyncDesktopAdapter,
    WatchedFoldersDesktopAdapter,
    AlbumsDesktopAdapter,
    AlbumAssignmentDesktopAdapter,
    AlbumCatalogueDesktopAdapter,
    TimelineDesktopAdapter,
    MapDesktopAdapter,
    AssetDesktopAdapter,
    AlbumTimelineDesktopAdapter,
    PeopleDesktopAdapter,
    PreferencesDesktopAdapter,
    UpdateDesktopAdapter,
    RemovableVolumeDesktopAdapter {}

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
  updateFolderExclusionPatterns: (id, exclusionPatterns) => invoke<void>("update_folder_exclusion_patterns", { id, exclusionPatterns }),
  removeFolder: (id, confirmCancellation) => invoke<RemoveFolderResult>(
    "remove_folder",
    confirmCancellation === undefined ? { id } : { id, confirmCancellation },
  ),
  openFolderDialog: open,

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
  confirm,

  getAlbums: () => invoke<Album[]>("get_albums"),
  getAlbumCatalogue: () => invoke<AlbumCatalogueItem[]>("get_album_catalogue"),
  createAlbum: (name) => invoke<Album>("create_album", { name }),
  addAssetsToAlbum: (assetIds, albumId) => invoke<AlbumAssignmentResult>("add_assets_to_album", { assetIds, albumId }),
  getMapMarkers: () => invoke<MapMarker[]>("get_map_markers"),
  getTimelineBuckets: () => invoke<TimelineBucket[]>("get_timeline_buckets"),
  getTimelineBucketAssets: (timeBucket) => invoke<TimelineAsset[]>("get_timeline_bucket_assets", { timeBucket }),
  getAssetDetails: (assetId) => invoke<AssetDetails>("get_asset_details", { assetId }),
  setAssetFavorite: (assetId, isFavorite) => invoke<void>("set_asset_favorite", { assetId, isFavorite }),
  chooseDownloadDestination: (originalFileName) => save({ defaultPath: originalFileName }),
  downloadAsset: (assetId, destinationPath) => invoke<void>("download_asset", { assetId, destinationPath }),
  downloadArchive: (assetIds) => invoke<void>("download_archive", { assetIds }),
  getAlbumTimeline: (albumId) => invoke<AlbumTimeline>("get_album_timeline", { albumId }),
  getPeople: (page, size) => invoke<PeoplePage>("get_people", { page, size }),
  getPersonTimelineBuckets: (personId) => invoke<TimelineBucket[]>("get_person_timeline_buckets", { personId }),
  getPersonTimelineBucketAssets: (personId, timeBucket) => invoke<TimelineAsset[]>("get_person_timeline_bucket_assets", { personId, timeBucket }),

  getLocaleCatalog: () => invoke<LocaleCatalogEntry[]>("get_locale_catalog"),
  updateLocale: (locale) => invoke<void>("update_locale", { locale }),
  getDatabaseRecoveryNotice: () => invoke<DatabaseRecoveryNotice | null>("get_database_recovery_notice"),
  isAutostartEnabled: () => isEnabled(),
  setAutostartEnabled: (enabled) => enabled ? enable() : disable(),
  recordWebviewStartupPhaseProbe: (phase) => invoke<void>("record_webview_startup_phase_probe", { phase }),
  getReleaseChannel: () => invoke<ReleaseChannel>("get_release_channel"),
  getAppVersion: getVersion,
  checkForUpdate: async () => {
    pendingUpdate = await check();
    return pendingUpdate && { version: pendingUpdate.version, body: pendingUpdate.body };
  },
  downloadAndInstallUpdate: (onEvent) => {
    if (!pendingUpdate) throw new Error("No update is available to install");
    return pendingUpdate.downloadAndInstall(onEvent);
  },
  relaunchApp: relaunch,
  openExternalUrl: openUrl,
  subscribeToRemovableVolumeDetected: (listener) => listen<DetectedVolume>(
    "removable-volume-detected",
    (event) => listener(event.payload),
  ),
  subscribeToRemovableVolumeRemoved: (listener) => listen<string>(
    "removable-volume-removed",
    (event) => listener(event.payload),
  ),
  subscribeToRemovableVolumeRemounted: (listener) => listen("removable-volume-remounted", listener),
  isDesktopNotificationPermissionGranted: () => isPermissionGranted(),
  sendDesktopNotification: (notification) => sendNotification(notification),
};

let pendingUpdate: Update | null = null;
