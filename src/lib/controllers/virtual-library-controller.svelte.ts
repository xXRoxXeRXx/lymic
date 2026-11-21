import type { VirtualLibrarySettings } from "../contracts";
import type { VirtualLibraryDesktopAdapter } from "../tauri/desktop-adapter";

/**
 * Owns the Virtual Library section of the settings (ADR-0003). The backend alone decides whether the
 * section exists (Windows, Store release channel, real Package Identity); until it says so, or when it
 * cannot say, the section stays hidden. Later settings (root folder, overlap refusals) extend `apply`
 * and the getters.
 */
export function createVirtualLibraryController(options: {
  reportError(error: unknown): void;
  adapter: VirtualLibraryDesktopAdapter;
}) {
  const { adapter } = options;
  let isAvailable = $state(false);
  let isEnabled = $state(false);
  let isSaving = $state(false);
  let unlisten: (() => void) | undefined;
  let stopped = true;

  function apply(settings: VirtualLibrarySettings | null | undefined) {
    isAvailable = settings?.available === true;
    isEnabled = isAvailable && settings?.enabled === true;
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
      options.reportError(error);
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
    hydrate,
    toggle,
    startLifecycle,
    stopLifecycle,
  };
}
