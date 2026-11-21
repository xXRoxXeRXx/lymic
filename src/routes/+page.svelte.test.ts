import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
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


/**
 * A handler key is the command name; a Filtered Timeline request is keyed by command and timeline,
 * for example `get_filtered_timeline_buckets:archive`.
 */
function handlerKey(command: string, args?: unknown) {
  const timeline = (args as { timeline?: string } | undefined)?.timeline;
  return command.startsWith("get_filtered_timeline_") && timeline ? `${command}:${timeline}` : command;
}

function mockAuthenticatedDashboard(handlers: Record<string, (args?: unknown) => Promise<unknown>> = {}) {
  tauri.invoke.mockImplementation((command: string, args?: unknown) => {
    const key = handlerKey(command, args);
    if (key in handlers) {
      return handlers[key](args);
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
      case "get_filtered_timeline_buckets":
        return Promise.resolve([]);
      case "get_filtered_timeline_bucket_assets":
        return Promise.resolve([]);
      case "get_album_timeline":
        return Promise.resolve({ buckets: [], assetsByBucket: {} });
      case "get_places":
        return Promise.resolve([]);
      case "get_tags":
        return Promise.resolve([]);
      case "get_memories":
        return Promise.resolve([]);
      // By default Smart Search is off, so a Search starts in Filename mode (Context is tested explicitly).
      case "get_search_features":
        return Promise.resolve({ smartSearch: false, ocr: true });
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

/** Stubs the observers so a timeline lays out and activates every bucket as soon as it renders. */
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

describe("main navigation", () => {
  it("lists the main navigation entries in the configured order and separates library, sync, and app sections", async () => {
    renderAuthenticatedPage();

    const nav = await screen.findByRole("navigation", { name: "Main navigation" });
    const groups = Array.from(nav.children).map((group) =>
      within(group as HTMLElement)
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    );

    expect(groups).toEqual([
      ["Photos", "Explore", "Map", "People", "Favorites", "Albums", "Tags", "Archive", "Trash"],
      ["Synchronization", "Logs"],
      ["Settings", "Info"],
    ]);
  });
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
  it("opens Favorites from navigation, loads month buckets and media with scrubber, hides refresh controls, and preserves timeline actions", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:favorites": async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
        { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
      ],
      "get_filtered_timeline_bucket_assets:favorites": async (args) => {
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
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "favorites" });

    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));

    expect(await screen.findByRole("heading", { name: "Favorites" })).toBeInTheDocument();
    expect(screen.getByLabelText("Favorites", { selector: "div" })).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("2 assets")).toBeInTheDocument();
    expect(screen.getByText("December 2024")).toBeInTheDocument();
    expect(screen.getByText("1 assets")).toBeInTheDocument();

    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "favorites" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "favorites", timeBucket: "2025-03-01T00:00:00.000Z" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "favorites", timeBucket: "2024-12-01T00:00:00.000Z" });

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
      "get_filtered_timeline_buckets:favorites": getFavoritesBuckets,
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
      "get_filtered_timeline_buckets:favorites": () => pendingBuckets,
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
      "get_filtered_timeline_buckets:favorites": () => Promise.reject(new Error("offline")),
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
      "get_filtered_timeline_buckets:favorites": getFavoritesBuckets,
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
      "get_filtered_timeline_buckets:favorites": getFavoritesBuckets,
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

  it("removes unfavorited assets via desktop IPC after confirmation, preserves Lightbox former-position navigation, and renders the empty state when none remain", async () => {
    stubImmediateIntersectionObserver();
    let resolveFirstRemoval!: () => void;
    let favoritingCall = 0;

    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:favorites": async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 2 },
        { timeBucket: "2024-12-01T00:00:00.000Z", count: 1 },
      ],
      "get_filtered_timeline_bucket_assets:favorites": async (args) => {
        const { timeBucket } = args as { timeBucket: string };
        if (timeBucket === "2025-03-01T00:00:00.000Z") {
          return [
            { id: "fav-1", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
            { id: "fav-2", width: 1920, height: 1080, type: "VIDEO", takenAt: "2025-03-04T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: 45, isFavorite: true },
          ];
        }
        return [
          { id: "fav-3", width: 1200, height: 900, type: "IMAGE", takenAt: "2024-12-10T08:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
        ];
      },
      get_asset_details: async (args) => {
        const { assetId } = args as { assetId: string };
        return {
          originalFileName: `${assetId}.jpg`,
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
        };
      },
      set_asset_favorite: () => {
        favoritingCall += 1;
        if (favoritingCall === 1) {
          return new Promise<void>((resolve) => {
            resolveFirstRemoval = resolve;
          });
        }
        return Promise.resolve();
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();

    // Remove fav-2 while open in Lightbox: stays in grid until Immich confirms
    await fireEvent.click(screen.getByRole("button", { name: "Open video" }));
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));

    expect(tauri.invoke).toHaveBeenCalledWith("set_asset_favorite", { assetId: "fav-2", isFavorite: false });
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(3);
    expect(screen.getByText("2 assets")).toBeInTheDocument();

    resolveFirstRemoval();
    await waitFor(() => expect(screen.queryByRole("button", { name: "Open video" })).not.toBeInTheDocument());
    expect(screen.getAllByText("1 assets")).toHaveLength(2);
    expect(screen.getByRole("dialog", { name: "Media lightbox" })).toBeInTheDocument();

    // Next navigates from fav-2's former position across the bucket boundary to fav-3
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-3");

    // Removing fav-3 drops December 2024; Next wraps to fav-1
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.queryByText("December 2024")).not.toBeInTheDocument());
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-1");

    // Removing the last remaining favorite shows empty state after closing Lightbox
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() =>
      expect(screen.getByText("No favorites yet. Mark photos or videos as favorites to see them here.")).toBeInTheDocument(),
    );
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
  });

  it("rolls back single favorite removal failures and handles bulk removal partial and complete failures via desktop IPC", async () => {
    stubImmediateIntersectionObserver();
    let callCount = 0;

    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:favorites": async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 3 },
      ],
      "get_filtered_timeline_bucket_assets:favorites": async () => [
        { id: "fav-1", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
        { id: "fav-2", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-04T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
        { id: "fav-3", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-03T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
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
      set_asset_favorite: () => {
        callCount += 1;
        // Call 1: single removal rollback; Call 2: bulk item 1 succeeds; Calls 3-5: bulk failures
        if (callCount === 2) return Promise.resolve();
        return Promise.reject(new Error("offline"));
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    expect(await screen.findAllByLabelText("Favorite")).toHaveLength(3);

    // Single removal failure keeps item and restores confirmed favorite state in grid and Lightbox
    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(within(lightbox).getByRole("alert")).toHaveTextContent("Could not update favorite"));
    expect(within(lightbox).getByRole("button", { name: "Remove from favorites" })).toBeEnabled();
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(3);

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

    // Bulk partial failure removes confirmed fav-1 and preserves fav-2
    let checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Updated 1 of 2 assets"));
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();

    // Bulk complete failure reports alert and leaves remaining items untouched
    checkboxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not update selected assets"));
    expect(screen.getAllByLabelText("Favorite")).toHaveLength(2);
    expect(screen.getByText("2 assets")).toBeInTheDocument();
  });

  it("keeps unfavorited assets in the Photos timeline when removing a favorite there", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
      ],
      get_timeline_bucket_assets: async () => [
        { id: "photo-1", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
      ],
      get_asset_details: async () => ({
        originalFileName: "photo-1.jpg",
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
      set_asset_favorite: async () => undefined,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Open photo" }));
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    await waitFor(() => expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeEnabled());

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.getByRole("button", { name: "Open photo" })).toBeInTheDocument();
    expect(screen.getByText("1 assets")).toBeInTheDocument();
  });

  it("surfaces navigated-away favorite removal rejection errors in the Favorites view without duplicating Lightbox alerts", async () => {
    stubImmediateIntersectionObserver();
    let rejectPendingRemoval!: (error: Error) => void;

    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:favorites": async () => [
        { timeBucket: "2025-03-01T00:00:00.000Z", count: 1 },
        { timeBucket: "2025-02-01T00:00:00.000Z", count: 1 },
      ],
      "get_filtered_timeline_bucket_assets:favorites": async (args) => {
        const { timeBucket } = args as { timeBucket: string };
        if (timeBucket === "2025-03-01T00:00:00.000Z") {
          return [
            { id: "fav-1", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
          ];
        }
        return [
          { id: "fav-2", width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-02-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: true },
        ];
      },
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
      set_asset_favorite: () =>
        new Promise<void>((_, reject) => {
          rejectPendingRemoval = reject;
        }),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Favorites" }));
    const openButtons = await screen.findAllByRole("button", { name: "Open photo" });
    expect(openButtons).toHaveLength(2);

    // Trigger pending removal on fav-1, navigate to fav-2, then reject fav-1
    await fireEvent.click(openButtons[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Remove from favorites" }));
    expect(tauri.invoke).toHaveBeenCalledWith("set_asset_favorite", { assetId: "fav-1", isFavorite: false });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Next asset" }));
    await waitFor(() => expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/fav-2"));

    rejectPendingRemoval(new Error("offline"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not update favorite"));
    expect(within(lightbox).queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("Explore Overview", () => {
  const people = [
    { id: "person-1", name: "Ada", thumbnailPath: "/p/1.jpeg" },
    { id: "person-2", name: null, thumbnailPath: null },
  ];

  it("adds an Explore entry directly after Photos while Photos stays the opened view", async () => {
    renderAuthenticatedPage();

    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
    const names = within(screen.getByRole("navigation", { name: "Main navigation" }))
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label"));
    expect(names.slice(0, 3)).toEqual(["Photos", "Explore", "Map"]);
    expect(names).toContain("People");
  });

  it("shows Recognized People in the People section like the People view does", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }) });

    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    const section = await screen.findByRole("region", { name: "People" });
    expect(within(section).getByRole("button", { name: "Ada" })).toBeInTheDocument();
    expect(within(section).getByRole("img", { name: "Ada" })).toHaveAttribute("src", "lymic-thumb://person/person-1?v=%2Fp%2F1.jpeg");
    expect(within(section).getByRole("button", { name: "Unnamed person" })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_people", { page: 1, size: 50 });
  });

  it("opens the People view from View all and a person's Photo Timeline from a tile", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }) });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    const section = await screen.findByRole("region", { name: "People" });

    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    expect(await screen.findByRole("heading", { name: "People", level: 1 })).toBeInTheDocument();

    await fireEvent.click(screen.getAllByRole("button", { name: "Explore" })[0]);
    await fireEvent.click(await screen.findByRole("button", { name: "Ada" }));
    expect(await screen.findByRole("heading", { name: "Ada", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_person_timeline_buckets", { personId: "person-1" });
  });

  it("returns from a person's Photo Timeline to the view it was opened from", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }) });

    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Ada" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));
    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "People" })).toBeInTheDocument();

    await fireEvent.click(within(screen.getByRole("navigation", { name: "Main navigation" })).getByRole("button", { name: "People" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Ada" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Back to People" }));
    expect(await screen.findByRole("heading", { name: "People", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "People" })).not.toBeInTheDocument();
  });

  it("restores the Explore scroll position after returning from a Photo Timeline", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }) });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    const tile = await screen.findByRole("button", { name: "Ada" });
    const main = tile.closest("main")!;
    main.scrollTop = 240;

    await fireEvent.click(tile);
    main.scrollTop = 0;
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));

    await waitFor(() => expect(main.scrollTop).toBe(240));
  });

  it("shows a People failure in its section only and retries it", async () => {
    renderAuthenticatedPage({
      get_people: vi.fn()
        .mockRejectedValueOnce("recognized_people_permission_denied:person.read")
        .mockResolvedValueOnce({ people, hasNextPage: false }),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    const section = await screen.findByRole("region", { name: "People" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("People is unavailable");
    expect(screen.queryByText(/Nothing to explore yet/)).not.toBeInTheDocument();

    await fireEvent.click(within(section).getByRole("button", { name: "Retry" }));
    expect(await within(section).findByRole("button", { name: "Ada" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows a generic People error for other failures", async () => {
    renderAuthenticatedPage({ get_people: async () => { throw new Error("boom"); } });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    expect(await within(await screen.findByRole("region", { name: "People" })).findByRole("alert")).toHaveTextContent("People could not be loaded.");
  });

  it("keeps cached people visible with an offline indication when a refresh fails", async () => {
    renderAuthenticatedPage({
      get_people: vi.fn()
        .mockResolvedValueOnce({ people, hasNextPage: false })
        .mockRejectedValueOnce(new Error("offline")),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    await screen.findByRole("button", { name: "Ada" });

    await fireEvent.click(screen.getAllByRole("button", { name: "Photos" })[0]);
    await fireEvent.click(screen.getAllByRole("button", { name: "Explore" })[0]);

    const section = await screen.findByRole("region", { name: "People" });
    expect(await within(section).findByRole("status")).toHaveTextContent("People is offline");
    expect(within(section).getByRole("button", { name: "Ada" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("hides an empty People section and shows the placeholder when nothing else is left", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people: [], hasNextPage: false }) });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    expect(await screen.findByText(/Nothing to explore yet/)).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "People" })).not.toBeInTheDocument();
  });

  it("refreshes when opened and after Synchronization settles while open", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }) });
    const peopleCalls = () => tauri.invoke.mock.calls.filter(([command]) => command === "get_people").length;

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(tauri.invoke.mock.calls.filter(([command]) => command === "get_timeline_buckets")).toHaveLength(2));
    expect(peopleCalls()).toBe(0);

    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    await screen.findByRole("button", { name: "Ada" });
    expect(peopleCalls()).toBe(1);

    syncIdle!();
    await waitFor(() => expect(peopleCalls()).toBe(2));
  });

  it("shows only as many people as fit in one row", async () => {
    vi.stubGlobal("ResizeObserver", class {
      constructor(private readonly callback: ResizeObserverCallback) {}
      observe(target: Element) {
        this.callback([{ target, contentRect: { width: 500 } } as unknown as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      disconnect() {}
    });
    const many = Array.from({ length: 8 }, (_, index) => ({ id: `p-${index}`, name: `Person ${index}`, thumbnailPath: null }));
    renderAuthenticatedPage({ get_people: async () => ({ people: many, hasNextPage: false }) });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    const section = await screen.findByRole("region", { name: "People" });
    await waitFor(() => expect(within(section).getAllByRole("button", { name: /^Person / })).toHaveLength(3));
    expect(within(section).getByRole("button", { name: "Person 0" })).toBeInTheDocument();
    expect(within(section).queryByRole("button", { name: "Person 3" })).not.toBeInTheDocument();
  });
});

describe("Explore Places", () => {
  const places = [
    { city: "Paris", thumbnailAssetId: "asset-1" },
    { city: "Berlin", thumbnailAssetId: "asset-2" },
  ];
  const openExplore = async () => fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

  it("shows one tile per Place with its thumbnail and city name as an accessible button", async () => {
    renderAuthenticatedPage({ get_places: async () => places });
    await openExplore();

    const section = await screen.findByRole("region", { name: "Places" });
    expect(await within(section).findByRole("button", { name: "Paris" })).toBeInTheDocument();
    expect(within(section).getByRole("button", { name: "Berlin" })).toBeInTheDocument();
    expect(within(section).getByRole("img", { name: "Paris" })).toHaveAttribute("src", "lymic-thumb://asset/asset-1");
    expect(within(section).getByText("Berlin")).toBeVisible();
    expect(tauri.invoke).toHaveBeenCalledWith("get_places");
  });

  it("hides the Places section when there are no Places", async () => {
    renderAuthenticatedPage({ get_places: async () => [], get_people: async () => ({ people: [{ id: "p", name: "Ada", thumbnailPath: null }], hasNextPage: false }) });
    await openExplore();

    await screen.findByRole("region", { name: "People" });
    expect(screen.queryByRole("region", { name: "Places" })).not.toBeInTheDocument();
  });

  it("opens All Places from View all with every Place and returns to Explore with Back", async () => {
    renderAuthenticatedPage({ get_places: async () => places });
    await openExplore();
    const section = await screen.findByRole("region", { name: "Places" });

    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));

    expect(await screen.findByRole("heading", { name: "All Places", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Paris" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Berlin" })).toBeInTheDocument();
    const navigation = within(screen.getByRole("navigation", { name: "Main navigation" }));
    expect(navigation.queryByRole("button", { name: /places/i })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Places" })).toBeInTheDocument();
  });

  it("shows every Place in All Places while the Explore row shows only as many as fit", async () => {
    vi.stubGlobal("ResizeObserver", class {
      constructor(private readonly callback: ResizeObserverCallback) {}
      observe(target: Element) {
        this.callback([{ target, contentRect: { width: 500 } } as unknown as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      disconnect() {}
    });
    const many = Array.from({ length: 8 }, (_, index) => ({ city: `City ${index}`, thumbnailAssetId: `a-${index}` }));
    renderAuthenticatedPage({ get_places: async () => many });
    await openExplore();

    const section = await screen.findByRole("region", { name: "Places" });
    await waitFor(() => expect(within(section).getAllByRole("button", { name: /^City / })).toHaveLength(3));

    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^City / })).toHaveLength(8));
  });

  it("reports a Places failure in its own section only and retries it", async () => {
    renderAuthenticatedPage({
      get_places: vi.fn().mockRejectedValueOnce(new Error("boom")).mockResolvedValueOnce(places),
      get_people: async () => ({ people: [{ id: "p", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
    });
    await openExplore();

    const section = await screen.findByRole("region", { name: "Places" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("Places could not be loaded.");
    expect(within(await screen.findByRole("region", { name: "People" })).queryByRole("alert")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Ada" })).toBeInTheDocument();

    await fireEvent.click(within(section).getByRole("button", { name: "Retry" }));
    expect(await within(section).findByRole("button", { name: "Paris" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("names the asset.read permission when Places is permission-denied", async () => {
    renderAuthenticatedPage({ get_places: async () => { throw "places_permission_denied:configure an API key with asset.read permission"; } });
    await openExplore();

    const section = await screen.findByRole("region", { name: "Places" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("asset.read");
    expect(within(section).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("keeps stale Places visible with an offline indication when a refresh fails", async () => {
    renderAuthenticatedPage({
      get_places: vi.fn().mockResolvedValueOnce(places).mockRejectedValueOnce(new Error("offline")),
    });
    await openExplore();
    await screen.findByRole("button", { name: "Paris" });

    await fireEvent.click(screen.getAllByRole("button", { name: "Photos" })[0]);
    await fireEvent.click(screen.getAllByRole("button", { name: "Explore" })[0]);

    const section = await screen.findByRole("region", { name: "Places" });
    expect(await within(section).findByRole("status")).toHaveTextContent("Places is offline");
    expect(within(section).getByRole("button", { name: "Paris" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("refreshes Places after Synchronization settles while Explore is open", async () => {
    renderAuthenticatedPage({ get_places: async () => places });
    const placeCalls = () => tauri.invoke.mock.calls.filter(([command]) => command === "get_places").length;
    await openExplore();
    await screen.findByRole("button", { name: "Paris" });
    expect(placeCalls()).toBe(1);

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(placeCalls()).toBe(2));
  });
});

describe("Place Timelines", () => {
  const places = [
    { city: "Paris", thumbnailAssetId: "asset-1" },
    { city: "Berlin", thumbnailAssetId: "asset-2" },
  ];
  const parisTimeline = {
    buckets: [{ timeBucket: "2025-03-01T00:00:00.000Z", count: 1 }],
    assetsByBucket: {
      "2025-03-01T00:00:00.000Z": [{ id: "asset-1", width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false }],
    },
  };
  const openExplore = async () => fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

  it("opens a Place from Explore as a Place Timeline titled with the city and returns to Explore", async () => {
    renderAuthenticatedPage({ get_places: async () => places, get_place_timeline: async () => parisTimeline });
    await openExplore();
    const section = await screen.findByRole("region", { name: "Places" });

    await fireEvent.click(await within(section).findByRole("button", { name: "Paris" }));

    expect(await screen.findByRole("heading", { name: "Paris", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_place_timeline", { city: "Paris" });
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Places" })).toBeInTheDocument();
  });

  it("opens a Place from All Places and returns to All Places with Back", async () => {
    renderAuthenticatedPage({ get_places: async () => places, get_place_timeline: async () => parisTimeline });
    await openExplore();
    const section = await screen.findByRole("region", { name: "Places" });
    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Berlin" }));

    expect(await screen.findByRole("heading", { name: "Berlin", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_place_timeline", { city: "Berlin" });

    await fireEvent.click(screen.getByRole("button", { name: "Back to All Places" }));
    expect(await screen.findByRole("heading", { name: "All Places", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Paris" })).toBeInTheDocument();
  });

  it("shows a loading error instead of an empty or partial Place Timeline when the load fails", async () => {
    renderAuthenticatedPage({
      get_places: async () => places,
      get_place_timeline: async () => { throw new Error("page 2 failed"); },
    });
    await openExplore();
    await fireEvent.click(await screen.findByRole("button", { name: "Paris" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("This place could not be loaded.");
    expect(screen.queryByText("This place has no reachable photos yet.")).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Timeline scrubber" })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
  });

  it("explains an empty Place Timeline", async () => {
    renderAuthenticatedPage({
      get_places: async () => places,
      get_place_timeline: async () => ({ buckets: [], assetsByBucket: {} }),
    });
    await openExplore();
    await fireEvent.click(await screen.findByRole("button", { name: "Paris" }));

    expect(await screen.findByText("This place has no reachable photos yet.")).toBeInTheDocument();
  });
});

describe("Tag Timelines", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const tags = [
    { id: "t-travel", name: "Travel", value: "Travel", parentId: null, color: null },
    { id: "t-italy", name: "Italy", value: "Travel/Italy", parentId: "t-travel", color: null },
    { id: "t-rome", name: "Rome", value: "Travel/Italy/Rome", parentId: "t-italy", color: null },
    { id: "t-family", name: "Family", value: "Family", parentId: null, color: null },
  ];
  const taggedAsset = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false });
  const handlers = {
    get_tags: async () => tags,
    get_tag_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
    get_tag_timeline_bucket_assets: async () => [taggedAsset("asset-1")],
  };
  const openTags = async () => fireEvent.click(await screen.findByRole("button", { name: "Tags" }));

  it("opens the Tag Timeline of a selected Tag, titled with its full path, and loads buckets before assets", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({ ...handlers, get_tag_timeline_bucket_assets: () => new Promise(() => {}) });
    await openTags();
    await fireEvent.click(within(await screen.findByRole("treeitem", { name: "Italy" })).getByTestId("tag-toggle"));

    await fireEvent.click(await screen.findByRole("treeitem", { name: "Rome" }));

    expect(await screen.findByRole("heading", { name: "Travel/Italy/Rome", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_buckets", { tagId: "t-rome" });
    // The scrubber and month labels appear while the first bucket's assets are still loading.
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_bucket_assets", { tagId: "t-rome", timeBucket: march }));
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  });

  it("opens a Tag with Enter or Space, and expanding a Tag does not open it", async () => {
    renderAuthenticatedPage(handlers);
    await openTags();
    const travel = await screen.findByRole("treeitem", { name: "Travel" });

    await fireEvent.click(within(travel).getByTestId("tag-toggle"));
    expect(screen.getByRole("tree")).toBeInTheDocument();
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_tag_timeline_buckets", expect.anything());

    await fireEvent.keyDown(travel, { key: "Enter" });
    expect(await screen.findByRole("heading", { name: "Travel", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_buckets", { tagId: "t-travel" });

    await fireEvent.click(screen.getByRole("button", { name: "Back to Tags" }));
    await fireEvent.keyDown(await screen.findByRole("treeitem", { name: "Family" }), { key: " " });
    expect(await screen.findByRole("heading", { name: "Family", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_buckets", { tagId: "t-family" });
  });

  it("shows the shared timeline empty state for a Tag without assets", async () => {
    renderAuthenticatedPage({ ...handlers, get_tag_timeline_buckets: async () => [] });
    await openTags();
    await fireEvent.click(await screen.findByRole("treeitem", { name: "Family" }));

    expect(await screen.findByText("This tag has no photos yet.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows the shared timeline load error, never an empty Tag, when the load fails", async () => {
    renderAuthenticatedPage({ ...handlers, get_tag_timeline_buckets: async () => { throw new Error("tags_permission_denied:tag.read"); } });
    await openTags();
    await fireEvent.click(await screen.findByRole("treeitem", { name: "Family" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("This tag could not be loaded.");
    expect(screen.queryByText("This tag has no photos yet.")).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Timeline scrubber" })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Tags" }));
    expect(await screen.findByRole("tree", { name: "Tags" })).toBeInTheDocument();
  });

  it("offers Favorite, Download and Add to album from the Lightbox of a Tag Timeline", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      ...handlers,
      get_albums: async () => [{ id: "album-1", albumName: "Holiday" }],
      get_asset_details: async () => ({ id: "asset-1", originalFileName: "IMG_0001.jpg", fileSize: 1024, width: 640, height: 480, isFavorite: false }),
      set_asset_favorite: async () => undefined,
    });
    await openTags();
    await fireEvent.click(await screen.findByRole("treeitem", { name: "Family" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Open photo" }));
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });

    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to album" })).toBeInTheDocument();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Add to favorites" }));
    expect(tauri.invoke).toHaveBeenCalledWith("set_asset_favorite", { assetId: "asset-1", isFavorite: true });
  });

  it("returns to the Tags tree with its scroll position restored", async () => {
    renderAuthenticatedPage(handlers);
    await openTags();
    await screen.findByRole("treeitem", { name: "Family" });
    const main = document.querySelector("main") as HTMLElement;
    main.scrollTop = 180;
    await fireEvent.scroll(main);

    await fireEvent.click(screen.getByRole("treeitem", { name: "Family" }));
    await screen.findByRole("heading", { name: "Family", level: 1 });
    main.scrollTop = 0;
    await fireEvent.click(screen.getByRole("button", { name: "Back to Tags" }));

    expect(await screen.findByRole("heading", { name: "Tags", level: 1 })).toBeInTheDocument();
    await waitFor(() => expect(main.scrollTop).toBe(180));
    expect(screen.getByRole("treeitem", { name: "Family" })).toBeInTheDocument();
  });

  it("starts at the top of the Tags tree again after signing out from a Tag Timeline", async () => {
    renderAuthenticatedPage(handlers);
    await openTags();
    await screen.findByRole("treeitem", { name: "Family" });
    const main = document.querySelector("main") as HTMLElement;
    main.scrollTop = 180;
    await fireEvent.scroll(main);
    await fireEvent.click(screen.getByRole("treeitem", { name: "Family" }));
    await screen.findByRole("heading", { name: "Family", level: 1 });

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    await fireEvent.input(screen.getByLabelText("API Key"), { target: { value: "secret-key" } });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));

    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to Tags" })).not.toBeInTheDocument();
    await openTags();
    expect(await screen.findByRole("tree", { name: "Tags" })).toBeInTheDocument();
    expect(document.querySelector("main")!.scrollTop).toBe(0);
  });
});

describe("Explore Memories", () => {
  const asset = (id: string, takenAt: string) => ({ id, width: 1600, height: 1200, type: "IMAGE", takenAt, cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false });
  const timelineOf = (...assets: ReturnType<typeof asset>[]) => {
    const assetsByBucket: Record<string, ReturnType<typeof asset>[]> = {};
    for (const item of assets) (assetsByBucket[`${item.takenAt.slice(0, 7)}-01T00:00:00.000Z`] ??= []).push(item);
    return { buckets: Object.entries(assetsByBucket).map(([timeBucket, items]) => ({ timeBucket, count: items.length })), assetsByBucket };
  };
  const today = new Date();
  const anniversary = new Date(Date.UTC(today.getFullYear() - 3, today.getMonth(), today.getDate())).toISOString();
  const memories = [
    { id: "m-today", kind: "onThisDay", memoryAt: anniversary, year: today.getFullYear() - 3, personName: null, coverAssetId: "cover-1", timeline: timelineOf(asset("cover-1", anniversary), asset("other-1", "2020-02-10T10:00:00.000Z")) },
    { id: "m-other", kind: "onThisDay", memoryAt: "2019-01-02T00:00:00.000Z", year: 2019, personName: null, coverAssetId: "cover-2", timeline: timelineOf(asset("cover-2", "2019-01-02T10:00:00.000Z")) },
    { id: "m-birthday", kind: "birthday", memoryAt: "2018-05-06T00:00:00.000Z", year: 2018, personName: "Ada", coverAssetId: "cover-3", timeline: timelineOf(asset("cover-3", "2018-05-06T10:00:00.000Z")) },
    { id: "m-unknown", kind: "unknown", memoryAt: "2017-07-08T00:00:00.000Z", year: null, personName: null, coverAssetId: "cover-4", timeline: timelineOf(asset("cover-4", "2017-07-08T10:00:00.000Z")) },
  ];
  const memoryAssetDetails = async () => ({
    originalFileName: "memory.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  });

  function stubImmediateObservers() {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    vi.stubGlobal("IntersectionObserver", class {
      constructor(private readonly callback: IntersectionObserverCallback) {}
      observe(target: Element) { this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as unknown as IntersectionObserver); }
      disconnect() {}
    });
  }

  it("shows Memory cards with cover thumbnails and titles for each Memory type", async () => {
    renderAuthenticatedPage({ get_memories: async () => memories });

    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    const section = await screen.findByRole("region", { name: "Memories" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_memories", { size: 12 });
    const cards = within(section).getAllByRole("button").filter((button) => button.textContent?.trim() !== "View all");
    expect(cards.map((card) => card.getAttribute("aria-label"))).toEqual(["3 years ago", "Jan 2, 2019", "Ada's birthday", "Jul 8, 2017"]);
    expect(cards[0].querySelector("img")).toHaveAttribute("src", "lymic-thumb://asset/cover-1");
  });

  it("opens a Memory Timeline containing exactly that Memory's assets and returns to Explore", async () => {
    stubImmediateObservers();
    renderAuthenticatedPage({ get_memories: async () => memories, get_asset_details: memoryAssetDetails });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Jan 2, 2019" }));

    expect(await screen.findByRole("heading", { name: "Jan 2, 2019", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("January 2019")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
    expect(screen.queryByText("February 2020")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Open photo" }));
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    expect(within(lightbox).getByRole("img", { name: "Photo preview" })).toHaveAttribute("src", "lymic-asset://asset/cover-2");
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Memories" })).toBeInTheDocument();
  });

  it("shows a permission failure naming memory.read in the Memories section only and retries it", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
      get_memories: vi.fn()
        .mockRejectedValueOnce("memories_permission_denied:configure an API key with memory.read and asset.read permissions")
        .mockResolvedValueOnce(memories),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    const section = await screen.findByRole("region", { name: "Memories" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("memory.read");
    expect(within(await screen.findByRole("region", { name: "People" })).queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ada" })).toBeInTheDocument();

    await fireEvent.click(within(section).getByRole("button", { name: "Retry" }));
    expect(await within(section).findByRole("button", { name: "3 years ago" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows a generic Memories error for other failures", async () => {
    renderAuthenticatedPage({ get_memories: async () => { throw "memories_invalid_response"; } });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    expect(await within(await screen.findByRole("region", { name: "Memories" })).findByRole("alert")).toHaveTextContent("Memories could not be loaded.");
  });

  it("shows the unavailable message in the Memories section when authentication is required", async () => {
    renderAuthenticatedPage({ get_memories: async () => { throw "memories_authentication_required"; } });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    expect(await within(await screen.findByRole("region", { name: "Memories" })).findByRole("alert")).toHaveTextContent("Memories is unavailable. Check your connection and sign-in.");
  });

  it("keeps cached Memories visible with an offline indication when a refresh fails", async () => {
    renderAuthenticatedPage({
      get_memories: vi.fn().mockResolvedValueOnce(memories).mockRejectedValueOnce(new Error("offline")),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
    await screen.findByRole("button", { name: "3 years ago" });

    await fireEvent.click(screen.getAllByRole("button", { name: "Photos" })[0]);
    await fireEvent.click(screen.getAllByRole("button", { name: "Explore" })[0]);

    const section = await screen.findByRole("region", { name: "Memories" });
    expect(await within(section).findByRole("status")).toHaveTextContent("Memories is offline");
    expect(within(section).getByRole("button", { name: "3 years ago" })).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("hides an empty Memories section", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
      get_memories: async () => [],
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

    await screen.findByRole("region", { name: "People" });
    expect(screen.queryByRole("region", { name: "Memories" })).not.toBeInTheDocument();
  });

  describe("All Memories", () => {
    const [today, other, birthday, unknown] = memories;
    const pageCalls = () => tauri.invoke.mock.calls.filter(([command]) => command === "get_memories_page").map(([, args]) => args);
    const cardLabels = () => screen.getAllByRole("button").map((button) => button.getAttribute("aria-label")).filter((label) => label && /ago|, 20|birthday/.test(label));

    /** Lets a test decide when the scroll sentinel becomes visible instead of firing on observe. */
    function stubScrollObservers() {
      const active = new Set<{ callback: IntersectionObserverCallback; target: Element }>();
      vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
      vi.stubGlobal("IntersectionObserver", class {
        private entry?: { callback: IntersectionObserverCallback; target: Element };
        constructor(private readonly callback: IntersectionObserverCallback) {}
        observe(target: Element) { this.entry = { callback: this.callback, target }; active.add(this.entry); }
        disconnect() { if (this.entry) active.delete(this.entry); }
      });
      return {
        scrollToEnd: async () => {
          for (const { callback, target } of [...active]) callback([{ isIntersecting: true, target } as IntersectionObserverEntry], {} as IntersectionObserver);
          await Promise.resolve();
        },
        observed: () => active.size,
      };
    }

    async function openAllMemories() {
      await fireEvent.click(await screen.findByRole("button", { name: "Explore" }));
      const section = await screen.findByRole("region", { name: "Memories" });
      await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    }

    it("opens All Memories from View all and loads further pages while scrolling until none are left", async () => {
      const scroll = stubScrollObservers();
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: async (args) => (args as { page: number }).page === 1
          ? { memories: [today, other], hasNextPage: true }
          : { memories: [birthday, unknown], hasNextPage: false },
      });

      await openAllMemories();

      expect(await screen.findByRole("heading", { name: "All Memories", level: 1 })).toBeInTheDocument();
      await waitFor(() => expect(cardLabels()).toEqual(["3 years ago", "Jan 2, 2019"]));
      expect(pageCalls()).toEqual([{ page: 1, size: 24 }]);

      await scroll.scrollToEnd();
      await waitFor(() => expect(cardLabels()).toEqual(["3 years ago", "Jan 2, 2019", "Ada's birthday", "Jul 8, 2017"]));
      expect(pageCalls()).toEqual([{ page: 1, size: 24 }, { page: 2, size: 24 }]);

      await waitFor(() => expect(scroll.observed()).toBe(0));
      await scroll.scrollToEnd();
      expect(pageCalls()).toHaveLength(2);
    });

    it("keeps the pages already shown when a further page fails and offers retry", async () => {
      const scroll = stubScrollObservers();
      const secondPage = vi.fn().mockRejectedValueOnce("memories_unavailable").mockResolvedValueOnce({ memories: [birthday], hasNextPage: false });
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: async (args) => (args as { page: number }).page === 1 ? { memories: [today, other], hasNextPage: true } : secondPage(),
      });
      await openAllMemories();
      await waitFor(() => expect(cardLabels()).toHaveLength(2));

      await scroll.scrollToEnd();

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("More memories could not be loaded.");
      expect(cardLabels()).toEqual(["3 years ago", "Jan 2, 2019"]);
      await scroll.scrollToEnd();
      expect(secondPage).toHaveBeenCalledOnce();

      await fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));

      await waitFor(() => expect(cardLabels()).toEqual(["3 years ago", "Jan 2, 2019", "Ada's birthday"]));
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(pageCalls()).toEqual([{ page: 1, size: 24 }, { page: 2, size: 24 }, { page: 2, size: 24 }]);
    });

    it("reports a failed first page with retry and names memory.read when permission is denied", async () => {
      stubScrollObservers();
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: vi.fn()
          .mockRejectedValueOnce("memories_permission_denied:configure an API key with memory.read and asset.read permissions")
          .mockResolvedValueOnce({ memories: [today], hasNextPage: false }),
      });
      await openAllMemories();

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("memory.read");

      await fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));
      expect(await screen.findByRole("button", { name: "3 years ago" })).toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });

    it("shows the unavailable message in All Memories when authentication is required", async () => {
      stubScrollObservers();
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: async () => { throw "memories_authentication_required"; },
      });
      await openAllMemories();

      expect(await screen.findByRole("alert")).toHaveTextContent("Memories is unavailable. Check your connection and sign-in.");
    });

    it("returns to All Memories from a Memory Timeline and to the Explore Overview from All Memories", async () => {
      stubScrollObservers();
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: async () => ({ memories: [today, other], hasNextPage: false }),
      });
      await openAllMemories();

      await fireEvent.click(await screen.findByRole("button", { name: "Jan 2, 2019" }));
      expect(await screen.findByRole("heading", { name: "Jan 2, 2019", level: 1 })).toBeInTheDocument();

      await fireEvent.click(screen.getByRole("button", { name: "Back to All Memories" }));
      expect(await screen.findByRole("heading", { name: "All Memories", level: 1 })).toBeInTheDocument();
      expect(cardLabels()).toEqual(["3 years ago", "Jan 2, 2019"]);
      expect(pageCalls()).toHaveLength(1);

      await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
      expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
      expect(screen.getByRole("region", { name: "Memories" })).toBeInTheDocument();
    });

    it("still returns a Memory opened from the Explore Overview to the Explore Overview", async () => {
      stubScrollObservers();
      renderAuthenticatedPage({
        get_memories: async () => memories,
        get_memories_page: async () => ({ memories: [today, other], hasNextPage: false }),
      });
      await openAllMemories();
      await screen.findByRole("heading", { name: "All Memories", level: 1 });
      await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));

      await fireEvent.click(await screen.findByRole("button", { name: "Jan 2, 2019" }));
      await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));

      expect(await screen.findByRole("heading", { name: "Explore", level: 1 })).toBeInTheDocument();
    });

    it("has no navigation entry of its own", async () => {
      renderAuthenticatedPage({ get_memories: async () => memories });
      await screen.findByRole("button", { name: "Explore" });
      expect(screen.queryByRole("button", { name: "All Memories" })).not.toBeInTheDocument();
    });
  });
});

describe("Explore Overview across sections", () => {
  const people = [{ id: "person-1", name: "Ada", thumbnailPath: null }];
  const places = [{ city: "Paris", thumbnailAssetId: "asset-1" }];
  const now = new Date();
  const memory = { id: "m-1", kind: "onThisDay", memoryAt: new Date(Date.UTC(now.getFullYear() - 3, now.getMonth(), now.getDate())).toISOString(), year: now.getFullYear() - 3, personName: null, coverAssetId: "cover-1", timeline: { buckets: [], assetsByBucket: {} } };
  const calls = (command: string) => tauri.invoke.mock.calls.filter(([name]) => name === command).length;
  const openExplore = async () => fireEvent.click(await screen.findByRole("button", { name: "Explore" }));

  it("shows the placeholder instead of sections when People, Places and Memories are all empty", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [], hasNextPage: false }),
      get_places: async () => [],
      get_memories: async () => [],
    });
    await openExplore();

    expect(await screen.findByText(/Nothing to explore yet/)).toBeInTheDocument();
    for (const name of ["People", "Places", "Memories"]) expect(screen.queryByRole("region", { name })).not.toBeInTheDocument();
  });

  it("hides only the empty People section and shows no placeholder while Places and Memories have data", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [], hasNextPage: false }),
      get_places: async () => places,
      get_memories: async () => [memory],
    });
    await openExplore();

    expect(await screen.findByRole("region", { name: "Places" })).toBeInTheDocument();
    expect(await screen.findByRole("region", { name: "Memories" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "People" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Nothing to explore yet/)).not.toBeInTheDocument();
  });

  it("names the person.read permission when People is permission denied", async () => {
    renderAuthenticatedPage({ get_people: async () => { throw "recognized_people_permission_denied:people:configure an API key with person.read and asset.read permissions"; } });
    await openExplore();

    const section = await screen.findByRole("region", { name: "People" });
    expect(await within(section).findByRole("alert")).toHaveTextContent("person.read");
  });

  it("refreshes Explore when returning to it after Synchronization settled in All Places", async () => {
    renderAuthenticatedPage({ get_people: async () => ({ people, hasNextPage: false }), get_places: async () => places });
    await openExplore();
    const section = await screen.findByRole("region", { name: "Places" });
    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    await screen.findByRole("heading", { name: "All Places", level: 1 });
    expect(calls("get_places")).toBe(1);

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(calls("get_timeline_buckets")).toBe(2));
    expect(calls("get_places")).toBe(1);

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    await waitFor(() => expect(calls("get_places")).toBe(2));
    expect(calls("get_people")).toBe(2);
  });

  it("refreshes Explore when returning to it from a Memory Timeline opened there after Synchronization settled", async () => {
    renderAuthenticatedPage({ get_places: async () => places, get_memories: async () => [memory] });
    await openExplore();
    await fireEvent.click(await screen.findByRole("button", { name: "3 years ago" }));
    await screen.findByRole("button", { name: "Back to Explore" });

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(calls("get_timeline_buckets")).toBe(2));
    expect(calls("get_memories")).toBe(1);

    await fireEvent.click(screen.getByRole("button", { name: "Back to Explore" }));
    await waitFor(() => expect(calls("get_memories")).toBe(2));
  });

  it("does not refresh again when returning to Explore if nothing changed meanwhile", async () => {
    renderAuthenticatedPage({ get_places: async () => places });
    await openExplore();
    const section = await screen.findByRole("region", { name: "Places" });
    await fireEvent.click(within(section).getByRole("button", { name: "View all" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));
    await screen.findByRole("region", { name: "Places" });

    expect(calls("get_places")).toBe(1);
  });
});

describe("Tags view", () => {
  const tags = [
    { id: "t-travel", name: "Travel", value: "Travel", parentId: null, color: "#ff0000" },
    { id: "t-italy", name: "Italy", value: "Travel/Italy", parentId: "t-travel", color: null },
    { id: "t-rome", name: "Rome", value: "Travel/Italy/Rome", parentId: "t-italy", color: "#00FF00" },
    { id: "t-family", name: "Family", value: "Family", parentId: null, color: "not-a-colour" },
  ];
  const calls = (command: string) => tauri.invoke.mock.calls.filter(([name]) => name === command).length;
  const openTags = async () => fireEvent.click(await screen.findByRole("button", { name: "Tags" }));
  const names = () => screen.queryAllByRole("treeitem").map((item) => item.textContent?.trim());
  /** The chevron of a Tag with children; expanding is separate from selecting the Tag. */
  const tagToggle = async (name: string) => within(await screen.findByRole("treeitem", { name })).getByTestId("tag-toggle");

  it("opens from the navigation as a tree while Photos stays the view opened after login", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();

    await openTags();

    expect(await screen.findByRole("heading", { name: "Tags", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("tree", { name: "Tags" })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tags");
  });

  it("follows the hierarchy with siblings sorted by name, the top level expanded and deeper levels collapsed", async () => {
    renderAuthenticatedPage({ get_tags: async () => [...tags].reverse() });
    await openTags();

    await screen.findByRole("treeitem", { name: "Travel" });
    expect(names()).toEqual(["Family", "Travel", "Italy"]);
    expect(screen.getByRole("treeitem", { name: "Travel" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("treeitem", { name: "Italy" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("treeitem", { name: "Family" })).not.toHaveAttribute("aria-expanded");
    expect(screen.getByRole("treeitem", { name: "Family" })).toHaveAttribute("aria-level", "1");
    expect(screen.getByRole("treeitem", { name: "Italy" })).toHaveAttribute("aria-level", "2");
    expect(screen.getByRole("treeitem", { name: "Travel" })).toHaveAttribute("aria-posinset", "2");
    expect(screen.getByRole("treeitem", { name: "Travel" })).toHaveAttribute("aria-setsize", "2");
  });

  it("shows each Tag's own name only, never its full path", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Italy" });
    await fireEvent.click(await tagToggle("Italy"));

    expect(await screen.findByRole("treeitem", { name: "Rome" })).toBeInTheDocument();
    expect(screen.queryByText("Travel/Italy/Rome")).not.toBeInTheDocument();
    expect(screen.queryByText("Travel/Italy")).not.toBeInTheDocument();
  });

  it("expands and collapses a Tag with children by click and keeps the choice while moving between views", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await fireEvent.click(await tagToggle("Italy"));
    expect(screen.getByRole("treeitem", { name: "Italy" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("treeitem", { name: "Rome" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await openTags();
    expect(await screen.findByRole("treeitem", { name: "Rome" })).toBeInTheDocument();

    await fireEvent.click(await tagToggle("Travel"));
    expect(screen.getByRole("treeitem", { name: "Travel" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("treeitem", { name: "Italy" })).not.toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: "Rome" })).not.toBeInTheDocument();
  });

  it("shows a colour dot only for a valid #RRGGBB colour", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await fireEvent.click(await tagToggle("Italy"));
    await screen.findByRole("treeitem", { name: "Rome" });

    expect(within(screen.getByRole("treeitem", { name: "Travel" })).getByTestId("tag-color-dot")).toHaveStyle({ backgroundColor: "#ff0000" });
    expect(within(screen.getByRole("treeitem", { name: "Rome" })).getByTestId("tag-color-dot")).toHaveStyle({ backgroundColor: "#00ff00" });
    expect(within(screen.getByRole("treeitem", { name: "Italy" })).queryByTestId("tag-color-dot")).not.toBeInTheDocument();
    expect(within(screen.getByRole("treeitem", { name: "Family" })).queryByTestId("tag-color-dot")).not.toBeInTheDocument();
  });

  it("rejects colours that are not exactly #RRGGBB", async () => {
    const odd = ["#fff", "#12345", "#1234567", "red", "#GGGGGG", "ff0000", "url(x)", ""].map((color, index) => ({ id: `c-${index}`, name: `Odd ${index}`, value: `Odd ${index}`, parentId: null, color }));
    renderAuthenticatedPage({ get_tags: async () => odd });
    await openTags();

    await screen.findByRole("treeitem", { name: "Odd 0" });
    expect(screen.queryAllByTestId("tag-color-dot")).toHaveLength(0);
  });

  it("puts a Tag whose parent is missing from the list at the top level", async () => {
    renderAuthenticatedPage({ get_tags: async () => [
      { id: "a", name: "Zebra", value: "Gone/Zebra", parentId: "gone", color: null },
      { id: "b", name: "Apple", value: "Apple", parentId: null, color: null },
    ] });
    await openTags();

    await screen.findByRole("treeitem", { name: "Zebra" });
    expect(names()).toEqual(["Apple", "Zebra"]);
    expect(screen.getByRole("treeitem", { name: "Zebra" })).toHaveAttribute("aria-level", "1");
  });

  it("neither hangs nor drops Tags on a self-reference or a cycle", async () => {
    renderAuthenticatedPage({ get_tags: async () => [
      { id: "self", name: "Self", value: "Self", parentId: "self", color: null },
      { id: "a", name: "A", value: "A", parentId: "b", color: null },
      { id: "b", name: "B", value: "B", parentId: "a", color: null },
      { id: "c", name: "C", value: "C", parentId: "a", color: null },
    ] });
    await openTags();

    await screen.findByRole("treeitem", { name: "Self" });
    // Every Tag is reachable: expand each node that has children until nothing new appears.
    for (let round = 0; round < 4; round += 1) {
      for (const item of screen.queryAllByRole("treeitem", { expanded: false })) await fireEvent.click(within(item).getByTestId("tag-toggle"));
    }
    expect(screen.getAllByRole("treeitem").map((item) => item.textContent?.trim()).sort()).toEqual(["A", "B", "C", "Self"]);
  });

  it("is operable with the keyboard through roving focus", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    const family = await screen.findByRole("treeitem", { name: "Family" });
    expect(family).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("treeitem", { name: "Travel" })).toHaveAttribute("tabindex", "-1");

    family.focus();
    await fireEvent.keyDown(family, { key: "ArrowDown" });
    const travel = screen.getByRole("treeitem", { name: "Travel" });
    expect(travel).toHaveFocus();
    expect(travel).toHaveAttribute("tabindex", "0");

    await fireEvent.keyDown(travel, { key: "ArrowDown" });
    const italy = screen.getByRole("treeitem", { name: "Italy" });
    expect(italy).toHaveFocus();

    await fireEvent.keyDown(italy, { key: "ArrowRight" });
    expect(italy).toHaveAttribute("aria-expanded", "true");
    expect(italy).toHaveFocus();
    await fireEvent.keyDown(italy, { key: "ArrowRight" });
    expect(screen.getByRole("treeitem", { name: "Rome" })).toHaveFocus();

    await fireEvent.keyDown(screen.getByRole("treeitem", { name: "Rome" }), { key: "ArrowLeft" });
    expect(italy).toHaveFocus();
    await fireEvent.keyDown(italy, { key: "ArrowLeft" });
    expect(italy).toHaveAttribute("aria-expanded", "false");
    await fireEvent.keyDown(italy, { key: "ArrowLeft" });
    expect(travel).toHaveFocus();

    await fireEvent.keyDown(travel, { key: "End" });
    expect(screen.getByRole("treeitem", { name: "Italy" })).toHaveFocus();
    await fireEvent.keyDown(screen.getByRole("treeitem", { name: "Italy" }), { key: "Home" });
    expect(screen.getByRole("treeitem", { name: "Family" })).toHaveFocus();
  });

  it("shows an informative empty state for a Library without Tags", async () => {
    renderAuthenticatedPage({ get_tags: async () => [] });
    await openTags();

    expect(await screen.findByText("No tags yet. Tags created in Immich appear here.")).toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  });

  it("shows a loading state while the Tags load", async () => {
    renderAuthenticatedPage({ get_tags: () => new Promise(() => {}) });
    await openTags();

    expect(await screen.findByRole("status")).toHaveTextContent("Loading tags…");
  });

  it("reloads the Tags every time the view opens", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    expect(calls("get_tags")).toBe(1);

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await openTags();
    await waitFor(() => expect(calls("get_tags")).toBe(2));
  });

  it("names the tag.read permission and offers Retry when the key lacks it", async () => {
    renderAuthenticatedPage({ get_tags: vi.fn().mockRejectedValueOnce("tags_permission_denied:tag.read").mockResolvedValueOnce(tags) });
    await openTags();

    expect(await screen.findByRole("alert")).toHaveTextContent("tag.read");
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports authentication-required and unavailable as unavailable and anything else as an error", async () => {
    const get_tags = vi.fn()
      .mockRejectedValueOnce("tags_authentication_required")
      .mockRejectedValueOnce("tags_unavailable")
      .mockRejectedValueOnce("tags_invalid_response");
    renderAuthenticatedPage({ get_tags });
    await openTags();

    expect(await screen.findByRole("alert")).toHaveTextContent("Tags is unavailable. Check your connection and sign-in.");
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(calls("get_tags")).toBe(2));
    expect(await screen.findByRole("alert")).toHaveTextContent("Tags is unavailable. Check your connection and sign-in.");
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Tags could not be loaded."));
  });

  it("keeps an already loaded tree visible with an offline note when a reload fails", async () => {
    renderAuthenticatedPage({ get_tags: vi.fn().mockResolvedValueOnce(tags).mockRejectedValueOnce("tags_unavailable") });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await openTags();

    expect(await screen.findByRole("status")).toHaveTextContent("Tags is offline. Showing cached tags.");
    expect(screen.getByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("ignores a late response of an older request once a newer one has answered", async () => {
    let answerOlder!: (value: unknown) => void;
    const older = new Promise((resolve) => { answerOlder = resolve; });
    const get_tags = vi.fn().mockImplementationOnce(() => older).mockResolvedValueOnce(tags);
    renderAuthenticatedPage({ get_tags });
    await openTags();
    expect(await screen.findByRole("status")).toHaveTextContent("Loading tags…");
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });

    answerOlder([{ id: "t-old", name: "Outdated", value: "Outdated", parentId: null, color: null }]);
    await Promise.resolve();
    await tick();

    expect(screen.getByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: "Outdated" })).not.toBeInTheDocument();
  });

  it("ignores a late failure of an older request once a newer one has answered", async () => {
    let failOlder!: (reason: unknown) => void;
    const older = new Promise((_, reject) => { failOlder = reject; });
    const get_tags = vi.fn().mockImplementationOnce(() => older).mockResolvedValueOnce(tags);
    renderAuthenticatedPage({ get_tags });
    await openTags();
    await fireEvent.click(await screen.findByRole("button", { name: "Photos" }));
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });

    failOlder("tags_unavailable");
    await Promise.resolve();
    await tick();

    expect(screen.getByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText("Tags is offline. Showing cached tags.")).not.toBeInTheDocument();
  });

  it("keeps the loaded tree and the offline note when a Synchronization reload fails", async () => {
    renderAuthenticatedPage({ get_tags: vi.fn().mockResolvedValueOnce(tags).mockRejectedValueOnce("tags_unavailable") });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();

    expect(await screen.findByRole("status")).toHaveTextContent("Tags is offline. Showing cached tags.");
    expect(screen.getByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("recovers from a failed first load through Retry without a restart", async () => {
    const get_tags = vi.fn().mockRejectedValueOnce("tags_invalid_response").mockResolvedValueOnce(tags);
    renderAuthenticatedPage({ get_tags });
    await openTags();
    expect(await screen.findByRole("alert")).toHaveTextContent("Tags could not be loaded.");

    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("treeitem", { name: "Travel" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does not show a response that arrives after sign-out to the next account", async () => {
    let answer!: (value: unknown) => void;
    const slow = new Promise((resolve) => { answer = resolve; });
    const get_tags = vi.fn().mockImplementationOnce(() => slow).mockImplementationOnce(() => new Promise(() => {}));
    renderAuthenticatedPage({ get_tags });
    await openTags();
    await screen.findByRole("status");

    await signOutAndBackIn();
    answer(tags);
    await Promise.resolve();
    await tick();
    await openTags();

    expect(await screen.findByRole("status")).toHaveTextContent("Loading tags…");
    expect(screen.queryByRole("treeitem")).not.toBeInTheDocument();
  });

  it("keeps the rest of Lymic working when the key lacks tag.read", async () => {
    renderAuthenticatedPage({ get_tags: async () => { throw "tags_permission_denied:tag.read"; }, get_places: async () => [{ city: "Paris", thumbnailAssetId: "a" }] });
    await openTags();
    await screen.findByRole("alert");

    await fireEvent.click(screen.getByRole("button", { name: "Explore" }));
    expect(await screen.findByRole("button", { name: "Paris" })).toBeInTheDocument();
  });

  it("reloads after Synchronization settles while the Tags view is open", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    expect(calls("get_tags")).toBe(1);

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(calls("get_tags")).toBe(2));
  });

  it("only marks the Tags stale when Synchronization settles elsewhere and reloads once on return", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));

    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    await waitFor(() => expect(calls("get_timeline_buckets")).toBeGreaterThan(1));
    expect(calls("get_tags")).toBe(1);

    await openTags();
    await waitFor(() => expect(calls("get_tags")).toBe(2));
  });

  it("remembers its own scroll position when leaving and returning", async () => {
    renderAuthenticatedPage({ get_tags: async () => tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    const main = document.querySelector("main") as HTMLElement;
    main.scrollTop = 140;
    await fireEvent.scroll(main);

    await fireEvent.click(screen.getByRole("button", { name: "Explore" }));
    main.scrollTop = 0;
    await openTags();

    await waitFor(() => expect(main.scrollTop).toBe(140));
  });

  /** Gives `main` a real layout: its scroll height is what the rendered tree gives it, and a scroll position beyond that is clamped, as in a browser. */
  function layOut(main: HTMLElement) {
    let top = 0;
    Object.defineProperty(main, "clientHeight", { configurable: true, get: () => 100 });
    Object.defineProperty(main, "scrollHeight", { configurable: true, get: () => 100 + main.querySelectorAll('[role="treeitem"]').length * 100 });
    Object.defineProperty(main, "scrollTop", {
      configurable: true,
      get: () => top,
      set: (value: number) => { top = Math.max(0, Math.min(value, main.scrollHeight - main.clientHeight)); },
    });
  }

  it("restores its scroll position on reopening while the tree reloads after a Synchronization settled elsewhere", async () => {
    const get_tags = vi.fn().mockResolvedValueOnce(tags).mockImplementation(() => new Promise(() => {}));
    renderAuthenticatedPage({ get_tags });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    const main = document.querySelector("main") as HTMLElement;
    layOut(main);
    main.scrollTop = 140;
    await fireEvent.scroll(main);
    await fireEvent.click(screen.getByRole("button", { name: "Explore" }));
    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    main.scrollTop = 0;

    await openTags();

    await waitFor(() => expect(calls("get_tags")).toBeGreaterThanOrEqual(2));
    expect(screen.getAllByRole("treeitem").length).toBeGreaterThan(1);
    await waitFor(() => expect(main.scrollTop).toBe(140));
  });

  it("restores its scroll position on returning from a Tag Timeline while the tree reloads", async () => {
    const get_tags = vi.fn().mockResolvedValueOnce(tags).mockImplementation(() => new Promise(() => {}));
    renderAuthenticatedPage({ get_tags, get_tag_timeline_buckets: async () => [], get_tag_timeline_bucket_assets: async () => [] });
    await openTags();
    await screen.findByRole("treeitem", { name: "Travel" });
    const main = document.querySelector("main") as HTMLElement;
    layOut(main);
    main.scrollTop = 140;
    await fireEvent.scroll(main);
    await fireEvent.click(screen.getByRole("treeitem", { name: "Family" }));
    await screen.findByRole("heading", { name: "Family", level: 1 });
    await waitFor(() => expect(syncIdle).toBeTypeOf("function"));
    syncIdle!();
    main.scrollTop = 0;

    await fireEvent.click(screen.getByRole("button", { name: "Back to Tags" }));

    await waitFor(() => expect(calls("get_tags")).toBeGreaterThanOrEqual(2));
    await waitFor(() => expect(main.scrollTop).toBe(140));
  });

  async function signOutAndBackIn() {
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    await fireEvent.input(screen.getByLabelText("API Key"), { target: { value: "secret-key" } });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));
  }

  it("clears the Tags, the expansion state and the scroll position on sign-out", async () => {
    const get_tags = vi.fn().mockResolvedValueOnce(tags).mockImplementationOnce(() => new Promise(() => {}));
    renderAuthenticatedPage({ get_tags });
    await openTags();
    await fireEvent.click(await tagToggle("Italy"));
    await screen.findByRole("treeitem", { name: "Rome" });
    const main = document.querySelector("main") as HTMLElement;
    main.scrollTop = 140;
    await fireEvent.scroll(main);

    await signOutAndBackIn();
    await openTags();

    expect(await screen.findByRole("status")).toHaveTextContent("Loading tags…");
    expect(screen.queryByRole("treeitem")).not.toBeInTheDocument();
    expect(document.querySelector("main")!.scrollTop).toBe(0);
  });

  it("starts with the top level expanded again for the next account", async () => {
    const get_tags = vi.fn().mockResolvedValue(tags);
    renderAuthenticatedPage({ get_tags });
    await openTags();
    await fireEvent.click(await tagToggle("Italy"));
    await screen.findByRole("treeitem", { name: "Rome" });

    await signOutAndBackIn();
    await openTags();

    expect(await screen.findByRole("treeitem", { name: "Italy" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("treeitem", { name: "Rome" })).not.toBeInTheDocument();
  });
});

describe("Archive Timeline", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const december = "2024-12-01T00:00:00.000Z";
  const archivedAsset = (id: string, type: "IMAGE" | "VIDEO" = "IMAGE", isFavorite = false) => ({
    id, width: 1600, height: 1200, type, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: type === "VIDEO" ? 65 : null, isFavorite,
  });
  const assetDetails = {
    originalFileName: "archived.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };

  it("lists Archive directly after Tags and before Trash in the navigation with an accessible name and no count badge", async () => {
    renderAuthenticatedPage();

    const tags = await screen.findByRole("button", { name: "Tags" });
    const archive = screen.getByRole("button", { name: "Archive" });
    const trash = screen.getByRole("button", { name: "Trash" });
    expect(tags.nextElementSibling).toBe(archive);
    expect(archive.nextElementSibling).toBe(trash);
    expect(archive).toHaveTextContent("Archive");
    expect(archive).not.toHaveTextContent(/\d/);
  });

  it("opens the Archive Timeline with buckets, scrubber and thumbnails, and offers no archive or unarchive action", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:archive": async () => [{ timeBucket: march, count: 2 }, { timeBucket: december, count: 1 }],
      "get_filtered_timeline_bucket_assets:archive": async (args) => {
        const { timeBucket } = args as { timeBucket: string };
        return timeBucket === march ? [archivedAsset("arch-photo"), archivedAsset("arch-video", "VIDEO")] : [archivedAsset("arch-photo-2")];
      },
      get_albums: async () => [{ id: "album-1", albumName: "Holiday" }],
      get_asset_details: async () => assetDetails,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));

    expect(await screen.findByRole("heading", { name: "Archive" })).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("December 2024")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "archive" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "archive", timeBucket: march });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "archive", timeBucket: december });
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "favorites" });
    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /unarchive|move to archive/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to Explore" })).not.toBeInTheDocument();
  });

  it("offers Favorite, Download and Add to album from the Lightbox and from multi-selection, without any archive action", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:archive": async () => [{ timeBucket: march, count: 2 }],
      "get_filtered_timeline_bucket_assets:archive": async () => [archivedAsset("arch-1"), archivedAsset("arch-2")],
      get_albums: async () => [{ id: "album-1", albumName: "Holiday" }],
      get_asset_details: async () => assetDetails,
      set_asset_favorite: async () => undefined,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });

    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to album" })).toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: /archive/i })).not.toBeInTheDocument();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Add to favorites" }));
    expect(tauri.invoke).toHaveBeenCalledWith("set_asset_favorite", { assetId: "arch-1", isFavorite: true });
    // Favoriting keeps the asset in the Archive Timeline.
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /unarchive|move to archive|add to archive/i })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Add to album" }));
    expect(await screen.findByText("Holiday")).toBeInTheDocument();
  });

  it("refreshes on every entry from another view and does nothing when already open", async () => {
    const getArchiveBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }, { timeBucket: december, count: 1 }])
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByText("December 2024")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(getArchiveBuckets).toHaveBeenCalledTimes(1);

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => {
      expect(getArchiveBuckets).toHaveBeenCalledTimes(2);
      expect(screen.queryByText("December 2024")).not.toBeInTheDocument();
    });
  });

  it("shows loading, then the empty state when nothing is archived", async () => {
    let resolveBuckets!: (value: unknown) => void;
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": () => new Promise((resolve) => { resolveBuckets = resolve; }) });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading archive…");
    expect(screen.queryByText("Your archive is empty.")).not.toBeInTheDocument();

    resolveBuckets([]);
    expect(await screen.findByText("Your archive is empty.")).toHaveAttribute("role", "status");
    expect(screen.queryByText("Loading archive…")).not.toBeInTheDocument();
  });

  it("shows an error with a retry that reloads the archive", async () => {
    const getArchiveBuckets = vi.fn()
      .mockRejectedValueOnce(new Error("rejected visibility"))
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The archive could not be loaded.");
    expect(screen.queryByText("Your archive is empty.")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(getArchiveBuckets).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("names the missing asset.read permission when Immich denies access to the archive, and still offers retry", async () => {
    const getArchiveBuckets = vi.fn()
      .mockRejectedValueOnce("archive_permission_denied:asset.read")
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The archive could not be loaded. Give the API key the asset.read permission.");
    expect(screen.queryByText("Your archive is empty.")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(getArchiveBuckets).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps the generic archive error for failures that are not permission denials", async () => {
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": () => Promise.reject("archive_unavailable") });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The archive could not be loaded.");
  });

  it("keeps already shown archived assets visible and announces offline when a refresh fails", async () => {
    const getArchiveBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockRejectedValueOnce(new Error("offline"));
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Archive" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Archive is offline. Showing cached archive.");
    expect(screen.getByText("March 2025")).toBeInTheDocument();
  });

  it("shows a permission denial on a refresh as an alert with retry, not as offline, and keeps the shown assets", async () => {
    const getArchiveBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockRejectedValueOnce("archive_permission_denied:asset.read")
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Archive" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The archive could not be loaded. Give the API key the asset.read permission.");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(within(alert).getByRole("button", { name: "Retry timeline connection" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(getArchiveBuckets).toHaveBeenCalledTimes(3);
    expect(screen.getByText("March 2025")).toBeInTheDocument();
  });

  it("shows a permission denial while loading bucket assets as an alert, not as offline", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:archive": async () => [{ timeBucket: march, count: 1 }],
      "get_filtered_timeline_bucket_assets:archive": () => Promise.reject("archive_permission_denied:asset.read"),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Give the API key the asset.read permission.");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("March 2025")).toBeInTheDocument();
  });

  it("clears the Archive Timeline on logout so the next session does not inherit it", async () => {
    const getArchiveBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockImplementationOnce(() => new Promise(() => {}));
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:archive": getArchiveBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    await fireEvent.input(screen.getByLabelText("API Key"), { target: { value: "secret-key" } });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading archive…");
    expect(screen.queryByText("March 2025")).not.toBeInTheDocument();
  });
});


describe("Trash Timeline", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const december = "2024-12-01T00:00:00.000Z";
  const trashedAsset = (id: string, type: "IMAGE" | "VIDEO" = "IMAGE") => ({
    id, width: 1600, height: 1200, type, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: type === "VIDEO" ? 65 : null, isFavorite: false,
  });
  const assetDetails = {
    originalFileName: "trashed.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };
  const retentionHint = "Immich permanently removes items from the trash after the retention period configured on the server.";

  it("lists Trash directly after Archive in the navigation with an accessible name and no count badge", async () => {
    renderAuthenticatedPage();

    const archive = await screen.findByRole("button", { name: "Archive" });
    const trash = screen.getByRole("button", { name: "Trash" });
    expect(archive.nextElementSibling).toBe(trash);
    expect(trash).toHaveTextContent("Trash");
    expect(trash).not.toHaveTextContent(/\d/);
  });

  it("opens the Trash Timeline with buckets, scrubber, thumbnails and the retention hint, and offers no deletion action", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 2 }, { timeBucket: december, count: 1 }],
      "get_filtered_timeline_bucket_assets:trash": async (args) => {
        const { timeBucket } = args as { timeBucket: string };
        return timeBucket === march ? [trashedAsset("trash-photo"), trashedAsset("trash-video", "VIDEO")] : [trashedAsset("trash-photo-2")];
      },
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));

    expect(await screen.findByRole("heading", { name: "Trash" })).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByText("December 2024")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "trash" });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "trash", timeBucket: march });
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_bucket_assets", { timeline: "trash", timeBucket: december });
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "archive" });
    expect(tauri.invoke).not.toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "favorites" });
    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open video" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();
    expect(screen.getByText(retentionHint)).toBeInTheDocument();
    expect(screen.getByText(retentionHint).textContent).not.toMatch(/\d/);
    expect(screen.queryByRole("button", { name: /delete|empty trash|move to trash|restore all/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to Explore" })).not.toBeInTheDocument();
  });

  it("offers only Restore and Download from the Lightbox and from multi-selection, without Favorite, Add to album or any deletion", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 2 }],
      "get_filtered_timeline_bucket_assets:trash": async () => [trashedAsset("trash-1"), trashedAsset("trash-2")],
      get_albums: async () => [{ id: "album-1", albumName: "Holiday" }],
      get_asset_details: async () => assetDetails,
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });

    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Restore" })).toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: /add to favorites|remove from favorites/i })).not.toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: "Add to album" })).not.toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: /delete|move to trash|empty trash|restore all/i })).not.toBeInTheDocument();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /add to favorites|remove from favorites/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add to album" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /delete|move to trash|empty trash|restore all/i })).not.toBeInTheDocument();
  });

  it("refreshes on every entry from another view and does nothing when already open", async () => {
    const getTrashBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }, { timeBucket: december, count: 1 }])
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": getTrashBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByText("December 2024")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Trash" }));
    expect(getTrashBuckets).toHaveBeenCalledTimes(1);

    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Trash" }));
    await waitFor(() => {
      expect(getTrashBuckets).toHaveBeenCalledTimes(2);
      expect(screen.queryByText("December 2024")).not.toBeInTheDocument();
    });
  });

  it("shows loading, then the empty state when the trash is empty", async () => {
    let resolveBuckets!: (value: unknown) => void;
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": () => new Promise((resolve) => { resolveBuckets = resolve; }) });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading trash…");
    expect(screen.queryByText("Your trash is empty.")).not.toBeInTheDocument();

    resolveBuckets([]);
    expect(await screen.findByText("Your trash is empty.")).toHaveAttribute("role", "status");
    expect(screen.queryByText("Loading trash…")).not.toBeInTheDocument();
  });

  it("shows an error with a retry that reloads the trash, including when Immich rejects the trashed filter", async () => {
    const getTrashBuckets = vi.fn()
      .mockRejectedValueOnce("Failed to fetch trash timeline buckets: 400 Bad Request")
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }]);
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": getTrashBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The trash could not be loaded.");
    expect(screen.queryByText("Your trash is empty.")).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(getTrashBuckets).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps already shown Trashed Assets visible and announces offline when a refresh fails", async () => {
    const getTrashBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockRejectedValueOnce(new Error("offline"));
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": getTrashBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Refresh timeline" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Trash is offline. Showing cached trash.");
    expect(screen.getByText("March 2025")).toBeInTheDocument();
  });

  it("resets the Trash Timeline when leaving the view so a stale list is never shown on return", async () => {
    const getTrashBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockImplementationOnce(() => new Promise(() => {}));
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": getTrashBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
    await fireEvent.click(screen.getByRole("button", { name: "Trash" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Loading trash…");
    expect(screen.queryByText("March 2025")).not.toBeInTheDocument();
  });

  it("clears the Trash Timeline on logout so the next session does not inherit it", async () => {
    const getTrashBuckets = vi.fn()
      .mockResolvedValueOnce([{ timeBucket: march, count: 1 }])
      .mockImplementationOnce(() => new Promise(() => {}));
    renderAuthenticatedPage({ "get_filtered_timeline_buckets:trash": getTrashBuckets });

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Logout" }));
    expect(await screen.findByRole("heading", { name: "Login" })).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    await fireEvent.input(screen.getByLabelText("API Key"), { target: { value: "secret-key" } });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));

    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading trash…");
    expect(screen.queryByText("March 2025")).not.toBeInTheDocument();
  });
});

