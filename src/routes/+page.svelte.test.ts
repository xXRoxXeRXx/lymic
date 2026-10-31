import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const tauri = vi.hoisted(() => ({
  getVersion: vi.fn(),
  invoke: vi.fn(),
  listen: vi.fn(),
  isEnabled: vi.fn(),
  confirm: vi.fn(),
  open: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
}));

type WatchedFolder = {
  id: number;
  path: string;
  recursive: boolean;
  target_album_id: string | null;
};

type FailedSyncEntry = {
  localPath: string;
  failureReason: string;
};

let watchedFolders: WatchedFolder[] = [];
let failedSyncs: FailedSyncEntry[] = [];
let uploadParallelism = 3;
let triggerSync: (() => void) | undefined;
let syncStarted: (() => void) | undefined;
let syncPaused: ((event: { payload: unknown }) => void) | undefined;
let syncResumed: ((event: { payload: unknown }) => void) | undefined;
let syncProgressSnapshot: ((event: { payload: unknown }) => void) | undefined;
let syncTransferStats: ((event: { payload: unknown }) => void) | undefined;
let logMessage: ((event: { payload: unknown }) => void) | undefined;
let removableVolumeDetected: ((event: { payload: unknown }) => void) | undefined;
let systemThemeIsDark = false;
let colorSchemeListeners = new Set<(event: MediaQueryListEvent) => void>();

vi.mock("@tauri-apps/api/app", () => ({ getVersion: tauri.getVersion }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(),
  disable: vi.fn(),
  isEnabled: tauri.isEnabled,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: tauri.open, confirm: tauri.confirm }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: tauri.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: tauri.relaunch }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));


function mockAuthenticatedDashboard(handlers: Record<string, (args?: unknown) => Promise<unknown>> = {}) {
  tauri.invoke.mockImplementation((command: string, args?: unknown) => {
    if (command in handlers) {
      return handlers[command](args);
    }
    switch (command) {
      case "get_auth_status":
        return Promise.resolve(true);
      case "get_server_url":
        return Promise.resolve("https://immich.example");
      case "get_current_user":
        return Promise.resolve({ displayName: "Meyer", profileImage: null });
      case "get_folders":
        return Promise.resolve(watchedFolders);
      case "get_failed_syncs":
        return Promise.resolve(failedSyncs);
      case "get_upload_parallelism":
        return Promise.resolve(uploadParallelism);
      case "get_sync_status":
        return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      case "update_locale":
        return Promise.resolve();
      case "get_release_channel":
        return Promise.resolve("github");
      default:
        return Promise.resolve();
    }
  });
}

