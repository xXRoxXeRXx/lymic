import { beforeEach, describe, expect, it, vi } from "vitest";
import { subscribeToSyncEvents, type SyncEventHandlers } from "./sync-event-subscription";

const tauri = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

function handlers(): SyncEventHandlers {
  return {
    onProgress: vi.fn(), onProgressPercent: vi.fn(), onSnapshot: vi.fn(), onStarted: vi.fn(),
    onTransferStats: vi.fn(), onError: vi.fn(), onLog: vi.fn(), onIdle: vi.fn(),
  };
}

beforeEach(() => {
  tauri.listen.mockClear();
});

describe("subscribeToSyncEvents", () => {
  it("forwards a sync error reason to its handler", async () => {
    tauri.listen.mockResolvedValue(vi.fn());
    const eventHandlers = handlers();

    subscribeToSyncEvents(eventHandlers, () => true);
    await Promise.resolve();

    const syncErrorListener = tauri.listen.mock.calls.find(([event]) => event === "sync-error")?.[1];
    expect(syncErrorListener).toBeTypeOf("function");
    syncErrorListener!({ payload: "Immich is unavailable" });

    expect(eventHandlers.onError).toHaveBeenCalledWith("Immich is unavailable");
  });

  it("forwards undefined for a non-string sync error payload", async () => {
    tauri.listen.mockResolvedValue(vi.fn());
    const eventHandlers = handlers();

    subscribeToSyncEvents(eventHandlers, () => true);
    await Promise.resolve();

    const syncErrorListener = tauri.listen.mock.calls.find(([event]) => event === "sync-error")?.[1];
    expect(syncErrorListener).toBeTypeOf("function");
    syncErrorListener!({ payload: { message: "Immich is unavailable" } });

    expect(eventHandlers.onError).toHaveBeenCalledWith(undefined);
  });

  it("forwards complete transfer statistics without subscribing to the retired trigger-sync event", async () => {
    tauri.listen.mockResolvedValue(vi.fn());
    const eventHandlers = handlers();

    subscribeToSyncEvents(eventHandlers, () => true);
    await Promise.resolve();

    const transferStatsListener = tauri.listen.mock.calls.find(([event]) => event === "sync-transfer-stats")?.[1];
    expect(transferStatsListener).toBeTypeOf("function");
    transferStatsListener!({
      payload: {
        transferredBytes: 42,
        transferRateBytesPerSecond: 7,
        estimatedSecondsRemaining: 12,
        status: "RUNNING",
        totalBytes: 100,
        completedBytes: 42,
      },
    });

    expect(eventHandlers.onTransferStats).toHaveBeenCalledWith({
      transferredBytes: 42,
      transferRateBytesPerSecond: 7,
      estimatedSecondsRemaining: 12,
      status: "RUNNING",
      totalBytes: 100,
      completedBytes: 42,
    });
    expect(tauri.listen).not.toHaveBeenCalledWith("trigger-sync", expect.any(Function));
  });

  it("unlistens registrations that resolve after disposal", async () => {
    let resolve!: (unlisten: () => void) => void;
    const unlisten = vi.fn();
    tauri.listen.mockReturnValue(new Promise((done) => { resolve = done; }));

    const cleanup = subscribeToSyncEvents(handlers(), () => true);
    cleanup();
    resolve(unlisten);
    await Promise.resolve();

    expect(unlisten).toHaveBeenCalledTimes(10);
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