describe("Trash Timeline Restore", () => {
  beforeEach(stubImmediateIntersectionObserver);

  const march = "2025-03-01T00:00:00.000Z";
  const trashed = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE" as const, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const details = {
    originalFileName: "trashed.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };
  const restoreAll = async (args?: unknown) => ({ restoredAssetIds: (args as { assetIds: string[] }).assetIds });

  function renderTrash(restore: (args?: unknown) => Promise<unknown>, assetIds = ["trash-1", "trash-2", "trash-3"]) {
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: assetIds.length }],
      "get_filtered_timeline_bucket_assets:trash": async () => assetIds.map(trashed),
      get_asset_details: async () => details,
      restore_trashed_assets: restore,
    });
  }

  async function openTrash() {
    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    await screen.findAllByRole("button", { name: "Open photo" });
  }

  async function select(count: number) {
    const boxes = screen.getAllByRole("checkbox", { name: "Select asset" });
    for (let index = 0; index < count; index += 1) await fireEvent.click(boxes[index]);
    await screen.findByText(`${count} selected`);
  }

  it("restores a Trashed Asset from the Lightbox without a confirmation and removes it from the view", async () => {
    const restore = vi.fn(restoreAll);
    renderTrash(restore);
    await openTrash();

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Restore" }));

    await waitFor(() => expect(restore).toHaveBeenCalledWith({ assetIds: ["trash-1"] }));
    expect(await screen.findByRole("status")).toHaveTextContent("1 restored to your Immich library.");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
  });

  it("keeps the Lightbox open and names the missing permission when a Restore from it is refused", async () => {
    renderTrash(vi.fn(async () => { throw "trash_restore_permission_denied:asset.delete"; }));
    await openTrash();

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Restore" }));

    expect(await within(lightbox).findByRole("alert")).toHaveTextContent("Restoring was refused. Give the API key the asset.delete permission.");
    expect(within(lightbox).getByRole("button", { name: "Restore" })).toBeEnabled();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
  });

  it("restores a multi-selection, removes the restored assets and announces the result", async () => {
    const restore = vi.fn(restoreAll);
    renderTrash(restore);
    await openTrash();

    await select(2);
    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    await waitFor(() => expect(restore).toHaveBeenCalledWith({ assetIds: ["trash-1", "trash-2"] }));
    expect(await screen.findByRole("status")).toHaveTextContent("2 restored to your Immich library.");
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("leaves failed Trashed Assets visible and selected on a partial failure and lets the user retry them", async () => {
    const restore = vi.fn()
      .mockResolvedValueOnce({ restoredAssetIds: ["trash-1"] })
      .mockImplementationOnce(restoreAll);
    renderTrash(restore);
    await openTrash();

    await select(3);
    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("2 of 3 could not be restored. They stay selected so you can retry.");
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(screen.getByText("2 selected")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(restore).toHaveBeenLastCalledWith({ assetIds: ["trash-2", "trash-3"] }));
    expect(await screen.findByText("2 restored to your Immich library.")).toHaveAttribute("role", "status");
    expect(screen.getByText("Your trash is empty.")).toBeInTheDocument();
    expect(screen.queryAllByRole("button", { name: "Open photo" })).toHaveLength(0);
  });

  it("names how many Trashed Assets failed when a Restore restores none of them and keeps them selected", async () => {
    renderTrash(vi.fn(async (args?: unknown) => ({ restoredAssetIds: [] })));
    await openTrash();

    await select(2);
    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("2 of 2 could not be restored. Please try again.");
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
  });

  it("names the failed count in the Lightbox when a Restore from it restores nothing", async () => {
    renderTrash(vi.fn(async (args?: unknown) => ({ restoredAssetIds: [] })));
    await openTrash();

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Restore" }));

    expect(await within(lightbox).findByRole("alert")).toHaveTextContent("1 of 1 could not be restored. Please try again.");
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
  });

  it("names the missing permission on a refused multi-selection Restore and keeps the view usable", async () => {
    renderTrash(vi.fn(async () => { throw "trash_restore_permission_denied:asset.delete"; }));
    await openTrash();

    await select(2);
    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Restoring was refused. Give the API key the asset.delete permission.");
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Restore" })).toBeEnabled();
  });

  it("shows a credential-free failure for other Restore errors and keeps the selection", async () => {
    renderTrash(vi.fn(async () => { throw new Error("Failed with key secret-key at https://immich.example"); }));
    await openTrash();

    await select(1);
    await fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not restore. Please try again.");
    expect(alert.textContent).not.toContain("secret-key");
    expect(screen.getByText("1 selected")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
  });

  it("offers Restore as a native button so the keyboard can operate it", async () => {
    renderTrash(vi.fn(restoreAll));
    await openTrash();
    await select(1);

    const button = screen.getByRole("button", { name: "Restore" });
    expect(button.tagName).toBe("BUTTON");
    expect(button).toHaveAttribute("type", "button");
    expect(button).not.toHaveAttribute("tabindex", "-1");
  });

  describe("Photo Timeline after a Restore", () => {
    function renderTrashAndPhotos(restore: (args?: unknown) => Promise<unknown>) {
      const photosBuckets = vi.fn<() => Promise<unknown>>().mockResolvedValue([]);
      renderAuthenticatedPage({
        get_timeline_buckets: photosBuckets,
        get_timeline_bucket_assets: async () => [trashed("trash-1")],
        "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 2 }],
        "get_filtered_timeline_bucket_assets:trash": async () => ["trash-1", "trash-2"].map(trashed),
        get_asset_details: async () => details,
        restore_trashed_assets: restore,
      });
      return photosBuckets;
    }

    async function restoreFirstFromTrash() {
      await openTrash();
      await select(1);
      await fireEvent.click(screen.getByRole("button", { name: "Restore" }));
      await screen.findByRole("status");
    }

    it("shows a restored Remote Asset in the Photo Timeline the next time it is opened", async () => {
      const photosBuckets = renderTrashAndPhotos(vi.fn(restoreAll));
      await waitFor(() => expect(photosBuckets).toHaveBeenCalledTimes(1));
      photosBuckets.mockResolvedValue([{ timeBucket: march, count: 1 }]);

      await restoreFirstFromTrash();
      await fireEvent.click(screen.getByRole("button", { name: "Photos" }));

      expect(await screen.findByText("March 2025")).toBeInTheDocument();
      expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(1);
    });

    it("refreshes the Photo Timeline once after a partial Restore and not on later visits", async () => {
      const photosBuckets = renderTrashAndPhotos(vi.fn(async () => ({ restoredAssetIds: ["trash-1"] })));
      await waitFor(() => expect(photosBuckets).toHaveBeenCalledTimes(1));

      await restoreFirstFromTrash();
      await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
      await waitFor(() => expect(photosBuckets).toHaveBeenCalledTimes(2));

      await fireEvent.click(screen.getByRole("button", { name: "Trash" }));
      await fireEvent.click(screen.getByRole("button", { name: "Photos" }));
      expect(photosBuckets).toHaveBeenCalledTimes(2);
    });

    it("leaves the Photo Timeline alone when nothing was restored or the Restore failed", async () => {
      const photosBuckets = renderTrashAndPhotos(vi.fn(async () => ({ restoredAssetIds: [] })));
      await waitFor(() => expect(photosBuckets).toHaveBeenCalledTimes(1));

      await openTrash();
      await select(1);
      await fireEvent.click(screen.getByRole("button", { name: "Restore" }));
      await screen.findByRole("alert");
      await fireEvent.click(screen.getByRole("button", { name: "Photos" }));

      expect(photosBuckets).toHaveBeenCalledTimes(1);
    });
  });

  it("offers no Restore outside the Trash Timeline", async () => {
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
      get_timeline_bucket_assets: async () => [trashed("photo-1")],
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Photos" }));
    await screen.findAllByRole("button", { name: "Open photo" });
    await fireEvent.click(screen.getAllByRole("checkbox", { name: "Select asset" })[0]);
    expect(await screen.findByText("1 selected")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
  });
});

describe("Photo Timeline Move to Trash", () => {
  beforeEach(stubImmediateIntersectionObserver);

  const march = "2025-03-01T00:00:00.000Z";
  const photo = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE" as const, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const details = {
    originalFileName: "photo.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };
  const moved = async (args?: unknown) => ({ movedAssetIds: (args as { assetIds: string[] }).assetIds });

  function renderPhotos(trash: (args?: unknown) => Promise<unknown>, assetIds = ["photo-1", "photo-2"]) {
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [{ timeBucket: march, count: assetIds.length }],
      get_timeline_bucket_assets: async () => assetIds.map(photo),
      get_asset_details: async () => details,
      trash_assets: trash,
    });
  }

  async function openFirstPhoto() {
    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    return screen.findByRole("dialog", { name: "Media lightbox" });
  }

  async function confirmMoveToTrash(lightbox: HTMLElement) {
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Move to Trash" }));
    const dialog = await screen.findByRole("dialog", { name: "Move to Trash?" });
    await fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
  }

  it("offers Move to Trash next to Download in the Lightbox of the Photo Timeline", async () => {
    renderPhotos(vi.fn(moved));
    const lightbox = await openFirstPhoto();

    const labels = within(lightbox).getAllByRole("button").map((button) => button.getAttribute("aria-label"));
    expect(labels.indexOf("Move to Trash")).toBeGreaterThan(-1);
    expect(labels.indexOf("Move to Trash")).toBe(labels.indexOf("Download original") + 1);
  });

  it("offers no Move to Trash in the Lightbox of the Trash Timeline", async () => {
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 1 }],
      "get_filtered_timeline_bucket_assets:trash": async () => [photo("trash-1")],
      get_asset_details: async () => details,
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
    const lightbox = await openFirstPhoto();

    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: "Move to Trash" })).not.toBeInTheDocument();
  });

  it("asks for confirmation naming the count and the way back, and cancelling does nothing", async () => {
    const trash = vi.fn(moved);
    renderPhotos(trash);
    const lightbox = await openFirstPhoto();

    await fireEvent.click(within(lightbox).getByRole("button", { name: "Move to Trash" }));

    const dialog = await screen.findByRole("dialog", { name: "Move to Trash?" });
    expect(dialog).toHaveTextContent("Move 1 selected to the Immich trash? You can restore them in the Trash.");
    await fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog", { name: "Move to Trash?" })).not.toBeInTheDocument();
    expect(trash).not.toHaveBeenCalled();
    await fireEvent.click(within(screen.getByRole("dialog", { name: "Media lightbox" })).getByRole("button", { name: "Close lightbox" }));
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
  });

  it("moves the asset on confirmation, removes it from the timeline and closes the Lightbox", async () => {
    const trash = vi.fn(moved);
    renderPhotos(trash);
    const lightbox = await openFirstPhoto();

    await confirmMoveToTrash(lightbox);

    await waitFor(() => expect(trash).toHaveBeenCalledWith({ assetIds: ["photo-1"] }));
    expect(trash).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent("1 moved to the Immich trash. You can restore them in the Trash.");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
  });

  it("names the missing permission and keeps the asset and the Lightbox when Immich refuses", async () => {
    renderPhotos(vi.fn(async () => { throw "trash_move_permission_denied:asset.delete"; }));
    const lightbox = await openFirstPhoto();

    await confirmMoveToTrash(lightbox);

    expect(await within(lightbox).findByRole("alert")).toHaveTextContent("Moving to the trash was refused. Give the API key the asset.delete permission.");
    expect(screen.queryByRole("dialog", { name: "Move to Trash?" })).not.toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Move to Trash" })).toBeEnabled();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
  });

  it("shows a credential-free failure for other errors and when Immich did not move the asset", async () => {
    const trash = vi.fn()
      .mockRejectedValueOnce(new Error("Failed with key secret-key at https://immich.example"))
      .mockResolvedValueOnce({ movedAssetIds: [] });
    renderPhotos(trash);
    const lightbox = await openFirstPhoto();

    for (let attempt = 1; attempt <= 2; attempt += 1) {
      await confirmMoveToTrash(lightbox);
      await waitFor(() => expect(trash).toHaveBeenCalledTimes(attempt));
      const alert = await within(lightbox).findByRole("alert");
      expect(alert).toHaveTextContent("Could not move to the trash. Please try again.");
      expect(alert.textContent).not.toContain("secret-key");
    }
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
  });

  it("disables Move to Trash and closing while the request runs", async () => {
    let finish!: (value: unknown) => void;
    renderPhotos(vi.fn(() => new Promise((resolve) => { finish = resolve; })));
    const lightbox = await openFirstPhoto();

    await confirmMoveToTrash(lightbox);

    await waitFor(() => expect(within(lightbox).getByRole("button", { name: "Move to Trash" })).toBeDisabled());
    expect(within(lightbox).getByRole("button", { name: "Close lightbox" })).toBeDisabled();
    finish({ movedAssetIds: ["photo-1"] });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("still closes the Lightbox and drops the asset when the timeline was refreshed while the move ran", async () => {
    let finish!: (value: unknown) => void;
    renderPhotos(vi.fn(() => new Promise((resolve) => { finish = resolve; })), ["photo-1", "photo-2", "photo-3"]);
    const lightbox = await openFirstPhoto();
    await confirmMoveToTrash(lightbox);
    await waitFor(() => expect(within(lightbox).getByRole("button", { name: "Move to Trash" })).toBeDisabled());
    const bucketRequests = tauri.invoke.mock.calls.filter(([name]) => name === "get_timeline_buckets").length;

    await fireEvent.click(screen.getByRole("button", { name: "Refresh timeline" }));
    await waitFor(() => expect(tauri.invoke.mock.calls.filter(([name]) => name === "get_timeline_buckets").length).toBeGreaterThan(bucketRequests));
    finish({ movedAssetIds: ["photo-1"] });

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
  });

  describe("multi-selection", () => {
    const three = ["photo-1", "photo-2", "photo-3"];

    async function select(count: number) {
      const boxes = await screen.findAllByRole("checkbox", { name: "Select asset" });
      for (let index = 0; index < count; index += 1) await fireEvent.click(boxes[index]);
      await screen.findByText(`${count} selected`);
    }

    /** The selection bar's button; the confirmation dialog has a button of the same name. */
    const barTrashButton = () => screen.getAllByRole("button", { name: "Move to Trash" }).find((button) => !button.closest("dialog"))!;

    /** Opens the confirmation from the selection bar and returns its dialog. */
    async function openConfirmation() {
      await fireEvent.click(screen.getByRole("button", { name: "Move to Trash" }));
      return screen.findByRole("dialog", { name: "Move to Trash?" });
    }

    async function confirmSelection() {
      const dialog = await openConfirmation();
      await fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
    }

    it("asks for confirmation naming the selected count, and cancelling keeps the selection and requests nothing", async () => {
      const trash = vi.fn(moved);
      renderPhotos(trash, three);
      await select(2);

      const dialog = await openConfirmation();
      expect(dialog).toHaveTextContent("Move 2 selected to the Immich trash? You can restore them in the Trash.");
      await fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

      expect(screen.queryByRole("dialog", { name: "Move to Trash?" })).not.toBeInTheDocument();
      expect(trash).not.toHaveBeenCalled();
      expect(screen.getByText("2 selected")).toBeInTheDocument();
    });

    it("moves the selected assets together, removes them at once and announces the count", async () => {
      const trash = vi.fn(moved);
      renderPhotos(trash, three);
      await select(2);

      await confirmSelection();

      await waitFor(() => expect(trash).toHaveBeenCalledWith({ assetIds: ["photo-1", "photo-2"] }));
      expect(trash).toHaveBeenCalledTimes(1);
      expect(await screen.findByRole("status")).toHaveTextContent("2 moved to the Immich trash. You can restore them in the Trash.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
      expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    it("leaves failed assets visible and selected on a partial failure and lets the user retry them", async () => {
      const trash = vi.fn()
        .mockResolvedValueOnce({ movedAssetIds: ["photo-1"] })
        .mockImplementationOnce(moved);
      renderPhotos(trash, three);
      await select(3);

      await confirmSelection();

      expect(await screen.findByRole("alert")).toHaveTextContent("2 of 3 could not be moved to the trash. They stay selected so you can retry.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
      expect(screen.getByText("2 selected")).toBeInTheDocument();
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

      await confirmSelection();
      await waitFor(() => expect(trash).toHaveBeenLastCalledWith({ assetIds: ["photo-2", "photo-3"] }));
      expect(await screen.findByText(/^2 moved to the Immich trash/)).toHaveAttribute("role", "status");
      expect(screen.queryAllByRole("button", { name: "Open photo" })).toHaveLength(0);
    });

    it("removes the assets moved before the request stopped, keeps the others selected and still names the refusal", async () => {
      const trash = vi.fn(async () => ({ movedAssetIds: ["photo-1"], failureCode: "trash_move_permission_denied:asset.delete" }));
      renderPhotos(trash, three);
      await select(3);

      await confirmSelection();

      expect(await screen.findByRole("alert")).toHaveTextContent("Moving to the trash was refused. Give the API key the asset.delete permission.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
      expect(screen.getByText("2 selected")).toBeInTheDocument();
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    });

    it("shows the failure message when the request stopped after some assets were moved for another reason", async () => {
      renderPhotos(vi.fn(async () => ({ movedAssetIds: ["photo-1", "photo-2"], failureCode: "trash_move_unavailable" })), three);
      await select(3);

      await confirmSelection();

      expect(await screen.findByRole("alert")).toHaveTextContent("Could not move to the trash. Please try again.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
      expect(screen.getByText("1 selected")).toBeInTheDocument();
    });

    it("names how many assets failed when none was moved and keeps them selected", async () => {
      renderPhotos(vi.fn(async () => ({ movedAssetIds: [] })), three);
      await select(2);

      await confirmSelection();

      expect(await screen.findByRole("alert")).toHaveTextContent("2 of 2 could not be moved to the trash. Please try again.");
      expect(screen.getByText("2 selected")).toBeInTheDocument();
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
    });

    it("names the missing permission on a refused request and keeps the selection and the view usable", async () => {
      renderPhotos(vi.fn(async () => { throw "trash_move_permission_denied:asset.delete"; }), three);
      await select(2);

      await confirmSelection();

      expect(await screen.findByRole("alert")).toHaveTextContent("Moving to the trash was refused. Give the API key the asset.delete permission.");
      expect(screen.getByText("2 selected")).toBeInTheDocument();
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
      expect(screen.getByRole("button", { name: "Move to Trash" })).toBeEnabled();
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
    });

    it("shows a credential-free failure for another error", async () => {
      renderPhotos(vi.fn(async () => { throw new Error("Failed with key secret-key at https://immich.example"); }), three);
      await select(2);

      await confirmSelection();

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("Could not move to the trash. Please try again.");
      expect(alert.textContent).not.toContain("secret-key");
      expect(screen.getByText("2 selected")).toBeInTheDocument();
    });

    it("ignores a second bulk action while the Move to Trash runs", async () => {
      let finish!: (value: unknown) => void;
      const trash = vi.fn(() => new Promise((resolve) => { finish = resolve; }));
      renderPhotos(trash, three);
      await select(2);

      await confirmSelection();

      await waitFor(() => expect(barTrashButton()).toHaveAttribute("aria-busy", "true"));
      expect(barTrashButton()).toBeDisabled();
      expect(screen.getByRole("button", { name: "Download" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Done" })).toBeDisabled();
      expect(trash).toHaveBeenCalledTimes(1);
      finish({ movedAssetIds: ["photo-1", "photo-2"] });
      expect(await screen.findByRole("status")).toHaveTextContent("2 moved to the Immich trash");
    });

    it("drops a result that arrives after the timeline was refreshed", async () => {
      let finish!: (value: unknown) => void;
      renderPhotos(vi.fn(() => new Promise((resolve) => { finish = resolve; })), three);
      await select(2);
      await confirmSelection();
      await waitFor(() => expect(barTrashButton()).toBeDisabled());

      await fireEvent.click(screen.getByRole("button", { name: "Refresh timeline" }));
      finish({ movedAssetIds: ["photo-1", "photo-2"] });

      await waitFor(() => expect(screen.queryByText(/selected/)).not.toBeInTheDocument());
      expect(screen.queryByText(/moved to the Immich trash/)).not.toBeInTheDocument();
    });

    it("offers no Move to Trash in the selection bar of the Trash Timeline", async () => {
      renderAuthenticatedPage({
        "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 1 }],
        "get_filtered_timeline_bucket_assets:trash": async () => [photo("trash-1")],
        get_asset_details: async () => details,
      });
      await fireEvent.click(await screen.findByRole("button", { name: "Trash" }));
      await select(1);

      expect(screen.queryByRole("button", { name: "Move to Trash" })).not.toBeInTheDocument();
    });
  });
});

describe("Move to Trash in every hosting view", () => {
  beforeEach(stubImmediateIntersectionObserver);

  const march = "2025-03-01T00:00:00.000Z";
  const asset = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE" as const, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const details = {
    originalFileName: "photo.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };
  const timelineOf = (ids: string[]) => ({ buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(asset) }, truncated: false });
  const nav = async (name: string) => fireEvent.click(await screen.findByRole("button", { name }));
  const moved = async (args?: unknown) => ({ movedAssetIds: (args as { assetIds: string[] }).assetIds });

  interface HostingView {
    name: string;
    handlers(ids: string[]): Record<string, (args?: unknown) => Promise<unknown>>;
    open(): Promise<void>;
  }
  const hostingViews: HostingView[] = [
    {
      name: "Favorites",
      handlers: (ids) => ({ "get_filtered_timeline_buckets:favorites": async () => [{ timeBucket: march, count: ids.length }], "get_filtered_timeline_bucket_assets:favorites": async () => ids.map(asset) }),
      open: async () => { await nav("Favorites"); },
    },
    {
      name: "Archive",
      handlers: (ids) => ({ "get_filtered_timeline_buckets:archive": async () => [{ timeBucket: march, count: ids.length }], "get_filtered_timeline_bucket_assets:archive": async () => ids.map(asset) }),
      open: async () => { await nav("Archive"); },
    },
    {
      name: "an Album Timeline",
      handlers: (ids) => ({
        get_album_catalogue: async () => [{ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: ids.length, isShared: true, order: null }],
        get_album_timeline: async () => timelineOf(ids),
      }),
      open: async () => { await nav("Albums"); await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ })); },
    },
    {
      name: "a Person Timeline",
      handlers: (ids) => ({
        get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
        get_person_timeline_buckets: async () => [{ timeBucket: march, count: ids.length }],
        get_person_timeline_bucket_assets: async () => ids.map(asset),
      }),
      open: async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Ada" })); },
    },
    {
      name: "a Place Timeline",
      handlers: (ids) => ({ get_places: async () => [{ city: "Paris", thumbnailAssetId: ids[0] }], get_place_timeline: async () => timelineOf(ids) }),
      open: async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Paris" })); },
    },
    {
      name: "a Memory Timeline",
      handlers: (ids) => ({
        get_memories: async () => [{ id: "m-1", kind: "onThisDay", memoryAt: "2019-01-02T00:00:00.000Z", year: 2019, personName: null, coverAssetId: ids[0], timeline: timelineOf(ids) }],
      }),
      open: async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Jan 2, 2019" })); },
    },
    {
      name: "a Tag Timeline",
      handlers: (ids) => ({
        get_tags: async () => [{ id: "t-family", name: "Family", value: "Family", parentId: null, color: null }],
        get_tag_timeline_buckets: async () => [{ timeBucket: march, count: ids.length }],
        get_tag_timeline_bucket_assets: async () => ids.map(asset),
      }),
      open: async () => { await nav("Tags"); await fireEvent.click(await screen.findByRole("treeitem", { name: "Family" })); },
    },
    {
      name: "Search Results",
      handlers: (ids) => ({
        get_search_support: async () => ({ supported: true, version: "3.4.1", requiredVersion: "3.2" }),
        search_assets: async () => timelineOf(ids),
      }),
      open: async () => {
        await fireEvent.input(await screen.findByRole("searchbox", { name: "Search your Immich library" }), { target: { value: "IMG_" } });
        await fireEvent.click(screen.getByRole("button", { name: "Search" }));
      },
    },
  ];

  async function renderView(view: HostingView, trash: (args?: unknown) => Promise<unknown>, ids = ["a-1", "a-2", "a-3"]) {
    renderAuthenticatedPage({ ...view.handlers(ids), get_asset_details: async () => details, trash_assets: trash });
    await view.open();
    await screen.findAllByRole("button", { name: "Open photo" });
  }

  /** The selection bar's button; the confirmation dialog has a button of the same name. */
  const barTrashButton = () => screen.getAllByRole("button", { name: "Move to Trash" }).find((button) => !button.closest("dialog"))!;

  async function select(count: number) {
    const boxes = await screen.findAllByRole("checkbox", { name: "Select asset" });
    for (let index = 0; index < count; index += 1) await fireEvent.click(boxes[index]);
    await screen.findByText(`${count} selected`);
  }

  async function confirmInDialog() {
    const dialog = await screen.findByRole("dialog", { name: "Move to Trash?" });
    await fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
  }

  describe.each(hostingViews)("in $name", (view) => {
    it("moves the asset from the Lightbox and removes it from the view", async () => {
      const trash = vi.fn(moved);
      await renderView(view, trash);

      await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
      const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Move to Trash" }));
      await confirmInDialog();

      await waitFor(() => expect(trash).toHaveBeenCalledWith({ assetIds: ["a-1"] }));
      expect(await screen.findByRole("status")).toHaveTextContent("1 moved to the Immich trash. You can restore them in the Trash.");
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    });

    it("moves the multi-selection and removes the assets from the view", async () => {
      const trash = vi.fn(moved);
      await renderView(view, trash);
      await select(2);

      await fireEvent.click(barTrashButton());
      await confirmInDialog();

      await waitFor(() => expect(trash).toHaveBeenCalledWith({ assetIds: ["a-1", "a-2"] }));
      expect(await screen.findByRole("status")).toHaveTextContent("2 moved to the Immich trash");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);
      expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
    });

    it("keeps an asset the server refuses visible and selected with a partial-failure message", async () => {
      // The server's answer decides: the second asset is, for example, owned by someone else in a shared Album.
      const trash = vi.fn(async () => ({ movedAssetIds: ["a-1"] }));
      await renderView(view, trash);
      await select(2);

      await fireEvent.click(barTrashButton());
      await confirmInDialog();

      expect(await screen.findByRole("alert")).toHaveTextContent("1 of 2 could not be moved to the trash. They stay selected so you can retry.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
      expect(screen.getByText("1 selected")).toBeInTheDocument();
    });

    it("keeps an asset the server refuses in the Lightbox with a message", async () => {
      await renderView(view, vi.fn(async () => ({ movedAssetIds: [] })));

      await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
      const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Move to Trash" }));
      await confirmInDialog();

      expect(await within(lightbox).findByRole("alert")).toHaveTextContent("Could not move to the trash. Please try again.");
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3);
    });
  });

  it("offers no Move to Trash in the Trash Timeline, which keeps Restore and Download", async () => {
    renderAuthenticatedPage({
      "get_filtered_timeline_buckets:trash": async () => [{ timeBucket: march, count: 2 }],
      "get_filtered_timeline_bucket_assets:trash": async () => ["t-1", "t-2"].map(asset),
      get_asset_details: async () => details,
      trash_assets: moved,
    });
    await nav("Trash");
    await select(1);
    expect(screen.queryByRole("button", { name: "Move to Trash" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Done" }));

    await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    expect(within(lightbox).queryByRole("button", { name: "Move to Trash" })).not.toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Restore" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
  });
});

describe("Cached views after a Move to Trash", () => {
  beforeEach(stubImmediateIntersectionObserver);

  const march = "2025-03-01T00:00:00.000Z";
  const asset = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE" as const, takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const details = {
    originalFileName: "photo.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024,
  };
  const timelineOf = (ids: string[]) => ({ buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(asset) }, truncated: false });
  const moved = async (args?: unknown) => ({ movedAssetIds: (args as { assetIds: string[] }).assetIds });
  const nav = async (name: string) => fireEvent.click(await screen.findByRole("button", { name }));
  const calls = (command: string) => tauri.invoke.mock.calls.filter(([name]) => name === command).length;

  async function openFirstPhotoAndConfirmMove() {
    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Move to Trash" }));
    const dialog = await screen.findByRole("dialog", { name: "Move to Trash?" });
    await fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
    return lightbox;
  }

  /** Moves the first asset of the open view to the trash from its Lightbox and waits until the Lightbox closed. */
  async function moveFirstToTrash() {
    await openFirstPhotoAndConfirmMove();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  }

  const favorites = {
    "get_filtered_timeline_buckets:favorites": async () => [{ timeBucket: march, count: 2 }],
    "get_filtered_timeline_bucket_assets:favorites": async () => ["fav-1", "fav-2"].map(asset),
  };

  describe("the Photo Timeline", () => {
    it("reloads the next time it opens after a move from another view", async () => {
      renderAuthenticatedPage({ ...favorites, get_asset_details: async () => details, trash_assets: moved });
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(1));
      await nav("Favorites");
      await moveFirstToTrash();

      await nav("Photos");

      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(2));
    });

    it("is not marked stale by a move inside it", async () => {
      renderAuthenticatedPage({
        get_timeline_buckets: async () => [{ timeBucket: march, count: 2 }],
        get_timeline_bucket_assets: async () => ["photo-1", "photo-2"].map(asset),
        get_asset_details: async () => details,
        trash_assets: moved,
      });
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(1));
      await moveFirstToTrash();
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);

      await nav("Favorites");
      await nav("Photos");

      expect(calls("get_timeline_buckets")).toBe(1);
    });

    it("stays as it is when the request moved nothing", async () => {
      renderAuthenticatedPage({ ...favorites, get_asset_details: async () => details, trash_assets: async () => ({ movedAssetIds: [] }) });
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(1));
      await nav("Favorites");
      const lightbox = await openFirstPhotoAndConfirmMove();
      await within(lightbox).findByRole("alert");
      await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

      await nav("Photos");

      expect(calls("get_timeline_buckets")).toBe(1);
    });

    it("reloads when Back from a Search started in it follows a move from the results", async () => {
      renderAuthenticatedPage({
        get_search_support: async () => ({ supported: true, version: "3.4.1", requiredVersion: "3.2" }),
        search_assets: async () => timelineOf(["found-1", "found-2"]),
        get_asset_details: async () => details,
        trash_assets: moved,
      });
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(1));
      await fireEvent.input(await screen.findByRole("searchbox", { name: "Search your Immich library" }), { target: { value: "IMG_" } });
      await fireEvent.click(screen.getByRole("button", { name: "Search" }));
      await moveFirstToTrash();

      await fireEvent.click(await screen.findByRole("button", { name: "Back to Photos" }));

      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(2));
    });
  });

  it("makes no Explore or Tags request after a move while neither is the open view, and reloads each once when it opens", async () => {
    renderAuthenticatedPage({
      ...favorites,
      get_people: async () => ({ people: [], hasNextPage: false }),
      get_places: async () => [],
      get_memories: async () => [],
      get_tags: async () => [],
      get_asset_details: async () => details,
      trash_assets: moved,
    });
    await nav("Explore");
    await waitFor(() => expect(calls("get_places")).toBe(1));
    await nav("Tags");
    await waitFor(() => expect(calls("get_tags")).toBe(1));
    await nav("Favorites");
    const explore = ["get_people", "get_places", "get_memories"].map(calls);
    const tags = calls("get_tags");

    await moveFirstToTrash();

    expect(["get_people", "get_places", "get_memories"].map(calls)).toEqual(explore);
    expect(calls("get_tags")).toBe(tags);

    await nav("Explore");
    await waitFor(() => expect(calls("get_places")).toBe(2));
    await nav("Tags");
    await waitFor(() => expect(calls("get_tags")).toBe(2));
  });

  it("reloads the Explore Overview on Back from a Person Timeline after a move there", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
      get_person_timeline_buckets: async () => [{ timeBucket: march, count: 2 }],
      get_person_timeline_bucket_assets: async () => ["p-1", "p-2"].map(asset),
      get_asset_details: async () => details,
      trash_assets: moved,
    });
    await nav("Explore");
    await fireEvent.click(await screen.findByRole("button", { name: "Ada" }));
    await waitFor(() => expect(calls("get_places")).toBe(1));
    await moveFirstToTrash();

    await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));

    await waitFor(() => expect(calls("get_places")).toBe(2));
  });

  it("keeps the Explore Overview as it is on Back when nothing was moved", async () => {
    renderAuthenticatedPage({
      get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
      get_person_timeline_buckets: async () => [{ timeBucket: march, count: 2 }],
      get_person_timeline_bucket_assets: async () => ["p-1", "p-2"].map(asset),
    });
    await nav("Explore");
    await fireEvent.click(await screen.findByRole("button", { name: "Ada" }));
    await waitFor(() => expect(calls("get_places")).toBe(1));

    await fireEvent.click(await screen.findByRole("button", { name: "Back to Explore" }));
    await screen.findByRole("button", { name: "Ada" });

    expect(calls("get_places")).toBe(1);
  });

  it("reloads the Tags tree on Back from a Tag Timeline after a move there", async () => {
    renderAuthenticatedPage({
      get_tags: async () => [{ id: "t-family", name: "Family", value: "Family", parentId: null, color: null }],
      get_tag_timeline_buckets: async () => [{ timeBucket: march, count: 2 }],
      get_tag_timeline_bucket_assets: async () => ["t-1", "t-2"].map(asset),
      get_asset_details: async () => details,
      trash_assets: moved,
    });
    await nav("Tags");
    await fireEvent.click(await screen.findByRole("treeitem", { name: "Family" }));
    await waitFor(() => expect(calls("get_tags")).toBe(1));
    await moveFirstToTrash();

    await fireEvent.click(await screen.findByRole("button", { name: "Back to Tags" }));

    await waitFor(() => expect(calls("get_tags")).toBe(2));
  });

  describe("a request that stopped after some assets were moved", () => {
    const stopped = async () => ({ movedAssetIds: ["fav-1"], failureCode: "trash_move_unavailable" });

    async function selectFirstAndConfirmMove() {
      await fireEvent.click((await screen.findAllByRole("checkbox", { name: "Select asset" }))[0]);
      await fireEvent.click((await screen.findAllByRole("button", { name: "Move to Trash" })).find((button) => !button.closest("dialog"))!);
      const dialog = await screen.findByRole("dialog", { name: "Move to Trash?" });
      await fireEvent.click(within(dialog).getByRole("button", { name: "Move to Trash" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    }

    it("marks Photos, Explore and Tags stale exactly as for a complete move", async () => {
      renderAuthenticatedPage({
        ...favorites,
        get_people: async () => ({ people: [], hasNextPage: false }),
        get_places: async () => [],
        get_memories: async () => [],
        get_tags: async () => [],
        trash_assets: stopped,
      });
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(1));
      await nav("Explore");
      await waitFor(() => expect(calls("get_places")).toBe(1));
      await nav("Tags");
      await waitFor(() => expect(calls("get_tags")).toBe(1));
      await nav("Favorites");

      await selectFirstAndConfirmMove();
      expect(await screen.findByRole("alert")).toHaveTextContent("Could not move to the trash. Please try again.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1);

      await nav("Photos");
      await waitFor(() => expect(calls("get_timeline_buckets")).toBe(2));
      await nav("Explore");
      await waitFor(() => expect(calls("get_places")).toBe(2));
      await nav("Tags");
      await waitFor(() => expect(calls("get_tags")).toBe(2));
    });
  });

  describe("the Albums catalogue", () => {
    const holiday = (assetCount: number) => ({ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount, isShared: false, order: null });
    const albumPages = (trash_assets: (args?: unknown) => Promise<unknown>) => ({
      get_album_catalogue: vi.fn().mockResolvedValueOnce([holiday(2)]).mockResolvedValue([holiday(1)]),
      get_album_timeline: async () => timelineOf(["a-1", "a-2"]),
      get_asset_details: async () => details,
      trash_assets,
    });

    it("reloads on Back from an Album Timeline after a move there", async () => {
      renderAuthenticatedPage(albumPages(moved));
      await nav("Albums");
      await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));
      await waitFor(() => expect(calls("get_album_catalogue")).toBe(1));
      await moveFirstToTrash();

      await fireEvent.click(await screen.findByRole("button", { name: "Back to Albums" }));

      await waitFor(() => expect(calls("get_album_catalogue")).toBe(2));
    });

    it("reloads on Back after a move from Search Results started in an Album Timeline", async () => {
      renderAuthenticatedPage({
        ...albumPages(moved),
        get_search_support: async () => ({ supported: true, version: "3.4.1", requiredVersion: "3.2" }),
        search_assets: async () => timelineOf(["found-1", "found-2"]),
      });
      await nav("Albums");
      await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));
      await waitFor(() => expect(calls("get_album_catalogue")).toBe(1));
      await fireEvent.input(await screen.findByRole("searchbox", { name: "Search your Immich library" }), { target: { value: "IMG_" } });
      await fireEvent.click(screen.getByRole("button", { name: "Search" }));
      await moveFirstToTrash();
      await fireEvent.click(await screen.findByRole("button", { name: "Back to Holiday" }));

      await fireEvent.click(await screen.findByRole("button", { name: "Back to Albums" }));

      await waitFor(() => expect(calls("get_album_catalogue")).toBe(2));
    });

    it("keeps the cached catalogue on Back when nothing was moved", async () => {
      renderAuthenticatedPage(albumPages(async () => ({ movedAssetIds: [] })));
      await nav("Albums");
      await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));
      await waitFor(() => expect(calls("get_album_timeline")).toBe(1));
      await screen.findAllByRole("button", { name: "Open photo" });

      await fireEvent.click(await screen.findByRole("button", { name: "Back to Albums" }));
      await screen.findByRole("heading", { name: "Albums", level: 1 });

      expect(calls("get_album_catalogue")).toBe(1);
    });
  });
});