function renderAuthenticatedPage(handlers: Record<string, (args?: unknown) => Promise<unknown>> = {}) {
  mockAuthenticatedDashboard(handlers);
  return render(Page);
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  systemThemeIsDark = false;
  colorSchemeListeners = new Set();
  vi.stubGlobal("matchMedia", vi.fn().mockImplementation(() => ({
    matches: systemThemeIsDark,
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (_type: "change", listener: (event: MediaQueryListEvent) => void) => colorSchemeListeners.add(listener),
    removeEventListener: (_type: "change", listener: (event: MediaQueryListEvent) => void) => colorSchemeListeners.delete(listener),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })));
  localStorage.setItem("lymic-locale", "en");
  watchedFolders = [];
  failedSyncs = [];
  uploadParallelism = 3;
  triggerSync = undefined;
  syncStarted = undefined;
  syncPaused = undefined;
  syncResumed = undefined;
  syncProgressSnapshot = undefined;
  syncTransferStats = undefined;
  logMessage = undefined;
  removableVolumeDetected = undefined;
  tauri.listen.mockImplementation((event: string, handler: () => void) => {
    if (event === "trigger-sync") triggerSync = handler;
    if (event === "sync-started") syncStarted = handler;
    if (event === "sync-paused") syncPaused = handler as (event: { payload: unknown }) => void;
    if (event === "sync-resumed") syncResumed = handler as (event: { payload: unknown }) => void;
    if (event === "sync-progress-snapshot") syncProgressSnapshot = handler as (event: { payload: unknown }) => void;
    if (event === "sync-transfer-stats") syncTransferStats = handler as (event: { payload: unknown }) => void;
    if (event === "log-message") logMessage = handler as (event: { payload: unknown }) => void;
    if (event === "removable-volume-detected") removableVolumeDetected = handler as (event: { payload: unknown }) => void;
    return Promise.resolve(() => {});
  });

  tauri.isEnabled.mockResolvedValue(false);
  tauri.confirm.mockResolvedValue(true);
  tauri.getVersion.mockResolvedValue("0.0.0");
  tauri.check.mockResolvedValue(null);
  tauri.relaunch.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("app version", () => {
  it("displays the version provided by the Tauri runtime in the info view", async () => {
    tauri.getVersion.mockResolvedValue("1.2.3");

    renderAuthenticatedPage();
    await fireEvent.click(await screen.findByRole("button", { name: "Info" }));

    expect(await screen.findByText("v1.2.3")).toBeInTheDocument();
    expect(screen.getByText("Lymic (Unofficial Immich Desktop Client)")).toBeInTheDocument();
    expect(screen.getByText("MIT")).toBeInTheDocument();
    expect(screen.getByText("https://github.com/xXRoxXeRXx/lymic/")).toBeInTheDocument();
    expect(screen.getByText("https://github.com/xXRoxXeRXx/lymic/issues")).toBeInTheDocument();
  });

  it("omits the version without interrupting initialization when loading fails", async () => {
    const error = new Error("Tauri IPC unavailable");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    tauri.getVersion.mockRejectedValue(error);

    renderAuthenticatedPage();

    expect(await screen.findByText("Synchronization", { selector: "h1" })).toBeInTheDocument();
    await fireEvent.click(await screen.findByRole("button", { name: "Info" }));
    expect(screen.queryByText(/v\d+/)).not.toBeInTheDocument();
    expect(warn).toHaveBeenCalledWith("Failed to load app version", error);
  });
});

describe("application updates", () => {
  it("silently checks for updates at startup and opens a dialog when one is found", async () => {
    tauri.check.mockResolvedValue({ version: "0.15.0", body: "New sync improvements", downloadAndInstall: vi.fn() });
    renderAuthenticatedPage();

    expect(await screen.findByRole("dialog", { name: "Update available" })).toBeInTheDocument();
    expect(screen.getByText("Version 0.15.0 is ready to install.")).toBeInTheDocument();
    expect(screen.getByText("New sync improvements")).toBeInTheDocument();
  });

  it("does not show an error when an update check fails", async () => {
    const error = new Error("GitHub unavailable");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    tauri.check.mockRejectedValue(error);
    renderAuthenticatedPage();

    expect(await screen.findByText("Synchronization", { selector: "h1" })).toBeInTheDocument();
    await waitFor(() => expect(warn).toHaveBeenCalledWith("Failed to check for updates", error));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("checks manually from settings and reports the current version", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));

    expect(await screen.findByText("Lymic is up to date.")).toBeInTheDocument();
    expect(tauri.check).toHaveBeenCalledTimes(2);
  });

  it("shows download progress, installs, and relaunches after confirmation", async () => {
    let finishDownload: (() => void) | undefined;
    const downloadAndInstall = vi.fn((onEvent: (event: { event: string; data: { contentLength?: number; chunkLength?: number } }) => void) => {
      onEvent({ event: "Started", data: { contentLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 40 } });
      return new Promise<void>((resolve) => { finishDownload = resolve; });
    });
    tauri.check.mockResolvedValue({ version: "0.15.0", body: "", downloadAndInstall });
    renderAuthenticatedPage();

    await fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));
    expect(await screen.findByText("Downloading update... 40%")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeDisabled();
    await waitFor(() => expect(syncStarted).toBeTypeOf("function"));
    syncStarted?.();
    expect(screen.getByText("Downloading update... 40%")).toBeInTheDocument();
    expect(screen.queryByText("Finish or resume and complete synchronization before installing this update.")).not.toBeInTheDocument();
    finishDownload?.();

    await waitFor(() => expect(tauri.relaunch).toHaveBeenCalledOnce());
    expect(downloadAndInstall).toHaveBeenCalledOnce();
  });

  it("keeps the dialog open and allows a retry when installation fails", async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue(new Error("Signature rejected"));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    tauri.check.mockResolvedValue({ version: "0.15.0", body: "", downloadAndInstall });
    renderAuthenticatedPage();

    await fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));

    expect(await screen.findByText("The update could not be installed. You can try again.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(error).toHaveBeenCalled();
  });

  it("resets a failed installation state after checking again", async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue(new Error("Signature rejected"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    tauri.check.mockResolvedValue({ version: "0.15.0", body: "", downloadAndInstall });
    renderAuthenticatedPage();

    await fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));
    await screen.findByText("The update could not be installed. You can try again.");
    await fireEvent.click(screen.getByText("Later", { selector: "button" }));
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));

    expect(await screen.findByRole("button", { name: "Install and restart" })).toBeEnabled();
    expect(screen.queryByText("The update could not be installed. You can try again.")).not.toBeInTheDocument();
  });

  it("blocks installation during a paused sync and enables it when idle", async () => {
    const downloadAndInstall = vi.fn();
    tauri.check.mockResolvedValue({ version: "0.15.0", body: "", downloadAndInstall });
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_sync_status") return Promise.resolve({ status: "PAUSED", total: 1, succeeded: 0, failed: 0, currentPath: null });
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve([]);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      return Promise.resolve();
    });
    render(Page);

    await screen.findByText("Sync paused");
    const install = await screen.findByRole("button", { name: "Install and restart" });
    expect(install).toBeDisabled();
    expect(screen.getByText("Finish or resume and complete synchronization before installing this update.")).toBeInTheDocument();
    await waitFor(() => expect(syncProgressSnapshot).toBeTypeOf("function"));
    syncProgressSnapshot?.({ payload: { status: "IDLE", total: 1, succeeded: 1, failed: 0, currentPath: null } });

    await waitFor(() => expect(screen.getByRole("button", { name: "Install and restart" })).toBeEnabled());
  });
});

