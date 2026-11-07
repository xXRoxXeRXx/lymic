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
  cameraMake: string | null;
  cameraModel: string | null;
  fileSize: number | null;
  duration: number | null;
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
