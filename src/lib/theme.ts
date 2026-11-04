import type { ThemePreference } from "./contracts";

const themeStorageKey = "lymic-theme";

export function applyThemePreference(themePreference: ThemePreference) {
  if (typeof window === "undefined") return;
  // jsdom and other non-browser runtimes may not expose matchMedia.
  document.documentElement.dataset.theme = themePreference === "system"
    ? (window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light")
    : themePreference;
}

export function initializeThemeFromStorage(): ThemePreference {
  if (typeof window === "undefined") return "system";

  const savedTheme = localStorage.getItem(themeStorageKey);
  const themePreference: ThemePreference = savedTheme === "system" || savedTheme === "light" || savedTheme === "dark"
    ? savedTheme
    : "system";

  if (savedTheme && savedTheme !== themePreference) {
    localStorage.removeItem(themeStorageKey);
  }

  applyThemePreference(themePreference);

  return themePreference;
}