describe("sync dashboard", () => {
  it("restores a paused queue and resumes it without starting a new scan", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_sync_status") {
        return Promise.resolve({ status: "PAUSED", total: 4, succeeded: 1, failed: 0, currentPath: "C:/photos/two.jpg" });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      if (command === "resume_sync") return Promise.resolve({ status: "RUNNING", total: 4, succeeded: 1, failed: 0, currentPath: "C:/photos/two.jpg" });
      return Promise.resolve();
    });
    render(Page);

    const resume = await screen.findByRole("button", { name: "Resume" });
    expect(screen.getByText("Sync paused")).toBeInTheDocument();
    expect(screen.getByText("25% completed")).toBeInTheDocument();
    await fireEvent.click(resume);

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("resume_sync"));
    expect(tauri.invoke).not.toHaveBeenCalledWith("start_sync");
  });

  it("ignores the legacy tray start event while a queue is paused", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_sync_status") {
        return Promise.resolve({ status: "PAUSED", total: 1, succeeded: 0, failed: 0, currentPath: null });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      return Promise.resolve();
    });
    render(Page);

    await screen.findByRole("button", { name: "Resume" });
    await waitFor(() => expect(triggerSync).toBeTypeOf("function"));
    triggerSync?.();

    expect(tauri.invoke).not.toHaveBeenCalledWith("start_sync");
  });

  it("computes progress preferentially from totalBytes and completedBytes", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_sync_status") {
        return Promise.resolve({
          status: "RUNNING",
          total: 10,
          succeeded: 1,
          failed: 0,
          currentPath: "C:/photos/large.mov",
          totalBytes: 10_000,
          completedBytes: 7_500,
        });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      return Promise.resolve();
    });
    render(Page);

    expect(await screen.findByText("75% completed")).toBeInTheDocument();
    expect(screen.getByText("Processed: 1 / 10")).toBeInTheDocument();
    expect(screen.getByText("large.mov")).toBeInTheDocument();
  });

  it("updates progress from sync-progress-snapshot with byte fields", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await waitFor(() => expect(syncProgressSnapshot).toBeTypeOf("function"));
    syncProgressSnapshot?.({
      payload: {
        status: "RUNNING",
        total: 50,
        succeeded: 5,
        failed: 0,
        currentPath: "C:/photos/clip.mp4",
        totalBytes: 20_000,
        completedBytes: 12_000,
      },
    });

    expect(await screen.findByText("60% completed")).toBeInTheDocument();
    expect(screen.getByText("Processed: 5 / 50")).toBeInTheDocument();
    expect(screen.getByText("clip.mp4")).toBeInTheDocument();
  });

  it("counts succeeded and failed queue entries as processed", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await waitFor(() => expect(syncProgressSnapshot).toBeTypeOf("function"));
    syncProgressSnapshot?.({
      payload: {
        status: "PAUSED",
        total: 10,
        succeeded: 3,
        failed: 2,
        currentPath: "C:/photos/five.jpg",
      },
    });

    expect(await screen.findByText("Processed: 5 / 10")).toBeInTheDocument();
  });

  it("keeps snapshot progress when sync-started follows it", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await waitFor(() => expect(syncProgressSnapshot).toBeTypeOf("function"));
    syncProgressSnapshot?.({
      payload: {
        status: "RUNNING",
        total: 50,
        succeeded: 3,
        failed: 2,
        currentPath: "C:/photos/five.jpg",
        totalBytes: 100,
        completedBytes: 20,
      },
    });
    syncStarted?.();

    expect(await screen.findByText("20% completed")).toBeInTheDocument();
    expect(screen.getByText("Processed: 5 / 50")).toBeInTheDocument();
  });

  it("renders transfer statistics, preserves them while paused, and resets them for a new run", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await waitFor(() => expect(syncTransferStats).toBeTypeOf("function"));
    syncProgressSnapshot?.({ payload: { status: "RUNNING", total: 1, succeeded: 0, failed: 0, currentPath: null } });
    syncTransferStats?.({ payload: { transferredBytes: 1_500_000, transferRateBytesPerSecond: 2_000_000, estimatedSecondsRemaining: 10, status: "RUNNING" } });
    expect(await screen.findByText(/Transferred: 1.4 MiB/)).toBeInTheDocument();
    expect(screen.getByText(/Rate: 2 MB\/s/)).toBeInTheDocument();
    syncPaused?.({ payload: { status: "PAUSED", total: 1, succeeded: 0, failed: 0, currentPath: null } });
    expect(screen.getByText(/Rate: 2 MB\/s/)).toBeInTheDocument();
    syncStarted?.();
    await waitFor(() => expect(screen.queryByText(/Transferred:/)).not.toBeInTheDocument());
  });

  it("changes sync controls from pause to resume when events arrive", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await waitFor(() => expect(syncPaused).toBeTypeOf("function"));
    syncPaused?.({ payload: { status: "PAUSED", total: 2, succeeded: 0, failed: 0, currentPath: "C:/photos/one.jpg" } });

    expect(await screen.findByRole("button", { name: "Resume" })).toBeEnabled();
    expect(screen.getByText("Processed: 0 / 2")).toBeInTheDocument();
    syncResumed?.({ payload: { status: "RUNNING", total: 2, succeeded: 0, failed: 0, currentPath: "C:/photos/one.jpg" } });
    expect(await screen.findByRole("button", { name: "Pause" })).toBeEnabled();
  });

  it("does not start sync without a watched folder", async () => {
    renderAuthenticatedPage();

    const syncButton = await screen.findByRole("button", { name: "Sync Now" });
    expect(syncButton).toBeDisabled();

    await fireEvent.click(syncButton);
    await waitFor(() => expect(triggerSync).toBeTypeOf("function"));
    triggerSync?.();

    expect(tauri.invoke).not.toHaveBeenCalledWith("start_sync");
  });

  it("returns to idle when an empty sync result is received", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "start_sync") {
        return Promise.resolve({ processed: 0, uploaded: 0, failed: 0 });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    const syncButton = await screen.findByRole("button", { name: "Sync Now" });
    await fireEvent.click(syncButton);

    await waitFor(() => {
      expect(screen.getByText("All up to date")).toBeInTheDocument();
      expect(syncButton).toBeEnabled();
    });
  });

  it("disables sync after the final folder is removed", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();

    await screen.findByText("photos");
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "remove_folder") {
        watchedFolders = [];
        return Promise.resolve();
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await fireEvent.click(screen.getByRole("button", { name: "Remove folder: C:/photos" }));

    await waitFor(() => expect(tauri.confirm).toHaveBeenCalledWith(
      "Do you really want to remove this folder?",
      { title: "Remove folder", kind: "warning" },
    ));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Sync Now" })).toBeDisabled();
    });
  });

  it("confirms before cancelling an active folder sync", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    let removeCalls = 0;
    tauri.invoke.mockImplementation((command: string, args?: { confirmCancellation?: boolean }) => {
      if (command === "remove_folder") {
        removeCalls += 1;
        if (!args?.confirmCancellation) return Promise.resolve({ requiresSyncCancellation: true });
        watchedFolders = [];
        return Promise.resolve({ requiresSyncCancellation: false });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      if (command === "get_sync_status") return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      return Promise.resolve();
    });
    render(Page);

    await fireEvent.click(await screen.findByRole("button", { name: "Remove folder: C:/photos" }));

    await waitFor(() => expect(tauri.confirm).toHaveBeenCalledWith(
      "Syncing this folder is in progress. Removing it will cancel active uploads and discard all pending transfers for this folder. Files already uploaded remain on the server. Remove the folder?",
      { title: "Remove folder", kind: "warning" },
    ));
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("remove_folder", { id: 1, confirmCancellation: true }));
    expect(removeCalls).toBe(2);
  });

  it("keeps an active folder when sync cancellation is declined", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    tauri.confirm.mockResolvedValue(false);
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "remove_folder") return Promise.resolve({ requiresSyncCancellation: true });
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      if (command === "get_sync_status") return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      return Promise.resolve();
    });
    render(Page);

    await fireEvent.click(await screen.findByRole("button", { name: "Remove folder: C:/photos" }));

    await waitFor(() => expect(tauri.confirm).toHaveBeenCalled());
    expect(tauri.invoke).not.toHaveBeenCalledWith("remove_folder", { id: 1, confirmCancellation: true });
    expect(screen.getByText("photos")).toBeInTheDocument();
  });

  it("keeps an idle folder when removal confirmation is declined", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    tauri.confirm.mockResolvedValue(false);
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "remove_folder") return Promise.resolve({ requiresSyncCancellation: false });
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve([]);
      if (command === "get_upload_parallelism") return Promise.resolve(3);
      if (command === "get_sync_status") return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      return Promise.resolve();
    });
    render(Page);

    await fireEvent.click(await screen.findByRole("button", { name: "Remove folder: C:/photos" }));

    await waitFor(() => expect(tauri.confirm).toHaveBeenCalledWith(
      "Do you really want to remove this folder?",
      { title: "Remove folder", kind: "warning" },
    ));
    expect(tauri.invoke).not.toHaveBeenCalledWith("remove_folder", { id: 1, confirmCancellation: true });
    expect(screen.getByText("photos")).toBeInTheDocument();
  });

  it("shows persisted failure details and retries them", async () => {
    failedSyncs = [{ localPath: "C:/photos/failed.jpg", failureReason: "Upload rejected" }];
    renderAuthenticatedPage();

    const detailsButton = await screen.findByRole("button", { name: "1 failed file(s)" });
    expect(screen.getByText("Sync completed with errors")).toBeInTheDocument();
    expect(detailsButton).toHaveAttribute("aria-controls", "failed-sync-details-list");
    await fireEvent.click(detailsButton);
    expect(screen.getByLabelText("Failed sync details")).toHaveAttribute("id", "failed-sync-details-list");
    expect(screen.getByText("C:/photos/failed.jpg")).toBeInTheDocument();
    expect(screen.getByText("Upload rejected")).toBeInTheDocument();

    tauri.invoke.mockImplementation((command: string) => {
      if (command === "retry_failed_syncs") {
        failedSyncs = [];
        return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await fireEvent.click(screen.getByRole("button", { name: "Retry failed files" }));

    await waitFor(() => {
      expect(tauri.invoke).toHaveBeenCalledWith("retry_failed_syncs");
      expect(screen.getByText("All up to date")).toBeInTheDocument();
      expect(screen.queryByText("C:/photos/failed.jpg")).not.toBeInTheDocument();
    });
  });

  it("shows all up to date when retry only discards missing files", async () => {
    failedSyncs = [{ localPath: "C:/photos/missing.jpg", failureReason: "Upload rejected" }];
    renderAuthenticatedPage();

    await screen.findByRole("button", { name: "1 failed file(s)" });
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "retry_failed_syncs") {
        failedSyncs = [];
        return Promise.resolve({ processed: 0, uploaded: 0, failed: 0 });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await fireEvent.click(screen.getByRole("button", { name: "Retry failed files" }));

    await waitFor(() => {
      expect(tauri.invoke).toHaveBeenCalledWith("retry_failed_syncs");
      expect(screen.getByText("All up to date")).toBeInTheDocument();
      expect(screen.queryByText("C:/photos/missing.jpg")).not.toBeInTheDocument();
    });
  });

  it("keeps error status when failures remain after a clean sync", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    failedSyncs = [{ localPath: "C:/photos/failed.jpg", failureReason: "Upload rejected" }];
    renderAuthenticatedPage();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "start_sync") return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await screen.findByText("photos");
    await fireEvent.click(await screen.findByRole("button", { name: "Sync Now" }));

    await waitFor(() => expect(screen.getByText("Sync completed with errors")).toBeInTheDocument());
  });
});

