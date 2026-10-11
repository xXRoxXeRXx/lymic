import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export function createUpdateController(isSyncActive: () => boolean) {
  let appVersion = $state<string | undefined>();
  let availableUpdate = $state<Update | null>(null);
  let isUpdateDialogOpen = $state(false);
  let updateStatus = $state<"unknown" | "current" | "available">("unknown");
  let isCheckingForUpdates = $state(false);
  let updateInstallState = $state<"idle" | "downloading" | "installing" | "error">("idle");
  let downloadedUpdateBytes = $state(0);
  let updateContentLength = $state<number | null>(null);
  let mounted = false;

  async function record(phase: string) {
    try {
      await invoke<void>("record_webview_startup_phase_probe", { phase });
    } catch (error) {
      console.warn("Failed to record webview startup phase", phase, error);
    }
  }

  async function checkForUpdates() {
    if (isCheckingForUpdates) return;
    isCheckingForUpdates = true;
    try {
      const update = await check();
      if (!mounted) return;
      availableUpdate = update;
      updateStatus = update ? "available" : "current";
      if (update) {
        updateInstallState = "idle";
        isUpdateDialogOpen = true;
      }
    } catch (error) {
      console.warn("Failed to check for updates", error);
    } finally {
      isCheckingForUpdates = false;
    }
  }

  function startLifecycle() {
    mounted = true;
  }

  function startup() {
    void (async () => {
      await record("update_check_started");
      await checkForUpdates();
      await record("update_check_complete");
    })();
    void (async () => {
      void record("version_load_started");
      try {
        const version = await getVersion();
        if (mounted) appVersion = version;
      } catch (error) {
        console.warn("Failed to load app version", error);
      }
      void record("version_load_complete");
    })();
  }

  async function installUpdate() {
    if (!availableUpdate || isSyncActive() || updateInstallState === "downloading" || updateInstallState === "installing") return;
    downloadedUpdateBytes = 0;
    updateContentLength = null;
    updateInstallState = "downloading";
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (!mounted) return;
        if (event.event === "Started") updateContentLength = event.data.contentLength ?? null;
        if (event.event === "Progress") downloadedUpdateBytes += event.data.chunkLength;
        if (event.event === "Finished") updateInstallState = "installing";
      });
      await relaunch();
    } catch (error) {
      console.error("Failed to install update", error);
      if (mounted) updateInstallState = "error";
    }
  }

  return {
    get appVersion() { return appVersion; },
    get availableUpdate() { return availableUpdate; },
    get isUpdateDialogOpen() { return isUpdateDialogOpen; },
    set isUpdateDialogOpen(value: boolean) { isUpdateDialogOpen = value; },
    get updateStatus() { return updateStatus; },
    get isCheckingForUpdates() { return isCheckingForUpdates; },
    get updateInstallState() { return updateInstallState; },
    get downloadedUpdateBytes() { return downloadedUpdateBytes; },
    get updateContentLength() { return updateContentLength; },
    startLifecycle,
    destroy() { mounted = false; },
    startup,
    checkForUpdates,
    installUpdate,
  };
}
