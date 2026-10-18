import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAuthController } from "./auth-controller.svelte";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: tauri.confirm }));

describe("auth controller", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.confirm.mockReset();
  });

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

  it("forwards a complete Cloudflare Access pair to Tauri", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_current_user_name") return Promise.resolve("User");
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
    controller.cloudflareClientId = "client-id";
    controller.cloudflareClientSecret = "client-secret";

    await controller.login();

    expect(tauri.invoke).toHaveBeenCalledWith("login", {
      serverUrl: "https://immich.example",
      apiKey: "token",
      allowLocalHttp: false,
      cloudflareClientId: "client-id",
      cloudflareClientSecret: "client-secret",
    });
  });

  it("rejects an incomplete Cloudflare Access pair before invoking Tauri", async () => {
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";
    controller.cloudflareClientId = "client-id";

    await controller.login();

    expect(controller.loginError).toBe("cloudflare_access_pair_required");
    expect(controller.loginErrorCode).toBe("CLOUDFLARE_ACCESS_PAIR_REQUIRED");
    expect(tauri.invoke).not.toHaveBeenCalled();
  });

  it("treats whitespace-only Cloudflare Access credentials as disabled", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_current_user_name") return Promise.resolve("User");
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
    controller.cloudflareClientId = " \t ";
    controller.cloudflareClientSecret = "\n";

    await controller.login();

    expect(tauri.invoke).toHaveBeenCalledWith("login", {
      serverUrl: "https://immich.example",
      apiKey: "token",
      allowLocalHttp: false,
      cloudflareClientId: null,
      cloudflareClientSecret: null,
    });
  });

  it("forwards the local HTTP opt-in to Tauri and resets it on logout", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_current_user_name") return Promise.resolve("User");
      return Promise.resolve();
    });
    tauri.confirm.mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });
    controller.serverUrl = "http://localhost";
    controller.apiKey = "token";
    controller.allowLocalHttp = true;

    await controller.login();

    expect(tauri.invoke).toHaveBeenCalledWith("login", expect.objectContaining({
      allowLocalHttp: true,
    }));
    await controller.logout();
    expect(controller.allowLocalHttp).toBe(false);
  });

  it("localizes the backend local HTTP validation error", async () => {
    tauri.invoke.mockRejectedValue("LOCAL_HTTP_NOT_ALLOWED: HTTP is allowed only for localhost");
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });
    controller.serverUrl = "http://immich.example";
    controller.apiKey = "token";

    await controller.login();

    expect(controller.loginError).toBe("local_http_not_allowed");
    expect(controller.loginErrorCode).toBe("LOCAL_HTTP_NOT_ALLOWED");
  });

  it("hydrates current user profile image and display name when authenticated", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") {
        return Promise.resolve({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,dGVzdA==",
        });
      }
      return Promise.resolve();
    });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });

    await controller.hydrate();

    expect(controller.isAuthenticated).toBe(true);
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBe("data:image/jpeg;base64,dGVzdA==");
  });

  it("refreshes profile image and display name after login", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "login") return Promise.resolve();
      if (command === "get_current_user") {
        return Promise.resolve({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,bmV3SW1hZ2U=",
        });
      }
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
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBe("data:image/jpeg;base64,bmV3SW1hZ2U=");
  });

  it("clears profile image and display name on logout", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") {
        return Promise.resolve({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,dGVzdA==",
        });
      }
      if (command === "logout") return Promise.resolve();
      return Promise.resolve();
    });
    tauri.confirm.mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });

    await controller.hydrate();
    expect(controller.profileImage).toBe("data:image/jpeg;base64,dGVzdA==");

    await controller.logout();
    expect(controller.isAuthenticated).toBe(false);
    expect(controller.currentUserName).toBe("");
    expect(controller.profileImage).toBeNull();
  });

  it("retains null profileImage when user has no profile image", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_auth_status") return Promise.resolve(true);
      if (command === "get_server_url") return Promise.resolve("https://immich.example");
      if (command === "get_current_user") {
        return Promise.resolve({
          displayName: "Meyer",
          profileImage: null,
        });
      }
      return Promise.resolve();
    });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
    });

    await controller.hydrate();
    expect(controller.isAuthenticated).toBe(true);
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBeNull();
  });
});