describe("activity log", () => {
  it("renders an ERROR payload with its formatted event timestamp", async () => {
    renderAuthenticatedPage();
    await fireEvent.click(await screen.findByRole("button", { name: "Logs" }));
    await waitFor(() => expect(logMessage).toBeTypeOf("function"));
    const timestamp = 1_700_000_000_123;

    logMessage?.({ payload: { level: "ERROR", message: "Upload failed", timestamp } });

    expect(await screen.findByText("Upload failed")).toBeInTheDocument();
    const expectedTime = new Date(timestamp)
      .toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", hour12: false });
    expect(screen.getByText(expectedTime)).toBeInTheDocument();
  });

  it("accepts SUCCESS and WARN structured payloads", async () => {
    renderAuthenticatedPage();
    await fireEvent.click(await screen.findByRole("button", { name: "Logs" }));
    await waitFor(() => expect(logMessage).toBeTypeOf("function"));

    logMessage?.({ payload: { level: "SUCCESS", message: "Sync completed", timestamp: 1_700_000_000_000 } });
    logMessage?.({ payload: { level: "WARN", message: "Storage is almost full", timestamp: 1_700_000_001_000 } });

    expect(await screen.findByText("Sync completed")).toBeInTheDocument();
    expect(screen.getByText("Storage is almost full")).toBeInTheDocument();
  });

  it("defaults unknown levels and malformed timestamps without interrupting logging", async () => {
    renderAuthenticatedPage();
    await fireEvent.click(await screen.findByRole("button", { name: "Logs" }));
    await waitFor(() => expect(logMessage).toBeTypeOf("function"));

    logMessage?.({ payload: { level: "UNKNOWN", message: "Unrecognized level", timestamp: Number.NaN } });

    expect(await screen.findByText("Unrecognized level")).toBeInTheDocument();
  });

  it("renders the localized text for keyed payloads instead of the raw message", async () => {
    const { locale } = await import("svelte-i18n");
    renderAuthenticatedPage();
    await waitFor(() => expect(logMessage).toBeTypeOf("function"));

    try {
      await locale.set("de");
      await fireEvent.click(await screen.findByRole("button", { name: "Protokolle" }));
      logMessage?.({
        payload: {
          level: "INFO",
          message: "raw backend message",
          key: "log_scanning_folder",
          params: { folder: "C:/Media" },
          timestamp: 1_700_000_000_000,
        },
      });

      expect(await screen.findByText("Ordner wird durchsucht: C:/Media")).toBeInTheDocument();
      expect(screen.queryByText("raw backend message")).not.toBeInTheDocument();
    } finally {
      await locale.set("en");
    }
  });
});

