import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import type { ThemePreference } from "../components/types";
import type { UiLogMessage } from "../contracts";
import { isSupportedLocale } from "../i18n";
import { localeCatalog, type LocaleCatalogEntry } from "../i18n/locale-catalog";
import type { PreferencesDesktopAdapter } from "../tauri/desktop-adapter";

export function createPreferencesController(options: {
  reportError(error: unknown): void;
  addLog(message: UiLogMessage): void;
  locale(): string;
  setLocale(locale: string): void;
  adapter: PreferencesDesktopAdapter;
}) {
  const { adapter } = options;
  const themeStorageKey = "lymic-theme";
  const savedTheme = typeof localStorage === "undefined" ? null : localStorage.getItem(themeStorageKey);
  const initialTheme: ThemePreference = savedTheme === "system" || savedTheme === "light" || savedTheme === "dark"
    ? savedTheme
    : "system";
  let themePreference = $state<ThemePreference>(initialTheme);
  let isAutostartEnabled = $state(false);
  let uploadParallelism = $state(3);
  let isSavingUploadParallelism = $state(false);
  let colorSchemeQuery: MediaQueryList | undefined;
  let listener: (() => void) | undefined;
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
      const catalog = await adapter.getLocaleCatalog();
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
      uploadParallelism = await adapter.getUploadParallelism();
    } catch (error) {
      console.error("Failed to load upload parallelism", error);
      options.reportError(error);
    }

    try {
      const notice = await adapter.getDatabaseRecoveryNotice();
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
      await adapter.updateLocale(value);
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

  async function changeUploadParallelism(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const nextValue = Number(select.value);
    const previousValue = uploadParallelism;
    isSavingUploadParallelism = true;
    try {
      await adapter.setUploadParallelism(nextValue);
      uploadParallelism = nextValue;
    } catch (error) {
      console.error("Failed to update upload parallelism", error);
      options.reportError(error);
      select.value = String(previousValue);
    } finally {
      isSavingUploadParallelism = false;
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

    const stored = typeof localStorage === "undefined" ? null : localStorage.getItem("lymic-locale");
    if (isSupportedLocale(stored)) void setLocale(stored);
    else {
      if (stored && typeof localStorage !== "undefined") localStorage.removeItem("lymic-locale");
      void setLocale(options.locale());
    }
  }

  function stopLifecycle() {
    if (colorSchemeQuery && listener) colorSchemeQuery.removeEventListener("change", listener);
  }

  return {
    get themePreference() { return themePreference; },
    get isAutostartEnabled() { return isAutostartEnabled; },
    get uploadParallelism() { return uploadParallelism; },
    get isSavingUploadParallelism() { return isSavingUploadParallelism; },
    get locales() { return locales; },
    hydrate,
    startLifecycle,
    stopLifecycle,
    setThemePreference,
    setLocale,
    toggleAutostart,
    changeUploadParallelism,
  };
}
