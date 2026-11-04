import { describe, expect, it, vi } from "vitest";
import { subscribeToSyncEvents, type SyncEventHandlers } from "./sync-event-subscription";

const tauri = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

function handlers(): SyncEventHandlers {
  return {
    onProgress: vi.fn(), onProgressPercent: vi.fn(), onSnapshot: vi.fn(), onStarted: vi.fn(),
    onTransferStats: vi.fn(), onError: vi.fn(), onTriggerSync: vi.fn(), onLog: vi.fn(), onIdle: vi.fn(),
  };
}

describe("subscribeToSyncEvents", () => {
  it("forwards a sync error reason to its handler", async () => {
    tauri.listen.mockResolvedValue(vi.fn());
    const eventHandlers = handlers();

    subscribeToSyncEvents(eventHandlers, () => true);
    await Promise.resolve();

    const syncErrorListener = tauri.listen.mock.calls.find(([event]) => event === "sync-error")?.[1];
    syncErrorListener?.({ payload: "Immich is unavailable" });

    expect(eventHandlers.onError).toHaveBeenCalledWith("Immich is unavailable");
  });

  it("unlistens registrations that resolve after disposal", async () => {
    let resolve!: (unlisten: () => void) => void;
    const unlisten = vi.fn();
    tauri.listen.mockReturnValue(new Promise((done) => { resolve = done; }));

    const cleanup = subscribeToSyncEvents(handlers(), () => true);
    cleanup();
    resolve(unlisten);
    await Promise.resolve();

    expect(unlisten).toHaveBeenCalledTimes(11);
  });

  it("reports failed listener registrations", async () => {
    const error = new Error("IPC unavailable");
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    tauri.listen.mockRejectedValue(error);

    subscribeToSyncEvents(handlers(), () => true);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(consoleError).toHaveBeenCalledWith("Failed to subscribe to sync-progress", error);
  });
});
