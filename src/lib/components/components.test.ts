import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import ActivityLog from "./ActivityLog.svelte";
import DashboardView from "./DashboardView.svelte";
import FailedSyncsModal from "./FailedSyncsModal.svelte";
import FolderList from "./FolderList.svelte";
import LoginView from "./LoginView.svelte";
import SettingsView from "./SettingsView.svelte";
import InfoView from "./InfoView.svelte";
import { localeCatalog } from "../i18n/locale-catalog";
import TargetAlbumModal from "./TargetAlbumModal.svelte";
import { createTargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
import type { Album } from "./types";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: tauri.openUrl }));

beforeEach(() => tauri.invoke.mockResolvedValue([]));
afterEach(cleanup);

function createCatalogue() {
  return createTargetAlbumCatalogue({
    getAlbums: () => tauri.invoke("get_albums") as Promise<Album[]>,
    createAlbum: (name) => tauri.invoke("create_album", { name }) as Promise<Album>,
  });
}

describe("LoginView", () => {
  it("binds fields, shows errors, and disables incomplete login", async () => {
    let serverUrl = "";
    let apiKey = "";
    render(LoginView, { serverUrl, apiKey, allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "Invalid key", onLogin: vi.fn() });
    expect(screen.getByRole("button", { name: "Login" })).toBeDisabled();
    expect(screen.getByText("Invalid key")).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText("Server URL"), { target: { value: "https://immich.example" } });
    expect(screen.getByLabelText("Server URL")).toHaveValue("https://immich.example");
  });

  it("calls login when both fields are valid", async () => {
    const onLogin = vi.fn();
    render(LoginView, { serverUrl: "https://immich.example", apiKey: "key", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin });
    await fireEvent.click(screen.getByRole("button", { name: "Login" }));
    expect(onLogin).toHaveBeenCalledOnce();
  });

  it("toggles API key and Cloudflare secret visibility", async () => {
    render(LoginView, {
      serverUrl: "https://immich.example",
      apiKey: "secret-key",
      allowLocalHttp: false,
      cloudflareClientId: "cf-id",
      cloudflareClientSecret: "cf-secret",
      isLoggingIn: false,
      loginError: "",
      onLogin: vi.fn(),
    });

    const apiKeyInput = screen.getByLabelText("API Key");
    expect(apiKeyInput).toHaveAttribute("type", "password");
    await fireEvent.click(screen.getByRole("button", { name: "Show API key" }));
    expect(apiKeyInput).toHaveAttribute("type", "text");
    await fireEvent.click(screen.getByRole("button", { name: "Hide API key" }));
    expect(apiKeyInput).toHaveAttribute("type", "password");

    await fireEvent.click(screen.getByText("Advanced login options"));
    const secretInput = screen.getByLabelText("Cloudflare Access client secret");
    expect(secretInput).toHaveAttribute("type", "password");
    await fireEvent.click(screen.getByRole("button", { name: "Show secret" }));
    expect(secretInput).toHaveAttribute("type", "text");
    await fireEvent.click(screen.getByRole("button", { name: "Hide secret" }));
    expect(secretInput).toHaveAttribute("type", "password");
  });

  it("opens API key help and closes it with the close button", async () => {
    render(LoginView, { serverUrl: "https://immich.example", apiKey: "key", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin: vi.fn() });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "API key help" }));
    expect(screen.getByRole("heading", { name: "Create an Immich API key" })).toBeInTheDocument();
    expect(screen.getByText("In Immich, create an API key under Account Settings -> API Keys.")).toBeInTheDocument();
    expect(screen.getByText("asset.upload")).toBeInTheDocument();
    expect(screen.getByText("asset.read")).toBeInTheDocument();
    expect(screen.getByText("album.read")).toBeInTheDocument();
    expect(screen.getByText("album.create")).toBeInTheDocument();
    expect(screen.getByText("albumAsset.create")).toBeInTheDocument();
    expect(screen.getByText("user.read")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Close API key help" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByLabelText("API Key")).toHaveValue("key");
  });

  it("closes API key help with Escape", async () => {
    render(LoginView, { serverUrl: "https://immich.example", apiKey: "key", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin: vi.fn() });
    const helpButton = screen.getByRole("button", { name: "API key help" });
    await fireEvent.click(helpButton);
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(helpButton).toHaveFocus();
  });

  it("closes API key help when its backdrop is clicked", async () => {
    render(LoginView, { serverUrl: "https://immich.example", apiKey: "key", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin: vi.fn() });
    await fireEvent.click(screen.getByRole("button", { name: "API key help" }));
    await fireEvent.click(screen.getByRole("presentation"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps Cloudflare Access credentials inside a closed and expandable disclosure", async () => {
    render(LoginView, {
      serverUrl: "https://immich.example",
      apiKey: "key",
      allowLocalHttp: false,
      cloudflareClientId: "",
      cloudflareClientSecret: "",
      isLoggingIn: false,
      loginError: "",
      onLogin: vi.fn(),
    });
    const disclosure = screen.getByText("Advanced login options").closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByLabelText("Cloudflare Access client ID")).not.toBeVisible();
    await fireEvent.click(screen.getByText("Advanced login options"));
    expect(disclosure).toHaveAttribute("open");
    expect(screen.getByLabelText("Cloudflare Access client ID")).toBeVisible();
    expect(screen.getByLabelText("Cloudflare Access client ID")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Cloudflare Access client secret")).toHaveAttribute("type", "password");
    expect(screen.getByRole("heading", { name: "Local HTTP" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Cloudflare Access" })).toBeVisible();
    expect(screen.getByLabelText("Allow HTTP for local server")).not.toBeChecked();
    expect(screen.getByText("HTTP exposes your API key. It works only with localhost and loopback addresses.")).toBeVisible();
    expect(screen.getByLabelText("Allow HTTP for local server")).toHaveAttribute("aria-describedby", "allow-local-http-description");
  });

  it("expands Cloudflare Access fields when pair validation fails", () => {
    render(LoginView, {
      serverUrl: "https://immich.example",
      apiKey: "key",
      allowLocalHttp: false,
      cloudflareClientId: "",
      cloudflareClientSecret: "",
      isLoggingIn: false,
      loginError: "Provide both Cloudflare Access client ID and client secret.",
      loginErrorCode: "CLOUDFLARE_ACCESS_PAIR_REQUIRED",
      onLogin: vi.fn(),
    });

    expect(screen.getByText("Advanced login options").closest("details")).toHaveAttribute("open");
    expect(screen.getByLabelText("Cloudflare Access client secret")).toBeVisible();
  });

  it("expands the advanced options when local HTTP is rejected", () => {
    render(LoginView, {
      serverUrl: "http://immich.example",
      apiKey: "key",
      allowLocalHttp: false,
      cloudflareClientId: "",
      cloudflareClientSecret: "",
      isLoggingIn: false,
      loginError: "HTTP is allowed only for localhost and loopback addresses. All other servers require HTTPS.",
      loginErrorCode: "LOCAL_HTTP_NOT_ALLOWED",
      onLogin: vi.fn(),
    });

    expect(screen.getByText("Advanced login options").closest("details")).toHaveAttribute("open");
    expect(screen.getByLabelText("Allow HTTP for local server")).toBeVisible();
  });
});

describe("DashboardView", () => {
  const props = {
    serverUrl: "https://immich.example",
    catalogue: createCatalogue(),
    dashboard: {
      syncStatus: "idle" as const, lastSync: "10:00", progress: 0, processed: 0, syncTotal: 0, currentFile: "",
      transferredBytes: 0, transferRateBytesPerSecond: null, estimatedSecondsRemaining: null,
      watchedFolders: [], failedSyncs: [], logs: [], isSyncActionPending: false,
      isTargetAlbumModalOpen: false, pendingFolderPath: null,
      onSyncAction: vi.fn(), onRetryFailedSyncs: vi.fn(), onAddFolder: vi.fn(), onRemoveFolder: vi.fn(), onUpdateFolderExclusionPatterns: vi.fn(),
      onConfirmAddFolder: vi.fn(), onCancelAddFolder: vi.fn(),
    },
  };

  it("disables sync without folders", () => {
    render(DashboardView, props);
    expect(screen.getByRole("button", { name: "Sync Now" })).toBeDisabled();
    expect(screen.getByText("No folders added yet.")).toBeInTheDocument();
  });

  it("shows progress and forwards sync actions with folders", async () => {
    const onSyncAction = vi.fn();
    render(DashboardView, {
      ...props,
      dashboard: {
        ...props.dashboard,
        syncStatus: "syncing",
        progress: 45,
        processed: 9,
        syncTotal: 20,
        currentFile: "photo.jpg",
        watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }],
        onSyncAction,
      },
    });
    expect(screen.getByText("45% completed")).toBeInTheDocument();
    expect(screen.getByText("Processed: 9 / 20")).toBeInTheDocument();
    expect(screen.getByText("photo.jpg")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(onSyncAction).toHaveBeenCalledOnce();
  });

  it("renders available transfer statistics and hides unavailable rate and ETA", () => {
    render(DashboardView, {
      ...props,
      dashboard: {
        ...props.dashboard,
        syncStatus: "syncing",
        watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }],
        transferredBytes: 1_024,
      },
    });
    expect(screen.getByText("Transferred: 1 KiB")).toBeInTheDocument();
    expect(screen.queryByText(/Rate:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Time remaining:/)).not.toBeInTheDocument();
  });
});

