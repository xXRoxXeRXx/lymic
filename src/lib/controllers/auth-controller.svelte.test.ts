import { describe, expect, it, vi } from "vitest";
import { createAuthController } from "./auth-controller.svelte";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: tauri.confirm }));

describe("auth controller", () => {
  it("retains successful authentication when the optional account lookup fails", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "login") return Promise.resolve();
      if (command === "get_current_user_name") return Promise.reject(new Error("Profile unavailable"));
      return Promise.resolve();
    });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";

    await controller.login();

    expect(controller.isAuthenticated).toBe(true);
    expect(controller.loginError).toBe("");
  });
});
