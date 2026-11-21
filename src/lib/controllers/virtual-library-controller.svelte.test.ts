import { describe, expect, it, vi } from "vitest";
import { createVirtualLibraryController } from "./virtual-library-controller.svelte";
import type { VirtualLibraryDesktopAdapter } from "../tauri/desktop-adapter";
import type { VirtualLibrarySettings } from "../contracts";

function createDesktopAdapter(overrides: Partial<VirtualLibraryDesktopAdapter> = {}): VirtualLibraryDesktopAdapter {
  return {
    getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: false, enabled: false }),
    setVirtualLibraryEnabled: vi.fn(),
    subscribeToVirtualLibrarySettings: vi.fn().mockResolvedValue(() => {}),
    ...overrides,
  };
}

function createController(adapter: VirtualLibraryDesktopAdapter, reportError = vi.fn()) {
  return { reportError, controller: createVirtualLibraryController({ reportError, adapter }) };
}

describe("virtual library controller", () => {
  it("is unavailable and disabled until the backend reports otherwise", () => {
    const { controller } = createController(createDesktopAdapter());

    expect(controller.isAvailable).toBe(false);
    expect(controller.isEnabled).toBe(false);
  });

  it("stays unavailable when the backend cannot say (unavailable platform, failed or empty response)", async () => {
    const empty = createController(createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue(undefined) }));
    await empty.controller.hydrate();
    expect(empty.controller.isAvailable).toBe(false);

    const failing = createController(createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockRejectedValue(new Error("boom")) }));
    await failing.controller.hydrate();
    expect(failing.controller.isAvailable).toBe(false);
    expect(failing.reportError).toHaveBeenCalledWith(expect.any(Error));
  });

  it("offers the section and the persisted enabled state when the backend reports it available", async () => {
    const adapter = createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: true }) });
    const { controller } = createController(adapter);

    await controller.hydrate();

    expect(controller.isAvailable).toBe(true);
    expect(controller.isEnabled).toBe(true);
  });

  it("toggles the enabled state through the backend and adopts the state it returns", async () => {
    const setVirtualLibraryEnabled = vi.fn().mockResolvedValue({ available: true, enabled: true });
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false }),
      setVirtualLibraryEnabled,
    });
    const { controller } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect(setVirtualLibraryEnabled).toHaveBeenCalledWith(true);
    expect(controller.isEnabled).toBe(true);
    expect(controller.isSaving).toBe(false);
  });

  it("keeps the previous state and reports the error when the backend refuses the change", async () => {
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false }),
      setVirtualLibraryEnabled: vi.fn().mockRejectedValue(new Error("Virtual Library is not available")),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect(controller.isEnabled).toBe(false);
    expect(controller.isSaving).toBe(false);
    expect(reportError).toHaveBeenCalledWith(expect.any(Error));
  });

  it("follows settings changes the backend announces until the lifecycle stops", async () => {
    let announce: (settings: VirtualLibrarySettings) => void = () => {};
    const unlisten = vi.fn();
    const adapter = createDesktopAdapter({
      subscribeToVirtualLibrarySettings: vi.fn().mockImplementation(async (listener) => {
        announce = listener;
        return unlisten;
      }),
    });
    const { controller } = createController(adapter);

    await controller.startLifecycle();
    announce({ available: true, enabled: true });
    expect(controller.isAvailable).toBe(true);
    expect(controller.isEnabled).toBe(true);

    controller.stopLifecycle();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("releases a subscription that resolves after the lifecycle already stopped", async () => {
    const unlisten = vi.fn();
    let resolveSubscription: (unlisten: () => void) => void = () => {};
    const adapter = createDesktopAdapter({
      subscribeToVirtualLibrarySettings: vi.fn().mockReturnValue(new Promise((resolve) => { resolveSubscription = resolve; })),
    });
    const { controller } = createController(adapter);

    const started = controller.startLifecycle();
    controller.stopLifecycle();
    resolveSubscription(unlisten);
    await started;

    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
