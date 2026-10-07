<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { confirm, open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { Settings as SettingsIcon, AlertCircle, X } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import { isSupportedLocale } from "../lib/i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
  import type { FailedSyncEntry, LogEntry, SyncStatus, ThemePreference, WatchedFolder } from "../lib/components/types";

  // --- State ---
  let isAuthenticated = $state(false);
  let currentView = $state("dashboard");
  let serverUrl = $state("");
  let currentUserName = $state("");
  let apiKey = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");
  let syncStatus = $state<SyncStatus>("idle");
  let lastSync = $state("Noch nie");
  let progress = $state(0);
  let processed = $state(0);
  let syncTotal = $state(0);
  let currentFile = $state("");
  let watchedFolders = $state<WatchedFolder[]>([]);
  let failedSyncs = $state<FailedSyncEntry[]>([]);
  let isAutostartEnabled = $state(false);
  let logs = $state<LogEntry[]>([]);
  let actionError = $state("");
  let uploadParallelism = $state(3);
  let isSavingUploadParallelism = $state(false);
  let componentMounted = false;
  let progressResetTimer: ReturnType<typeof setTimeout> | undefined;
  let isSyncActionPending = $state(false);
  let appVersion = $state<string | undefined>();

  const themeStorageKey = "lymic-theme";

  function isThemePreference(value: string | null): value is ThemePreference {
    return value === "system" || value === "light" || value === "dark";
  }

  const savedTheme = typeof localStorage !== "undefined" ? localStorage.getItem(themeStorageKey) : null;
  let themePreference = $state<ThemePreference>("system");
  if (isThemePreference(savedTheme)) {
    themePreference = savedTheme;
  } else if (savedTheme) {
    localStorage.removeItem(themeStorageKey);
  }

  function applyTheme() {
    const systemThemeIsDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    const effectiveTheme = themePreference === "system"
      ? (systemThemeIsDark ? "dark" : "light")
      : themePreference;
    document.documentElement.dataset.theme = effectiveTheme;
  }

  function setThemePreference(preference: ThemePreference) {
    themePreference = preference;
    localStorage.setItem(themeStorageKey, preference);
    applyTheme();
  }

  if (typeof window !== "undefined") {
    applyTheme();
  }
  
  // Initialize locale
  const savedLocale = typeof localStorage !== 'undefined' ? localStorage.getItem('lymic-locale') : null;
  if (isSupportedLocale(savedLocale)) {
    $locale = savedLocale;
  } else if (savedLocale) {
    localStorage.removeItem('lymic-locale');
  }

  // Persist locale and notify backend
  $effect(() => {
    if (!isSupportedLocale($locale)) {
      $locale = 'en';
      return;
    }

    const currentLocale = $locale;
    localStorage.setItem('lymic-locale', currentLocale);
    invoke('update_locale', { locale: currentLocale }).catch(console.error);
  });

  interface UiLogMessage {
    level?: string;
    message?: string;
    timestamp?: number;
  }

  interface SyncSummary {
    processed: number;
    uploaded: number;
    failed: number;
  }

  interface SyncSnapshot {
    status: "RUNNING" | "PAUSED" | "IDLE";
    total: number;
    succeeded: number;
    failed: number;
    currentPath: string | null;
    totalBytes?: number;
    completedBytes?: number;
  }

  let logCounter = 0;

  function formatError(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    if (
      typeof error === "object" &&
      error !== null &&
      "message" in error &&
      typeof error.message === "string"
    ) {
      return error.message;
    }
    return $t("unknown_error");
  }

  function toLogEntry(payload: UiLogMessage | null | undefined): LogEntry {
    let level: LogEntry["level"] = "info";
    switch (payload?.level) {
      case "ERROR": level = "error"; break;
      case "SUCCESS": level = "success"; break;
      case "WARN": level = "warn"; break;
    }

    const timestamp = typeof payload?.timestamp === "number" && Number.isFinite(payload.timestamp)
      ? payload.timestamp
      : Date.now();
    const time = new Date(timestamp).toLocaleTimeString(
      $locale === 'de' ? 'de-DE' : 'en-US',
      { hour: "2-digit", minute: "2-digit", hour12: false },
    );
    return { id: ++logCounter, time, level, message: payload?.message ?? "" };
  }

  function applySyncSnapshot(snapshot: SyncSnapshot | null | undefined) {
    if (!snapshot) return;
    syncStatus = snapshot.status === "PAUSED" ? "paused" : snapshot.status === "RUNNING" ? "syncing" : snapshot.failed > 0 ? "error" : "idle";
    processed = snapshot.succeeded + snapshot.failed;
    syncTotal = snapshot.total;
    if (snapshot.totalBytes && snapshot.totalBytes > 0) {
      progress = Math.min(100, Math.round(((snapshot.completedBytes ?? 0) / snapshot.totalBytes) * 100));
    } else if (snapshot.total > 0) {
      progress = Math.min(100, Math.round(((snapshot.succeeded + snapshot.failed) / snapshot.total) * 100));
    } else {
      progress = 0;
    }
    if (snapshot.currentPath) currentFile = snapshot.currentPath.split(/[/\\]/).pop() || "";
  }

  // Store unlisteners outside the async IIFE so onMount can return them synchronously.
  // Previously the cleanup was returned from the IIFE (a Promise), which Svelte ignores —
  // all event listeners were permanently leaking on unmount.
  onMount(() => {
    let unlisteners: (() => void)[] = [];
    const colorSchemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleColorSchemeChange = () => {
      if (themePreference === "system") applyTheme();
    };
    colorSchemeQuery.addEventListener("change", handleColorSchemeChange);
    componentMounted = true;

    void getVersion()
      .then((version) => {
        if (componentMounted) appVersion = version;
      })
      .catch((error) => {
        console.warn("Failed to load app version", error);
      });

    async function setupListeners() {
      const listeners = await Promise.all([
        listen("sync-progress", (event) => {
          if (!componentMounted) return;
          syncStatus = "syncing";
          currentFile = (event.payload as string).split(/[/\\]/).pop() || "";
        }),
        listen("sync-progress-percent", (event) => {
          if (!componentMounted) return;
          progress = event.payload as number;
        }),
        listen("sync-started", () => {
          if (!componentMounted) return;
          if (progressResetTimer) clearTimeout(progressResetTimer);
          progressResetTimer = undefined;
          syncStatus = "syncing";
        }),
        listen("sync-progress-snapshot", (event) => {
          if (componentMounted) applySyncSnapshot(event.payload as SyncSnapshot);
        }),
        listen("sync-paused", (event) => {
          if (componentMounted) applySyncSnapshot(event.payload as SyncSnapshot);
        }),
        listen("sync-resumed", (event) => {
          if (componentMounted) applySyncSnapshot(event.payload as SyncSnapshot);
        }),
        listen("sync-error", () => {
          if (!componentMounted) return;
          syncStatus = "error";
          progress = 0;
        }),
        listen("trigger-sync", () => {
          if (!componentMounted) return;
          handleStartSync();
        }),
        listen("log-message", (event) => {
          if (!componentMounted) return;
          const entry = toLogEntry(event.payload as UiLogMessage);
          logs = [entry, ...logs].slice(0, 50);
        }),
        listen("sync-idle", () => {
          void finishSync();
        }),
      ]);

      if (!componentMounted) {
        listeners.forEach(un => un());
      } else {
        unlisteners = listeners;
      }
    }

    (async () => {
      try {
        isAuthenticated = await invoke<boolean>("get_auth_status");
        if (isAuthenticated) {
          serverUrl = await invoke<string>("get_server_url");
          try {
            currentUserName = await invoke<string>("get_current_user_name");
          } catch (e) {
            console.warn("Failed to load current user", e);
          }
          await refreshFailedSyncs();
        }
          await refreshFolders();
          try {
            applySyncSnapshot(await invoke<SyncSnapshot>("get_sync_status"));
          } catch (e) {
            console.warn("Failed to load sync status", e);
          }
        isAutostartEnabled = await isEnabled();
        try {
          uploadParallelism = await invoke<number>("get_upload_parallelism");
        } catch (e) {
          console.error("Failed to load upload parallelism", e);
          actionError = formatError(e);
        }

        try {
          const recoveryNotice = await invoke<string | null>("get_database_recovery_notice");
          if (recoveryNotice && componentMounted) {
            const entry = toLogEntry({ level: "WARN", message: recoveryNotice, timestamp: Date.now() });
            logs = [entry, ...logs].slice(0, 50);
          }
        } catch (e) {
          console.warn("Failed to check database recovery notice", e);
        }
        
        if (componentMounted) {
          await setupListeners();
        }
      } catch (e) {
        console.error("Init failed", e);
      }
    })();

    return () => {
      componentMounted = false;
      if (progressResetTimer) clearTimeout(progressResetTimer);
      unlisteners.forEach(fn => fn());
      colorSchemeQuery.removeEventListener("change", handleColorSchemeChange);
    };
  });

  async function finishSync(summary?: SyncSummary) {
    if (!componentMounted) return;
    await refreshFailedSyncs();
    if (!componentMounted) return;
    if (syncStatus === "error" || (summary && summary.failed > 0) || failedSyncs.length > 0) {
      syncStatus = "error";
      progress = 0;
      return;
    }
    syncStatus = "idle";
    progress = 100;
    lastSync = new Date().toLocaleTimeString($locale === 'de' ? 'de-DE' : 'en-US', { hour: "2-digit", minute: "2-digit" });
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = setTimeout(() => {
      if (componentMounted) progress = 0;
      progressResetTimer = undefined;
    }, 2000);
  }

  async function handleStartSync() {
    if (syncStatus === "syncing" || isSyncActionPending) {
      console.warn("Sync already in progress, ignoring trigger.");
      return;
    }
    if (watchedFolders.length === 0) {
      console.warn("Sync requires at least one watched folder, ignoring trigger.");
      return;
    }
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    syncStatus = "syncing";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    try {
      const summary = await invoke<SyncSummary>("start_sync");
      // The command response is reliable even when an event is missed.
      await finishSync(summary);
    } catch (e) {
      console.error("Sync failed", e);
      syncStatus = "error";
    }
  }

  async function handleSyncAction() {
    if (isSyncActionPending) return;
    if (syncStatus === "syncing" || syncStatus === "paused") {
      isSyncActionPending = true;
      try {
        const command = syncStatus === "syncing" ? "pause_sync" : "resume_sync";
        applySyncSnapshot(await invoke<SyncSnapshot>(command));
      } catch (e) {
        actionError = formatError(e);
      } finally {
        isSyncActionPending = false;
      }
      return;
    }
    await handleStartSync();
  }

  async function refreshFailedSyncs() {
    try {
      failedSyncs = await invoke<FailedSyncEntry[]>("get_failed_syncs");
      if (failedSyncs.length > 0 && syncStatus === "idle") {
        syncStatus = "error";
      }
    } catch (e) {
      console.error("Failed to load sync failures", e);
      actionError = formatError(e);
    }
  }

  async function handleRetryFailedSyncs() {
    if (syncStatus === "syncing" || failedSyncs.length === 0) return;
    actionError = "";
    if (progressResetTimer) clearTimeout(progressResetTimer);
    progressResetTimer = undefined;
    syncStatus = "syncing";
    progress = 0;
    processed = 0;
    syncTotal = 0;
    try {
      const summary = await invoke<SyncSummary>("retry_failed_syncs");
      await finishSync(summary);
    } catch (e) {
      console.error("Failed to retry sync failures", e);
      actionError = formatError(e);
      await refreshFailedSyncs();
      syncStatus = "error";
    }
  }

  // Log errors instead of silently swallowing them.
  async function refreshFolders() {
    try {
      watchedFolders = await invoke<WatchedFolder[]>("get_folders");
    } catch (e) {
      console.error("Failed to load folders", e);
      actionError = formatError(e);
    }
  }

  async function handleAddFolder() {
    actionError = "";
    try {
      const selected = await open({ directory: true, multiple: false, title: $t('select_folder') });
      if (selected && typeof selected === "string") {
        await invoke<number>("add_folder", { path: selected });
        await refreshFolders();
      }
    } catch (e) {
      console.error("Failed to add folder", e);
      actionError = formatError(e);
    }
  }

  async function handleRemoveFolder(id: number) {
    actionError = "";
    try {
      await invoke<void>("remove_folder", { id });
      await refreshFolders();
    } catch (e) {
      console.error("Failed to remove folder", e);
      actionError = formatError(e);
    }
  }

  async function toggleAutostart() {
    actionError = "";
    try {
      if (isAutostartEnabled) await disable(); else await enable();
      isAutostartEnabled = await isEnabled();
    } catch (e) {
      console.error("Failed to update autostart", e);
      actionError = formatError(e);
    }
  }

  async function handleUploadParallelismChange(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const nextValue = Number(select.value);
    const previousValue = uploadParallelism;
    actionError = "";
    isSavingUploadParallelism = true;

    try {
      await invoke<void>("set_upload_parallelism", { uploadParallelism: nextValue });
      uploadParallelism = nextValue;
    } catch (e) {
      console.error("Failed to update upload parallelism", e);
      actionError = formatError(e);
      select.value = String(previousValue);
    } finally {
      isSavingUploadParallelism = false;
    }
  }

  async function handleLogin() {
    if (!serverUrl || !apiKey) return;
    isLoggingIn = true;
    loginError = "";
    try {
      await invoke<void>("login", { serverUrl, apiKey });
      isAuthenticated = true;
      await refreshFailedSyncs();
      try {
        currentUserName = await invoke<string>("get_current_user_name");
      } catch (e) {
        // The credentials are valid even if the optional account label cannot load.
        console.warn("Failed to load current user", e);
      }
    } catch (e) {
      loginError = formatError(e);
    } finally {
      isLoggingIn = false;
    }
  }

  // Extracted from inline onclick. Awaits backend call before resetting UI state.
  // Previously invoke("logout") was fire-and-forget; on failure credentials stayed in keyring
  // while the UI showed the login screen.
  async function handleLogout() {
    const shouldLogout = await confirm($t("logout_confirmation"), {
      title: $t("logout"),
      kind: "warning",
    });
    if (!shouldLogout) return;

    actionError = "";
    try {
      await invoke<void>("logout");
      isAuthenticated = false;
      apiKey = "";
      currentUserName = "";
      failedSyncs = [];
      currentView = "dashboard";
    } catch (e) {
      console.error("Logout failed", e);
      actionError = formatError(e);
    }
  }
</script>

<div class="h-screen flex selection:bg-blue-600 selection:text-white overflow-hidden bg-[#F1F5F9] dark:bg-slate-950">
  
  <!-- Left Side: Hero Image (Permanent) -->
  <aside class="w-[38%] h-full shrink-0 relative overflow-hidden">
    <div class="hero-container">
      <img src={hero} 
           alt="Hero Landscape" 
           class="hero-image" />
      <div class="absolute inset-0 bg-gradient-to-t from-black/50 to-transparent"></div>
    </div>
    
    <div class="absolute top-12 left-12 flex items-center gap-3 text-white">
      <img src={logo} alt="Lymic Logo" class="w-16 h-16 object-contain" />
      <span class="font-bold tracking-tight text-3xl">Lymic</span>
    </div>

    <div class="absolute bottom-12 left-12 right-12 text-white">
      <p class="text-xs font-semibold uppercase tracking-[0.3em] opacity-60 mb-2">{$t('hero_tagline')}</p>
      <h1 class="text-4xl font-bold tracking-tight">
        {$t('hero_title_line_1')}<br />
        {$t('hero_title_line_2')}
      </h1>
    </div>
  </aside>

  <!-- Right Side: Modern Glass Content -->
  <div class="flex-1 flex flex-col h-full relative overflow-hidden">
    
    <!-- Header (Simple & Glass) -->
    <header class="h-20 px-10 flex items-center justify-between shrink-0 z-10">
      {#if isAuthenticated}
        <div class="flex items-center gap-4">
          <!-- h2 → h1 for correct heading hierarchy in the content column -->
          <h1 class="text-xl font-bold text-slate-800 dark:text-slate-100">
            {currentView === "dashboard" ? $t('dashboard') : $t('settings')}
          </h1>
        </div>
        <div class="flex items-center gap-2">
          <button class="w-10 h-10 rounded-full flex items-center justify-center hover:bg-white/50 dark:hover:bg-white/10 transition-colors"
                  onclick={() => currentView = currentView === "settings" ? "dashboard" : "settings"}>
            {#if currentView === "settings"}
              <X class="w-5 h-5 text-slate-900 dark:text-slate-100" />
            {:else}
              <SettingsIcon class="w-5 h-5 text-slate-400 dark:text-slate-400" />
            {/if}
          </button>
        </div>
      {/if}
    </header>

    <main class="flex-1 overflow-y-auto p-10 flex flex-col {!isAuthenticated ? 'justify-center' : 'space-y-10'}">
      {#if isAuthenticated && actionError}
        <div class="p-4 bg-red-50 dark:bg-red-950/50 text-red-600 dark:text-red-300 text-xs rounded-xl flex items-center gap-3" role="alert">
          <AlertCircle class="w-4 h-4 shrink-0" />
          <span>{actionError}</span>
          <button
            type="button"
            class="ml-auto p-1 rounded hover:bg-red-100 dark:hover:bg-red-900/50 transition-colors"
            aria-label={$t('dismiss_error')}
            onclick={() => actionError = ""}
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      {/if}
      
      {#if !isAuthenticated}
        <LoginView bind:serverUrl bind:apiKey {isLoggingIn} {loginError} onLogin={handleLogin} />

      <!-- ===== DASHBOARD VIEW ===== -->
      {:else if currentView === "dashboard"}
        <DashboardView
          {syncStatus}
          {lastSync}
          {progress}
          {processed}
          {syncTotal}
          {currentFile}
          {serverUrl}
          {watchedFolders}
          {failedSyncs}
          {logs}
          {isSyncActionPending}
          onSyncAction={handleSyncAction}
          onRetryFailedSyncs={handleRetryFailedSyncs}
          onAddFolder={handleAddFolder}
          onRemoveFolder={handleRemoveFolder}
        />
       <!-- ===== SETTINGS VIEW ===== -->
      {:else if currentView === "settings"}
        <SettingsView
          {currentUserName}
          {serverUrl}
          {isAutostartEnabled}
          {themePreference}
          {uploadParallelism}
          {isSavingUploadParallelism}
          onToggleAutostart={toggleAutostart}
          onLocaleChange={(nextLocale) => $locale = nextLocale}
          onThemeChange={setThemePreference}
          onUploadParallelismChange={handleUploadParallelismChange}
          onLogout={handleLogout}
        />
       {/if}

    </main>

    <!-- Global Footer -->
    <footer class="h-16 px-10 border-t border-black/5 dark:border-white/10 flex items-center justify-end text-[10px] font-bold text-slate-400 dark:text-slate-400 uppercase tracking-widest shrink-0">
      <span>Lymic - Unofficial Immich Desktop Client</span>
      {#if appVersion}
        <span class="ml-2">v{appVersion}</span>
      {/if}
    </footer>

  </div>
</div>
