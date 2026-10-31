import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRemovableVolumeController } from "./removable-volume-controller.svelte";

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  sendNotification: vi.fn(),
  isPermissionGranted: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauri.listen,
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  sendNotification: tauri.sendNotification,
  isPermissionGranted: tauri.isPermissionGranted,
}));

describe("removable volume controller", () => {
  let eventListeners: Map<string, (event: { payload: unknown }) => void>;
  let unlistenFn: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    eventListeners = new Map();
    unlistenFn = vi.fn();
    tauri.listen.mockImplementation((event: string, handler: (event: { payload: unknown }) => void) => {
      eventListeners.set(event, handler);
      return Promise.resolve(unlistenFn);
    });
    tauri.isPermissionGranted.mockResolvedValue(true);
  });

  it("clears detected and dismissed volumes on logout", async () => {
    const controller = createRemovableVolumeController({
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
      isAppInBackground: () => false,
      t: (key) => key,
    });
    await controller.startSubscriptions();
    controller.destroy();

    expect(unlistenFn).toHaveBeenCalled();
  });
});