describe("logout", () => {
  it("returns to a clean login view after a confirmed logout", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("logout"));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    expect(screen.queryByText("Synchronization", { selector: "h1" })).not.toBeInTheDocument();
  });

  it("does not log out when the confirmation is cancelled", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const logoutButton = await screen.findByRole("button", { name: "Logout" });
    tauri.confirm.mockResolvedValue(false);

    await fireEvent.click(logoutButton);

    expect(tauri.confirm).toHaveBeenCalledWith("Do you really want to log out?", {
      title: "Logout",
      kind: "warning",
    });
    expect(tauri.invoke).not.toHaveBeenCalledWith("logout");
    expect(screen.getByRole("button", { name: "Logout" })).toBeInTheDocument();
  });

  it("keeps the authenticated view and displays an error when logout fails", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const logoutButton = await screen.findByRole("button", { name: "Logout" });
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "logout") return Promise.reject(new Error("Keyring unavailable"));
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_folders") return Promise.resolve([]);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await fireEvent.click(logoutButton);

    expect(await screen.findByRole("alert")).toHaveTextContent("Keyring unavailable");
    expect(screen.getByRole("button", { name: "Logout" })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("logout");
  });
});

describe("settings", () => {
  it("shows the authenticated user name", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(await screen.findByText("Logged in as Meyer")).toBeInTheDocument();
  });

  it("displays language settings and switches language", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(await screen.findByText("Language")).toBeInTheDocument();
    expect(screen.getByText("Choose your preferred language.")).toBeInTheDocument();

    const localeSelect = screen.getByLabelText("Language");
    await fireEvent.change(localeSelect, { target: { value: "de" } });

    await waitFor(() => {
      expect(screen.getByText("Sprache")).toBeInTheDocument();
      expect(screen.getByText("Wähle deine bevorzugte Sprache.")).toBeInTheDocument();
      expect(tauri.invoke).toHaveBeenCalledWith("update_locale", { locale: "de" });
    });
  });

  it("sets and persists an explicit theme preference", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const darkButton = await screen.findByRole("button", { name: "Dark" });
    await fireEvent.click(darkButton);

    expect(darkButton).toHaveAttribute("aria-pressed", "true");
    expect(localStorage.getItem("lymic-theme")).toBe("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("loads and saves upload parallelism", async () => {
    uploadParallelism = 6;
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    const select = await screen.findByLabelText("Parallel uploads");
    await waitFor(() => expect(select).toHaveValue("6"));

    let resolveSave: (() => void) | undefined;
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "set_upload_parallelism") {
        return new Promise<void>((resolve) => {
          resolveSave = resolve;
        });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      if (command === "get_upload_parallelism") return Promise.resolve(uploadParallelism);
      return Promise.resolve();
    });

    await fireEvent.change(select, { target: { value: "5" } });
    expect(select).toBeDisabled();
    resolveSave?.();

    await waitFor(() => {
      expect(tauri.invoke).toHaveBeenCalledWith("set_upload_parallelism", { uploadParallelism: 5 });
      expect(select).toHaveValue("5");
      expect(select).toBeEnabled();
    });
  });

  it("shows an error and restores upload parallelism when saving fails", async () => {
    uploadParallelism = 4;
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "set_upload_parallelism") return Promise.reject(new Error("Database unavailable"));
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") return Promise.resolve({ displayName: "Meyer", profileImage: null });
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      if (command === "get_upload_parallelism") return Promise.resolve(uploadParallelism);
      return Promise.resolve();
    });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const select = await screen.findByLabelText("Parallel uploads");
    await waitFor(() => expect(select).toHaveValue("4"));

    await fireEvent.change(select, { target: { value: "7" } });

    expect(await screen.findByRole("alert")).toHaveTextContent("Database unavailable");
    expect(select).toHaveValue("4");
  });
});

