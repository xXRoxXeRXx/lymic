import { describe, expect, it, vi } from "vitest";
import { createPreferencesController } from "./preferences-controller.svelte";
import { localeCatalog } from "../i18n/locale-catalog";
import type { PreferencesDesktopAdapter } from "../tauri/desktop-adapter";

function createDesktopAdapter(overrides: Partial<PreferencesDesktopAdapter> = {}): PreferencesDesktopAdapter {
  return {
    getUploadParallelism: vi.fn().mockResolvedValue(3),
    setUploadParallelism: vi.fn(),
    getLocaleCatalog: vi.fn().mockResolvedValue(localeCatalog),
    updateLocale: vi.fn(),
    getDatabaseRecoveryNotice: vi.fn().mockResolvedValue(null),
    isAutostartEnabled: vi.fn().mockResolvedValue(false),
    setAutostartEnabled: vi.fn(),
    ...overrides,
  };
}

describe("preferences controller", () => {
  it("restores the select value when upload parallelism persistence fails", async () => {
    const adapter = createDesktopAdapter({ setUploadParallelism: vi.fn().mockRejectedValue(new Error("Database unavailable")) });
    const reportError = vi.fn();
    const controller = createPreferencesController({ reportError, addLog: vi.fn(), locale: () => "en", setLocale: vi.fn(), adapter });
    const select = document.createElement("select");
    select.innerHTML = '<option value="3">3</option><option value="7">7</option>';
    select.value = "7";

    await controller.changeUploadParallelism({ currentTarget: select } as unknown as Event);

    expect(select.value).toBe("3");
    expect(controller.uploadParallelism).toBe(3);
    expect(controller.isSavingUploadParallelism).toBe(false);
    expect(reportError).toHaveBeenCalledWith(expect.any(Error));
  });

  it("adds structured database recovery notice to activity log", async () => {
    const adapter = createDesktopAdapter({
      getDatabaseRecoveryNotice: vi.fn().mockResolvedValue({
          message: "Database rebuilt",
          key: "log_database_repaired_notice",
          reason: "disk corruption",
          backup: "/backups/db.bak",
          count: 2,
      }),
    });
    const addLog = vi.fn();
    const controller = createPreferencesController({ reportError: vi.fn(), addLog, locale: () => "en", setLocale: vi.fn(), adapter });
    await controller.hydrate();
    expect(controller.locales).toEqual(localeCatalog);
    expect(addLog).toHaveBeenCalledWith(expect.objectContaining({
      level: "WARN",
      key: "log_database_repaired_notice",
      params: {
        reason: "disk corruption",
        backup: "/backups/db.bak",
        count: "2",
      },
    }));
  });

  describe("autostart state", () => {
    const createController = (adapter: PreferencesDesktopAdapter) =>
      createPreferencesController({ reportError: vi.fn(), addLog: vi.fn(), locale: () => "en", setLocale: vi.fn(), adapter });

    it("is unknown until the backend has reported it, so nothing can react to a guessed value", async () => {
      const controller = createController(createDesktopAdapter({ isAutostartEnabled: vi.fn().mockResolvedValue(true) }));
      expect(controller.autostartState).toBeNull();

      await controller.hydrate();

      expect(controller.autostartState).toBe(true);
    });

    it("stays unknown when the backend cannot read it", async () => {
      const controller = createController(createDesktopAdapter({ isAutostartEnabled: vi.fn().mockRejectedValue(new Error("no access")) }));

      await controller.hydrate();

      expect(controller.autostartState).toBeNull();
    });

    it("follows the state the backend reports after a toggle, including a refusal to enable", async () => {
      const isAutostartEnabled = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true).mockResolvedValueOnce(true);
      const setAutostartEnabled = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("blocked in Windows Settings"));
      const controller = createController(createDesktopAdapter({ isAutostartEnabled, setAutostartEnabled }));
      await controller.hydrate();

      await controller.toggleAutostart();
      expect(setAutostartEnabled).toHaveBeenLastCalledWith(true);
      expect(controller.autostartState).toBe(true);

      await controller.toggleAutostart();
      expect(setAutostartEnabled).toHaveBeenLastCalledWith(false);
      expect(controller.autostartState).toBe(true);
    });
  });
});