describe("Search", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const found = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const results = (...ids: string[]) => ({ buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(found) }, truncated: false });
  const supported = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
  const holiday = { id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null };
  const searchBox = () => screen.findByRole("searchbox", { name: "Search your Immich library" });

  async function search(text: string) {
    await fireEvent.input(await searchBox(), { target: { value: text } });
    await fireEvent.click(screen.getByRole("button", { name: "Search" }));
  }

  it("searches from an open Album Timeline, opens a result in the Lightbox and Back returns to that album", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_album_catalogue: async () => [holiday],
      get_album_timeline: async () => results("album-asset"),
      get_search_support: async () => supported,
      search_assets: async () => results("found-1", "found-2"),
      get_asset_details: async () => ({ originalFileName: "IMG_0001.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null, fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024 }),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Albums" }));
    await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));
    expect(await screen.findByRole("heading", { name: "Holiday" })).toBeInTheDocument();
    // The bar is part of the content area of every page, also while a sub-view is open.
    expect(await searchBox()).toBeInTheDocument();

    await search("IMG_");

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "IMG_", mode: "filename", filters: {} } }));
    expect(tauri.invoke).toHaveBeenCalledWith("get_search_support");
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Search", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Timeline scrubber" })).toBeInTheDocument();
    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(2);
    expect(await searchBox()).toHaveValue("IMG_");

    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to album" })).toBeInTheDocument();
    expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeInTheDocument();
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Close lightbox" }));

    await fireEvent.click(screen.getByRole("button", { name: "Back to Holiday" }));

    expect(await screen.findByRole("heading", { name: "Holiday", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Back to Albums" })).toBeInTheDocument();
    expect(tauri.invoke.mock.calls.filter(([command]) => command === "get_album_timeline")).toHaveLength(1);
    // The text stays in the bar for the session.
    expect(await searchBox()).toHaveValue("IMG_");
  });

  it("returns to the page it searched from at the same scroll position", async () => {
    renderAuthenticatedPage({
      get_album_catalogue: async () => [holiday],
      get_search_support: async () => supported,
      search_assets: async () => results("found-1"),
    });
    await fireEvent.click(await screen.findByRole("button", { name: "Albums" }));
    const album = await screen.findByRole("button", { name: /Holiday/ });
    const main = album.closest("main")!;
    main.scrollTop = 240;

    await search("IMG");
    main.scrollTop = 0;
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Albums" }));

    expect(await screen.findByRole("heading", { name: "Albums", level: 1 })).toBeInTheDocument();
    await waitFor(() => expect(main.scrollTop).toBe(240));
  });

  it("returns to the Photo Timeline at its scroll position", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
      get_timeline_bucket_assets: async () => [found("photo-1")],
      get_search_support: async () => supported,
      search_assets: async () => results("found-1"),
    });
    const viewportOf = async () => (await screen.findByRole("button", { name: "Open photo" })).closest(".overflow-y-auto") as HTMLElement;
    const viewport = await viewportOf();
    viewport.scrollTop = 300;
    await fireEvent.scroll(viewport);

    await search("IMG");
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Photos" }));

    await waitFor(async () => expect((await viewportOf()).scrollTop).toBe(300));
  });

  it("does nothing for an empty text", async () => {
    renderAuthenticatedPage({ get_search_support: async () => supported });

    await search("   ");

    expect(tauri.invoke).not.toHaveBeenCalledWith("get_search_support");
    expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    expect(screen.getByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
  });

  it("clears the Search, empties the bar and leaves the results", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_search_support: async () => supported,
      search_assets: async () => results("found-1"),
    });
    await search("IMG");
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Clear search" }));

    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
    expect(await searchBox()).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
  });

  it("shows an unsupported state naming the required version and leaves other views working", async () => {
    renderAuthenticatedPage({ get_search_support: async () => ({ supported: false, version: "3.1.0", requiredVersion: "3.2" }) });

    await search("IMG");

    expect(await screen.findByRole("alert")).toHaveTextContent("Search needs Immich 3.2 or later, but this server runs 3.1.0.");
    expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    await fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(await screen.findByRole("heading", { name: "Favorites", level: 1 })).toBeInTheDocument();
  });

  it("names the missing asset.read permission and retries the same Search", async () => {
    stubImmediateIntersectionObserver();
    const searchAssets = vi.fn()
      .mockRejectedValueOnce("search_permission_denied:asset.read")
      .mockResolvedValueOnce(results("found-1"));
    renderAuthenticatedPage({ get_search_support: async () => supported, search_assets: searchAssets });

    await search("IMG");

    expect(await screen.findByRole("alert")).toHaveTextContent("Give the API key the asset.read permission in Immich.");
    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(searchAssets).toHaveBeenCalledTimes(2);
    expect(searchAssets.mock.calls[1]).toEqual(searchAssets.mock.calls[0]);
  });

  it("asks to sign in again when the API key is rejected", async () => {
    renderAuthenticatedPage({ get_search_support: async () => supported, search_assets: () => Promise.reject("search_authentication_required") });

    await search("IMG");

    expect(await screen.findByRole("alert")).toHaveTextContent("Immich did not accept the API key. Sign in again to search.");
  });

  it("says the response was unreadable, not that the server is unavailable or offline", async () => {
    stubImmediateIntersectionObserver();
    const searchAssets = vi.fn().mockRejectedValueOnce("search_invalid_response");
    renderAuthenticatedPage({ get_search_support: async () => supported, search_assets: searchAssets });

    await search("IMG");

    expect(await screen.findByRole("alert")).toHaveTextContent("Immich sent a response Lymic could not read.");
    expect(screen.getByRole("button", { name: "Retry timeline connection" })).toBeInTheDocument();

    // Over earlier results it is still an error, not the offline status.
    searchAssets.mockResolvedValueOnce(results("asset-1")).mockRejectedValueOnce("search_invalid_response");
    await fireEvent.click(screen.getByRole("button", { name: "Retry timeline connection" }));
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    await search("second");
    expect(await screen.findByRole("alert")).toHaveTextContent("Immich sent a response Lymic could not read.");
    expect(screen.queryByText("You are offline. The previous results stay visible until you retry.")).not.toBeInTheDocument();
  });

  it("shows an unavailable server with Retry on a first Search", async () => {
    renderAuthenticatedPage({ get_search_support: async () => supported, search_assets: () => Promise.reject("search_unavailable") });

    await search("IMG");

    expect(await screen.findByRole("alert")).toHaveTextContent("The search could not be completed because the Immich Server is unavailable.");
    expect(screen.getByRole("button", { name: "Retry timeline connection" })).toBeInTheDocument();
  });

  it("keeps the previous text and results visible with Retry when a later Search goes offline", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_search_support: async () => supported,
      search_assets: vi.fn().mockResolvedValueOnce(results("found-1")).mockRejectedValueOnce("search_unavailable"),
    });
    await search("first");
    expect(await screen.findByText("March 2025")).toBeInTheDocument();

    await search("second");

    expect(await screen.findByText("You are offline. The previous results stay visible until you retry.")).toBeInTheDocument();
    expect(screen.getByText("March 2025")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry timeline connection" })).toBeInTheDocument();
    expect(await searchBox()).toHaveValue("second");
  });

  it("says nothing matched when the Search finds nothing", async () => {
    renderAuthenticatedPage({
      get_search_support: async () => supported,
      search_assets: async () => ({ buckets: [], assetsByBucket: {}, truncated: false }),
    });

    await search("zzz");

    expect(await screen.findByText("Nothing matched your search. Try a different text or fewer filters.")).toBeInTheDocument();
  });

  it("shows a notice when the Search was cut at the result limit", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      get_search_support: async () => supported,
      search_assets: async () => ({ ...results("found-1"), truncated: true }),
    });

    await search("IMG");

    expect(await screen.findByText("Showing only the newest 5,000 results. Narrow your search to see the rest.")).toBeInTheDocument();
  });

  describe("Search Mode", () => {
    const openOptions = async () => {
      await fireEvent.click(await screen.findByRole("button", { name: "Search options" }));
      await fireEvent.click(await screen.findByRole("button", { name: /^Search mode/ }));
      return screen.findByRole("group", { name: "Search mode" });
    };

    it("offers Context, Filename, Description, Folder Path and OCR as one exclusive choice that starts on Context", async () => {
      renderAuthenticatedPage({ get_search_features: async () => ({ smartSearch: true, ocr: true }) });

      const group = await openOptions();

      const radios = within(group).getAllByRole("radio");
      expect(radios.map((radio) => (radio as HTMLInputElement).labels?.[0]?.textContent?.trim())).toEqual(["Context", "Filename", "Description", "Folder Path", "OCR"]);
      expect(within(group).getByRole("radio", { name: "Context" })).toBeChecked();
      expect(within(group).getByRole("radio", { name: "Context" })).toBeEnabled();
      await fireEvent.click(within(group).getByRole("radio", { name: "Folder Path" }));
      expect(within(group).getByRole("radio", { name: "Folder Path" })).toBeChecked();
      expect(within(group).getByRole("radio", { name: "Context" })).not.toBeChecked();
      expect(within(group).getByRole("radio", { name: "OCR" })).toBeEnabled();
    });

    it("searches in the chosen mode, with the features read before the first Search", async () => {
      renderAuthenticatedPage({
        get_search_features: async () => ({ smartSearch: true, ocr: true }),
        get_search_support: async () => supported,
        search_assets: async () => results("hit-1"),
      });
      const group = await openOptions();

      await fireEvent.click(within(group).getByRole("radio", { name: "Description" }));
      await search("sunset");

      expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "sunset", mode: "description", filters: {} } });
      const calls = tauri.invoke.mock.calls.map(([command]) => command);
      expect(calls.filter((command) => command === "get_search_features")).toHaveLength(1);
      expect(calls.indexOf("get_search_features")).toBeLessThan(calls.indexOf("search_assets"));
      expect(screen.getByRole("searchbox", { name: "Search your Immich library" })).toHaveAttribute("placeholder", "Search by description");
    });

    it("disables OCR with an explanation when the server has OCR switched off, and Filename keeps working", async () => {
      renderAuthenticatedPage({
        get_search_features: async () => ({ smartSearch: true, ocr: false }),
        get_search_support: async () => supported,
        search_assets: async () => results("hit-1"),
      });
      const group = await openOptions();
      await fireEvent.click(within(group).getByRole("radio", { name: "Filename" }));

      const ocr = await within(group).findByRole("radio", { name: "OCR" });
      await waitFor(() => expect(ocr).toBeDisabled());
      expect(ocr).toHaveAccessibleDescription("OCR is switched off on this Immich server, so text inside photos cannot be searched.");
      await search("IMG");
      expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "IMG", mode: "filename", filters: {} } });
    });

    it("keeps Filename working when the server features cannot be read", async () => {
      renderAuthenticatedPage({
        get_search_features: () => Promise.reject("search_unavailable"),
        get_search_support: async () => supported,
        search_assets: async () => results("hit-1"),
      });
      const group = await openOptions();

      expect(await within(group).findByText("The server's features could not be read. OCR may not be available.")).toBeInTheDocument();
      await fireEvent.click(within(group).getByRole("radio", { name: "Filename" }));
      await search("IMG");
      expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "IMG", mode: "filename", filters: {} } });
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });
  });

  describe("Context Search", () => {
    const openOptions = async () => {
      await fireEvent.click(await screen.findByRole("button", { name: "Search options" }));
      await fireEvent.click(await screen.findByRole("button", { name: /^Search mode/ }));
      return screen.findByRole("group", { name: "Search mode" });
    };
    const smartOn = async () => ({ smartSearch: true, ocr: true });
    const page = (ids: string[], nextPage: number | null, limitReached = false) => ({ assets: ids.map(found), nextPage, limitReached });
    const contextRequest = (text: string) => ({ text, mode: "context", filters: {} });
    const requestedPages = () =>
      tauri.invoke.mock.calls.filter(([command]) => command === "search_context_page").map(([, args]) => (args as { page: number }).page);

    it("shows a Context text as a flat relevance-ordered grid that loads further pages while scrolling, with Lightbox actions", async () => {
      stubImmediateIntersectionObserver();
      const searchContextPage = vi.fn()
        .mockResolvedValueOnce(page(["best", "second"], 2))
        .mockResolvedValueOnce(page(["third"], null));
      renderAuthenticatedPage({
        get_search_features: smartOn,
        get_search_support: async () => supported,
        search_context_page: searchContextPage,
        get_asset_details: async () => ({ originalFileName: "IMG_0001.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null, fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 1600, imageHeight: 1200, fileSize: 1024 }),
      });

      await search("dog on a beach");

      await waitFor(() => expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3));
      expect(tauri.invoke).toHaveBeenCalledWith("search_context_page", { request: contextRequest("dog on a beach"), page: 1 });
      expect(tauri.invoke).toHaveBeenCalledWith("search_context_page", { request: contextRequest("dog on a beach"), page: 2 });
      expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
      // A flat list: no Timeline Bucket heading and no Scrubber.
      expect(screen.queryByText("March 2025")).not.toBeInTheDocument();
      expect(screen.queryByRole("navigation", { name: "Timeline scrubber" })).not.toBeInTheDocument();
      expect(screen.getByRole("heading", { name: "Search", level: 1 })).toBeInTheDocument();

      await fireEvent.click(screen.getAllByRole("button", { name: "Open photo" })[2]);
      const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
      expect(within(lightbox).getByRole("button", { name: "Download original" })).toBeInTheDocument();
      expect(within(lightbox).getByRole("button", { name: "Add to album" })).toBeInTheDocument();
      expect(within(lightbox).getByRole("button", { name: "Add to favorites" })).toBeInTheDocument();
    });

    it("keeps the pages already shown when a further page fails and loads it on Retry", async () => {
      stubImmediateIntersectionObserver();
      const searchContextPage = vi.fn()
        .mockResolvedValueOnce(page(["best", "second"], 2))
        .mockRejectedValueOnce("search_unavailable")
        .mockResolvedValueOnce(page(["third"], null));
      renderAuthenticatedPage({ get_search_features: smartOn, get_search_support: async () => supported, search_context_page: searchContextPage });

      await search("dog");

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("More results could not be loaded. The results shown so far are kept.");
      expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(2);
      await fireEvent.click(within(alert).getByRole("button", { name: "Retry" }));

      await waitFor(() => expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(3));
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(requestedPages()).toEqual([1, 2, 2]);
    });

    it("shows Context unavailable when the server rejects the search, without changing the Search Mode", async () => {
      stubImmediateIntersectionObserver();
      renderAuthenticatedPage({
        get_search_features: smartOn,
        get_search_support: async () => supported,
        search_context_page: () => Promise.reject("search_context_unavailable"),
      });
      const group = await openOptions();

      await search("dog");

      expect(await screen.findByRole("alert")).toHaveTextContent("Context is unavailable");
      expect(within(group).getByRole("radio", { name: "Context" })).toBeChecked();
      expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    });

    it("shows Context's own message when Immich fails the smart search with a server error, without changing the Search Mode", async () => {
      stubImmediateIntersectionObserver();
      renderAuthenticatedPage({
        get_search_features: smartOn,
        get_search_support: async () => supported,
        search_context_page: () => Promise.reject("search_context_service_unavailable"),
      });
      const group = await openOptions();

      await search("dog");

      expect(await screen.findByRole("alert")).toHaveTextContent("Context needs Immich's machine-learning service, which could not be reached.");
      expect(within(group).getByRole("radio", { name: "Context" })).toBeChecked();
      expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    });

    it("tells the user that a Context Search reached its limit of the best 1,000 results", async () => {
      stubImmediateIntersectionObserver();
      renderAuthenticatedPage({
        get_search_features: smartOn,
        get_search_support: async () => supported,
        search_context_page: async () => page(["best"], null, true),
      });

      await search("dog");

      expect(await screen.findByText("Showing only the best 1,000 results. Narrow your search to see more.")).toBeInTheDocument();
    });

    it("shows no limit notice for a Context Search that ended on its own", async () => {
      stubImmediateIntersectionObserver();
      renderAuthenticatedPage({
        get_search_features: smartOn,
        get_search_support: async () => supported,
        search_context_page: async () => page(["best"], null),
      });

      await search("dog");

      await waitFor(() => expect(screen.getAllByRole("button", { name: "Open photo" })).toHaveLength(1));
      expect(screen.queryByText(/Showing only the best/)).not.toBeInTheDocument();
    });

    it("disables Context with an explanation and searches by Filename when Smart Search is switched off", async () => {
      stubImmediateIntersectionObserver();
      renderAuthenticatedPage({
        get_search_features: async () => ({ smartSearch: false, ocr: true }),
        get_search_support: async () => supported,
        search_assets: async () => results("hit-1"),
      });
      const group = await openOptions();

      const context = await within(group).findByRole("radio", { name: "Context" });
      await waitFor(() => expect(context).toBeDisabled());
      expect(context).toHaveAccessibleDescription(/Smart Search is switched off on this Immich server/);
      expect(within(group).getByRole("radio", { name: "Filename" })).toBeChecked();
      await search("IMG");

      expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "IMG", mode: "filename", filters: {} } });
      expect(tauri.invoke).not.toHaveBeenCalledWith("search_context_page", expect.anything());
    });
  });
});

