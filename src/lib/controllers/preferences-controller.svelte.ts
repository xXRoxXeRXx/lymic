import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import type { ThemePreference } from "../components/types";
import { isSupportedLocale } from "../i18n";
import { localeCatalog, type LocaleCatalogEntry } from "../i18n/locale-catalog";
import type { UiLogMessage } from "./types";

export function createPreferencesController(options: {
  reportError(error: unknown): void;
  addLog(message: UiLogMessage): void;
  locale(): string;
  setLocale(locale: string): void;
}) {
  const themeStorageKey = "lymic-theme";
  const savedTheme = typeof localStorage === "undefined" ? null : localStorage.getItem(themeStorageKey);
  const initialTheme: ThemePreference = savedTheme === "system" || savedTheme === "light" || savedTheme === "dark"
    ? savedTheme
    : "system";
  let themePreference = $state<ThemePreference>(initialTheme);
  let isAutostartEnabled = $state(false);
  let uploadParallelism = $state(3);
  let isSavingUploadParallelism = $state(false);
  let thumbnailCacheCapBytes = $state(250 * 1024 * 1024);
  let isClearingThumbnailCache = $state(false);
  let isClearingVirtualLibraryCache = $state(false);
  let virtualLibraryCacheClearResult = $state<{ bytesFreed: number; filesDehydrated: number; pinnedFilesPreserved: number } | null>(null);
  let virtualLibraryEnabled = $state(false);
  let virtualLibraryPath = $state("%USERPROFILE%\\Pictures\\Lymic");
  let virtualLibraryConnectionStatus = $state("disconnected");
  let colorSchemeQuery: MediaQueryList | undefined;
  let listener: (() => void) | undefined;
  let unlistenVirtualLibrary: (() => void) | undefined;
  let locales = $state<readonly LocaleCatalogEntry[]>(localeCatalog);

  if (savedTheme && savedTheme !== initialTheme && typeof localStorage !== "undefined") {
    localStorage.removeItem(themeStorageKey);
  }

  function applyTheme() {
    if (typeof window === "undefined") return;
    document.documentElement.dataset.theme = themePreference === "system"
      ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
      : themePreference;
  }

  function setThemePreference(value: ThemePreference) {
    themePreference = value;
    if (typeof localStorage !== "undefined") localStorage.setItem(themeStorageKey, value);
    applyTheme();
  }

  async function hydrate() {
    try {
      const catalog = await invoke<LocaleCatalogEntry[]>("get_locale_catalog");
      if (Array.isArray(catalog)) locales = catalog;
    } catch (error) {
      options.reportError(error);
    }

    try {
      isAutostartEnabled = await isEnabled();
    } catch (error) {
      options.reportError(error);
    }

    try {
      uploadParallelism = await invoke<number>("get_upload_parallelism");
    } catch (error) {
      console.error("Failed to load upload parallelism", error);
      options.reportError(error);
    }

    try {
      thumbnailCacheCapBytes = await invoke<number>("get_thumbnail_cache_cap_bytes");
    } catch (error) {
      console.error("Failed to load thumbnail cache cap", error);
      options.reportError(error);
    }

    try {
      const status = await invoke<{ enabled: boolean; path: string; connectionStatus: string }>("get_virtual_library_status");
      virtualLibraryEnabled = status.enabled;
      virtualLibraryPath = status.path;
      virtualLibraryConnectionStatus = status.connectionStatus;
    } catch (error) {
      console.error("Failed to load virtual library status", error);
      options.reportError(error);
    }

    try {
      const notice = await invoke<{
        message: string;
        key: string;
        reason: string;
        backup: string;
        count: number;
      } | null>("get_database_recovery_notice");
      if (notice) {
        options.addLog({
          level: "WARN",
          message: notice.message,
          key: notice.key,
          params: {
            reason: notice.reason,
            backup: notice.backup,
            count: String(notice.count),
          },
          timestamp: Date.now(),
        });
      }    } catch (error) {
      console.warn("Failed to check database recovery notice", error);
    }
  }

  async function setLocale(value: string) {
    if (!isSupportedLocale(value)) return;
    options.setLocale(value);
    if (typeof localStorage !== "undefined") localStorage.setItem("lymic-locale", value);
    try {
      await invoke("update_locale", { locale: value });
    } catch (error) {
      console.error(error);
    }
  }

  async function toggleAutostart() {
    try {
      if (isAutostartEnabled) await disable();
      else await enable();
      isAutostartEnabled = await isEnabled();
    } catch (error) {
      console.error("Failed to update autostart", error);
      options.reportError(error);
    }
  }

  async function toggleVirtualLibrary() {
    const nextEnabled = !virtualLibraryEnabled;
    try {
      await invoke<void>("set_virtual_library_enabled", { enabled: nextEnabled });
      virtualLibraryEnabled = nextEnabled;
    } catch (error) {
      console.error("Failed to update virtual library status", error);
      options.reportError(error);
    }
  }

  async function changeUploadParallelism(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const nextValue = Number(select.value);
    const previousValue = uploadParallelism;
    isSavingUploadParallelism = true;
    try {
      await invoke<void>("set_upload_parallelism", { uploadParallelism: nextValue });
      uploadParallelism = nextValue;
    } catch (error) {
      console.error("Failed to update upload parallelism", error);
      options.reportError(error);
      select.value = String(previousValue);
    } finally {
      isSavingUploadParallelism = false;
    }
  }

  async function changeThumbnailCacheCap(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const nextValue = Number(select.value);
    const previousValue = thumbnailCacheCapBytes;
    try {
      await invoke<void>("set_thumbnail_cache_cap_bytes", { thumbnailCacheCapBytes: nextValue });
      thumbnailCacheCapBytes = nextValue;
    } catch (error) {
      console.error("Failed to update thumbnail cache cap", error);
      options.reportError(error);
      select.value = String(previousValue);
    }
  }

  async function clearThumbnailCache() {
    isClearingThumbnailCache = true;
    try {
      await invoke<void>("clear_thumbnail_cache");
    } catch (error) {
      console.error("Failed to clear thumbnail cache", error);
      options.reportError(error);
    } finally {
      isClearingThumbnailCache = false;
    }
  }

  async function clearVirtualLibraryCache() {
    isClearingVirtualLibraryCache = true;
    try {
      virtualLibraryCacheClearResult = await invoke("clear_virtual_library_cache");
    } catch (error) {
      console.error("Failed to clear Virtual Library cache", error);
      options.reportError(error);
    } finally {
      isClearingVirtualLibraryCache = false;
    }
  }

  function startLifecycle() {
    if (typeof window === "undefined") return;
    applyTheme();
    colorSchemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
    listener = () => {
      if (themePreference === "system") applyTheme();
    };
    colorSchemeQuery.addEventListener("change", listener);
    void listen<{ enabled: boolean; path: string; connectionStatus: string }>("virtual-library-status", ({ payload }) => {
      virtualLibraryEnabled = payload.enabled;
      virtualLibraryPath = payload.path;
      virtualLibraryConnectionStatus = payload.connectionStatus;
    }).then((unlisten) => { unlistenVirtualLibrary = unlisten; });

    const stored = typeof localStorage === "undefined" ? null : localStorage.getItem("lymic-locale");
    if (isSupportedLocale(stored)) void setLocale(stored);
    else {
      if (stored && typeof localStorage !== "undefined") localStorage.removeItem("lymic-locale");
      void setLocale(options.locale());
    }
  }

  function stopLifecycle() {
    if (colorSchemeQuery && listener) colorSchemeQuery.removeEventListener("change", listener);
    unlistenVirtualLibrary?.();
  }

  return {
    get themePreference() { return themePreference; },
    get isAutostartEnabled() { return isAutostartEnabled; },
    get uploadParallelism() { return uploadParallelism; },
    get isSavingUploadParallelism() { return isSavingUploadParallelism; },
    get thumbnailCacheCapBytes() { return thumbnailCacheCapBytes; },
    get isClearingThumbnailCache() { return isClearingThumbnailCache; },
    get isClearingVirtualLibraryCache() { return isClearingVirtualLibraryCache; },
    get virtualLibraryCacheClearResult() { return virtualLibraryCacheClearResult; },
    get virtualLibraryEnabled() { return virtualLibraryEnabled; },
    get virtualLibraryPath() { return virtualLibraryPath; },
    get virtualLibraryConnectionStatus() { return virtualLibraryConnectionStatus; },
    get locales() { return locales; },
    hydrate,
    startLifecycle,
    stopLifecycle,
    setThemePreference,
    setLocale,
    toggleAutostart,
    toggleVirtualLibrary,
    changeUploadParallelism,
    changeThumbnailCacheCap,
    clearThumbnailCache,
    clearVirtualLibraryCache,
  };
}