describe("FailedSyncsModal", () => {
  it("toggles details and retries failures", async () => {
    const onRetry = vi.fn();
    render(FailedSyncsModal, { failedSyncs: [{ localPath: "C:/failed.jpg", failureReason: "Rejected" }], syncStatus: "error", onRetry });
    await fireEvent.click(screen.getByRole("button", { name: "1 failed file(s)" }));
    expect(screen.getByText("C:/failed.jpg")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Retry failed files" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});

describe("FolderList and ActivityLog", () => {
  it("renders data and forwards folder callbacks", async () => {
    const onAddFolder = vi.fn();
    const onRemoveFolder = vi.fn();
    tauri.invoke.mockResolvedValue([{ id: "album-1", albumName: "Summer holidays" }]);
    render(FolderList, { watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: "album-1" }], onAddFolder, onRemoveFolder, onUpdateFolderExclusionPatterns: vi.fn(), catalogue: createCatalogue() });
    expect(await screen.findByText("Target Album: Summer holidays")).toBeInTheDocument();
    await fireEvent.click(screen.getAllByRole("button")[0]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove folder: C:/photos" }));
    expect(onAddFolder).toHaveBeenCalledOnce();
    expect(onRemoveFolder).toHaveBeenCalledWith(1);
  });

  it("edits exclusion patterns as a validated list", async () => {
    const onUpdateFolderExclusionPatterns = vi.fn();
    render(FolderList, {
      watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }],
      onAddFolder: vi.fn(), onRemoveFolder: vi.fn(), onUpdateFolderExclusionPatterns, catalogue: createCatalogue(),
    });

    await fireEvent.click(screen.getByText("Exclusions"));
    const input = screen.getByPlaceholderText("e.g. **/exports/** or *.mp4");
    await fireEvent.input(input, { target: { value: "*.{jpg,png}" } });
    await fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Use a relative pattern");

    await fireEvent.input(input, { target: { value: "*.mp4" } });
    await fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByText("*.mp4")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Save exclusions" }));
    expect(onUpdateFolderExclusionPatterns).toHaveBeenCalledWith(1, ["*.mp4"]);
  });

  it("loads the name when a folder is assigned an album created after mount", async () => {
    tauri.invoke
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([{ id: "new-album", albumName: "Roadtrip" }]);
    const { rerender } = render(FolderList, {
      watchedFolders: [],
      onAddFolder: vi.fn(),
      onRemoveFolder: vi.fn(),
      onUpdateFolderExclusionPatterns: vi.fn(),
      catalogue: createCatalogue(),
    });

    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("get_albums"));
    rerender({
      watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: "new-album" }],
      onAddFolder: vi.fn(),
      onRemoveFolder: vi.fn(),
    });

    expect(await screen.findByText("Target Album: Roadtrip")).toBeInTheDocument();
  });

  it("renders the empty folder state", () => {
    render(FolderList, { watchedFolders: [], onAddFolder: vi.fn(), onRemoveFolder: vi.fn(), onUpdateFolderExclusionPatterns: vi.fn(), catalogue: createCatalogue() });
    expect(screen.getByText("No folders added yet.")).toBeInTheDocument();
  });

  it("renders empty and populated logs", () => {
    const { rerender } = render(ActivityLog, { logs: [] });
    expect(screen.getByText("No recent activity.")).toBeInTheDocument();
    rerender({ logs: [{ id: 1, time: "10:00", level: "info", message: "Synced" }] });
    expect(screen.getByText("Synced")).toBeInTheDocument();
  });

  it("translates keyed log entries across locales", async () => {
    const { locale } = await import("svelte-i18n");
    try {
      await locale.set("de");
      const { rerender } = render(ActivityLog, {
        logs: [
          { id: 1, time: "10:00", level: "info", message: "Starting manual synchronization...", key: "log_starting_sync" },
          { id: 2, time: "10:01", level: "info", message: "Scanning folder: D:/Fotos", key: "log_scanning_folder", params: { folder: "D:/Fotos" } },
          { id: 3, time: "10:02", level: "info", message: "Sync queued, waiting for running synchronization.", key: "log_sync_queued_waiting" },
          { id: 4, time: "10:03", level: "warn", message: "Database notice", key: "log_database_repaired_notice", params: { reason: "corruption", backup: "C:/backup.db", count: "3" } },
          { id: 5, time: "10:04", level: "error", message: "File scan failed", key: "log_file_scan_failed", params: { file: "photo.jpg", error: "I/O error" } },
        ],
      });
      expect(screen.getByRole("heading", { name: "Aktivitätsverlauf" })).toBeInTheDocument();
      expect(screen.getByText("Manuelle Synchronisierung wird gestartet...")).toBeInTheDocument();
      expect(screen.getByText("Ordner wird durchsucht: D:/Fotos")).toBeInTheDocument();
      expect(screen.getByText("Synchronisierung eingereiht, warte auf laufenden Vorgang.")).toBeInTheDocument();
      expect(screen.getByText("Datenbank konnte nicht verwendet werden (corruption). Sie wurde in 'C:/backup.db' gesichert und neu erstellt; 3 Ordner wiederhergestellt. Andere App-Daten konnten nicht wiederhergestellt werden.")).toBeInTheDocument();
      expect(screen.getByText("Dateiüberprüfung für 'photo.jpg' fehlgeschlagen: I/O error")).toBeInTheDocument();

      await locale.set("en");
      rerender({
        logs: [
          { id: 1, time: "10:00", level: "info", message: "Starting manual synchronization...", key: "log_starting_sync" },
          { id: 2, time: "10:01", level: "info", message: "Scanning folder: D:/Fotos", key: "log_scanning_folder", params: { folder: "D:/Fotos" } },
          { id: 3, time: "10:02", level: "info", message: "Sync queued, waiting for running synchronization.", key: "log_sync_queued_waiting" },
          { id: 4, time: "10:03", level: "warn", message: "Database notice", key: "log_database_repaired_notice", params: { reason: "corruption", backup: "C:/backup.db", count: "3" } },
          { id: 5, time: "10:04", level: "error", message: "File scan failed", key: "log_file_scan_failed", params: { file: "photo.jpg", error: "I/O error" } },
        ],
      });
      expect(screen.getByRole("heading", { name: "Activity Log" })).toBeInTheDocument();
      expect(screen.getByText("Starting manual synchronization...")).toBeInTheDocument();
      expect(screen.getByText("Scanning folder: D:/Fotos")).toBeInTheDocument();
      expect(screen.getByText("Sync queued, waiting for running synchronization.")).toBeInTheDocument();
      expect(screen.getByText("Database could not be used (corruption). It was backed up to 'C:/backup.db' and rebuilt; restored 3 watched folder(s). Other stored app data could not be restored.")).toBeInTheDocument();
      expect(screen.getByText("File scan failed for 'photo.jpg': I/O error")).toBeInTheDocument();
    } finally {
      await locale.set("en");
    }
  });
});

