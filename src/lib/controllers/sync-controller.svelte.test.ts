import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SyncDesktopAdapter } from "../tauri/desktop-adapter";
import { createSyncController } from "./sync-controller.svelte";

const events = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => events);

function createSyncAdapter(overrides: Partial<SyncDesktopAdapter> = {}): SyncDesktopAdapter {
  return {
    startSync: vi.fn().mockResolvedValue({ processed: 1, uploaded: 1, failed: 0 }),
    pauseSync: vi.fn().mockResolvedValue({ status: "PAUSED", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    resumeSync: vi.fn().mockResolvedValue({ status: "RUNNING", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getSyncStatus: vi.fn().mockResolvedValue({ status: "IDLE", total: 0, succeeded: 0, failed: 0, currentPath: null }),
    getFailedSyncs: vi.fn().mockResolvedValue([]),
    retryFailedSyncs: vi.fn().mockResolvedValue({ processed: 1, uploaded: 1, failed: 0 }),
    ...overrides,
  };
}

function createController(adapter: SyncDesktopAdapter, overrides = {}) {
  return createSyncController({
    adapter,
    reportError: vi.fn(),
    locale: () => "en",
    ...overrides,
  });
}

beforeEach(() => {
  events.listen.mockClear();
});

afterEach(() => vi.useRealTimers());

describe("sync controller", () => {
  it("clears the progress reset timer on disposal", async () => {
    vi.useFakeTimers();
    const adapter = createSyncAdapter();
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();
    await controller.status.onSyncAction();

    expect(controller.status.progress).toBe(100);
    controller.destroy();
    vi.advanceTimersByTime(2000);
    expect(controller.status.progress).toBe(100);
  });

  it("exposes synchronization status and actions through the status view-state interface", async () => {
    const adapter = createSyncAdapter({
      getSyncStatus: vi.fn().mockResolvedValue({
        status: "PAUSED",
        total: 4,
        succeeded: 1,
        failed: 0,
        currentPath: "C:/photos/two.jpg",
      }),
    });
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();

    expect(controller.status).toMatchObject({
      syncStatus: "paused",
      processed: 1,
      syncTotal: 4,
      currentFile: "two.jpg",
    });
    expect("watchedFolders" in controller.status).toBe(false);
    expect("onAddFolder" in controller.status).toBe(false);
  });

  it("shows a durable failed synchronization even without failed queue entries", async () => {
    const adapter = createSyncAdapter({
      getSyncStatus: vi.fn().mockResolvedValue({
        status: "FAILED",
        total: 0,
        succeeded: 0,
        failed: 0,
        currentPath: null,
      }),
    });
    const controller = createController(adapter);
    controller.startLifecycle();

    await controller.hydrate();

    expect(controller.status.syncStatus).toBe("error");
  });

  it("pauses and resumes synchronization through the adapter", async () => {
    const adapter = createSyncAdapter({
      getSyncStatus: vi.fn().mockResolvedValue({ status: "RUNNING", total: 4, succeeded: 1, failed: 0, currentPath: null }),
      pauseSync: vi.fn().mockResolvedValue({ status: "PAUSED", total: 4, succeeded: 1, failed: 0, currentPath: null }),
      resumeSync: vi.fn().mockResolvedValue({ status: "RUNNING", total: 4, succeeded: 1, failed: 0, currentPath: null }),
    });
    const controller = createController(adapter);
    controller.startLifecycle();
    await controller.hydrate();

    await controller.status.onSyncAction();
    await controller.status.onSyncAction();

    expect(adapter.pauseSync).toHaveBeenCalledOnce();
    expect(adapter.resumeSync).toHaveBeenCalledOnce();
    expect(controller.status.syncStatus).toBe("syncing");
  });

  it("does not start synchronization when canStartSync returns false", async () => {
    const adapter = createSyncAdapter();
    const controller = createController(adapter, { canStartSync: () => false });
    controller.startLifecycle();
    await controller.hydrate();

    await controller.status.onSyncAction();

    expect(adapter.startSync).not.toHaveBeenCalled();
    expect(controller.status.syncStatus).toBe("idle");
  });

  it("retries failed synchronization through the adapter and notifies onSyncFinished", async () => {
    const adapter = createSyncAdapter({
      getFailedSyncs: vi
        .fn()
        .mockResolvedValueOnce([{ localPath: "C:/photos/failed.jpg", failureReason: "temporary failure" }])
        .mockResolvedValueOnce([]),
    });
    const onSyncFinished = vi.fn();
    const controller = createController(adapter, { onSyncFinished });
    controller.startLifecycle();
    await controller.refreshFailedSyncs();

    await controller.status.onRetryFailedSyncs();

    expect(adapter.retryFailedSyncs).toHaveBeenCalledOnce();
    expect(onSyncFinished).toHaveBeenCalledOnce();
    expect(controller.status.syncStatus).toBe("idle");
  });

  it("keeps event subscription and cleanup active once per mount and unmount", async () => {
    const unlisten = vi.fn();
    events.listen.mockResolvedValue(unlisten);
    const adapter = createSyncAdapter();
    const controller = createController(adapter);

    controller.startSubscriptions();
    expect(events.listen).not.toHaveBeenCalled();

    controller.startLifecycle();
    controller.startSubscriptions();
    const listenerCount = events.listen.mock.calls.length;
    expect(listenerCount).toBeGreaterThan(0);

    controller.startSubscriptions();
    expect(events.listen).toHaveBeenCalledTimes(listenerCount);

    await Promise.resolve();
    controller.destroy();
    expect(unlisten).toHaveBeenCalledTimes(listenerCount);
  });
});
