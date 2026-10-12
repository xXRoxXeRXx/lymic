import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";

const LOCAL_HTTP_NOT_ALLOWED_ERROR = "LOCAL_HTTP_NOT_ALLOWED";
const CLOUDFLARE_ACCESS_PAIR_REQUIRED_ERROR = "CLOUDFLARE_ACCESS_PAIR_REQUIRED";

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
  let allowLocalHttp = $state(false);
  let cloudflareClientId = $state("");
  let cloudflareClientSecret = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");
  let loginErrorCode = $state("");

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
    const normalizedCloudflareClientId = cloudflareClientId.trim();
    const normalizedCloudflareClientSecret = cloudflareClientSecret.trim();
    if (Boolean(normalizedCloudflareClientId) !== Boolean(normalizedCloudflareClientSecret)) {
      loginError = options.t("cloudflare_access_pair_required");
      loginErrorCode = CLOUDFLARE_ACCESS_PAIR_REQUIRED_ERROR;
      return;
    }
    isLoggingIn = true;
    loginError = "";
    loginErrorCode = "";
    try {
      await invoke<void>("login", {
        serverUrl,
        apiKey,
        allowLocalHttp,
        cloudflareClientId: normalizedCloudflareClientId || null,
        cloudflareClientSecret: normalizedCloudflareClientSecret || null,
      });
      isAuthenticated = true;
      await options.onLogin();
      await loadCurrentUser();
    } catch (error) {
      const message = options.formatError(error);
      if (message.startsWith(`${LOCAL_HTTP_NOT_ALLOWED_ERROR}:`)) {
        loginError = options.t("local_http_not_allowed");
        loginErrorCode = LOCAL_HTTP_NOT_ALLOWED_ERROR;
      } else {
        loginError = message;
      }
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
      allowLocalHttp = false;
      loginErrorCode = "";
      cloudflareClientId = "";
      cloudflareClientSecret = "";
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
    get allowLocalHttp() { return allowLocalHttp; },
    set allowLocalHttp(value: boolean) { allowLocalHttp = value; },
    get cloudflareClientId() { return cloudflareClientId; },
    set cloudflareClientId(value: string) { cloudflareClientId = value; },
    get cloudflareClientSecret() { return cloudflareClientSecret; },
    set cloudflareClientSecret(value: string) { cloudflareClientSecret = value; },
    get isLoggingIn() { return isLoggingIn; },
    get loginError() { return loginError; },
    get loginErrorCode() { return loginErrorCode; },
    hydrate,
    login,
    logout,
  };
}
