export interface LogEntry {
  id: number;
  time: string;
  level: "info" | "success" | "error" | "warn";
  message: string;
  key?: string;
  params?: Record<string, string>;
}

export type SyncStatus = "idle" | "syncing" | "paused" | "error";

export type ThemePreference = "system" | "light" | "dark";
