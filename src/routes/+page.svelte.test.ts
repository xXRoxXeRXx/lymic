import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Page from "./+page.svelte";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  isEnabled: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/plugin-autostart", () => ({
  enable: vi.fn(),
  disable: vi.fn(),
  isEnabled: tauri.isEnabled,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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
        return Promise.resolve([]);
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
  tauri.listen.mockResolvedValue(() => {});
  tauri.isEnabled.mockResolvedValue(false);
});

afterEach(() => {
  cleanup();
});

describe("sync dashboard", () => {
  it("returns to idle when an empty sync result is received", async () => {
    renderAuthenticatedPage();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "start_sync") {
        return Promise.resolve({ processed: 0, uploaded: 0, failed: 0 });
      }
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user_name") return Promise.resolve("Meyer");
      if (command === "get_folders") return Promise.resolve([]);
      return Promise.resolve();
    });

    const syncButton = await screen.findByRole("button", { name: "Sync Now" });
    await fireEvent.click(syncButton);

    await waitFor(() => {
      expect(screen.getByText("All up to date")).toBeInTheDocument();
      expect(syncButton).toBeEnabled();
    });
  });
});

describe("logout", () => {
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
      return Promise.resolve();
    });

    await fireEvent.click(logoutButton);

    expect(await screen.findByRole("alert")).toHaveTextContent("Keyring unavailable");
    expect(screen.getByRole("button", { name: "Logout" })).toBeInTheDocument();
    expect(tauri.invoke).toHaveBeenCalledWith("logout");
  });
});

describe("account settings", () => {
  it("shows the authenticated user name", async () => {
    renderAuthenticatedPage();
    await screen.findByText("Dashboard", { selector: "h1" });

    await fireEvent.click(document.querySelector("header button")!);

    expect(await screen.findByText("Logged in as Meyer")).toBeInTheDocument();
  });
});