describe("Search filter panel", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const found = (id: string) => ({
    id, width: 1600, height: 1200, type: "VIDEO", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: "0:00:05.000000", isFavorite: false,
  });
  const results = (...ids: string[]) => ({ buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(found) }, truncated: false });
  const supported = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
  const features = { smartSearch: true, ocr: true };
  const searchBox = () => screen.findByRole("searchbox", { name: "Search your Immich library" });
  const filtersButton = () => screen.findByRole("button", { name: "Search options" });
  const submit = () => fireEvent.click(screen.getByRole("button", { name: "Search" }));

  async function openPanel() {
    const button = await filtersButton();
    if (button.getAttribute("aria-expanded") !== "true") await fireEvent.click(button);
  }
  async function openEditor(name: string) {
    const chip = await within(document.getElementById("search-panel")!).findByRole("button", { name: new RegExp(`^${name}`) });
    if (chip.getAttribute("aria-expanded") !== "true") await fireEvent.click(chip);
  }
  const setDate = (label: "From" | "To", value: string) => fireEvent.input(screen.getByLabelText(label), { target: { value } });

  it("searches the chosen Tags from the hierarchy in the panel, all of them, without their children", async () => {
    stubImmediateIntersectionObserver();
    const libraryTags = [
      { id: "t-travel", name: "Travel", value: "Travel", parentId: null, color: null },
      { id: "t-italy", name: "Italy", value: "Travel/Italy", parentId: "t-travel", color: null },
      { id: "t-family", name: "Family", value: "Family", parentId: null, color: null },
    ];
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features, get_tags: async () => libraryTags, search_assets: async () => results("tagged-1") });
    await openPanel();
    await openEditor("Tags");

    await fireEvent.click(await screen.findByRole("treeitem", { name: "Travel" }));
    await fireEvent.click(screen.getByRole("treeitem", { name: "Family" }));
    await submit();

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "", mode: "context", filters: { tagIds: ["t-travel", "t-family"] } } }));
  });

  it("disables only the Tag filter and names tag.read when the key lacks it", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features, get_tags: async () => { throw "tags_permission_denied:tag.read"; }, search_context_page: async () => ({ assets: [found("found-1")], nextPage: null, limitReached: false }) });
    await openPanel();
    await openEditor("Tags");

    expect(await screen.findByText(/tag\.read/)).toBeInTheDocument();
    await fireEvent.input(await searchBox(), { target: { value: "IMG" } });
    await submit();

    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("search_context_page", { request: { text: "IMG", mode: "context", filters: {} }, page: 1 }));
  });

  it("opens the panel from the bar and runs a Search with filters and no text, shown chronologically", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features, search_assets: async () => results("video-1") });
    expect(await filtersButton()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("group", { name: "Date" })).not.toBeInTheDocument();

    await openPanel();
    expect(await filtersButton()).toHaveAttribute("aria-expanded", "true");
    await openEditor("Media type");
    await fireEvent.click(screen.getByRole("radio", { name: "Videos" }));
    await openEditor("Date");
    await setDate("From", "2025-03-01");
    await setDate("To", "2025-03-31");
    await submit();

    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("search_assets", {
      request: {
        text: "",
        mode: "context",
        filters: {
          mediaType: "video",
          date: { from: new Date(2025, 2, 1, 0, 0, 0, 0).toISOString(), to: new Date(2025, 2, 31, 23, 59, 59, 999).toISOString() },
        },
      },
    });
    expect(screen.getByRole("heading", { name: "Search", level: 1 })).toBeInTheDocument();
  });

  it("offers All, Images and Videos with All selected by default", async () => {
    renderAuthenticatedPage({ get_search_features: async () => features });
    await openPanel();
    await openEditor("Media type");

    const group = screen.getByRole("radiogroup", { name: "Media type" });
    expect(within(group).getAllByRole("radio").map((radio) => (radio as HTMLInputElement).labels?.[0].textContent?.trim())).toEqual(["All", "Images", "Videos"]);
    expect(screen.getByRole("radio", { name: "All" })).toBeChecked();

    await fireEvent.click(screen.getByRole("radio", { name: "Images" }));
    expect(screen.getByRole("radio", { name: "Images" })).toBeChecked();
    await fireEvent.click(screen.getByRole("radio", { name: "All" }));
    expect(screen.getByRole("radio", { name: "All" })).toBeChecked();
  });

  it("rejects a start date after the end date with a message and does not search", async () => {
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features });
    await openPanel();
    await openEditor("Date");

    await setDate("From", "2025-03-31");
    await setDate("To", "2025-03-01");
    await submit();

    expect(await screen.findByText("The start date is after the end date. Choose a start date on or before the end date.")).toBeInTheDocument();
    expect(screen.getByLabelText("From")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("From")).toHaveValue("2025-03-31");
    expect(screen.getByLabelText("To")).toHaveValue("2025-03-01");
    expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();

    await setDate("From", "2025-03-01");
    expect(screen.queryByText(/The start date is after the end date/)).not.toBeInTheDocument();
  });

  it("opens the closed panel to show the message when Enter is pressed with a start date after the end date", async () => {
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features });
    await openPanel();
    await openEditor("Date");
    await setDate("From", "2025-03-31");
    await setDate("To", "2025-03-01");
    await fireEvent.click(await filtersButton());
    expect(screen.queryByRole("group", { name: "Date" })).not.toBeInTheDocument();

    await fireEvent.submit(screen.getByRole("search", { name: "Search your Immich library" }));

    expect(await screen.findByText("The start date is after the end date. Choose a start date on or before the end date.")).toBeInTheDocument();
    expect(await filtersButton()).toHaveAttribute("aria-expanded", "true");
    expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    expect(screen.getByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
  });

  it("clears the date range on its own and keeps the other filters", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features, search_assets: async () => results("video-1") });
    await openPanel();
    await openEditor("Media type");
    await fireEvent.click(screen.getByRole("radio", { name: "Videos" }));
    await openEditor("Date");
    await setDate("From", "2025-03-01");

    await fireEvent.click(screen.getByRole("button", { name: "Clear date range" }));

    expect(screen.getByLabelText("From")).toHaveValue("");
    expect(screen.queryByRole("button", { name: "Clear date range" })).not.toBeInTheDocument();
    await openEditor("Media type");
    expect(screen.getByRole("radio", { name: "Videos" })).toBeChecked();
    await submit();
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "", mode: "context", filters: { mediaType: "video" } } }));
  });

  it("keeps the filters while the Search is left and entered again, and resets text and filters on clear", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({ get_search_support: async () => supported, get_search_features: async () => features, search_assets: async () => results("video-1") });
    await openPanel();
    await openEditor("Media type");
    await fireEvent.click(screen.getByRole("radio", { name: "Videos" }));
    await openEditor("Date");
    await setDate("To", "2025-03-31");
    await fireEvent.input(await searchBox(), { target: { value: "IMG" } });
    await submit();
    await fireEvent.click(await screen.findByRole("button", { name: "Back to Photos" }));

    expect(await searchBox()).toHaveValue("IMG");
    await openPanel();
    await openEditor("Media type");
    expect(screen.getByRole("radio", { name: "Videos" })).toBeChecked();
    await openEditor("Date");
    expect(screen.getByLabelText("To")).toHaveValue("2025-03-31");

    await fireEvent.click(screen.getByRole("button", { name: "Clear search" }));

    expect(await searchBox()).toHaveValue("");
    await openPanel();
    await openEditor("Media type");
    expect(screen.getByRole("radio", { name: "All" })).toBeChecked();
    await openEditor("Date");
    expect(screen.getByLabelText("To")).toHaveValue("");
  });

  it("offers the clear control as soon as a filter is set, without any text", async () => {
    renderAuthenticatedPage({ get_search_features: async () => features });
    await openPanel();
    await openEditor("Media type");
    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole("radio", { name: "Images" }));

    expect(screen.getByRole("button", { name: "Clear search" })).toBeInTheDocument();
  });

  it("closes the open editor with the first Escape and the panel with the second, returning focus each time", async () => {
    renderAuthenticatedPage({ get_search_features: async () => features });
    await openPanel();
    await openEditor("Media type");

    await fireEvent.keyDown(screen.getByRole("radiogroup", { name: "Media type" }), { key: "Escape" });

    expect(screen.queryByRole("radiogroup", { name: "Media type" })).not.toBeInTheDocument();
    expect(await filtersButton()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: /^Media type/ })).toHaveFocus();

    await fireEvent.keyDown(screen.getByRole("button", { name: /^Media type/ }), { key: "Escape" });

    expect(await filtersButton()).toHaveAttribute("aria-expanded", "false");
    expect(await filtersButton()).toHaveFocus();
  });
});

