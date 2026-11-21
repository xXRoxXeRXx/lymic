export type SyncJobStatus = "RUNNING" | "PAUSED" | "IDLE";
export type SyncSnapshotStatus = SyncJobStatus | "FAILED";
export type WebviewStartupPhase =
  | "update_check_started"
  | "update_check_complete"
  | "version_load_started"
  | "version_load_complete";

export type SyncStatus = "idle" | "syncing" | "paused" | "error";

export type ThemePreference = "system" | "light" | "dark";

export interface LogEntry {
  id: number;
  time: string;
  message: string;
  key?: string;
  params?: Record<string, string>;
}

export interface WatchedFolder {
  id: number;
  path: string;
  recursive: boolean;
  target_album_id: string | null;
  exclusion_patterns?: string[];
}

export interface FailedSyncEntry {
  localPath: string;
  failureReason: string;
}

export interface Album {
  id: string;
  albumName: string;
}

export interface AlbumAssignmentResult {
  succeeded: number;
  failed: number;
}

/**
 * The Filtered Timelines: main views that each show the Immich Library through one fixed filter
 * (`get_filtered_timeline_buckets` / `get_filtered_timeline_bucket_assets` name them), in navigation order.
 */
export const FILTERED_TIMELINES = ["favorites", "archive", "trash"] as const;
export type FilteredTimeline = (typeof FILTERED_TIMELINES)[number];

/** The requested Trashed Assets a Restore returned to the Immich Library; every other requested asset is still in the trash. */
export interface TrashRestoreResult {
  restoredAssetIds: string[];
}

/**
 * The requested Remote Assets a Move to Trash put into the Immich trash; every other requested asset is still in the library.
 * `failureCode` (credential-free, for example `trash_move_permission_denied:asset.delete`) is present only when the
 * per-asset fallback was aborted after at least one asset had been moved: it says why the rest was not moved. A failure
 * before any asset was moved rejects with the code instead.
 */
export interface TrashMoveResult {
  movedAssetIds: string[];
  failureCode?: string;
}

/** The only visibilities Lymic writes: Archive sends `archive`, Unarchive sends `timeline`. Never `locked` or `hidden`. */
export type AssetVisibility = "archive" | "timeline";

/** The requested Remote Assets Immich confirmed as changed by an Archive or Unarchive; every other requested asset kept its visibility. */
export interface AssetVisibilityResult {
  changedAssetIds: string[];
}

export interface AlbumCatalogueItem extends Album {
  albumThumbnailAssetId: string | null;
  assetCount: number;
  isShared: boolean;
  order: string | null;
}

export interface TimelineBucket {
  timeBucket: string;
  count: number;
}

export interface TimelineAsset {
  id: string;
  width: number;
  height: number;
  type: "IMAGE" | "VIDEO";
  takenAt: string | null;
  cameraMake?: string | null;
  cameraModel?: string | null;
  fileSize: number | null;
  duration: number | null;
  isFavorite: boolean;
  /** The Immich visibility when the response names one; absent when unknown. Archive skips `hidden` assets. */
  visibility?: string;
}

export interface AssetDetails {
  assetType?: string | null;
  takenAt?: string | null;
  originalFileName: string | null;
  isFavorite: boolean;
  cameraMake: string | null;
  cameraModel: string | null;
  city: string | null;
  state: string | null;
  country: string | null;
  fNumber: number | null;
  exposureTime: string | null;
  iso: number | null;
  focalLength: number | null;
  lensModel: string | null;
  imageWidth: number | null;
  imageHeight: number | null;
  fileSize: number | null;
  /** The Tags of the Remote Asset. Absent when the server does not report them: no chips, never an error. */
  tags?: AssetTag[];
  /** The Immich visibility (`timeline`, `archive`, `locked`, `hidden`, or a value this version does not know). Absent when the server does not report it: the Lightbox then offers no Archive. */
  visibility?: string;
}

/** A Tag of one Remote Asset, as the Lightbox shows it. */
export interface AssetTag {
  id: string;
  /** The Tag's own name, without its ancestors. */
  name: string;
  /** The Tag's full path, for example `Travel/Italy/Rome`. */
  value: string;
  /** A colour as Immich returns it; only a valid `#RRGGBB` is shown. */
  color: string | null;
}

export interface AlbumTimeline {
  buckets: TimelineBucket[];
  assetsByBucket: Record<string, TimelineAsset[]>;
}

/** A Place Timeline has the same complete, locally indexed shape as an Album Timeline. */
export type PlaceTimeline = AlbumTimeline;

export interface RecognizedPerson {
  id: string;
  name: string | null;
  thumbnailPath: string | null;
}

export interface PeoplePage {
  people: RecognizedPerson[];
  hasNextPage: boolean;
}

/** A city in which the user has Remote Assets, shown with one Remote Asset's thumbnail. */
export interface Place {
  city: string;
  thumbnailAssetId: string;
}

/** A Tag of the Immich Library. Immich returns Tags as a flat list; the hierarchy is `parentId`. */
export interface Tag extends AssetTag {
  parentId: string | null;
}

export type MemoryKind = "onThisDay" | "birthday" | "unknown";

/** An Immich Memory with the Remote Assets it selected, already grouped for the Memory Timeline. */
export interface Memory {
  id: string;
  kind: MemoryKind;
  /** ISO timestamp of the day the Memory looks back on; absent for some Immich versions. */
  memoryAt: string | null;
  /** Year an on-this-day Memory looks back on. */
  year: number | null;
  /** Name of the Recognized Person a birthday Memory celebrates. */
  personName: string | null;
  coverAssetId: string;
  timeline: AlbumTimeline;
}

