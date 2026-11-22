import { describe, expect, it, vi } from "vitest";
import { createVirtualLibraryController } from "./virtual-library-controller.svelte";
import type { VirtualLibraryDesktopAdapter } from "../tauri/desktop-adapter";
import type { VirtualLibraryCleanupReport, VirtualLibrarySettings } from "../contracts";

function createDesktopAdapter(overrides: Partial<VirtualLibraryDesktopAdapter> = {}): VirtualLibraryDesktopAdapter {
  return {
    getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: false, enabled: false, root_folder: null }),
    setVirtualLibraryEnabled: vi.fn(),
    setVirtualLibraryRootFolder: vi.fn(),
    openFolderDialog: vi.fn(),
    subscribeToVirtualLibrarySettings: vi.fn().mockResolvedValue(() => {}),
    subscribeToVirtualLibraryCleanup: vi.fn().mockResolvedValue(() => {}),
    confirm: vi.fn().mockResolvedValue(true),
    notify: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

function createController(adapter: VirtualLibraryDesktopAdapter, reportError = vi.fn(), autostart: () => boolean | null = () => true) {
  return { reportError, controller: createVirtualLibraryController({ reportError, adapter, autostartEnabled: autostart, t: (key) => key }) };
}

const ROOT = "C:\\Users\\Me\\Pictures\\Lymic";

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
    const adapter = createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: true, root_folder: ROOT }) });
    const { controller } = createController(adapter);

    await controller.hydrate();

    expect(controller.isAvailable).toBe(true);
    expect(controller.isEnabled).toBe(true);
  });

  it("toggles the enabled state through the backend and adopts the state it returns", async () => {
    const setVirtualLibraryEnabled = vi.fn().mockResolvedValue({ available: true, enabled: true, root_folder: ROOT });
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
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
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      setVirtualLibraryEnabled: vi.fn().mockRejectedValue(new Error("Virtual Library is not available")),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect(controller.isEnabled).toBe(false);
    expect(controller.isSaving).toBe(false);
    expect(reportError).toHaveBeenCalledWith(expect.any(Error));
  });

  it("shows the effective folder the backend reports and none while unavailable", async () => {
    const { controller } = createController(createDesktopAdapter());
    expect(controller.rootFolder).toBeNull();

    const available = createController(createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }) }));
    await available.controller.hydrate();
    expect(available.controller.rootFolder).toBe(ROOT);

    const unresolved = createController(createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: null }) }));
    await unresolved.controller.hydrate();
    expect(unresolved.controller.rootFolder).toBeNull();
  });

  it("changes the folder through the folder picker and adopts the folder the backend stored", async () => {
    const chosen = "D:\\Photos\\Virtual";
    const openFolderDialog = vi.fn().mockResolvedValue(chosen);
    const setVirtualLibraryRootFolder = vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: chosen });
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      openFolderDialog,
      setVirtualLibraryRootFolder,
    });
    const { controller } = createController(adapter);
    await controller.hydrate();

    await controller.chooseRootFolder();

    expect(openFolderDialog).toHaveBeenCalledWith({ directory: true, multiple: false, title: "select_folder" });
    expect(setVirtualLibraryRootFolder).toHaveBeenCalledWith(chosen);
    expect(controller.rootFolder).toBe(chosen);
    expect(controller.isSaving).toBe(false);
  });

  it("leaves the folder alone when the picker is cancelled", async () => {
    const setVirtualLibraryRootFolder = vi.fn();
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      openFolderDialog: vi.fn().mockResolvedValue(null),
      setVirtualLibraryRootFolder,
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.chooseRootFolder();

    expect(setVirtualLibraryRootFolder).not.toHaveBeenCalled();
    expect(controller.rootFolder).toBe(ROOT);
    expect(reportError).not.toHaveBeenCalled();
  });

  it("keeps the folder and reports the backend's explanation when it refuses the chosen folder", async () => {
    const refusal = "'D:\\Camera\\Lymic' cannot be the Virtual Library folder: the Watched Folder 'D:\\Camera' contains the Virtual Library folder.";
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      openFolderDialog: vi.fn().mockResolvedValue("D:\\Camera\\Lymic"),
      setVirtualLibraryRootFolder: vi.fn().mockRejectedValue(refusal),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.chooseRootFolder();

    expect(controller.rootFolder).toBe(ROOT);
    expect(controller.isSaving).toBe(false);
    expect(reportError).toHaveBeenCalledWith(refusal);
  });

  it("cannot change the folder while the Virtual Library is unavailable", async () => {
    const openFolderDialog = vi.fn();
    const { controller } = createController(createDesktopAdapter({ openFolderDialog }));
    await controller.hydrate();

    await controller.chooseRootFolder();

    expect(openFolderDialog).not.toHaveBeenCalled();
  });

  it("stays disabled and reports the refusal naming the overlapping Watched Folder when enabling is refused", async () => {
    const refusal = "The Virtual Library cannot be enabled: the Watched Folder 'C:\\Users\\Me\\Pictures' contains the Virtual Library folder '" + ROOT + "'.";
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      setVirtualLibraryEnabled: vi.fn().mockRejectedValue(refusal),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect(controller.isEnabled).toBe(false);
    expect(reportError).toHaveBeenCalledWith(refusal);
    expect(reportError.mock.calls[0][0]).toContain("C:\\Users\\Me\\Pictures");
  });

  it("asks for another folder when the backend refuses one inside another sync root, and stays disabled", async () => {
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      setVirtualLibraryEnabled: vi.fn().mockRejectedValue("VIRTUAL_LIBRARY_NESTED_SYNC_ROOT: C:\\Users\\Me\\OneDrive"),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect(controller.isEnabled).toBe(false);
    expect(reportError).toHaveBeenCalledTimes(1);
    const reported = reportError.mock.calls[0][0] as Error;
    expect(reported.message).toBe("virtual_library_nested_root (C:\\Users\\Me\\OneDrive)");
  });

  it("explains a failure to start with the technical reason after the localized text", async () => {
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: false, root_folder: ROOT }),
      setVirtualLibraryEnabled: vi.fn().mockRejectedValue(new Error("VIRTUAL_LIBRARY_START_FAILED: could not register the sync folder: 0x80070490")),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.toggle();

    expect((reportError.mock.calls[0][0] as Error).message).toBe("virtual_library_start_failed could not register the sync folder: 0x80070490");
  });

  it("asks to disable the Virtual Library first when the folder is changed while it is enabled", async () => {
    const adapter = createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue({ available: true, enabled: true, root_folder: ROOT }),
      openFolderDialog: vi.fn().mockResolvedValue("D:\\Photos"),
      setVirtualLibraryRootFolder: vi.fn().mockRejectedValue("VIRTUAL_LIBRARY_DISABLE_TO_CHANGE_FOLDER: disable the Virtual Library before changing its folder"),
    });
    const { controller, reportError } = createController(adapter);
    await controller.hydrate();

    await controller.chooseRootFolder();

    expect(controller.rootFolder).toBe(ROOT);
    expect((reportError.mock.calls[0][0] as Error).message).toBe("virtual_library_disable_to_change_folder");
  });

  describe("disabling and cleanup (#283)", () => {
    const enabledSettings = { available: true, enabled: true, root_folder: ROOT };
    const disabledSettings = { available: true, enabled: false, root_folder: ROOT };
    const enabledAdapter = (overrides: Partial<VirtualLibraryDesktopAdapter> = {}) => createDesktopAdapter({
      getVirtualLibrarySettings: vi.fn().mockResolvedValue(enabledSettings),
      setVirtualLibraryEnabled: vi.fn().mockResolvedValue(disabledSettings),
      ...overrides,
    });

    it("confirms before disabling, naming that hydrated and pinned originals are removed and that other files stay", async () => {
      const adapter = enabledAdapter();
      const { controller } = createController(adapter);
      await controller.hydrate();

      await controller.toggle();

      expect(adapter.confirm).toHaveBeenCalledTimes(1);
      const [message, options] = vi.mocked(adapter.confirm).mock.calls[0];
      expect(message).toContain("virtual_library_disable_confirmation");
      expect(message).toContain("virtual_library_cleanup_warning");
      expect(options).toEqual({ title: "virtual_library", kind: "warning" });
      expect(adapter.setVirtualLibraryEnabled).toHaveBeenCalledWith(false);
      expect(controller.isEnabled).toBe(false);
    });

    it("leaves everything as it was when the confirmation is declined", async () => {
      const adapter = enabledAdapter({ confirm: vi.fn().mockResolvedValue(false) });
      const { controller } = createController(adapter);
      await controller.hydrate();

      await controller.toggle();

      expect(adapter.setVirtualLibraryEnabled).not.toHaveBeenCalled();
      expect(controller.isEnabled).toBe(true);
      expect(controller.isSaving).toBe(false);
    });

    it("does not ask for a confirmation when enabling", async () => {
      const adapter = createDesktopAdapter({
        getVirtualLibrarySettings: vi.fn().mockResolvedValue(disabledSettings),
        setVirtualLibraryEnabled: vi.fn().mockResolvedValue(enabledSettings),
      });
      const { controller } = createController(adapter);
      await controller.hydrate();

      await controller.toggle();

      expect(adapter.confirm).not.toHaveBeenCalled();
    });

    it("keeps the Virtual Library enabled and localizes the error when the cleanup could not run", async () => {
      const adapter = enabledAdapter({
        setVirtualLibraryEnabled: vi.fn().mockRejectedValue("VIRTUAL_LIBRARY_CLEANUP_FAILED: could not unregister the sync folder: 0x80070005"),
      });
      const { controller, reportError } = createController(adapter);
      await controller.hydrate();

      await controller.toggle();

      expect(controller.isEnabled).toBe(true);
      expect((reportError.mock.calls[0][0] as Error).message).toBe("virtual_library_cleanup_failed could not unregister the sync folder: 0x80070005");
    });

    const report = (overrides: Partial<VirtualLibraryCleanupReport> = {}): VirtualLibraryCleanupReport => ({
      root_folder: ROOT,
      removed_files: 4,
      removed_folders: 2,
      kept_count: 0,
      kept_examples: [],
      root_kept: false,
      failed_count: 0,
      failed_examples: [],
      error: null,
      ...overrides,
    });

    async function started(adapter: VirtualLibraryDesktopAdapter) {
      let announce: (report: VirtualLibraryCleanupReport) => void = () => {};
      adapter.subscribeToVirtualLibraryCleanup = vi.fn().mockImplementation(async (listener) => {
        announce = listener;
        return () => {};
      });
      const created = createController(adapter);
      await created.controller.startLifecycle();
      return { ...created, announce: (value: VirtualLibraryCleanupReport) => announce(value) };
    }

    it("tells the user that files Lymic did not create stayed, naming the folder and the files", async () => {
      const adapter = createDesktopAdapter();
      const { announce } = await started(adapter);

      announce(report({ kept_count: 7, kept_examples: ["mine.txt", "Edits/a.psd"], root_kept: true }));

      expect(adapter.notify).toHaveBeenCalledTimes(1);
      const [message, options] = vi.mocked(adapter.notify).mock.calls[0];
      expect(message).toContain("virtual_library_cleanup_kept");
      expect(message).toContain(ROOT);
      expect(message).toContain("mine.txt");
      expect(message).toContain("Edits/a.psd");
      expect(message).toContain("7");
      expect(options).toEqual({ title: "virtual_library", kind: "warning" });
    });

    it("tells the user which items could not be removed and why", async () => {
      const adapter = createDesktopAdapter();
      const { announce } = await started(adapter);

      announce(report({ failed_count: 1, failed_examples: ["All Photos/a.jpg: access denied"], root_kept: true }));

      const [message] = vi.mocked(adapter.notify).mock.calls[0];
      expect(message).toContain("virtual_library_cleanup_failed_items");
      expect(message).toContain("All Photos/a.jpg: access denied");
    });

    it("tells the user when the cleanup could not run at all, with the technical reason", async () => {
      const adapter = createDesktopAdapter();
      const { announce } = await started(adapter);

      announce(report({ error: "VIRTUAL_LIBRARY_CLEANUP_FAILED: could not unregister the sync folder: denied" }));

      const [message] = vi.mocked(adapter.notify).mock.calls[0];
      expect(message).toContain("virtual_library_cleanup_not_run");
      expect(message).toContain("denied");
    });

    it("stays silent when the cleanup left nothing behind", async () => {
      const adapter = createDesktopAdapter();
      const { announce } = await started(adapter);

      announce(report());

      expect(adapter.notify).not.toHaveBeenCalled();
    });

    it("stops listening for cleanup reports with the lifecycle", async () => {
      const unlistenCleanup = vi.fn();
      const adapter = createDesktopAdapter({ subscribeToVirtualLibraryCleanup: vi.fn().mockResolvedValue(unlistenCleanup) });
      const { controller } = createController(adapter);

      await controller.startLifecycle();
      controller.stopLifecycle();

      expect(unlistenCleanup).toHaveBeenCalledTimes(1);
    });
  });

  describe("autostart hint", () => {
    const enabledSettings = { available: true, enabled: true, root_folder: ROOT };
    const adapterWith = (settings: VirtualLibrarySettings) => createDesktopAdapter({ getVirtualLibrarySettings: vi.fn().mockResolvedValue(settings) });

    it("is shown while the Virtual Library is enabled and autostart is off", async () => {
      const { controller } = createController(adapterWith(enabledSettings), vi.fn(), () => false);
      await controller.hydrate();

      expect(controller.showAutostartHint).toBe(true);
    });

    it("is not shown while autostart is on", async () => {
      const { controller } = createController(adapterWith(enabledSettings), vi.fn(), () => true);
      await controller.hydrate();

      expect(controller.showAutostartHint).toBe(false);
    });

    it("is not shown before the autostart state is known", async () => {
      const { controller } = createController(adapterWith(enabledSettings), vi.fn(), () => null);
      await controller.hydrate();

      expect(controller.showAutostartHint).toBe(false);
    });

    it("is not shown while the Virtual Library is disabled or unavailable", async () => {
      const disabled = createController(adapterWith({ ...enabledSettings, enabled: false }), vi.fn(), () => false);
      await disabled.controller.hydrate();
      expect(disabled.controller.showAutostartHint).toBe(false);

      const unavailable = createController(adapterWith({ available: false, enabled: false, root_folder: null }), vi.fn(), () => false);
      await unavailable.controller.hydrate();
      expect(unavailable.controller.showAutostartHint).toBe(false);
    });

    it("appears when the user enables the Virtual Library and follows the autostart state afterwards", async () => {
      let autostart = false;
      const adapter = createDesktopAdapter({
        getVirtualLibrarySettings: vi.fn().mockResolvedValue({ ...enabledSettings, enabled: false }),
        setVirtualLibraryEnabled: vi.fn().mockResolvedValue(enabledSettings),
      });
      const { controller } = createController(adapter, vi.fn(), () => autostart);
      await controller.hydrate();
      expect(controller.showAutostartHint).toBe(false);

      await controller.toggle();
      expect(controller.showAutostartHint).toBe(true);

      autostart = true;
      expect(controller.showAutostartHint).toBe(false);
    });
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
    announce({ available: true, enabled: true, root_folder: ROOT });
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
