export type SyncJobStatus = "RUNNING" | "PAUSED" | "IDLE" | "FAILED";

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
  status: SyncJobStatus;
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