describe("SettingsView", () => {
  it("forwards settings callbacks and disables saves in progress", async () => {
    const onLocaleChange = vi.fn();
    const onThemeChange = vi.fn();
    const onToggleAutostart = vi.fn();
    const onUploadParallelismChange = vi.fn();
    const onCheckForUpdates = vi.fn();
    const onLogout = vi.fn();
    const { rerender } = render(SettingsView, { currentUserName: "Meyer", serverUrl: "https://immich.example", isAutostartEnabled: false, themePreference: "system", uploadParallelism: 3, isSavingUploadParallelism: true, updateStatus: "unknown", isCheckingForUpdates: false, locales: localeCatalog, onToggleAutostart, onLocaleChange, onThemeChange, onUploadParallelismChange, onCheckForUpdates, onLogout });
    expect(screen.getByLabelText("Parallel uploads")).toBeDisabled();
    rerender({ currentUserName: "Meyer", serverUrl: "https://immich.example", isAutostartEnabled: false, themePreference: "system", uploadParallelism: 3, isSavingUploadParallelism: false, updateStatus: "current", isCheckingForUpdates: false, onToggleAutostart, onLocaleChange, onThemeChange, onUploadParallelismChange, onCheckForUpdates, onLogout });
    await fireEvent.click(screen.getByRole("button", { name: "Enable autostart" }));
    await fireEvent.change(screen.getByLabelText("Language"), { target: { value: "de" } });
    await fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    await fireEvent.change(screen.getByLabelText("Parallel uploads"), { target: { value: "5" } });
    await fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await fireEvent.click(screen.getByRole("button", { name: "Logout" }));
    expect(onToggleAutostart).toHaveBeenCalledOnce();
    expect(onLocaleChange).toHaveBeenCalledWith("de");
    expect(onThemeChange).toHaveBeenCalledWith("dark");
    expect(onUploadParallelismChange).toHaveBeenCalledOnce();
    expect(onCheckForUpdates).toHaveBeenCalledOnce();
    expect(onLogout).toHaveBeenCalledOnce();
  });
});

