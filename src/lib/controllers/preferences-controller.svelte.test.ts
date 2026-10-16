import { describe, expect, it, vi } from "vitest";
import { createPreferencesController } from "./preferences-controller.svelte";
import { localeCatalog } from "../i18n/locale-catalog";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), enable: vi.fn(), disable: vi.fn(), isEnabled: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-autostart", () => tauri);

describe("preferences controller", () => {
  it("loads and persists the virtual library setting", async () => {
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === "get_locale_catalog") return localeCatalog;
      if (command === "get_upload_parallelism") return 3;
      if (command === "get_virtual_library_status") {
        return { enabled: false, path: "%USERPROFILE%\\Pictures\\Lymic" };
      }
      return null;
    });
    const controller = createPreferencesController({ reportError: vi.fn(), addLog: vi.fn(), locale: () => "en", setLocale: vi.fn() });

    await controller.hydrate();
    await controller.toggleVirtualLibrary();

    expect(controller.virtualLibraryEnabled).toBe(true);
    expect(controller.virtualLibraryPath).toBe("%USERPROFILE%\\Pictures\\Lymic");
    expect(tauri.invoke).toHaveBeenCalledWith("set_virtual_library_enabled", { enabled: true });
  });

  it("restores the select value when upload parallelism persistence fails", async () => {
    tauri.invoke.mockRejectedValue(new Error("Database unavailable"));
    const reportError = vi.fn();
    const controller = createPreferencesController({ reportError, addLog: vi.fn(), locale: () => "en", setLocale: vi.fn() });
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
    tauri.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_database_recovery_notice") {
        return {
          message: "Database rebuilt",
          key: "log_database_repaired_notice",
          reason: "disk corruption",
          backup: "/backups/db.bak",
          count: 2,
        };
      }
      if (cmd === "get_upload_parallelism") return 3;
      if (cmd === "get_locale_catalog") {
        return localeCatalog;
      }
      return null;
    });
    const addLog = vi.fn();
    const controller = createPreferencesController({ reportError: vi.fn(), addLog, locale: () => "en", setLocale: vi.fn() });
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
