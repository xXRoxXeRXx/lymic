import type { VirtualLibrarySettings } from "../contracts";
import type { VirtualLibraryDesktopAdapter } from "../tauri/desktop-adapter";
import { failureMessage } from "./request-failure";

/**
 * Backend error codes (`cloud_files::activation`, `commands/virtual_library.rs`): the text after the
 * code and `": "` is a path or a technical reason. Other errors are explanations the backend wrote
 * for the user and are shown unchanged.
 */
const ERROR_CODES: Record<string, { key: string; detail: "path" | "reason" | "none" }> = {
  VIRTUAL_LIBRARY_NESTED_SYNC_ROOT: { key: "virtual_library_nested_root", detail: "path" },
  VIRTUAL_LIBRARY_START_FAILED: { key: "virtual_library_start_failed", detail: "reason" },
  VIRTUAL_LIBRARY_DISABLE_TO_CHANGE_FOLDER: { key: "virtual_library_disable_to_change_folder", detail: "none" },
};

/**
 * Owns the Virtual Library section of the settings (ADR-0003). The backend alone decides whether the
 * section exists (Windows, Store release channel, real Package Identity); until it says so, or when it
 * cannot say, the section stays hidden. The backend also owns the root folder rule: it reports the
 * effective folder and refuses an enable or folder change that would overlap a Watched Folder; the
 * controller surfaces its explanation unchanged. Enabling also registers the sync root, and the
 * backend refuses a folder that lies inside another sync root (such as OneDrive) with a code the
 * controller turns into a localized request for another folder.
 */
export function createVirtualLibraryController(options: {
  reportError(error: unknown): void;
  t(key: string): string;
  adapter: VirtualLibraryDesktopAdapter;
}) {
  const { adapter } = options;
  let isAvailable = $state(false);
  let isEnabled = $state(false);
  let rootFolder = $state<string | null>(null);
  let isSaving = $state(false);
  let unlisten: (() => void) | undefined;
  let stopped = true;

  /** The localized error for a coded backend error; anything else is returned as it came. */
  function localized(error: unknown): unknown {
    const message = failureMessage(error);
    for (const [code, { key, detail }] of Object.entries(ERROR_CODES)) {
      if (!message.startsWith(`${code}:`)) continue;
      const text = options.t(key);
      const rest = message.slice(code.length + 1).trim();
      if (!rest || detail === "none") return new Error(text);
      return new Error(detail === "path" ? `${text} (${rest})` : `${text} ${rest}`);
    }
    return error;
  }

  function apply(settings: VirtualLibrarySettings | null | undefined) {
    isAvailable = settings?.available === true;
    isEnabled = isAvailable && settings?.enabled === true;
    rootFolder = isAvailable ? (settings?.root_folder ?? null) : null;
  }

  async function hydrate() {
    try {
      apply(await adapter.getVirtualLibrarySettings());
    } catch (error) {
      apply(null);
      options.reportError(error);
    }
  }

  async function toggle() {
    if (!isAvailable || isSaving) return;
    isSaving = true;
    try {
      apply(await adapter.setVirtualLibraryEnabled(!isEnabled));
    } catch (error) {
      options.reportError(localized(error));
    } finally {
      isSaving = false;
    }
  }

  async function chooseRootFolder() {
    if (!isAvailable || isSaving) return;
    isSaving = true;
    try {
      const selected = await adapter.openFolderDialog({
        directory: true,
        multiple: false,
        title: options.t("select_folder"),
      });
      if (typeof selected === "string") {
        apply(await adapter.setVirtualLibraryRootFolder(selected));
      }
    } catch (error) {
      options.reportError(localized(error));
    } finally {
      isSaving = false;
    }
  }

  async function startLifecycle() {
    stopped = false;
    try {
      const release = await adapter.subscribeToVirtualLibrarySettings(apply);
      if (stopped) release?.();
      else unlisten = release;
    } catch (error) {
      console.error("Failed to subscribe to Virtual Library settings", error);
    }
  }

  function stopLifecycle() {
    stopped = true;
    unlisten?.();
    unlisten = undefined;
  }

  return {
    get isAvailable() { return isAvailable; },
    get isEnabled() { return isEnabled; },
    get isSaving() { return isSaving; },
    get rootFolder() { return rootFolder; },
    hydrate,
    toggle,
    chooseRootFolder,
    startLifecycle,
    stopLifecycle,
  };
}
