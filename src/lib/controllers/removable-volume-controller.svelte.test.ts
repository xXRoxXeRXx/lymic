import { describe, it, expect, vi, beforeEach } from "vitest";
import type { RemovableVolumeDesktopAdapter } from "../tauri/desktop-adapter";
import { createRemovableVolumeController } from "./removable-volume-controller.svelte";

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  sendNotification: vi.fn(),
  isPermissionGranted: vi.fn(),
}));

let eventListeners: Map<string, (event: { payload: unknown }) => void>;
let unlistenFn: () => void;

function createDesktopAdapter(): RemovableVolumeDesktopAdapter {
  return {
    subscribeToRemovableVolumeDetected: (listener) => {
      eventListeners.set("removable-volume-detected", (event) => listener(event.payload as { volumeId: string; mountPath: string; displayName: string }));
      return Promise.resolve(unlistenFn);
    },
    subscribeToRemovableVolumeRemoved: (listener) => {
      eventListeners.set("removable-volume-removed", (event) => listener(event.payload as string));
      return Promise.resolve(unlistenFn);
    },
    subscribeToRemovableVolumeRemounted: (listener) => {
      eventListeners.set("removable-volume-remounted", () => listener());
      return Promise.resolve(unlistenFn);
    },
    isDesktopNotificationPermissionGranted: tauri.isPermissionGranted,
    sendDesktopNotification: tauri.sendNotification,
  };
}

describe("removable volume controller", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventListeners = new Map();
    unlistenFn = vi.fn();
    tauri.isPermissionGranted.mockResolvedValue(true);
  });

  it("clears detected and dismissed volumes on logout", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();
    const card = { volumeId: "vol-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" };
    const handler = eventListeners.get("removable-volume-detected")!;
    handler({ payload: card });
    controller.dismiss(card.volumeId);

    controller.clear();
    expect(controller.detectedVolumes).toEqual([]);
    handler({ payload: card });

    expect(controller.detectedVolumes).toEqual([card]);
  });

  it("queues detected volumes in FIFO order and exposes the current prompt", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();

    const card1 = { volumeId: "vol-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" };
    const card2 = { volumeId: "vol-2", mountPath: "F:\\", displayName: "NIKON_D850" };

    const handler = eventListeners.get("removable-volume-detected");
    expect(handler).toBeDefined();

    handler!({ payload: card1 });
    handler!({ payload: card2 });

    expect(controller.detectedVolumes).toHaveLength(2);
    expect(controller.currentPrompt).toEqual(card1);
  });

  it("dismisses prompt, moves to next in FIFO queue, and ignores repeated events for dismissed volume in session", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();

    const card1 = { volumeId: "vol-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" };
    const card2 = { volumeId: "vol-2", mountPath: "F:\\", displayName: "NIKON_D850" };

    const handler = eventListeners.get("removable-volume-detected")!;
    handler({ payload: card1 });
    handler({ payload: card2 });

    expect(controller.currentPrompt).toEqual(card1);

    controller.dismiss(card1.volumeId);

    expect(controller.currentPrompt).toEqual(card2);
    expect(controller.detectedVolumes).toHaveLength(1);

    // Re-arrival of card1 in same session should be ignored
    handler({ payload: card1 });
    expect(controller.detectedVolumes).toHaveLength(1);
    expect(controller.currentPrompt).toEqual(card2);
  });

  it("sends a desktop notification when a card arrives while app is in background", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => true,
      t: (key, params) => `${key}:${params?.name ?? ""}`,
    });
    await controller.startSubscriptions();

    const card = { volumeId: "vol-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" };
    const handler = eventListeners.get("removable-volume-detected")!;
    handler({ payload: card });

    await vi.waitFor(() => {
      expect(tauri.sendNotification).toHaveBeenCalledWith(
        expect.objectContaining({
          title: expect.any(String),
          body: expect.stringContaining("EOS_DIGITAL"),
        })
      );
    });
  });

  it("clears pending detection prompt when a volume is removed", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();

    const card1 = { volumeId: "vol-1", mountPath: "E:\\", displayName: "EOS_DIGITAL" };
    const card2 = { volumeId: "vol-2", mountPath: "F:\\", displayName: "NIKON_D850" };

    const detectedHandler = eventListeners.get("removable-volume-detected")!;
    detectedHandler({ payload: card1 });
    detectedHandler({ payload: card2 });

    expect(controller.detectedVolumes).toHaveLength(2);
    expect(controller.currentPrompt).toEqual(card1);

    const removedHandler = eventListeners.get("removable-volume-removed");
    expect(removedHandler).toBeDefined();

    removedHandler!({ payload: "vol-1" });

    expect(controller.detectedVolumes).toHaveLength(1);
    expect(controller.currentPrompt).toEqual(card2);
  });

  it("cleans up event subscription on destroy", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();
    controller.destroy();

    expect(unlistenFn).toHaveBeenCalled();
  });

  it("unlistens subscriptions that resolve after destroy", async () => {
    let resolveDetected!: (unlisten: () => void) => void;
    let resolveRemoved!: (unlisten: () => void) => void;
    const detectedUnlisten = vi.fn();
    const removedUnlisten = vi.fn();

    const adapter = createDesktopAdapter();
    adapter.subscribeToRemovableVolumeDetected = () => new Promise<() => void>((resolve) => { resolveDetected = resolve; });
    adapter.subscribeToRemovableVolumeRemoved = () => new Promise<() => void>((resolve) => { resolveRemoved = resolve; });

    const controller = createRemovableVolumeController({
      adapter,
      isAppInBackground: () => false,
      t: (key) => key,
    });
    const starting = controller.startSubscriptions();
    controller.destroy();

    resolveDetected(detectedUnlisten);
    resolveRemoved(removedUnlisten);
    await starting;

    expect(detectedUnlisten).toHaveBeenCalledOnce();
    expect(removedUnlisten).toHaveBeenCalledOnce();
  });

  it("tears down one listener for each removable-volume event after concurrent starts", async () => {
    const controller = createRemovableVolumeController({
      adapter: createDesktopAdapter(),
      isAppInBackground: () => false,
      t: (key) => key,
    });

    await Promise.all([controller.startSubscriptions(), controller.startSubscriptions()]);
    controller.destroy();

    expect(unlistenFn).toHaveBeenCalledTimes(3);
  });
});