describe("TargetAlbumModal", () => {
  it("renders with 'No album' selected by default and fetches existing albums", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_albums") {
        return Promise.resolve([
          { id: "1", albumName: "Vacation" },
          { id: "2", albumName: "Family" },
        ]);
      }
      return Promise.resolve();
    });

    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(TargetAlbumModal, {
      isOpen: true,
      folderPath: "C:/Users/test/Pictures/Holiday",
      onConfirm,
      onCancel,
      catalogue: createCatalogue(),
    });

    expect(screen.getByRole("heading", { name: "Select Target Album" })).toBeInTheDocument();
    expect(screen.getByText('Select an optional Immich album for "Holiday"')).toBeInTheDocument();
    expect(screen.getByText("No album (Default)")).toBeInTheDocument();

    expect(await screen.findByText("Family")).toBeInTheDocument();
    expect(screen.getByText("Vacation")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("selects an existing album and confirms", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_albums") {
        return Promise.resolve([
          { id: "alb-1", albumName: "Summer 2026" },
        ]);
      }
      return Promise.resolve();
    });

    const onConfirm = vi.fn();
    render(TargetAlbumModal, {
      isOpen: true,
      folderPath: "D:/Photos",
      onConfirm,
      onCancel: vi.fn(),
      catalogue: createCatalogue(),
    });

    const albumBtn = await screen.findByText("Summer 2026");
    await fireEvent.click(albumBtn);
    await fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
    expect(onConfirm).toHaveBeenCalledWith("alb-1");
  });

  it("validates album creation rejecting empty or whitespace names", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_albums") return Promise.resolve([]);
      return Promise.resolve();
    });

    render(TargetAlbumModal, {
      isOpen: true,
      folderPath: "D:/Photos",
      onConfirm: vi.fn(),
      onCancel: vi.fn(),
      catalogue: createCatalogue(),
    });

    await fireEvent.click(await screen.findByRole("button", { name: "Create new album" }));
    const input = screen.getByPlaceholderText("Album name...");
    const createBtn = screen.getByRole("button", { name: "Create" });

    await fireEvent.input(input, { target: { value: "   " } });
    await fireEvent.click(createBtn);
    expect(screen.getByText("Album name cannot be empty")).toBeInTheDocument();

    tauri.invoke.mockImplementation((command: string, args?: { name?: string }) => {
      if (command === "get_albums") {
        return Promise.resolve([{ id: "new-alb", albumName: "Roadtrip" }]);
      }
      if (command === "create_album") {
        return Promise.resolve({ id: "new-alb", albumName: args?.name ?? "" });
      }
      return Promise.resolve();
    });

    await fireEvent.input(input, { target: { value: "Roadtrip" } });
    await fireEvent.click(createBtn);
    expect(tauri.invoke).toHaveBeenCalledWith("create_album", { name: "Roadtrip" });
    expect(await screen.findByText("Roadtrip")).toBeInTheDocument();
  });

  it("handles loading failure with retry and proceed without album", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_albums") return Promise.reject(new Error("Network offline"));
      return Promise.resolve();
    });

    const onConfirm = vi.fn();
    render(TargetAlbumModal, {
      isOpen: true,
      folderPath: "D:/Photos",
      onConfirm,
      onCancel: vi.fn(),
      catalogue: createCatalogue(),
    });

    expect(await screen.findByText("Failed to load albums")).toBeInTheDocument();
    expect(screen.getByText("Network offline")).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: "Proceed without album" }));
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("aborts folder addition on cancel or Escape", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_albums") return Promise.resolve([]);
      return Promise.resolve();
    });

    const onCancel = vi.fn();
    render(TargetAlbumModal, {
      isOpen: true,
      folderPath: "D:/Photos",
      onConfirm: vi.fn(),
      onCancel,
      catalogue: createCatalogue(),
    });

    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(2);
  });
});

