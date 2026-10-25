import { describe, expect, it, vi } from "vitest";
import { createPreferencesController } from "./preferences-controller.svelte";
import { localeCatalog } from "../i18n/locale-catalog";
import type { DesktopAdapter } from "../tauri/desktop-adapter";

const tauri = vi.hoisted(() => ({ enable: vi.fn(), disable: vi.fn(), isEnabled: vi.fn() }));
vi.mock("@tauri-apps/plugin-autostart", () => tauri);

function createDesktopAdapter(overrides: Partial<DesktopAdapter> = {}): DesktopAdapter {
  return {
    startSync: vi.fn(), pauseSync: vi.fn(), resumeSync: vi.fn(), getSyncStatus: vi.fn(), getFailedSyncs: vi.fn(), retryFailedSyncs: vi.fn(),
    getUploadParallelism: vi.fn().mockResolvedValue(3), setUploadParallelism: vi.fn(), addFolder: vi.fn(), acceptRemovableMediaFolder: vi.fn(), getFolders: vi.fn(), updateFolderExclusionPatterns: vi.fn(), removeFolder: vi.fn(),
    getAuthStatus: vi.fn(), getServerUrl: vi.fn(), getCurrentUser: vi.fn(), login: vi.fn(), logout: vi.fn(), getAlbums: vi.fn(), createAlbum: vi.fn(),
    getLocaleCatalog: vi.fn().mockResolvedValue(localeCatalog), updateLocale: vi.fn(), getDatabaseRecoveryNotice: vi.fn().mockResolvedValue(null), recordWebviewStartupPhaseProbe: vi.fn(),
    getReleaseChannel: vi.fn().mockResolvedValue("github"),
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
});