/** One page of Memories, newest first, for the All Memories view. */
export interface MemoriesPage {
  memories: Memory[];
  hasNextPage: boolean;
}

export interface UiLogMessage {
  level?: string;
  message?: string;
  timestamp?: number;
  key?: string;
  params?: Record<string, string>;
}

export interface SyncSummary {
  processed: number;
  uploaded: number;
  failed: number;
}

export interface SyncSnapshot {
  status: SyncSnapshotStatus;
  total: number;
  succeeded: number;
  failed: number;
  currentPath: string | null;
  totalBytes?: number;
  completedBytes?: number;
}

export interface TransferStatsEvent {
  transferredBytes: number;
  transferRateBytesPerSecond: number;
  estimatedSecondsRemaining: number | null;
  status: SyncJobStatus;
  totalBytes: number;
  completedBytes: number;
}

export interface RemoveFolderResult {
  requiresSyncCancellation: boolean;
}

export interface DetectedVolume {
  volumeId: string;
  mountPath: string;
  displayName: string;
}

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

export type ReleaseChannel = "github" | "store";

/**
 * The Virtual Library section of the settings (ADR-0003). Mirrors `VirtualLibrarySettings` in
 * `src-tauri/src/commands/virtual_library.rs`. `available` is decided by the backend at runtime
 * (Windows, Store release channel and real Package Identity); `enabled` is the persisted toggle
 * and is always false while the Virtual Library is unavailable.
 */
export interface VirtualLibrarySettings {
  available: boolean;
  enabled: boolean;
}

export interface MapMarker { id: string; lat: number; lon: number; }

// Search (ADR-0008, ADR-0009). Mirrors `src-tauri/src/immich/search.rs`.

/** How Immich interprets the text of a Search. Each Search Mode adds a member. */
export type SearchMode = "context" | "filename" | "description" | "folderPath" | "ocr";

/** Images or videos; unset means all. */
export type SearchMediaType = "image" | "video";

/** One exact Rating level in stars. */
export type SearchRating = 1 | 2 | 3 | 4 | 5;

/** A date range. In `SearchFilters` the bounds are the chosen local days (`YYYY-MM-DD`); in a `SearchRequest` they are instants. */
export interface SearchDateRange { from?: string; to?: string; }

/** The Search Filters the user has set (held in memory for the session). Each Search Filter adds one optional field. */
export interface SearchFilters {
  date?: SearchDateRange;
  mediaType?: SearchMediaType;
  /** Recognized People who must all appear in a Remote Asset; unset or empty means no restriction. */
  personIds?: string[];
  /** Tags that must all be on a Remote Asset (only these exact Tags, not their children); unset or empty means no restriction. */
  tagIds?: string[];
  /** Display options: each is set only while switched on; unset (or off) adds no condition. */
  favorite?: boolean;
  /** On shows only Archived Assets; Archived Assets are left out otherwise. */
  archived?: boolean;
  unfiled?: boolean;
  rating?: SearchRating;
  camera?: SearchCamera;
  location?: SearchLocation;
}

/** The camera Search Filter: each part is a value Immich suggested (Immich compares them exactly). */
export interface SearchCamera { make?: string; model?: string; lensModel?: string; }

/**
 * The Location Search Filter: each part is a value Immich offered (Immich compares them exactly): country and
 * state from its suggestions, the city from the city listing of the Explore Places. The parts combine.
 */
export interface SearchLocation { country?: string; state?: string; city?: string; }

/** A date range as the backend receives it: the bounds are ISO 8601 instants (UTC), not local days. */
export interface SearchRequestDateRange { from?: string; to?: string; }

/** The Search Filters as the backend receives them: the same fields, with the date bounds as instants. */
export interface SearchRequestFilters extends Omit<SearchFilters, "date"> {
  date?: SearchRequestDateRange;
}

export interface SearchRequest {
  text: string;
  mode: SearchMode;
  filters: SearchRequestFilters;
}

/** Chronological Search Results: an Album Timeline plus whether the Search was cut at the limit. */
export interface SearchResults extends AlbumTimeline {
  truncated: boolean;
}

/**
 * One page of Context Search Results in the order of relevance; `nextPage` is null on the last page.
 * `limitReached` is set on the last page the limit of 1 000 results allows when Immich filled it, so more
 * results may exist than are shown.
 */
export interface ContextSearchPage {
  assets: TimelineAsset[];
  nextPage: number | null;
  limitReached: boolean;
}

/** Which optional Search Modes the connected Immich Server has switched on. */
export interface SearchFeatures {
  smartSearch: boolean;
  ocr: boolean;
}

/** What Immich suggests values for. Each Search Filter chosen from suggestions adds a member. */
export type SearchSuggestionType = "country" | "state" | "cameraMake" | "cameraModel" | "cameraLensModel";

/** One suggestions request: the type, plus optional narrowing (the models of one make, the states of one country). Mirrors `src-tauri/src/immich/search_suggestions.rs`. */
export interface SearchSuggestionsRequest {
  type: SearchSuggestionType;
  make?: string;
  country?: string;
  state?: string;
}

/** Whether the connected Immich Server is new enough to answer a Search. */
export interface SearchSupport {
  supported: boolean;
  version: string;
  requiredVersion: string;
}
