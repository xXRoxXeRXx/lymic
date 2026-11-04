import { describe, expect, it, vi } from "vitest";
import type { UpdateDesktopAdapter } from "../tauri/desktop-adapter";
import { createUpdateController } from "./update-controller.svelte";

function createDesktopAdapter(overrides: Partial<UpdateDesktopAdapter> = {}): UpdateDesktopAdapter {
  return {
    recordWebviewStartupPhaseProbe: vi.fn(),
    getReleaseChannel: vi.fn().mockResolvedValue("github"),
    getAppVersion: vi.fn().mockResolvedValue("1.0.0"),
    checkForUpdate: vi.fn().mockResolvedValue(null),
    downloadAndInstallUpdate: vi.fn(),
    relaunchApp: vi.fn(),
    openExternalUrl: vi.fn(),
    ...overrides,
  };
}

describe("update controller", () => {

  it("records startup phases through the desktop adapter", async () => {
    const adapter = createDesktopAdapter();
    const controller = createUpdateController(() => false, adapter);
    controller.startLifecycle();

    expect(controller.isUpdatesSupported).toBe(true);
    expect(controller.releaseChannel).toBe("github");

    controller.startup();

    await vi.waitFor(() => expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("update_check_complete"));
    expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("version_load_started");
    expect(adapter.recordWebviewStartupPhaseProbe).toHaveBeenCalledWith("version_load_complete");
  });

  it("does not install an available update while sync is active", async () => {
    const adapter = createDesktopAdapter({
      checkForUpdate: vi.fn().mockResolvedValue({ version: "1.0.0", body: "" }),
      downloadAndInstallUpdate: vi.fn(),
    });
    const controller = createUpdateController(() => true, adapter);
    controller.startLifecycle();

    await controller.checkForUpdates();
    await controller.installUpdate();

    expect(adapter.downloadAndInstallUpdate).not.toHaveBeenCalled();
    expect(adapter.relaunchApp).not.toHaveBeenCalled();
  });

  it("in Store channel, does not contact updater endpoint and disables in-app update checks", async () => {
    const adapter = createDesktopAdapter({ getReleaseChannel: vi.fn().mockResolvedValue("store") });
    const controller = createUpdateController(() => false, adapter, "store");
    controller.startLifecycle();

    expect(controller.isUpdatesSupported).toBe(false);
    expect(controller.releaseChannel).toBe("store");

    controller.startup();

    await vi.waitFor(() => expect(controller.appVersion).toBe("1.0.0"));
    expect(adapter.checkForUpdate).not.toHaveBeenCalled();
    expect(adapter.recordWebviewStartupPhaseProbe).not.toHaveBeenCalledWith("update_check_started");
    expect(adapter.recordWebviewStartupPhaseProbe).not.toHaveBeenCalledWith("update_check_complete");

    await controller.checkForUpdates();
    expect(adapter.checkForUpdate).not.toHaveBeenCalled();
    expect(controller.availableUpdate).toBeNull();
    expect(controller.isUpdateDialogOpen).toBe(false);

    await controller.installUpdate();
    expect(adapter.relaunchApp).not.toHaveBeenCalled();
  });
});
