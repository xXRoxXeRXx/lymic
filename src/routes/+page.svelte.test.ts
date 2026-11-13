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
let syncStarted: (() => void) | undefined;
let syncIdle: (() => void) | undefined;
let syncPaused: ((event: { payload: unknown }) => void) | undefined;
let syncResumed: ((event: { payload: unknown }) => void) | undefined;
let syncProgressSnapshot: ((event: { payload: unknown }) => void) | undefined;
let syncTransferStats: ((event: { payload: unknown }) => void) | undefined;
let logMessage: ((event: { payload: unknown }) => void) | undefined;
let removableVolumeDetected: ((event: { payload: unknown }) => void) | undefined;
let removableVolumeRemounted: ((event: { payload: unknown }) => void) | undefined;
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
      case "get_timeline_buckets":
        return Promise.resolve([]);
      case "get_timeline_bucket_assets":
        return Promise.resolve([]);
      case "get_favorites_timeline_buckets":
        return Promise.resolve([]);
      case "get_favorites_timeline_bucket_assets":
        return Promise.resolve([]);
      case "get_album_timeline":
        return Promise.resolve({ buckets: [], assetsByBucket: {} });
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

async function openDashboard() {
  await fireEvent.click(await screen.findByRole("button", { name: "Synchronization" }));
  return screen.findByText("Synchronization", { selector: "h1" });
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
  syncStarted = undefined;
  syncIdle = undefined;
  syncPaused = undefined;
  syncResumed = undefined;
  syncProgressSnapshot = undefined;
  syncTransferStats = undefined;
  logMessage = undefined;
  removableVolumeDetected = undefined;
  removableVolumeRemounted = undefined;
  tauri.listen.mockImplementation((event: string, handler: () => void) => {
    if (event === "sync-started") syncStarted = handler;
    if (event === "sync-idle") syncIdle = handler;
    if (event === "sync-paused") syncPaused = handler as (event: { payload: unknown }) => void;
    if (event === "sync-resumed") syncResumed = handler as (event: { payload: unknown }) => void;
    if (event === "sync-progress-snapshot") syncProgressSnapshot = handler as (event: { payload: unknown }) => void;
    if (event === "sync-transfer-stats") syncTransferStats = handler as (event: { payload: unknown }) => void;
    if (event === "log-message") logMessage = handler as (event: { payload: unknown }) => void;
    if (event === "removable-volume-detected") removableVolumeDetected = handler as (event: { payload: unknown }) => void;
    if (event === "removable-volume-remounted") removableVolumeRemounted = handler as (event: { payload: unknown }) => void;
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

    await openDashboard();
    await fireEvent.click(await screen.findByRole("button", { name: "Info" }));
    expect(screen.queryByText(/v\d+/)).not.toBeInTheDocument();
    expect(warn).toHaveBeenCalledWith("Failed to load app version", error);
  });
});

describe("Photo Timeline", () => {
  it("opens Photos by default and renders descending month buckets", async () => {
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 12 },
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 3 },
      ],
    });

    expect(await screen.findByRole("heading", { name: "Photos" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Photos" })[0]).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("12 assets")).toBeInTheDocument();
    expect(screen.getByText("February 2025")).toBeInTheDocument();
    expect(screen.getByText("3 assets")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_timeline_buckets");
  });

  it("refreshes cached timeline buckets after synchronization settles", async () => {
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [],
    });

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();

    await waitFor(() => {
      const timelineRefreshes = tauri.invoke.mock.calls.filter(([command]) => command === "get_timeline_buckets");
      expect(timelineRefreshes).toHaveLength(2);
    });
  });
});

