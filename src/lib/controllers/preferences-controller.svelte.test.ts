import { describe, expect, it, vi } from "vitest";
import {
  createPreferencesController,
  initializeThemeFromStorage,
} from "./preferences-controller.svelte";
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
  it("initializes a valid stored theme and applies it to the document", () => {
    localStorage.setItem("lymic-theme", "dark");

    expect(initializeThemeFromStorage()).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("clears an invalid stored theme and applies the system theme", () => {
    localStorage.setItem("lymic-theme", "sepia");
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));

    expect(initializeThemeFromStorage()).toBe("system");
    expect(localStorage.getItem("lymic-theme")).toBeNull();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");

    vi.unstubAllGlobals();
  });

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
});
