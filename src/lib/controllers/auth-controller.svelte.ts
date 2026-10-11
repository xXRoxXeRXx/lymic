import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";

export function createAuthController(options: {
  formatError: (error: unknown) => string;
  onLogin: () => Promise<void> | void;
  onLogout: () => Promise<void> | void;
  reportError: (error: unknown) => void;
  t: (key: string) => string;
}) {
  let isAuthenticated = $state(false);
  let serverUrl = $state("");
  let currentUserName = $state("");
  let apiKey = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");

  async function loadCurrentUser() {
    try {
      currentUserName = await invoke<string>("get_current_user_name");
    } catch (error) {
      console.warn("Failed to load current user", error);
    }
  }

  async function hydrate() {
    isAuthenticated = await invoke<boolean>("get_auth_status");
    if (!isAuthenticated) return;
    serverUrl = await invoke<string>("get_server_url");
    await loadCurrentUser();
  }

  async function login() {
    if (!serverUrl || !apiKey) return;
    isLoggingIn = true;
    loginError = "";
    try {
      await invoke<void>("login", { serverUrl, apiKey });
      isAuthenticated = true;
      await options.onLogin();
      await loadCurrentUser();
    } catch (error) {
      loginError = options.formatError(error);
    } finally {
      isLoggingIn = false;
    }
  }

  async function logout() {
    const shouldLogout = await confirm(options.t("logout_confirmation"), {
      title: options.t("logout"),
      kind: "warning",
    });
    if (!shouldLogout) return;
    try {
      await invoke<void>("logout");
      isAuthenticated = false;
      apiKey = "";
      currentUserName = "";
      await options.onLogout();
    } catch (error) {
      console.error("Logout failed", error);
      options.reportError(error);
    }
  }

  return {
    get isAuthenticated() { return isAuthenticated; },
    get serverUrl() { return serverUrl; },
    set serverUrl(value: string) { serverUrl = value; },
    get currentUserName() { return currentUserName; },
    get apiKey() { return apiKey; },
    set apiKey(value: string) { apiKey = value; },
    get isLoggingIn() { return isLoggingIn; },
    get loginError() { return loginError; },
    hydrate,
    login,
    logout,
  };
}