describe("Album Timelines", () => {
  it("opens a selected album in the shared timeline and returns to its catalogue", async () => {
    renderAuthenticatedPage({
      get_album_catalogue: async () => [{ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null }],
      get_album_timeline: async () => ({
        buckets: [{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }],
        assetsByBucket: {
          "2025-03-01T00:00:00.000Z": [{ id: "asset-1", width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null }],
        },
      }),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Albums" }));
    await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));

    expect(await screen.findByRole("heading", { name: "Holiday" })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_album_timeline", { albumId: "album-1" });
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Albums" }));
    expect(await screen.findByRole("heading", { name: "Albums" })).toBeInTheDocument();
    expect(screen.getByText("Holiday")).toBeInTheDocument();
  });

  it("keeps cached album browsing visible and offers recovery when refresh goes offline", async () => {
    renderAuthenticatedPage({
      get_album_catalogue: vi.fn()
        .mockResolvedValueOnce([{ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null }])
        .mockRejectedValueOnce(new Error("offline")),
      get_album_timeline: vi.fn()
        .mockResolvedValueOnce({
          buckets: [{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }],
          assetsByBucket: {
            "2025-03-01T00:00:00.000Z": [{ id: "asset-1", width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null }],
          },
        })
        .mockRejectedValueOnce(new Error("offline")),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Albums" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Refresh albums" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Albums are offline");
    expect(screen.getByText("Holiday")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: /Holiday/ }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Refresh timeline" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Photo Timeline is offline");
    expect(screen.getByText("March 2025")).toBeInTheDocument();
  });

  it("shows an album loading error instead of an empty album when its first timeline request fails", async () => {
    renderAuthenticatedPage({
      get_album_catalogue: async () => [{ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null }],
      get_album_timeline: async () => { throw new Error("offline"); },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Albums" }));
    await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Albums could not be loaded.");
    expect(screen.queryByText("This album has no reachable photos yet.")).not.toBeInTheDocument();
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

    await openDashboard();
    await waitFor(() => expect(warn).toHaveBeenCalledWith("Failed to check for updates", error));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("checks manually from settings and reports the current version", async () => {
    renderAuthenticatedPage();
    await openDashboard();
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
    await openDashboard();

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
    await openDashboard();

    const resume = await screen.findByRole("button", { name: "Resume" });
    expect(screen.getByText("Sync paused")).toBeInTheDocument();
    expect(screen.getByText("25% completed")).toBeInTheDocument();
    await fireEvent.click(resume);

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("resume_sync"));
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
    await openDashboard();

    expect(await screen.findByText("75% completed")).toBeInTheDocument();
    expect(screen.getByText("Processed: 1 / 10")).toBeInTheDocument();
    expect(screen.getByText("large.mov")).toBeInTheDocument();
  });

  it("updates progress from sync-progress-snapshot with byte fields", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await openDashboard();
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
    await openDashboard();
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
    await openDashboard();
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
    await openDashboard();
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
    await openDashboard();
    await waitFor(() => expect(syncPaused).toBeTypeOf("function"));
    syncPaused?.({ payload: { status: "PAUSED", total: 2, succeeded: 0, failed: 0, currentPath: "C:/photos/one.jpg" } });

    expect(await screen.findByRole("button", { name: "Resume" })).toBeEnabled();
    expect(screen.getByText("Processed: 0 / 2")).toBeInTheDocument();
    syncResumed?.({ payload: { status: "RUNNING", total: 2, succeeded: 0, failed: 0, currentPath: "C:/photos/one.jpg" } });
    expect(await screen.findByRole("button", { name: "Pause" })).toBeEnabled();
  });

  it("does not start sync without a watched folder", async () => {
    renderAuthenticatedPage();
    await openDashboard();

    const syncButton = await screen.findByRole("button", { name: "Sync Now" });
    expect(syncButton).toBeDisabled();

    await fireEvent.click(syncButton);
    expect(tauri.invoke).not.toHaveBeenCalledWith("start_sync");
  });

  it("returns to idle when an empty sync result is received", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await openDashboard();
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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();
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
    await openDashboard();
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("logout"));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    expect(screen.queryByText("Synchronization", { selector: "h1" })).not.toBeInTheDocument();
  });

  it("does not log out when the confirmation is cancelled", async () => {
    renderAuthenticatedPage();
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    expect(await screen.findByText("Logged in as Meyer")).toBeInTheDocument();
  });

  it("displays language settings and switches language", async () => {
    renderAuthenticatedPage();
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();
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

    await openDashboard();
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Dark" }));
    systemThemeIsDark = false;
    colorSchemeListeners.forEach((listener) => listener({ matches: false } as MediaQueryListEvent));

    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });
});

describe("removable volume setup flow", () => {
  it("refreshes watched folders when a removable volume remounts", async () => {
    watchedFolders = [{ id: 10, path: "E:/DCIM", recursive: true, target_album_id: null }];
    renderAuthenticatedPage();
    await openDashboard();

    expect(await screen.findByText("E:/DCIM")).toBeInTheDocument();

    watchedFolders = [{ id: 10, path: "F:/DCIM", recursive: true, target_album_id: null }];
    removableVolumeRemounted!({
      payload: {
        volumeId: "vol-eos-123",
        mountPath: "F:\\",
        displayName: "EOS_DIGITAL",
      },
    });

    expect(await screen.findByText("F:/DCIM")).toBeInTheDocument();
    expect(screen.queryByText("E:/DCIM")).not.toBeInTheDocument();
  });

  it("detects removable card, prompts user, opens picker with mount path, and invokes accept_removable_media_folder", async () => {
    tauri.open.mockResolvedValue("E:/DCIM");
    renderAuthenticatedPage();
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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
    await openDashboard();

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

describe("Favorites Timeline", () => {
  function stubImmediateIntersectionObserver() {
    vi.stubGlobal("ResizeObserver", class {
      observe() {}
      disconnect() {}
    });
    vi.stubGlobal("IntersectionObserver", class {
      constructor(private readonly callback: IntersectionObserverCallback) {}
      observe(target: Element) {
        this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
      }
      disconnect() {}
    });
  }

  it("opens Favorites from navigation, loads month buckets and media with scrubber, hides refresh controls, and preserves timeline actions", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_favorites_timeline_buckets: async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
        { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
      ],
      get_favorites_timeline_bucket_assets: async (args) => {
        const { timeBucket } = args as { timeBucket: string };
        if (timeBucket === "2025-03-01T00:00:00.000Z") {
          return [
            { id: "fav-photo-1", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
            { id: "fav-video-1", width: 1920, height: 1080, type: "VIDEO", takenAt: "2025-03-04T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: 65, isFavorite: true },
          ];
        }
        if (timeBucket === "2024-12-01T00:00:00.000Z") {
          return [
            { id: "fav-photo-2", width: 1200, height: 900, type: "IMAGE", takenAt: "2024-12-10T08:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
          ];
        }
        return [];
      },
      get_albums: async () => [
        { id: "album-1", albumName: "Holiday" },
      ],
      get_asset_details: async () => ({
        originalFileName: "favorite.jpg",
        isFavorite: true,
        cameraMake: null,
        cameraModel: null,
        city: null,
        state: null,
        country: null,
        fNumber: null,
        exposureTime: null,
        iso: null,
        focalLength: null,
        lensModel: null,
        imageWidth: 1600,
        imageHeight: 1200,
        fileSize: 1024,
      }),
    });

    expect(await screen.findByRole("heading", { name: "Photos" })).toBeInTheDocument();
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_favorites_timeline_buckets");

    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));

    expect(await screen.findByRole("heading", { name: "Favorites" })).toBeInTheDocument();
    expect(screen.getByLabelText("Favorites", { selector: "div" })).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("2 assets")).toBeInTheDocument();
    expect(screen.getByText("December 2024")).toBeInTheDocument();
    expect(screen.getByText("1 assets")).toBeInTheDocument();

    expect(tauri.invoke).toHaveBeenCalledWith("get_favorites_timeline_buckets");
    expect(tauri.invoke).toHaveBeenCalledWith("get_favorites_timeline_bucket_assets", { timeBucket: "2025-03-01T00:00:00.000Z" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_favorites_timeline_bucket_assets", { timeBucket: "2024-12-01T00:00:00.000Z" });

    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh timeline" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();

    // Selection + Download + Add to album actions remain available
    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    expect(await screen.findByText("Holiday")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // Lightbox + Download original + Add to album remain available
    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to album" })).toBeInTheDocument();
  });

  it("refreshes Favorites on every entry from another view, no-ops when already active, prunes removed buckets, and does not poll", async () => {
    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");
    const getFavoritesBuckets = vi.fn()
      .mockResolvedValueOnce([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
        { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
      ])
      .mockResolvedValueOnce([
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
      ]);

    renderAuthenticatedPage({
      get_favorites_timeline_buckets: getFavoritesBuckets,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByText("December 2024")).toBeInTheDocument();
    expect(getFavoritesBuckets).toHaveBeenCalledTimes(1);

    // Clicking Favorites again while already active is a no-op
    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(getFavoritesBuckets).toHaveBeenCalledTimes(1);

    // Switching away and back refreshes from Immich and removes old buckets
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    await waitFor(() => {
      expect(getFavoritesBuckets).toHaveBeenCalledTimes(2);
      expect(screen.queryByText("December 2024")).not.toBeInTheDocument();
    });
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(getFavoritesBuckets).toHaveBeenCalledTimes(2);
    const appIntervals = setIntervalSpy.mock.calls.filter(
      ([callback]) => typeof callback !== "function" || callback.name !== "checkRealTimersCallback",
    );
    expect(appIntervals).toHaveLength(0);
    setIntervalSpy.mockRestore();
  });

  it("shows a loading state before resolving an empty Favorites state, and an error alert on initial load failure", async () => {
    let resolveBuckets!: (value: unknown) => void;
    const pendingBuckets = new Promise((resolve) => {
      resolveBuckets = resolve;
    });

    renderAuthenticatedPage({
      get_favorites_timeline_buckets: () => pendingBuckets,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading favorites…");
    expect(screen.queryByText("No favorites yet. Mark photos or videos as favorites to see them here.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh timeline" })).not.toBeInTheDocument();

    resolveBuckets([]);
    expect(
      await screen.findByText("No favorites yet. Mark photos or videos as favorites to see them here."),
    ).toHaveAttribute("role", "status");
    expect(screen.queryByText("Loading favorites…")).not.toBeInTheDocument();

    cleanup();

    renderAuthenticatedPage({
      get_favorites_timeline_buckets: () => Promise.reject(new Error("offline")),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Favorites could not be loaded.");
    expect(screen.queryByText("No favorites yet. Mark photos or videos as favorites to see them here.")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  it("keeps previously loaded favorites visible and shows an offline status banner when re-entry refresh fails", async () => {
    const getFavoritesBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }])
      .mockRejectedValueOnce(new Error("offline"));

    renderAuthenticatedPage({
      get_favorites_timeline_buckets: getFavoritesBuckets,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "Favorites Timeline is offline. Showing cached favorites.",
    );
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh timeline" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  it("clears Favorites state on logout so the next session does not retain prior favorites", async () => {
    const getFavoritesBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }])
      .mockImplementationOnce(() => new Promise(() => {}));

    renderAuthenticatedPage({
      get_favorites_timeline_buckets: getFavoritesBuckets,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();

    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    await fireEvent.input(screen.getByLabelText("API Key"), { target: { value: "secret-key" } });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading favorites…");
    expect(screen.queryByText("March 2025")).not.toBeInTheDocument();
  });
});