describe("Search display options and Rating", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const found = (id: string) => ({
    id, width: 1600, height: 1200, type: "IMAGE", takenAt: "2025-03-05T12:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false,
  });
  const results = (...ids: string[]) => ({ buckets: [{ timeBucket: march, count: ids.length }], assetsByBucket: { [march]: ids.map(found) }, truncated: false });
  const supported = { supported: true, version: "3.4.1", requiredVersion: "3.2" };
  const features = { smartSearch: true, ocr: true };
  const searchBox = () => screen.findByRole("searchbox", { name: "Search your Immich library" });
  const submit = () => fireEvent.click(screen.getByRole("button", { name: "Search" }));
  const handlers = { get_search_support: async () => supported, get_search_features: async () => features, search_assets: async () => results("photo-1") };
  const display = () => screen.getByRole("group", { name: "Display" });

  async function openPanel() {
    const button = await screen.findByRole("button", { name: "Search options" });
    if (button.getAttribute("aria-expanded") !== "true") await fireEvent.click(button);
  }
  async function openEditor(name: string) {
    const chip = await within(document.getElementById("search-panel")!).findByRole("button", { name: new RegExp(`^${name}`) });
    if (chip.getAttribute("aria-expanded") !== "true") await fireEvent.click(chip);
  }

  it("titles the group Display and offers Favorite, Archived and Unfiled, all off by default", async () => {
    renderAuthenticatedPage(handlers);
    await openPanel();
    await openEditor("Display");

    const group = display();
    expect(within(group).getAllByRole("checkbox").map((box) => (box as HTMLInputElement).labels?.[0].textContent?.trim())).toEqual(["Favorites", "Archived", "Unfiled"]);
    for (const box of within(group).getAllByRole("checkbox")) expect(box).not.toBeChecked();
    expect(screen.queryByText("Library")).not.toBeInTheDocument();
  });

  it("restricts the Search to Favorites, Archived and Unfiled Assets and sends nothing for options that are off", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers);
    await openPanel();
    await openEditor("Display");
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Favorites" }));
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Archived" }));
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Unfiled" }));
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Archived" }));
    await submit();

    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "", mode: "context", filters: { favorite: true, unfiled: true } } });
  });

  it("selects one Rating level, replaces it with another, and clears it by selecting it again", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers);
    await openPanel();
    await openEditor("Rating");
    const group = screen.getByRole("group", { name: "Rating" });
    expect(within(group).getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["1 star", "2 stars", "3 stars", "4 stars", "5 stars"]);
    for (const button of within(group).getAllByRole("button")) expect(button).toHaveAttribute("aria-pressed", "false");

    await fireEvent.click(within(group).getByRole("button", { name: "4 stars" }));
    expect(within(group).getByRole("button", { name: "4 stars" })).toHaveAttribute("aria-pressed", "true");
    await fireEvent.click(within(group).getByRole("button", { name: "2 stars" }));
    expect(within(group).getByRole("button", { name: "4 stars" })).toHaveAttribute("aria-pressed", "false");
    expect(within(group).getByRole("button", { name: "2 stars" })).toHaveAttribute("aria-pressed", "true");
    await submit();
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("search_assets", { request: { text: "", mode: "context", filters: { rating: 2 } } }));

    // Submitting closes the panel; reopened, it still shows the Rating.
    await openPanel();
    await openEditor("Rating");
    const reopened = screen.getByRole("group", { name: "Rating" });
    expect(within(reopened).getByRole("button", { name: "2 stars" })).toHaveAttribute("aria-pressed", "true");
    await fireEvent.click(within(reopened).getByRole("button", { name: "2 stars" }));
    expect(within(reopened).getByRole("button", { name: "2 stars" })).toHaveAttribute("aria-pressed", "false");
  });

  it("opens the Archive from the Archived option like its navigation entry, with no Back origin, and keeps the Search session", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      ...handlers,
      "get_filtered_timeline_buckets:archive": async () => [{ timeBucket: march, count: 1 }],
      "get_filtered_timeline_bucket_assets:archive": async () => [found("archived-1")],
    });
    await fireEvent.input(await searchBox(), { target: { value: "IMG" } });
    await openPanel();
    await openEditor("Display");
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Archived" }));
    await openEditor("Rating");
    await fireEvent.click(screen.getByRole("group", { name: "Rating" }).querySelector("button[aria-label='3 stars']")!);
    await openEditor("Display");

    await fireEvent.click(within(display()).getByRole("button", { name: "Open Archive" }));

    expect(await screen.findByRole("heading", { name: "Archive", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("March 2025")).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_filtered_timeline_buckets", { timeline: "archive" });
    expect(screen.queryByRole("button", { name: /^Back to/ })).not.toBeInTheDocument();
    expect(tauri.invoke).not.toHaveBeenCalledWith("search_assets", expect.anything());
    expect(screen.getByRole("button", { name: "Archive" })).toHaveClass("active");

    expect(await searchBox()).toHaveValue("IMG");
    await openPanel();
    await openEditor("Display");
    expect(within(display()).getByRole("checkbox", { name: "Archived" })).toBeChecked();
    await openEditor("Rating");
    expect(screen.getByRole("button", { name: "3 stars" })).toHaveAttribute("aria-pressed", "true");
  });

  it("offers Open Archive whether or not the Archived option is on", async () => {
    renderAuthenticatedPage(handlers);
    await openPanel();
    await openEditor("Display");

    expect(within(display()).getByRole("button", { name: "Open Archive" })).toBeInTheDocument();
    await fireEvent.click(within(display()).getByRole("checkbox", { name: "Archived" }));
    expect(within(display()).getByRole("button", { name: "Open Archive" })).toBeInTheDocument();
  });
});

