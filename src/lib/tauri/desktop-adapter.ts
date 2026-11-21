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
  TrashRestoreResult,
  TrashMoveResult,
  AssetVisibility,
  AssetVisibilityResult,
  FilteredTimeline,
  AlbumCatalogueItem,
  AlbumTimeline,
  PlaceTimeline,
  AssetDetails,
  CurrentUser,
  DatabaseRecoveryNotice,
  FailedSyncEntry,
  LoginRequest,
  ReleaseChannel,
  RemoveFolderResult,
  SyncSnapshot,
  SyncSummary,
  VirtualLibrarySettings,
  WebviewStartupPhase,
  DetectedVolume,
  WatchedFolder,
  TimelineBucket,
  TimelineAsset,
  MemoriesPage,
  Memory,
  PeoplePage,
  Place,
  Tag,
  ContextSearchPage,
  SearchFeatures,
  SearchRequest,
  SearchSuggestionsRequest,
  SearchSupport,
  SearchResults,
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

/** The two requests of every Filtered Timeline (Favorites, Archive, Trash), which one named by `timeline`. */
export interface FilteredTimelineDesktopAdapter {
  getFilteredTimelineBuckets(timeline: FilteredTimeline): Promise<TimelineBucket[]>;
  getFilteredTimelineBucketAssets(timeline: FilteredTimeline, timeBucket: string): Promise<TimelineAsset[]>;
}

export interface TrashRestoreDesktopAdapter {
  restoreTrashedAssets(assetIds: string[]): Promise<TrashRestoreResult>;
}

export interface TrashMoveDesktopAdapter {
  /**
   * Moves Remote Assets to the Immich trash (a soft delete); the result lists those Immich confirmed as moved and,
   * when the per-asset fallback stopped after some were moved, the credential-free `failureCode` that says why.
   * A failure before any asset was moved rejects with that code.
   */
  trashAssets(assetIds: string[]): Promise<TrashMoveResult>;
}

/** The one write that Archives or Unarchives Remote Assets; Lymic sends only `archive` and `timeline`. */
export interface AssetVisibilityDesktopAdapter {
  setAssetsVisibility(assetIds: string[], visibility: AssetVisibility): Promise<AssetVisibilityResult>;
}

export interface AssetDesktopAdapter {
  getAssetDetails(assetId: string): Promise<AssetDetails>;
  setAssetFavorite(assetId: string, isFavorite: boolean): Promise<void>;
  chooseDownloadDestination(originalFileName: string): Promise<string | null>;
  downloadAsset(assetId: string, destinationPath: string): Promise<void>;
  downloadArchive(assetIds: string[]): Promise<void>;
}

/** Every Remote Asset operation a view with a Lightbox needs: details, favorite, Download, add to Album and opening in Immich. */
export type RemoteAssetDesktopAdapter = AssetDesktopAdapter & AlbumAssignmentDesktopAdapter & Pick<UpdateDesktopAdapter, "openExternalUrl">;

export interface AlbumTimelineDesktopAdapter {
  getAlbumTimeline(albumId: string): Promise<AlbumTimeline>;
}
export interface PlaceTimelineDesktopAdapter {
  getPlaceTimeline(city: string): Promise<PlaceTimeline>;
}

export interface MemoriesDesktopAdapter {
  /** The newest Memories, at most `size`, each with its assets grouped for the Memory Timeline. */
  getMemories(size: number): Promise<Memory[]>;
}
export interface AllMemoriesDesktopAdapter {
  /** One page (1-based) of the newest Memories, without the overview cap. */
  getMemoriesPage(page: number, size: number): Promise<MemoriesPage>;
}
export interface PeopleDesktopAdapter {
  getPeople(page: number, size: number): Promise<PeoplePage>;
  getPersonTimelineBuckets(personId: string): Promise<TimelineBucket[]>;
  getPersonTimelineBucketAssets(personId: string, timeBucket: string): Promise<TimelineAsset[]>;
}

export interface PlacesDesktopAdapter {
  getPlaces(): Promise<Place[]>;
}

export interface TagsDesktopAdapter {
  /** Every Tag of the Immich Library as a flat list; the hierarchy is built from `parentId`. */
  getTags(): Promise<Tag[]>;
}

export interface TagTimelineDesktopAdapter {
  getTagTimelineBuckets(tagId: string): Promise<TimelineBucket[]>;
  getTagTimelineBucketAssets(tagId: string, timeBucket: string): Promise<TimelineAsset[]>;
}