describe("theme preference", () => {
  it("applies a saved preference when rendering", () => {
    localStorage.setItem("lymic-theme", "dark");

    render(Page);

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("removes an invalid preference and falls back to the system theme", () => {
    localStorage.setItem("lymic-theme", "sepia");

    render(Page);

    expect(localStorage.getItem("lymic-theme")).toBeNull();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("responds to system changes only while using the system preference", async () => {
    systemThemeIsDark = true;
    renderAuthenticatedPage();

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    systemThemeIsDark = false;
    colorSchemeListeners.forEach((listener) => listener({ matches: false } as MediaQueryListEvent));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");

    await screen.findByText("Synchronization", { selector: "h1" });
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Dark" }));
    systemThemeIsDark = false;
    colorSchemeListeners.forEach((listener) => listener({ matches: false } as MediaQueryListEvent));

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });
});

describe("removable volume setup flow", () => {
  it("detects removable card, prompts user, opens picker with mount path, and invokes accept_removable_media_folder", async () => {
    tauri.open.mockResolvedValue("E:/DCIM");
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({ displayName: "Meyer", profileImage: null });
        case "get_folders":
          return Promise.resolve(watchedFolders);
        case "get_failed_syncs":
          return Promise.resolve(failedSyncs);
        case "get_upload_parallelism":
          return Promise.resolve(uploadParallelism);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "get_albums":
          return Promise.resolve([{ id: "alb-holiday", albumName: "Holiday 2026" }]);
        case "accept_removable_media_folder":
          watchedFolders = [{ id: 10, path: "E:/DCIM", recursive: true, target_album_id: "alb-holiday" }];
          return Promise.resolve(10);
        default:
          return Promise.resolve();
      }
    });

    expect(removableVolumeDetected).toBeDefined();
    removableVolumeDetected!({
      payload: {
        volumeId: "vol-eos-123",
        mountPath: "E:\\",
        displayName: "EOS_DIGITAL",
      },
    });

    expect(await screen.findByText(/EOS_DIGITAL/)).toBeInTheDocument();
    expect(screen.getByText(/E:\\/)).toBeInTheDocument();

    const syncButton = screen.getByRole("button", { name: "Sync Card" });
    await fireEvent.click(syncButton);

    expect(tauri.open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      defaultPath: "E:\\",
      title: "Select Folder",
    });

    expect(await screen.findByText('Select an optional Immich album for "DCIM"')).toBeInTheDocument();
    expect(await screen.findByText("Holiday 2026")).toBeInTheDocument();

    await fireEvent.click(screen.getByText("Holiday 2026"));

    const confirmButton = screen.getByRole("button", { name: "Add folder" });
    await fireEvent.click(confirmButton);

    expect(tauri.invoke).toHaveBeenCalledWith("accept_removable_media_folder", {
      volumeId: "vol-eos-123",
      path: "E:/DCIM",
      targetAlbumId: "alb-holiday",
    });

    await waitFor(() => {
      expect(screen.queryByText(/EOS_DIGITAL/)).not.toBeInTheDocument();
    });

    expect(await screen.findByText("E:/DCIM")).toBeInTheDocument();
  });

  it("surfaces typed backend errors in feedback banner when accepting outside volume", async () => {
    tauri.open.mockResolvedValue("C:/NotOnCard");
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({ displayName: "Meyer", profileImage: null });
        case "get_folders":
          return Promise.resolve(watchedFolders);
        case "get_failed_syncs":
          return Promise.resolve(failedSyncs);
        case "get_upload_parallelism":
          return Promise.resolve(uploadParallelism);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "get_albums":
          return Promise.resolve([]);
        case "accept_removable_media_folder":
          return Promise.reject(new Error("Path 'C:/NotOnCard' does not reside within removable volume 'vol-eos-123' (mounted at 'E:\\')."));
        default:
          return Promise.resolve();
      }
    });

    removableVolumeDetected!({
      payload: {
        volumeId: "vol-eos-123",
        mountPath: "E:\\",
        displayName: "EOS_DIGITAL",
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Sync Card" }));
    await screen.findByText('Select an optional Immich album for "NotOnCard"');
    await fireEvent.click(screen.getByRole("button", { name: "Add folder" }));




    expect(await screen.findByRole("alert")).toHaveTextContent("does not reside within removable volume");
  });

  it("dismisses prompt without opening dialog when dismiss button is clicked", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Synchronization", { selector: "h1" });

    removableVolumeDetected!({
      payload: {
        volumeId: "vol-eos-123",
        mountPath: "E:\\",
        displayName: "EOS_DIGITAL",
      },
    });

    expect(await screen.findByText(/EOS_DIGITAL/)).toBeInTheDocument();

    const dismissButton = screen.getByRole("button", { name: "Dismiss" });
    await fireEvent.click(dismissButton);

    await waitFor(() => {
      expect(screen.queryByText(/EOS_DIGITAL/)).not.toBeInTheDocument();
    });
    expect(tauri.open).not.toHaveBeenCalled();
  });

  it("renders profile image in sidebar when available", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({
            displayName: "Meyer",
            profileImage: "data:image/jpeg;base64,dGVzdA==",
          });
        case "get_folders":
          return Promise.resolve([]);
        case "get_failed_syncs":
          return Promise.resolve([]);
        case "get_upload_parallelism":
          return Promise.resolve(3);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "update_locale":
          return Promise.resolve();
        default:
          return Promise.resolve();
      }
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    const avatar = await screen.findByTestId("sidebar-profile-image");
    expect(avatar).toHaveAttribute("src", "data:image/jpeg;base64,dGVzdA==");
    expect(screen.getByText("Meyer")).toBeInTheDocument();
  });

  it("degrades silently to standard user icon when profile image fails to load", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({
            displayName: "Meyer",
            profileImage: "data:image/jpeg;base64,invalid",
          });
        case "get_folders":
          return Promise.resolve([]);
        case "get_failed_syncs":
          return Promise.resolve([]);
        case "get_upload_parallelism":
          return Promise.resolve(3);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "update_locale":
          return Promise.resolve();
        default:
          return Promise.resolve();
      }
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    const avatar = await screen.findByTestId("sidebar-profile-image");
    await fireEvent.error(avatar);

    expect(screen.queryByTestId("sidebar-profile-image")).not.toBeInTheDocument();
    expect(screen.getByTestId("sidebar-user-icon")).toBeInTheDocument();
    expect(screen.getByText("Meyer")).toBeInTheDocument();
  });

  it("renders update actions in settings when running in GitHub release channel", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({ displayName: "Meyer", profileImage: null });
        case "get_folders":
          return Promise.resolve([]);
        case "get_failed_syncs":
          return Promise.resolve([]);
        case "get_upload_parallelism":
          return Promise.resolve(3);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "get_locale_catalog":
          return Promise.resolve([{ code: "en", name: "English" }]);
        case "update_locale":
          return Promise.resolve();
        case "get_release_channel":
          return Promise.resolve("github");
        default:
          return Promise.resolve();
      }
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("button", { name: "Check for updates" })).toBeInTheDocument();
  });

  it("omits update actions in settings when running in Store release channel", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      switch (command) {
        case "get_auth_status":
          return Promise.resolve(true);
        case "get_server_url":
          return Promise.resolve("https://immich.example");
        case "get_current_user":
          return Promise.resolve({ displayName: "Meyer", profileImage: null });
        case "get_folders":
          return Promise.resolve([]);
        case "get_failed_syncs":
          return Promise.resolve([]);
        case "get_upload_parallelism":
          return Promise.resolve(3);
        case "get_sync_status":
          return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
        case "get_locale_catalog":
          return Promise.resolve([{ code: "en", name: "English" }]);
        case "update_locale":
          return Promise.resolve();
        case "get_release_channel":
          return Promise.resolve("store");
        default:
          return Promise.resolve();
      }
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await screen.findByText("Language");
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
    expect(screen.queryByText("Updates")).toBeNull();
  });
});

