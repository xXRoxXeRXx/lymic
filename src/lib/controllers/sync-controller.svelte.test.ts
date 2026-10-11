import { afterEach, describe, expect, it, vi } from "vitest";
import { createSyncController } from "./sync-controller.svelte";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn(), open: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: tauri.confirm, open: tauri.open }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

afterEach(() => vi.useRealTimers());

describe("sync controller", () => {
  it("clears the progress reset timer on disposal", async () => {
    vi.useFakeTimers();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "get_folders") return Promise.resolve([{ id: 1, path: "C:/photos" }]);
      if (command === "get_sync_status") return Promise.resolve({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null });
      if (command === "start_sync") return Promise.resolve({ processed: 1, uploaded: 1, failed: 0 });
      if (command === "get_failed_syncs") return Promise.resolve([]);
      return Promise.resolve();
    });
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });
    controller.startLifecycle();
    await controller.hydrate();
    await controller.syncAction();

    expect(controller.progress).toBe(100);
    controller.destroy();
    vi.advanceTimersByTime(2000);

    expect(controller.progress).toBe(100);
  });

  it("uses the confirmed cancellation command only when required", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "remove_folder") return Promise.resolve({ requiresSyncCancellation: true });
      if (command === "get_folders" || command === "get_failed_syncs") return Promise.resolve([]);
      return Promise.resolve();
    });
    tauri.confirm.mockResolvedValue(true);
    const controller = createSyncController({ reportError: vi.fn(), t: (key) => key, locale: () => "en" });

    await controller.removeFolder(4);

    expect(tauri.invoke).toHaveBeenCalledWith("remove_folder", { id: 4 });
    expect(tauri.invoke).toHaveBeenCalledWith("remove_folder", { id: 4, confirmCancellation: true });
  });
});
