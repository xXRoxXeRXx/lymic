import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  isEnabled: vi.fn(),
  confirm: vi.fn(),
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
let triggerSync: (() => void) | undefined;

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(),
  disable: vi.fn(),
  isEnabled: tauri.isEnabled,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), confirm: tauri.confirm }));

function renderAuthenticatedPage() {
  tauri.invoke.mockImplementation((command: string) => {
    switch (command) {
      case "get_auth_status":
        return Promise.resolve(true);
      case "get_server_url":
        return Promise.resolve("https://immich.example");
      case "get_current_user_name":
        return Promise.resolve("Meyer");
      case "get_folders":
        return Promise.resolve(watchedFolders);
      case "get_failed_syncs":
        return Promise.resolve(failedSyncs);
      case "update_locale":
        return Promise.resolve();
      default:
        return Promise.resolve();
    }
  });
  return render(Page);
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.setItem("lymic-locale", "en");
  watchedFolders = [];
  failedSyncs = [];
  triggerSync = undefined;
  tauri.listen.mockImplementation((event: string, handler: () => void) => {
    if (event === "trigger-sync") triggerSync = handler;
    return Promise.resolve(() => {});
  });
  tauri.isEnabled.mockResolvedValue(false);
  tauri.confirm.mockResolvedValue(true);
});

afterEach(() => {
  cleanup();
});

describe("sync dashboard", () => {
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
      if (command === "get_current_user_name") return Promise.resolve("Meyer");
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
      if (command === "get_current_user_name") return Promise.resolve("Meyer");
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await fireEvent.click(screen.getByRole("button", { name: "Remove folder: C:/photos" }));

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Sync Now" })).toBeDisabled();
    });
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
      if (command === "get_current_user_name") return Promise.resolve("Meyer");
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

  it("keeps error status when failures remain after a clean sync", async () => {
    watchedFolders = [{ id: 1, path: "C:/photos", recursive: true, target_album_id: null }];
    failedSyncs = [{ localPath: "C:/photos/failed.jpg", failureReason: "Upload rejected" }];
    renderAuthenticatedPage();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "start_sync") return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user_name") return Promise.resolve("Meyer");
      if (command === "get_folders") return Promise.resolve(watchedFolders);
      if (command === "get_failed_syncs") return Promise.resolve(failedSyncs);
      return Promise.resolve();
    });

    await screen.findByText("photos");
    await fireEvent.click(await screen.findByRole("button", { name: "Sync Now" }));

    await waitFor(() => expect(screen.getByText("Sync completed with errors")).toBeInTheDocument());
  });
});

describe("logout", () => {
  it("does not log out when the confirmation is cancelled", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Dashboard", { selector: "h1" });

    await fireEvent.click(document.querySelector("header button")!);
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
    await screen.findByText("Dashboard", { selector: "h1" });

    await fireEvent.click(document.querySelector("header button")!);
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
    await screen.findByText("Dashboard", { selector: "h1" });

    await fireEvent.click(document.querySelector("header button")!);

    expect(await screen.findByText("Logged in as Meyer")).toBeInTheDocument();
  });

  it("displays language settings and switches language", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Dashboard", { selector: "h1" });

    await fireEvent.click(document.querySelector("header button")!);

    expect(await screen.findByText("Language")).toBeInTheDocument();
    expect(screen.getByText("Choose your preferred language.")).toBeInTheDocument();

    const deButton = screen.getByRole("button", { name: "DE" });
    await fireEvent.click(deButton);

    await waitFor(() => {
      expect(screen.getByText("Sprache")).toBeInTheDocument();
      expect(screen.getByText("Wähle deine bevorzugte Sprache.")).toBeInTheDocument();
      expect(tauri.invoke).toHaveBeenCalledWith("update_locale", { locale: "de" });
    });
  });
});