export interface SearchDesktopAdapter {
  /** Reads the server version once to decide whether a Search can be answered (Immich 3.2 or later). */
  getSearchSupport(): Promise<SearchSupport>;
  /** Reads which optional Search Modes (Context, OCR) the server has switched on. */
  getSearchFeatures(): Promise<SearchFeatures>;
  searchAssets(request: SearchRequest): Promise<SearchResults>;
  /** One page of a Context Search (Immich smart search), in the order of relevance. */
  searchContextPage(request: SearchRequest, page: number): Promise<ContextSearchPage>;
  /** The values Immich suggests for a Search Filter, optionally narrowed (for example the models of one make). */
  getSearchSuggestions(request: SearchSuggestionsRequest): Promise<string[]>;
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

export interface VirtualLibraryDesktopAdapter {
  /** Whether this run can offer the Virtual Library, and whether the user switched it on. */
  getVirtualLibrarySettings(): Promise<VirtualLibrarySettings>;
  /** Persists the toggle and resolves to the settings as stored. Rejects when the Virtual Library is unavailable. */
  setVirtualLibraryEnabled(enabled: boolean): Promise<VirtualLibrarySettings>;
  /** Persists the chosen folder. Rejects, naming the Watched Folder, when it would overlap one. */
  setVirtualLibraryRootFolder(path: string): Promise<VirtualLibrarySettings>;
  openFolderDialog(options: { directory: true; multiple: false; defaultPath?: string; title: string }): Promise<string | string[] | null>;
  /** Backend announcements of changed Virtual Library settings. */
  subscribeToVirtualLibrarySettings(listener: (settings: VirtualLibrarySettings) => void): Promise<() => void>;
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
    FilteredTimelineDesktopAdapter,
    TrashRestoreDesktopAdapter,
    TrashMoveDesktopAdapter,
    AssetVisibilityDesktopAdapter,
    MapDesktopAdapter,
    AssetDesktopAdapter,
    AlbumTimelineDesktopAdapter,
    PlaceTimelineDesktopAdapter,
    PeopleDesktopAdapter,
    PlacesDesktopAdapter,
    TagsDesktopAdapter,
    TagTimelineDesktopAdapter,
    MemoriesDesktopAdapter,
    AllMemoriesDesktopAdapter,
    SearchDesktopAdapter,
    PreferencesDesktopAdapter,
    VirtualLibraryDesktopAdapter,
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
  getFilteredTimelineBuckets: (timeline) => invoke<TimelineBucket[]>("get_filtered_timeline_buckets", { timeline }),
  getFilteredTimelineBucketAssets: (timeline, timeBucket) => invoke<TimelineAsset[]>("get_filtered_timeline_bucket_assets", { timeline, timeBucket }),
  restoreTrashedAssets: (assetIds) => invoke<TrashRestoreResult>("restore_trashed_assets", { assetIds }),
  trashAssets: (assetIds) => invoke<TrashMoveResult>("trash_assets", { assetIds }),
  setAssetsVisibility: (assetIds, visibility) => invoke<AssetVisibilityResult>("set_assets_visibility", { assetIds, visibility }),
  getAssetDetails: (assetId) => invoke<AssetDetails>("get_asset_details", { assetId }),
  setAssetFavorite: (assetId, isFavorite) => invoke<void>("set_asset_favorite", { assetId, isFavorite }),
  chooseDownloadDestination: (originalFileName) => save({ defaultPath: originalFileName }),
  downloadAsset: (assetId, destinationPath) => invoke<void>("download_asset", { assetId, destinationPath }),
  downloadArchive: (assetIds) => invoke<void>("download_archive", { assetIds }),
  getAlbumTimeline: (albumId) => invoke<AlbumTimeline>("get_album_timeline", { albumId }),
  getPlaceTimeline: (city) => invoke<PlaceTimeline>("get_place_timeline", { city }),
  getMemories: (size) => invoke<Memory[]>("get_memories", { size }),
  getMemoriesPage: (page, size) => invoke<MemoriesPage>("get_memories_page", { page, size }),
  getPeople: (page, size) => invoke<PeoplePage>("get_people", { page, size }),
  getPersonTimelineBuckets: (personId) => invoke<TimelineBucket[]>("get_person_timeline_buckets", { personId }),
  getPersonTimelineBucketAssets: (personId, timeBucket) => invoke<TimelineAsset[]>("get_person_timeline_bucket_assets", { personId, timeBucket }),
  getPlaces: () => invoke<Place[]>("get_places"),
  getTags: () => invoke<Tag[]>("get_tags"),
  getTagTimelineBuckets: (tagId) => invoke<TimelineBucket[]>("get_tag_timeline_buckets", { tagId }),
  getTagTimelineBucketAssets: (tagId, timeBucket) => invoke<TimelineAsset[]>("get_tag_timeline_bucket_assets", { tagId, timeBucket }),
  getSearchSupport: () => invoke<SearchSupport>("get_search_support"),
  getSearchFeatures: () => invoke<SearchFeatures>("get_search_features"),
  searchAssets: (request) => invoke<SearchResults>("search_assets", { request }),
  searchContextPage: (request, page) => invoke<ContextSearchPage>("search_context_page", { request, page }),
  getSearchSuggestions: (request) => invoke<string[]>("get_search_suggestions", { request }),

  getLocaleCatalog: () => invoke<LocaleCatalogEntry[]>("get_locale_catalog"),
  updateLocale: (locale) => invoke<void>("update_locale", { locale }),
  getDatabaseRecoveryNotice: () => invoke<DatabaseRecoveryNotice | null>("get_database_recovery_notice"),
  isAutostartEnabled: () => isEnabled(),
  setAutostartEnabled: (enabled) => enabled ? enable() : disable(),
  getVirtualLibrarySettings: () => invoke<VirtualLibrarySettings>("get_virtual_library_settings"),
  setVirtualLibraryEnabled: (enabled) => invoke<VirtualLibrarySettings>("set_virtual_library_enabled", { enabled }),
  setVirtualLibraryRootFolder: (path) => invoke<VirtualLibrarySettings>("set_virtual_library_root_folder", { path }),
  subscribeToVirtualLibrarySettings: (listener) => listen<VirtualLibrarySettings>(
    "virtual-library-settings-changed",
    (event) => listener(event.payload),
  ),
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
