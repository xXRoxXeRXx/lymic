<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { confirm, open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import {
    Settings as SettingsIcon, FolderPlus, Folder,
    Trash2, ArrowLeft, Cloud, CloudUpload,
    AlertCircle, Info, RefreshCw, Clock, X, User,
    ChevronRight, ExternalLink, Minus, Activity, Power, Globe
  } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import { isSupportedLocale } from "../lib/i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";

  // --- State ---
  let isAuthenticated = $state(false);
  let currentView = $state("dashboard");
  let serverUrl = $state("");
  let currentUserName = $state("");
  let apiKey = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");
  let syncStatus = $state<"idle" | "syncing" | "error">("idle");
  let lastSync = $state("Noch nie");
  let progress = $state(0);
  let currentFile = $state("");
  let watchedFolders = $state<WatchedFolder[]>([]);
  let isAutostartEnabled = $state(false);
  let logs = $state<LogEntry[]>([]);
  let actionError = $state("");
  let componentMounted = false;
  let progressResetTimer: ReturnType<typeof setTimeout> | undefined;
  
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

  interface LogEntry {
    id: number;
    time: string;
    level: "info" | "success" | "error" | "warn";
    message: string;
    raw: string;
  }

  interface SyncSummary {
    processed: number;
    uploaded: number;
    failed: number;
  }

  interface WatchedFolder {
    id: number;
    path: string;
    recursive: boolean;
    target_album_id: string | null;
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

  function parseLog(raw: string): LogEntry {
    const timeMatch = raw.match(/\[(\d{2}:\d{2}:\d{2})\]/);
    const levelMatch = raw.match(/\[(INFO|ERROR|SUCCESS|WARN)\]/i);
    const time = timeMatch?.[1] ?? new Date().toLocaleTimeString($locale === 'de' ? 'de-DE' : 'en-US', { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const rawLevel = (levelMatch?.[1] ?? "INFO").toUpperCase();

    let level: LogEntry["level"] = "info";
    if (rawLevel === "ERROR") level = "error";
    else if (rawLevel === "SUCCESS") level = "success";
    else if (rawLevel === "WARN") level = "warn";

    let message = raw.replace(/\[\d{2}:\d{2}:\d{2}\]/, "").replace(/\[(INFO|ERROR|SUCCESS|WARN)\]/i, "").trim();

    return { id: ++logCounter, time, level, message, raw };
  }

  // Store unlisteners outside the async IIFE so onMount can return them synchronously.
  // Previously the cleanup was returned from the IIFE (a Promise), which Svelte ignores —
  // all event listeners were permanently leaking on unmount.
  onMount(() => {
    let unlisteners: (() => void)[] = [];
    componentMounted = true;

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
          progress = 0;
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
          const entry = parseLog(event.payload as string);
          logs = [entry, ...logs].slice(0, 50);
        }),
        listen("sync-idle", () => {
          finishSync();
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
        }
        await refreshFolders();
        isAutostartEnabled = await isEnabled();

        try {
          const recoveryNotice = await invoke<string | null>("get_database_recovery_notice");
          if (recoveryNotice && componentMounted) {
            const entry = parseLog(`[WARN] ${recoveryNotice}`);
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
    };
  });

  function finishSync(summary?: SyncSummary) {
    if (!componentMounted) return;
    if (syncStatus === "error" || (summary && summary.failed > 0)) {
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
    if (syncStatus === "syncing") {
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
    try {
      const summary = await invoke<SyncSummary>("start_sync");
      // The command response is reliable even when an event is missed.
      finishSync(summary);
    } catch (e) {
      console.error("Sync failed", e);
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

  async function handleLogin() {
    if (!serverUrl || !apiKey) return;
    isLoggingIn = true;
    loginError = "";
    try {
      await invoke<void>("login", { serverUrl, apiKey });
      isAuthenticated = true;
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
      currentView = "dashboard";
    } catch (e) {
      console.error("Logout failed", e);
      actionError = formatError(e);
    }
  }
</script>

<div class="h-screen flex selection:bg-blue-600 selection:text-white overflow-hidden bg-[#F1F5F9]">
  
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
          <h1 class="text-xl font-bold text-slate-800">
            {currentView === "dashboard" ? $t('dashboard') : $t('settings')}
          </h1>
        </div>
        <div class="flex items-center gap-2">
          <button class="w-10 h-10 rounded-full flex items-center justify-center hover:bg-white/50 transition-colors"
                  onclick={() => currentView = currentView === "settings" ? "dashboard" : "settings"}>
            {#if currentView === "settings"}
              <X class="w-5 h-5 text-slate-900" />
            {:else}
              <SettingsIcon class="w-5 h-5 text-slate-400" />
            {/if}
          </button>
        </div>
      {/if}
    </header>

    <main class="flex-1 overflow-y-auto p-10 flex flex-col {!isAuthenticated ? 'justify-center' : 'space-y-10'}">
      {#if isAuthenticated && actionError}
        <div class="p-4 bg-red-50 text-red-600 text-xs rounded-xl flex items-center gap-3" role="alert">
          <AlertCircle class="w-4 h-4 shrink-0" />
          <span>{actionError}</span>
          <button
            type="button"
            class="ml-auto p-1 rounded hover:bg-red-100 transition-colors"
            aria-label="Fehlermeldung schließen"
            onclick={() => actionError = ""}
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      {/if}
      
      <!-- ===== LOGIN VIEW ===== -->
      {#if !isAuthenticated}
        <div class="max-w-md w-full mx-auto space-y-10 animate-in">
          <div class="text-center space-y-4">
            <!-- h2 → h1; the aside's h1 is in a separate sectioning element -->
            <h1 class="text-3xl font-bold text-slate-900">{$t('login')}</h1>
            <p class="text-slate-500 text-sm">{$t('login_subtitle')}</p>
          </div>

          <div class="glass-pane space-y-8">
            <div class="space-y-6">
              <!-- Added for/id association so labels activate their inputs on click -->
              <div class="space-y-2">
                <label for="server-url" class="text-xs font-bold text-slate-400 uppercase tracking-wider ml-1">{$t('server_url')}</label>
                <input
                  id="server-url"
                  type="text"
                  placeholder={$t('server_url_placeholder')}
                  class="glass-input"
                  bind:value={serverUrl}
                />
              </div>
              <div class="space-y-2">
                <label for="api-key" class="text-xs font-bold text-slate-400 uppercase tracking-wider ml-1">{$t('api_key')}</label>
                <input
                  id="api-key"
                  type="password"
                  placeholder={$t('api_key_placeholder')}
                  class="glass-input"
                  bind:value={apiKey}
                />
              </div>
            </div>

            {#if loginError}
              <div class="p-4 bg-red-50 text-red-600 text-xs rounded-xl flex items-center gap-3">
                <AlertCircle class="w-4 h-4" />
                <span>{loginError}</span>
              </div>
            {/if}

            <button
              class="btn-action w-full"
              onclick={handleLogin}
              disabled={isLoggingIn || !serverUrl || !apiKey}
            >
              {isLoggingIn ? $t('connect') : $t('login')}
            </button>
          </div>
        </div>

      <!-- ===== DASHBOARD VIEW ===== -->
      {:else if currentView === "dashboard"}
        <!-- Sync Status Card -->
        <section class="glass-pane space-y-8 animate-in">
          <div class="flex items-center justify-between">
            <div class="space-y-1">
              <div class="flex items-center gap-2">
                  <div class="w-2 h-2 rounded-full {syncStatus === 'syncing' ? 'bg-blue-500 animate-pulse' : syncStatus === 'error' ? 'bg-red-500' : 'bg-emerald-500'}"></div>
                 <span class="text-xs font-bold text-slate-400 uppercase tracking-wider">{$t('status')}</span>
              </div>
              <h3 class="text-3xl font-bold text-slate-900">
                 {syncStatus === 'syncing' ? $t('sync_running') : syncStatus === 'error' ? $t('sync_completed_with_errors') : $t('all_up_to_date')}
              </h3>
            </div>
            <button
              class="btn-action"
              onclick={handleStartSync}
              disabled={syncStatus === 'syncing' || watchedFolders.length === 0}
              title={watchedFolders.length === 0 ? $t('sync_requires_folder') : undefined}
              aria-describedby={watchedFolders.length === 0 ? 'sync-requires-folder' : undefined}
            >
              {syncStatus === 'syncing' ? $t('syncing') : $t('sync_now')}
            </button>
            {#if watchedFolders.length === 0}
              <span id="sync-requires-folder" class="sr-only">{$t('sync_requires_folder')}</span>
            {/if}
          </div>

          {#if syncStatus === 'syncing'}
            <div class="space-y-3">
              <div class="glass-progress">
                <div class="glass-progress-fill" style="width: {progress}%;"></div>
              </div>
              <div class="flex justify-between text-xs font-medium text-slate-400">
                <span>{progress}% {$t('completed')}</span>
                <span class="truncate max-w-[250px]">{currentFile}</span>
              </div>
            </div>
          {:else}
            <div class="flex gap-10 pt-2 border-t border-black/5">
              <div class="space-y-1">
                <p class="text-xs font-bold text-slate-300 uppercase tracking-wider">{$t('last_sync')}</p>
                <p class="font-semibold text-slate-600">{lastSync === 'Noch nie' ? $t('never') : lastSync}</p>
              </div>
              <div class="space-y-1">
                <p class="text-xs font-bold text-slate-300 uppercase tracking-wider">{$t('server')}</p>
                <p class="font-semibold text-slate-600 truncate max-w-[150px]">{serverUrl.replace(/https?:\/\//, '')}</p>
              </div>
            </div>
          {/if}
        </section>

        <!-- Folders & Logs Grid -->
        <div class="grid grid-cols-1 xl:grid-cols-2 gap-8">
          
          <!-- Folder Management -->
          <div class="space-y-6 animate-in" style="animation-delay: 100ms">
            <div class="flex items-center justify-between px-2">
              <h4 class="text-sm font-bold text-slate-800 uppercase tracking-widest">{$t('synced_folders')}</h4>
              <button class="w-8 h-8 rounded-full bg-blue-600 text-white flex items-center justify-center hover:bg-blue-700 transition-colors" 
                      onclick={handleAddFolder}>
                <FolderPlus class="w-4 h-4" />
              </button>
            </div>
            
            <div class="space-y-4">
              {#each watchedFolders as folder}
                <div class="glass-pane !p-4 flex items-center justify-between group">
                  <div class="flex items-center gap-4 min-w-0">
                    <div class="w-10 h-10 rounded-xl bg-slate-100 flex items-center justify-center text-slate-400 group-hover:bg-blue-50 group-hover:text-blue-600 transition-colors">
                      <Folder class="w-5 h-5" />
                    </div>
                    <div class="min-w-0">
                      <p class="text-sm font-bold text-slate-900 truncate">{folder.path.split(/[/\\]/).pop()}</p>
                      <p class="text-[10px] font-mono text-slate-400 truncate">{folder.path}</p>
                    </div>
                  </div>
                  <button
                    class="p-2 text-slate-300 hover:text-red-500 opacity-0 group-hover:opacity-100 transition-all"
                    aria-label={`${$t('remove_folder')}: ${folder.path}`}
                    onclick={() => handleRemoveFolder(folder.id)}
                  >
                    <Trash2 class="w-4 h-4" />
                  </button>
                </div>
              {/each}
              {#if watchedFolders.length === 0}
                  <div class="glass-pane !p-12 text-center border-dashed border-2">
                    <p class="text-sm text-slate-400 italic">{$t('no_folders')}</p>
                  </div>
              {/if}
            </div>
          </div>

          <!-- Activity Log -->
          <div class="space-y-6 animate-in" style="animation-delay: 200ms">
             <h4 class="text-sm font-bold text-slate-800 uppercase tracking-widest px-2">{$t('activity_log')}</h4>
             <!-- Was slicing to 10 in the template while storing 50 in state — now shows all stored entries -->
             <div class="glass-pane !p-6 h-full max-h-[400px] overflow-y-auto space-y-4">
               {#if logs.length === 0}
                 <p class="text-xs text-slate-300 italic">{$t('no_activity')}</p>
               {:else}
                 {#each logs as log}
                   <div class="flex items-start gap-4 text-[11px] leading-relaxed group">
                     <span class="text-slate-300 font-mono w-14 shrink-0">{log.time.split(':').slice(0,2).join(':')}</span>
                     <span class="text-slate-500 group-hover:text-slate-900 transition-colors">{log.message}</span>
                   </div>
                 {/each}
               {/if}
             </div>
          </div>
        </div>

      <!-- ===== SETTINGS VIEW ===== -->
      {:else if currentView === "settings"}
        <div class="space-y-10 animate-in">
          
          <div class="glass-pane space-y-2 !p-0 overflow-hidden">
            <div class="p-8 flex items-center justify-between border-b border-black/5">
              <div class="space-y-1">
                <p class="font-bold text-slate-900">{$t('autostart')}</p>
                <p class="text-xs text-slate-500">{$t('autostart_subtitle')}</p>
              </div>
              <button
                class="w-12 h-6 border rounded-full relative transition-all {isAutostartEnabled ? 'bg-blue-600 border-blue-600' : 'bg-slate-200 border-slate-200'}"
                onclick={toggleAutostart}
                aria-label={isAutostartEnabled ? 'Autostart deaktivieren' : 'Autostart aktivieren'}
                aria-pressed={isAutostartEnabled}
              >
                <div class="absolute top-1 left-1 w-4 h-4 rounded-full bg-white shadow-sm transition-all {isAutostartEnabled ? 'translate-x-6' : 'translate-x-0'}"></div>
              </button>
            </div>

            <div class="p-8 flex items-center justify-between border-b border-black/5">
              <div class="space-y-1">
                <p class="font-bold text-slate-900">Language / Sprache</p>
                <p class="text-xs text-slate-500">Choose your preferred language.</p>
              </div>
              <div class="flex gap-2">
                <button 
                  class="px-3 py-1 rounded-lg text-xs font-bold transition-colors {$locale === 'en' ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-600 hover:bg-slate-200'}"
                  onclick={() => $locale = 'en'}>
                  EN
                </button>
                <button 
                  class="px-3 py-1 rounded-lg text-xs font-bold transition-colors {$locale === 'de' ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-600 hover:bg-slate-200'}"
                  onclick={() => $locale = 'de'}>
                  DE
                </button>
              </div>
            </div>

            <div class="p-8 flex items-center justify-between">
              <div class="space-y-1">
                <p class="font-bold text-slate-900">{$t('account')}</p>
                {#if currentUserName}
                  <p class="text-sm font-semibold text-slate-700 flex items-center gap-2">
                    <User class="w-4 h-4 text-slate-400" />
                    {$t('logged_in_as')} {currentUserName}
                  </p>
                {/if}
                <p class="text-xs text-slate-500">{$t('logged_in_at')} {serverUrl}</p>
              </div>
              <!-- Was fire-and-forget inline onclick; now uses async handleLogout -->
              <button class="text-xs font-bold text-red-500 hover:bg-red-50 px-4 py-2 rounded-lg transition-colors" 
                      onclick={handleLogout}>
                {$t('logout')}
              </button>
            </div>
          </div>
        </div>
      {/if}

    </main>

    <!-- Global Footer -->
    <footer class="h-16 px-10 border-t border-black/5 flex items-center justify-end text-[10px] font-bold text-slate-400 uppercase tracking-widest shrink-0">
      <span>Lymic - Unofficial Immich Desktop Client</span>
      <span class="ml-2">v0.10.0</span>
    </footer>

  </div>
</div>