describe("Lightbox Tag chips", () => {
  const march = "2025-03-01T00:00:00.000Z";
  const photo = (id: string) => ({ id, width: 640, height: 480, type: "IMAGE", takenAt: "2025-03-02T00:00:00.000Z", cameraMake: null, cameraModel: null, fileSize: null, duration: null, isFavorite: false });
  const timelineOf = (id: string) => ({ buckets: [{ timeBucket: march, count: 1 }], assetsByBucket: { [march]: [photo(id)] } });
  const rome = { id: "t-rome", name: "Rome", value: "Travel/Italy/Rome", color: "#336699" };
  const food = { id: "t-food", name: "Food", value: "Food" };
  const detailsWith = (tags?: unknown[]) => ({
    originalFileName: "IMG_0001.jpg", isFavorite: false, cameraMake: null, cameraModel: null, city: null, state: null, country: null,
    fNumber: null, exposureTime: null, iso: null, focalLength: null, lensModel: null, imageWidth: 640, imageHeight: 480, fileSize: 1024,
    ...(tags ? { tags } : {}),
  });
  const handlers = (tags?: unknown[]) => ({
    get_asset_details: async () => detailsWith(tags),
    get_tags: async () => [{ id: "t-rome", name: "Rome", value: "Travel/Italy/Rome", parentId: null, color: null }],
    get_tag_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
    get_tag_timeline_bucket_assets: async () => [photo("tagged-1")],
    get_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
    get_timeline_bucket_assets: async () => [photo("photo-1")],
    "get_filtered_timeline_buckets:favorites": async () => [{ timeBucket: march, count: 1 }],
    "get_filtered_timeline_bucket_assets:favorites": async () => [photo("favorite-1")],
    get_album_catalogue: async () => [{ id: "album-1", albumName: "Holiday", albumThumbnailAssetId: null, assetCount: 1, isShared: false, order: null }],
    get_album_timeline: async () => timelineOf("album-asset"),
    get_people: async () => ({ people: [{ id: "person-1", name: "Ada", thumbnailPath: null }], hasNextPage: false }),
    get_person_timeline_buckets: async () => [{ timeBucket: march, count: 1 }],
    get_person_timeline_bucket_assets: async () => [photo("person-asset")],
    get_places: async () => [{ city: "Paris", thumbnailAssetId: "place-asset" }],
    get_place_timeline: async () => timelineOf("place-asset"),
    get_memories: async () => [{ id: "m-1", kind: "onThisDay", memoryAt: "2019-01-02T00:00:00.000Z", year: 2019, personName: null, coverAssetId: "memory-asset", timeline: timelineOf("memory-asset") }],
  });
  const openLightbox = async () => {
    await fireEvent.click((await screen.findAllByRole("button", { name: "Open photo" }))[0]);
    const lightbox = await screen.findByRole("dialog", { name: "Media lightbox" });
    await fireEvent.click(within(lightbox).getByRole("button", { name: "Info" }));
    return lightbox;
  };
  const clickChip = async (lightbox: HTMLElement, path = "Travel/Italy/Rome") =>
    fireEvent.click(await within(lightbox).findByRole("button", { name: `Show photos tagged ${path}` }));
  const nav = async (name: string) => fireEvent.click(await screen.findByRole("button", { name }));

  const origins: Array<[string, string, () => Promise<void>]> = [
    ["Photos", "Photos", async () => {}],
    ["Favorites", "Favorites", async () => { await nav("Favorites"); }],
    ["an Album", "Holiday", async () => { await nav("Albums"); await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ })); }],
    ["a Person", "Ada", async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Ada" })); }],
    ["a Place", "Paris", async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Paris" })); }],
    ["a Memory", "Jan 2, 2019", async () => { await nav("Explore"); await fireEvent.click(await screen.findByRole("button", { name: "Jan 2, 2019" })); }],
  ];

  it.each(origins)("opens the Tag Timeline from a chip in %s, closes the Lightbox and Back returns there", async (_name, backTo, openOrigin) => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers([rome, food]));
    await openOrigin();
    const lightbox = await openLightbox();

    await clickChip(lightbox);

    expect(await screen.findByRole("heading", { name: "Travel/Italy/Rome", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_buckets", { tagId: "t-rome" });

    await fireEvent.click(screen.getByRole("button", { name: `Back to ${backTo}` }));

    expect(await screen.findByRole("heading", { name: backTo, level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Travel/Italy/Rome" })).not.toBeInTheDocument();
  });

  it("shows each Tag as a read-only chip with its name, its full path for assistive technology and its colour", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers([rome, food]));
    const lightbox = await openLightbox();

    const chip = await within(lightbox).findByRole("button", { name: "Show photos tagged Travel/Italy/Rome" });
    expect(chip).toHaveTextContent("Rome");
    expect(chip).toHaveAttribute("title", "Travel/Italy/Rome");
    expect(chip.querySelector("[data-testid='tag-color-dot']")).toHaveStyle({ backgroundColor: "#336699" });
    const second = within(lightbox).getByRole("button", { name: "Show photos tagged Food" });
    expect(second.querySelector("[data-testid='tag-color-dot']")).toBeNull();
    expect(within(lightbox).getByRole("heading", { name: "Tags" })).toBeInTheDocument();
    // Read-only: nothing offers to add, rename or remove a Tag.
    expect(within(lightbox).queryByRole("button", { name: /add tag|remove tag|rename/i })).not.toBeInTheDocument();
  });

  it.each([
    ["the server does not report Tags", undefined],
    ["the asset has no Tags", []],
  ])("shows no chips and no error when %s", async (_name, tags) => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers(tags as unknown[] | undefined));
    const lightbox = await openLightbox();

    await within(lightbox).findByText("IMG_0001.jpg");
    expect(within(lightbox).queryByRole("heading", { name: "Tags" })).not.toBeInTheDocument();
    expect(within(lightbox).queryByRole("button", { name: /Show photos tagged/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("opens the Tag Timeline from the Lightbox of a Tag Timeline and Back returns to the origin of the chain, not to the first Tag", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers([rome, { id: "t-italy", name: "Italy", value: "Travel/Italy" }]));
    const viewportOf = async () => (await screen.findByRole("button", { name: "Open photo" })).closest(".overflow-y-auto") as HTMLElement;
    const viewport = await viewportOf();
    viewport.scrollTop = 300;
    await fireEvent.scroll(viewport);
    await clickChip(await openLightbox());
    await screen.findByRole("heading", { name: "Travel/Italy/Rome", level: 1 });
    const tagViewport = await viewportOf();
    tagViewport.scrollTop = 50;
    await fireEvent.scroll(tagViewport);

    await clickChip(await openLightbox(), "Travel/Italy");

    expect(await screen.findByRole("heading", { name: "Travel/Italy", level: 1 })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("get_tag_timeline_buckets", { tagId: "t-italy" });
    expect(screen.queryByRole("dialog", { name: "Media lightbox" })).not.toBeInTheDocument();
    // The new Tag Timeline starts at its top, not at the previous Tag's scroll position.
    await waitFor(async () => expect((await viewportOf()).scrollTop).toBe(0));
    expect(screen.getByRole("button", { name: "Back to Photos" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Back to Photos" }));

    expect(await screen.findByRole("heading", { name: "Photos", level: 1 })).toBeInTheDocument();
    await waitFor(async () => expect((await viewportOf()).scrollTop).toBe(300));
    expect(screen.queryByRole("button", { name: /Back to Travel/ })).not.toBeInTheDocument();
  });

  it("restores the scroll position of a sub-view the chip was selected in", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage(handlers([rome]));
    await nav("Albums");
    await fireEvent.click(await screen.findByRole("button", { name: /Holiday/ }));
    const viewportOf = async () => (await screen.findByRole("button", { name: "Open photo" })).closest(".overflow-y-auto") as HTMLElement;
    const viewport = await viewportOf();
    viewport.scrollTop = 220;
    await fireEvent.scroll(viewport);

    await clickChip(await openLightbox());
    await screen.findByRole("heading", { name: "Travel/Italy/Rome", level: 1 });
    await fireEvent.click(screen.getByRole("button", { name: "Back to Holiday" }));

    await waitFor(async () => expect((await viewportOf()).scrollTop).toBe(220));
    expect(screen.getByRole("button", { name: "Back to Albums" })).toBeInTheDocument();
  });

  it("opens the Tag Timeline from a chip in the Lightbox of Search results and Back returns to the Search", async () => {
    stubImmediateIntersectionObserver();
    renderAuthenticatedPage({
      ...handlers([rome]),
      get_search_support: async () => ({ supported: true, version: "3.4.1", requiredVersion: "3.2" }),
      search_assets: async () => ({ ...timelineOf("found-1"), truncated: false }),
    });
    await fireEvent.input(await screen.findByRole("searchbox", { name: "Search your Immich library" }), { target: { value: "IMG" } });
    await fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByRole("heading", { name: "Search", level: 1 });

    await clickChip(await openLightbox());

    expect(await screen.findByRole("heading", { name: "Travel/Italy/Rome", level: 1 })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Back to Search" }));
    expect(await screen.findByRole("heading", { name: "Search", level: 1 })).toBeInTheDocument();
    expect(await screen.findAllByRole("button", { name: "Open photo" })).toHaveLength(1);
  });
});