describe("InfoView", () => {
  it("renders app name, version, license, and opens links via openUrl", async () => {
    tauri.openUrl.mockResolvedValue(undefined);
    render(InfoView, { appVersion: "1.0.0" });
    expect(screen.getByText("Lymic (Unofficial Immich Desktop Client)")).toBeInTheDocument();
    expect(screen.getByText("v1.0.0")).toBeInTheDocument();
    expect(screen.getByText("MIT")).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "https://github.com/xXRoxXeRXx/lymic/" });
    expect(link).toHaveAttribute("href", "https://github.com/xXRoxXeRXx/lymic/");
    await fireEvent.click(link);
    expect(tauri.openUrl).toHaveBeenCalledWith("https://github.com/xXRoxXeRXx/lymic/");

    const issueLink = screen.getByRole("link", { name: "https://github.com/xXRoxXeRXx/lymic/issues" });
    expect(issueLink).toHaveAttribute("href", "https://github.com/xXRoxXeRXx/lymic/issues");
    await fireEvent.click(issueLink);
    expect(tauri.openUrl).toHaveBeenCalledWith("https://github.com/xXRoxXeRXx/lymic/issues");
  });

  it("handles missing version gracefully", () => {
    render(InfoView, { appVersion: null });
    expect(screen.getByText("-")).toBeInTheDocument();
  });
});