describe("watched folder setup flow with target album", () => {
  it("prompts for target album when adding folder from dashboard and adds folder with selected target album", async () => {
    tauri.open.mockResolvedValue("D:/Photos");
    mockAuthenticatedDashboard({
      get_albums: () => Promise.resolve([{ id: "alb-family", albumName: "Family 2026" }]),
      add_folder: () => {
        watchedFolders = [{ id: 1, path: "D:/Photos", recursive: true, target_album_id: "alb-family" }];
        return Promise.resolve(1);
      },
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    const addFolderButton = screen.getByRole("button", { name: "Add" });
    await fireEvent.click(addFolderButton);

    expect(tauri.open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Select Folder",
    });

    const dialog = await screen.findByRole("dialog", { name: "Select Target Album" });
    expect(within(dialog).getByText('Select an optional Immich album for "Photos"')).toBeInTheDocument();
    expect(await within(dialog).findByText("Family 2026")).toBeInTheDocument();

    await fireEvent.click(within(dialog).getByText("Family 2026"));

    const confirmButton = within(dialog).getByRole("button", { name: "Add folder" });
    await fireEvent.click(confirmButton);

    expect(tauri.invoke).toHaveBeenCalledWith("add_folder", {
      path: "D:/Photos",
      targetAlbumId: "alb-family",
    });

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Select Target Album" })).not.toBeInTheDocument();
    });

    expect(await screen.findByText("D:/Photos")).toBeInTheDocument();
  });

  it("creates watched folder with no target album when confirmed without selecting album", async () => {
    tauri.open.mockResolvedValue("D:/Photos");
    mockAuthenticatedDashboard({
      get_albums: () => Promise.resolve([{ id: "alb-family", albumName: "Family 2026" }]),
      add_folder: () => {
        watchedFolders = [{ id: 1, path: "D:/Photos", recursive: true, target_album_id: null }];
        return Promise.resolve(1);
      },
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    const addFolderButton = screen.getByRole("button", { name: "Add" });
    await fireEvent.click(addFolderButton);

    const dialog = await screen.findByRole("dialog", { name: "Select Target Album" });
    expect(within(dialog).getByText('Select an optional Immich album for "Photos"')).toBeInTheDocument();

    const confirmButton = within(dialog).getByRole("button", { name: "Add folder" });
    await fireEvent.click(confirmButton);

    expect(tauri.invoke).toHaveBeenCalledWith("add_folder", {
      path: "D:/Photos",
      targetAlbumId: null,
    });

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Select Target Album" })).not.toBeInTheDocument();
    });

    expect(await screen.findByText("D:/Photos")).toBeInTheDocument();
  });

  it("leaves no pending folder state and does not create folder when cancelled", async () => {
    tauri.open.mockResolvedValue("D:/Photos");
    mockAuthenticatedDashboard({
      get_albums: () => Promise.resolve([{ id: "alb-family", albumName: "Family 2026" }]),
    });
    render(Page);
    await screen.findByText("Synchronization", { selector: "h1" });

    const addFolderButton = screen.getByRole("button", { name: "Add" });
    await fireEvent.click(addFolderButton);

    const dialog = await screen.findByRole("dialog", { name: "Select Target Album" });
    expect(within(dialog).getByText('Select an optional Immich album for "Photos"')).toBeInTheDocument();

    const cancelButton = within(dialog).getByRole("button", { name: "Cancel" });
    await fireEvent.click(cancelButton);

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Select Target Album" })).not.toBeInTheDocument();
    });

    expect(tauri.invoke).not.toHaveBeenCalledWith("add_folder", expect.anything());
    expect(screen.queryByText("D:/Photos")).not.toBeInTheDocument();
  });
});
