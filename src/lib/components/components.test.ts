import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import ActivityLog from "./ActivityLog.svelte";
import DashboardView from "./DashboardView.svelte";
import FailedSyncsModal from "./FailedSyncsModal.svelte";
import FolderList from "./FolderList.svelte";
import LoginView from "./LoginView.svelte";
import SettingsView from "./SettingsView.svelte";

afterEach(cleanup);

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

  it("opens API key help and closes it with the close button", async () => {
    render(LoginView, { serverUrl: "https://immich.example", apiKey: "key", allowLocalHttp: false, cloudflareClientId: "", cloudflareClientSecret: "", isLoggingIn: false, loginError: "", onLogin: vi.fn() });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "API key help" }));
    expect(screen.getByRole("heading", { name: "Create an Immich API key" })).toBeInTheDocument();
    expect(screen.getByText("In Immich, create an API key under Account Settings -> API Keys.")).toBeInTheDocument();
    expect(screen.getByText("asset.upload")).toBeInTheDocument();
    expect(screen.getByText("asset.read")).toBeInTheDocument();
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
  const props = { syncStatus: "idle" as const, lastSync: "10:00", progress: 0, processed: 0, syncTotal: 0, currentFile: "", serverUrl: "https://immich.example", watchedFolders: [], failedSyncs: [], logs: [], isSyncActionPending: false, onSyncAction: vi.fn(), onRetryFailedSyncs: vi.fn(), onAddFolder: vi.fn(), onRemoveFolder: vi.fn() };

  it("disables sync without folders", () => {
    render(DashboardView, props);
    expect(screen.getByRole("button", { name: "Sync Now" })).toBeDisabled();
    expect(screen.getByText("No folders added yet.")).toBeInTheDocument();
  });

  it("shows progress and forwards sync actions with folders", async () => {
    const onSyncAction = vi.fn();
    render(DashboardView, {
      ...props,
      syncStatus: "syncing",
      progress: 45,
      processed: 9,
      syncTotal: 20,
      currentFile: "photo.jpg",
      watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }],
      onSyncAction
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
      syncStatus: "syncing",
      watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }],
      transferredBytes: 1_024,
      transferRateBytesPerSecond: null,
      estimatedSecondsRemaining: null,
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
    render(FolderList, { watchedFolders: [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }], onAddFolder, onRemoveFolder });
    await fireEvent.click(screen.getAllByRole("button")[0]);
    await fireEvent.click(screen.getByRole("button", { name: "Remove folder: C:/photos" }));
    expect(onAddFolder).toHaveBeenCalledOnce();
    expect(onRemoveFolder).toHaveBeenCalledWith(1);
  });

  it("renders the empty folder state", () => {
    render(FolderList, { watchedFolders: [], onAddFolder: vi.fn(), onRemoveFolder: vi.fn() });
    expect(screen.getByText("No folders added yet.")).toBeInTheDocument();
  });

  it("renders empty and populated logs", () => {
    const { rerender } = render(ActivityLog, { logs: [] });
    expect(screen.getByText("No recent activity.")).toBeInTheDocument();
    rerender({ logs: [{ id: 1, time: "10:00", level: "info", message: "Synced" }] });
    expect(screen.getByText("Synced")).toBeInTheDocument();
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
    const { rerender } = render(SettingsView, { currentUserName: "Meyer", serverUrl: "https://immich.example", isAutostartEnabled: false, themePreference: "system", uploadParallelism: 3, isSavingUploadParallelism: true, updateStatus: "unknown", isCheckingForUpdates: false, onToggleAutostart, onLocaleChange, onThemeChange, onUploadParallelismChange, onCheckForUpdates, onLogout });
    expect(screen.getByLabelText("Parallel uploads")).toBeDisabled();
    rerender({ currentUserName: "Meyer", serverUrl: "https://immich.example", isAutostartEnabled: false, themePreference: "system", uploadParallelism: 3, isSavingUploadParallelism: false, updateStatus: "current", isCheckingForUpdates: false, onToggleAutostart, onLocaleChange, onThemeChange, onUploadParallelismChange, onCheckForUpdates, onLogout });
    await fireEvent.click(screen.getByRole("button", { name: "Enable autostart" }));
    await fireEvent.click(screen.getByRole("button", { name: "DE" }));
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
