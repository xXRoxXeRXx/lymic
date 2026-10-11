import { describe, expect, it, vi } from "vitest";
import { createPreferencesController } from "./preferences-controller.svelte";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), enable: vi.fn(), disable: vi.fn(), isEnabled: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/plugin-autostart", () => tauri);

describe("preferences controller", () => {
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
});
