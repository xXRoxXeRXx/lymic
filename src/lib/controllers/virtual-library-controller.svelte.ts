import type { VirtualLibraryCleanupReport, VirtualLibraryNotice, VirtualLibrarySettings } from "../contracts";
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
  VIRTUAL_LIBRARY_CLEANUP_FAILED: { key: "virtual_library_cleanup_failed", detail: "reason" },
};

/**
 * Owns the Virtual Library section of the settings (ADR-0003). The backend alone decides whether the
 * section exists (Windows, Store release channel, real Package Identity); until it says so, or when it
 * cannot say, the section stays hidden. The backend also owns the root folder rule: it reports the
 * effective folder and refuses an enable or folder change that would overlap a Watched Folder; the
 * controller surfaces its explanation unchanged. Enabling also registers the sync root, and the
 * backend refuses a folder that lies inside another sync root (such as OneDrive) with a code the
 * controller turns into a localized request for another folder.
 *
 * Unhydrated placeholders only work while Lymic runs (ADR-0003), so while the Virtual Library is on
 * and autostart is off the controller offers a hint. The autostart state belongs to the preferences
 * controller and is read lazily through `autostartEnabled`; `null` means it is not known yet, which
 * must never flash the hint.
 *
 * A read that fails (Immich unreachable, expired sign-in, photo removed) is announced by the backend
 * once per failure episode; the controller holds that one notice until it is dismissed. An expired
 * sign-in offers to sign in again, through `beginRelogin`, which keeps the session (and the Virtual
 * Library) as it is until the new key is accepted.
 *
 * Disabling removes what the Virtual Library created, including hydrated and pinned originals
 * (ADR-0003, #283), so it asks first: the confirmation names that and says that files Lymic did not
 * create stay with their folder. Logout asks the same through the auth controller. After either
 * cleanup the backend announces what it left (files that are not Lymic's, items it could not
 * remove) and the controller tells the user with a native notice; a clean cleanup is silent.
 */
export function createVirtualLibraryController(options: {
  reportError(error: unknown): void;
  t(key: string): string;
  autostartEnabled(): boolean | null;
  beginRelogin(): void;
  adapter: VirtualLibraryDesktopAdapter;
}) {
  const { adapter } = options;
  let isAvailable = $state(false);
  let isEnabled = $state(false);
  let rootFolder = $state<string | null>(null);
  let isSaving = $state(false);
  let notice = $state<(VirtualLibraryNotice & { key: string; canRelogin: boolean }) | null>(null);
  let unlisten: (() => void) | undefined;
  let unlistenNotices: (() => void) | undefined;
  let unlistenCleanup: (() => void) | undefined;
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

  /** The text of the notice for a cleanup that left something, or null when there is nothing to say. */
  function cleanupNotice(report: VirtualLibraryCleanupReport): string | null {
    const paragraphs: string[] = [];
    if (report.error) {
      const reason = report.error.replace(/^[A-Z_]+:\s*/, "");
      paragraphs.push(`${options.t("virtual_library_cleanup_not_run")} ${reason}`.trim());
    }
    if (report.kept_count > 0 || report.root_kept) {
      const lines = [`${options.t("virtual_library_cleanup_kept")} ${report.root_folder}`];
      lines.push(...report.kept_examples);
      if (report.kept_count > report.kept_examples.length) lines.push(`… (${report.kept_count})`);
      paragraphs.push(lines.join("\n"));
    }
    if (report.failed_count > 0) {
      const lines = [options.t("virtual_library_cleanup_failed_items"), ...report.failed_examples];
      if (report.failed_count > report.failed_examples.length) lines.push(`… (${report.failed_count})`);
      paragraphs.push(lines.join("\n"));
    }
    return paragraphs.length > 0 ? paragraphs.join("\n\n") : null;
  }

  async function announceCleanup(report: VirtualLibraryCleanupReport) {
    const text = cleanupNotice(report);
    if (!text) return;
    try {
      await adapter.notify(text, { title: options.t("virtual_library"), kind: "warning" });
    } catch (error) {
      console.error("Failed to show the Virtual Library cleanup notice", error);
      options.reportError(text);
    }
  }

  async function toggle() {
    if (!isAvailable || isSaving) return;
    isSaving = true;
    try {
      if (isEnabled) {
        const confirmed = await adapter.confirm(
          `${options.t("virtual_library_disable_confirmation")}\n\n${options.t("virtual_library_cleanup_warning")}`,
          { title: options.t("virtual_library"), kind: "warning" },
        );
        if (!confirmed) return;
      }
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

  function showNotice(announced: VirtualLibraryNotice) {
    notice = {
      ...announced,
      key: `log_virtual_library_${announced.kind}`,
      canRelogin: announced.kind === "unauthorized",
    };
  }

  function dismissNotice() {
    notice = null;
  }

  function relogin() {
    if (!notice?.canRelogin) return;
    notice = null;
    options.beginRelogin();
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
    try {
      const release = await adapter.subscribeToVirtualLibraryNotices(showNotice);
      if (stopped) release?.();
      else unlistenNotices = release;
    } catch (error) {
      console.error("Failed to subscribe to Virtual Library notices", error);
    }
    try {
      const release = await adapter.subscribeToVirtualLibraryCleanup((report) => void announceCleanup(report));
      if (stopped) release?.();
      else unlistenCleanup = release;
    } catch (error) {
      console.error("Failed to subscribe to Virtual Library cleanup reports", error);
    }
  }

  function stopLifecycle() {
    stopped = true;
    unlisten?.();
    unlisten = undefined;
    unlistenNotices?.();
    unlistenNotices = undefined;
    unlistenCleanup?.();
    unlistenCleanup = undefined;
  }

  return {
    get isAvailable() { return isAvailable; },
    get isEnabled() { return isEnabled; },
    get isSaving() { return isSaving; },
    get rootFolder() { return rootFolder; },
    get notice() { return notice; },
    get showAutostartHint() { return isEnabled && options.autostartEnabled() === false; },
    hydrate,
    toggle,
    chooseRootFolder,
    dismissNotice,
    relogin,
    startLifecycle,
    stopLifecycle,
  };
}
