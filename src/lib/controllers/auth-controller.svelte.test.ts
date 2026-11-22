import { describe, expect, it, vi } from "vitest";
import type { AuthDesktopAdapter } from "../tauri/desktop-adapter";
import { createAuthController } from "./auth-controller.svelte";

function createDesktopAdapter(overrides: Partial<AuthDesktopAdapter> = {}): AuthDesktopAdapter {
  return {
    getAuthStatus: vi.fn().mockResolvedValue(false),
    getServerUrl: vi.fn(),
    getCurrentUser: vi.fn(),
    login: vi.fn(),
    logout: vi.fn(),
    confirm: vi.fn().mockResolvedValue(true),
    ...overrides,
  };
}

describe("auth controller", () => {

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
    expect(controller.apiKey).toBe("");
    expect(controller.cloudflareClientSecret).toBe("");
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
    adapter.confirm = vi.fn().mockResolvedValue(true);
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

  it("asks only the plain logout question while the Virtual Library is off or unavailable", async () => {
    const adapter = createDesktopAdapter();
    adapter.confirm = vi.fn().mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
      virtualLibraryEnabled: () => false,
    });

    await controller.logout();

    expect(adapter.confirm).toHaveBeenCalledWith("logout_confirmation", { title: "logout", kind: "warning" });
  });

  it("names that the Virtual Library cleanup removes hydrated and pinned originals when logging out while it is on", async () => {
    const adapter = createDesktopAdapter();
    adapter.confirm = vi.fn().mockResolvedValue(true);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
      virtualLibraryEnabled: () => true,
    });

    await controller.logout();

    expect(adapter.confirm).toHaveBeenCalledWith(
      "logout_confirmation\n\nvirtual_library_cleanup_warning",
      { title: "logout", kind: "warning" },
    );
    expect(adapter.logout).toHaveBeenCalledTimes(1);
  });

  it("does not log out or clean up when the confirmation naming the Virtual Library cleanup is declined", async () => {
    const adapter = createDesktopAdapter();
    adapter.confirm = vi.fn().mockResolvedValue(false);
    const controller = createAuthController({
      formatError: String,
      onLogin: vi.fn(),
      onLogout: vi.fn(),
      reportError: vi.fn(),
      t: (key) => key,
      adapter,
      virtualLibraryEnabled: () => true,
    });

    await controller.logout();

    expect(adapter.logout).not.toHaveBeenCalled();
  });

  it("clears profile image and display name on logout", async () => {
    const adapter = createDesktopAdapter({
      getAuthStatus: vi.fn().mockResolvedValue(true), getServerUrl: vi.fn().mockResolvedValue("https://immich.example"),
      getCurrentUser: vi.fn().mockResolvedValue({
          displayName: "Meyer",
          profileImage: "data:image/jpeg;base64,dGVzdA==",
      }),
    });
    adapter.confirm = vi.fn().mockResolvedValue(true);
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

  describe("signing in again after the session expired", () => {
    async function signedInController(overrides: Partial<AuthDesktopAdapter> = {}) {
      const adapter = createDesktopAdapter({
        getAuthStatus: vi.fn().mockResolvedValue(true),
        getServerUrl: vi.fn().mockResolvedValue("https://immich.example"),
        getCurrentUser: vi.fn().mockResolvedValue({ displayName: "User", profileImage: null }),
        login: vi.fn().mockResolvedValue(undefined),
        ...overrides,
      });
      const onLogin = vi.fn();
      const onLogout = vi.fn();
      const controller = createAuthController({ formatError: String, onLogin, onLogout, reportError: vi.fn(), t: (key) => key, adapter });
      await controller.hydrate();
      onLogin.mockClear();
      return { controller, adapter, onLogin, onLogout };
    }

    it("shows the sign-in form for the same server while staying signed in, without logging out", async () => {
      const { controller, adapter, onLogout } = await signedInController();

      controller.beginRelogin();

      expect(controller.isRelogin).toBe(true);
      expect(controller.isAuthenticated).toBe(true);
      expect(controller.serverUrl).toBe("https://immich.example");
      expect(controller.apiKey).toBe("");
      expect(adapter.logout).not.toHaveBeenCalled();
      expect(onLogout).not.toHaveBeenCalled();
    });

    it("ends the sign-in again after a successful login and refreshes what depends on the session", async () => {
      const { controller, adapter, onLogin } = await signedInController();
      controller.beginRelogin();
      controller.apiKey = "new-key";

      await controller.login();

      expect(adapter.login).toHaveBeenCalledWith(expect.objectContaining({ serverUrl: "https://immich.example", apiKey: "new-key" }));
      expect(controller.isRelogin).toBe(false);
      expect(controller.isAuthenticated).toBe(true);
      expect(onLogin).toHaveBeenCalledTimes(1);
    });

    it("stays on the form with the error when the new key is refused", async () => {
      const { controller } = await signedInController({ login: vi.fn().mockRejectedValue(new Error("Invalid key")) });
      controller.beginRelogin();
      controller.apiKey = "bad";

      await controller.login();

      expect(controller.isRelogin).toBe(true);
      expect(controller.loginError).toBe("Error: Invalid key");
    });

    it("can be dismissed without signing in", async () => {
      const { controller } = await signedInController();
      controller.beginRelogin();
      controller.apiKey = "half-typed";

      controller.cancelRelogin();

      expect(controller.isRelogin).toBe(false);
      expect(controller.apiKey).toBe("");
      expect(controller.isAuthenticated).toBe(true);
    });

    it("does nothing when nobody is signed in", () => {
      const controller = createAuthController({ formatError: String, onLogin: vi.fn(), onLogout: vi.fn(), reportError: vi.fn(), t: (key) => key, adapter: createDesktopAdapter() });

      controller.beginRelogin();

      expect(controller.isRelogin).toBe(false);
    });

    it("leaves the form when the user logs out", async () => {
      const { controller } = await signedInController({ logout: vi.fn().mockResolvedValue(undefined) });
      controller.beginRelogin();

      await controller.logout();

      expect(controller.isRelogin).toBe(false);
      expect(controller.isAuthenticated).toBe(false);
    });
  });
});
