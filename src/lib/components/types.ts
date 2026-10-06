export interface LogEntry {
  id: number;
  time: string;
  level: "info" | "success" | "error" | "warn";
  message: string;
}

export interface WatchedFolder {
  id: number;
  path: string;
  recursive: boolean;
  target_album_id: string | null;
}

export interface FailedSyncEntry {
  localPath: string;
  failureReason: string;
}

export type SyncStatus = "idle" | "syncing" | "paused" | "error";

export type ThemePreference = "system" | "light" | "dark";
