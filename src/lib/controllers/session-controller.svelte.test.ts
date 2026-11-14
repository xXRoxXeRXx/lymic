import { describe, expect, it, vi } from "vitest";
import { createSyncController } from "./sync-controller.svelte";
import { createSessionController } from "./session-controller.svelte";
import type { AuthDesktopAdapter } from "../tauri/desktop-adapter";

function setup(overrides: Partial<AuthDesktopAdapter> = {}) {
  const adapter: AuthDesktopAdapter = {
    getAuthStatus: vi.fn().mockResolvedValue(false), getServerUrl: vi.fn().mockResolvedValue("https://example.com"),
    getCurrentUser: vi.fn().mockResolvedValue({ displayName: "Alice", profileImage: null }),
    login: vi.fn().mockResolvedValue(undefined), logout: vi.fn().mockResolvedValue(undefined),
    confirm: vi.fn().mockResolvedValue(true), ...overrides,
  };
  const clear = vi.fn();
  const refresh = vi.fn().mockResolvedValue(undefined);
  const session = createSessionController({ adapter, formatError: String, reportError: vi.fn(), t: (key) => key, state: [{ clear }], refresh: [refresh], resetView: vi.fn() });
  session.serverUrl = "https://example.com";
  session.apiKey = "key";
  return { session, clear, refresh };
}

describe("authenticated session", () => {
  it("clears and refreshes state on each sign-in and clears it on sign-out", async () => {
    const { session, clear, refresh } = setup();
    await session.login();
    expect(clear).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledTimes(1);
    session.apiKey = "other key";
    await session.login();
    expect(clear).toHaveBeenCalledTimes(2);
    expect(refresh).toHaveBeenCalledTimes(2);
    await session.logout();
    expect(clear).toHaveBeenCalledTimes(3);
    expect(session.currentUserName).toBe("");
  });

  it("does not restore a previous profile when its request completes after logout", async () => {
    let resolveUser!: (value: { displayName: string; profileImage: null }) => void;
    const { session } = setup({ getCurrentUser: () => new Promise((resolve) => { resolveUser = resolve; }) });
    const login = session.login();
    await vi.waitFor(() => expect(resolveUser).toBeDefined());
    await session.logout();
    resolveUser({ displayName: "Alice", profileImage: null });
    await login;
    expect(session.isAuthenticated).toBe(false);
    expect(session.currentUserName).toBe("");
  });
  it("discards pending synchronization data after the session ends", async () => {
    let resolveFailures!: (value: { localPath: string; failureReason: string }[]) => void;
    const sync = createSyncController({
      adapter: {
        getFailedSyncs: () => new Promise((resolve) => { resolveFailures = resolve; }),
        getSyncStatus: vi.fn(), startSync: vi.fn(), pauseSync: vi.fn(), resumeSync: vi.fn(), retryFailedSyncs: vi.fn(),
      },
      reportError: vi.fn(), locale: () => "en",
    });
    const adapter: AuthDesktopAdapter = {
      getAuthStatus: vi.fn(), getServerUrl: vi.fn(), getCurrentUser: vi.fn().mockResolvedValue({ displayName: "Alice", profileImage: null }),
      login: vi.fn(), logout: vi.fn(), confirm: vi.fn().mockResolvedValue(true),
    };
    const session = createSessionController({ adapter, formatError: String, reportError: vi.fn(), t: (key) => key,
      state: [{ clear: sync.clearUserState }], refresh: [sync.refreshFailedSyncs], resetView: vi.fn() });
    session.serverUrl = "https://example.com";
    session.apiKey = "key";
    const login = session.login();
    await vi.waitFor(() => expect(resolveFailures).toBeDefined());
    await session.logout();
    resolveFailures([{ localPath: "old.jpg", failureReason: "failed" }]);
    await login;
    expect(sync.status.failedSyncs).toEqual([]);
    expect(sync.status.syncStatus).toBe("idle");
  });

  it("clears state when hydration discovers the session has ended", async () => {
    const { session, clear } = setup({ getAuthStatus: vi.fn().mockResolvedValue(false) });
    await session.login();
    await session.hydrate();
    expect(session.isAuthenticated).toBe(false);
    expect(session.currentUserName).toBe("");
    expect(clear).toHaveBeenCalledTimes(2);
  });

  it("ignores a previous account profile after signing in to another account", async () => {
    let resolveFirst!: (value: { displayName: string; profileImage: null }) => void;
    const getCurrentUser = vi.fn().mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ displayName: "Bob", profileImage: null });
    const { session } = setup({ getCurrentUser });
    const first = session.login();
    await vi.waitFor(() => expect(resolveFirst).toBeDefined());
    session.apiKey = "other key";
    await session.login();
    resolveFirst({ displayName: "Alice", profileImage: null });
    await first;
    expect(session.currentUserName).toBe("Bob");
  });

});
