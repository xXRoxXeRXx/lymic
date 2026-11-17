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

/** The requested Trashed Assets a Restore returned to the Immich Library; every other requested asset is still in the trash. */
export interface TrashRestoreResult {
  restoredAssetIds: string[];
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

export interface MapMarker { id: string; lat: number; lon: number; }

// Search (ADR-0008, ADR-0009). Mirrors `src-tauri/src/immich/search.rs`.

/** How Immich interprets the text of a Search. Each Search Mode adds a member. */
export type SearchMode = "filename" | "description" | "folderPath" | "ocr";

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
  /** Display options: each is set only while switched on; unset (or off) adds no condition. */
  favorite?: boolean;
  /** On shows only Archived Assets; Archived Assets are left out otherwise. */
  archived?: boolean;
  unfiled?: boolean;
  rating?: SearchRating;
  camera?: SearchCamera;
}

/** The camera Search Filter: each part is a value Immich suggested (Immich compares them exactly). */
export interface SearchCamera { make?: string; model?: string; lensModel?: string; }

/** The Search Filters as the backend receives them: the same fields, with date bounds as ISO 8601 instants. */
export type SearchRequestFilters = SearchFilters;

export interface SearchRequest {
  text: string;
  mode: SearchMode;
  filters: SearchRequestFilters;
}

/** Chronological Search Results: an Album Timeline plus whether the Search was cut at the limit. */
export interface SearchResults extends AlbumTimeline {
  truncated: boolean;
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
