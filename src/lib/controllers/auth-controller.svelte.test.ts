import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthDesktopAdapter } from "../tauri/desktop-adapter";
import { createAuthController } from "./auth-controller.svelte";

const tauri = vi.hoisted(() => ({ confirm: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: tauri.confirm }));

function createDesktopAdapter(overrides: Partial<AuthDesktopAdapter> = {}): AuthDesktopAdapter {
  return {
    getAuthStatus: vi.fn().mockResolvedValue(false),
    getServerUrl: vi.fn(),
    getCurrentUser: vi.fn(),
    login: vi.fn(),
    logout: vi.fn(),
    ...overrides,
  };
}

describe("auth controller", () => {
  beforeEach(() => {
    tauri.confirm.mockReset();
  });

  it("retains successful authentication when the optional account lookup fails", async () => {
    const adapter = createDesktopAdapter({ getCurrentUser: vi.fn().mockRejectedValue(new Error("Profile unavailable")) });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";

    await controller.login();

    expect(controller.isAuthenticated).toBe(true);
    expect(controller.loginError).toBe("");
  });

  it("forwards a complete Cloudflare Access pair to Tauri", async () => {
    const adapter = createDesktopAdapter({ getCurrentUser: vi.fn().mockResolvedValue({ displayName: "User", profileImage: null }) });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";
    controller.cloudflareClientId = "client-id";
    controller.cloudflareClientSecret = "client-secret";

    await controller.login();

    expect(adapter.login).toHaveBeenCalledWith({
      serverUrl: "https://immich.example",
      apiKey: "token",
      allowLocalHttp: false,
      cloudflareClientId: "client-id",
      cloudflareClientSecret: "client-secret",
    });
  });

  it("rejects an incomplete Cloudflare Access pair before invoking Tauri", async () => {
    const adapter = createDesktopAdapter();
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";
    controller.cloudflareClientId = "client-id";

    await controller.login();

    expect(controller.loginError).toBe("cloudflare_access_pair_required");
    expect(controller.loginErrorCode).toBe("CLOUDFLARE_ACCESS_PAIR_REQUIRED");
    expect(adapter.login).not.toHaveBeenCalled();
  });

  it("treats whitespace-only Cloudflare Access credentials as disabled", async () => {
    const adapter = createDesktopAdapter({ getCurrentUser: vi.fn().mockResolvedValue({ displayName: "User", profileImage: null }) });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";
    controller.cloudflareClientId = " \t ";
    controller.cloudflareClientSecret = "\n";

    await controller.login();

    expect(adapter.login).toHaveBeenCalledWith({
      serverUrl: "https://immich.example",
      apiKey: "token",
      allowLocalHttp: false,
      cloudflareClientId: null,
      cloudflareClientSecret: null,
    });
  });

  it("forwards the local HTTP opt-in to Tauri and resets it on logout", async () => {
    const adapter = createDesktopAdapter({ getCurrentUser: vi.fn().mockResolvedValue({ displayName: "User", profileImage: null }) });
    tauri.confirm.mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "http://localhost";
    controller.apiKey = "token";
    controller.allowLocalHttp = true;

    await controller.login();

    expect(adapter.login).toHaveBeenCalledWith(expect.objectContaining({
      allowLocalHttp: true,
    }));
    await controller.logout();
    expect(controller.allowLocalHttp).toBe(false);
  });

  it("localizes the backend local HTTP validation error", async () => {
    const adapter = createDesktopAdapter({ login: vi.fn().mockRejectedValue("LOCAL_HTTP_NOT_ALLOWED: HTTP is allowed only for localhost") });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "http://immich.example";
    controller.apiKey = "token";

    await controller.login();

    expect(controller.loginError).toBe("local_http_not_allowed");
    expect(controller.loginErrorCode).toBe("LOCAL_HTTP_NOT_ALLOWED");
  });

  it("hydrates current user profile image and display name when authenticated", async () => {
    const adapter = createDesktopAdapter({
      getAuthStatus: vi.fn().mockResolvedValue(true), getServerUrl: vi.fn().mockResolvedValue("https://immich.example"),
      getCurrentUser: vi.fn().mockResolvedValue({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,dGVzdA==",
      }),
    });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });

    await controller.hydrate();

    expect(controller.isAuthenticated).toBe(true);
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBe("data:image/jpeg;base64,dGVzdA==");
  });

  it("refreshes profile image and display name after login", async () => {
    const adapter = createDesktopAdapter({ getCurrentUser: vi.fn().mockResolvedValue({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,bmV3SW1hZ2U=",
    }) });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });
    controller.serverUrl = "https://immich.example";
    controller.apiKey = "token";

    await controller.login();

    expect(controller.isAuthenticated).toBe(true);
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBe("data:image/jpeg;base64,bmV3SW1hZ2U=");
  });

  it("clears profile image and display name on logout", async () => {
    const adapter = createDesktopAdapter({
      getAuthStatus: vi.fn().mockResolvedValue(true), getServerUrl: vi.fn().mockResolvedValue("https://immich.example"),
      getCurrentUser: vi.fn().mockResolvedValue({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,dGVzdA==",
      }),
    });
    tauri.confirm.mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });

    await controller.hydrate();
    expect(controller.profileImage).toBe("data:image/jpeg;base64,dGVzdA==");

    await controller.logout();
    expect(controller.isAuthenticated).toBe(false);
    expect(controller.currentUserName).toBe("");
    expect(controller.profileImage).toBeNull();
  });

  it("retains null profileImage when user has no profile image", async () => {
    const adapter = createDesktopAdapter({
      getAuthStatus: vi.fn().mockResolvedValue(true), getServerUrl: vi.fn().mockResolvedValue("https://immich.example"),
      getCurrentUser: vi.fn().mockResolvedValue({
          displayName: "Meyer",
          profileImage: null,
      }),
    });
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
    });

    await controller.hydrate();
    expect(controller.isAuthenticated).toBe(true);
    expect(controller.currentUserName).toBe("Meyer");
    expect(controller.profileImage).toBeNull();
  });
});
