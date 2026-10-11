import { describe, expect, it, vi } from "vitest";
import { createUpdateController } from "./update-controller.svelte";

const tauri = vi.hoisted(() => ({ getVersion: vi.fn(), invoke: vi.fn(), check: vi.fn(), relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: tauri.getVersion }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: tauri.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: tauri.relaunch }));

describe("update controller", () => {
  it("does not install an available update while sync is active", async () => {
    const downloadAndInstall = vi.fn();
    tauri.check.mockResolvedValue({ version: "1.0.0", body: "", downloadAndInstall });
    const controller = createUpdateController(() => true);
    controller.startLifecycle();

    await controller.checkForUpdates();
    await controller.installUpdate();

    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(tauri.relaunch).not.toHaveBeenCalled();
  });
});
