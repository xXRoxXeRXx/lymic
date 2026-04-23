<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import {
    LogOut, Settings as SettingsIcon, Play, FolderPlus, Folder,
    Trash2, ArrowLeft, Cloud, CloudUpload, CheckCircle2,
    AlertCircle, Info, RefreshCw, Clock, ChevronRight, X
  } from "lucide-svelte";

  // --- State ---
  let isAuthenticated = $state(false);
  let currentView = $state("dashboard");
  let serverUrl = $state("");
  let apiKey = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");
  let syncStatus = $state<"idle" | "syncing" | "error">("idle");
  let lastSync = $state("Noch nie");
  let progress = $state(0);
  let currentFile = $state("");
  let watchedFolders = $state<any[]>([]);
  let isAutostartEnabled = $state(false);
  let logs = $state<LogEntry[]>([]);
  let logEndEl: HTMLElement;

  interface LogEntry {
    id: number;
    time: string;
    level: "info" | "success" | "error" | "warn";
    message: string;
    raw: string;
  }

  let logCounter = 0;

  function parseLog(raw: string): LogEntry {
    const timeMatch = raw.match(/\[(\d{2}:\d{2}:\d{2})\]/);
    const levelMatch = raw.match(/\[(INFO|ERROR|SUCCESS|WARN)\]/i);
    const time = timeMatch?.[1] ?? new Date().toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const rawLevel = (levelMatch?.[1] ?? "INFO").toUpperCase();

    let level: LogEntry["level"] = "info";
    if (rawLevel === "ERROR") level = "error";
    else if (rawLevel === "SUCCESS") level = "success";
    else if (rawLevel === "WARN") level = "warn";

    let message = raw.replace(/\[\d{2}:\d{2}:\d{2}\]/, "").replace(/\[(INFO|ERROR|SUCCESS|WARN)\]/i, "").trim();

    // Menschenlesbare Übersetzungen
    message = message
      .replace(/Starting manual synchronization\.\.\./, "Manuelle Synchronisation gestartet")
      .replace(/Scanning folder: (.+)/, (_, p) => `📂 Ordner wird gescannt: ${p.split(/[\\/]/).pop()}`)
      .replace(/Auto-sync: Starting pipeline for (\d+) files \((.+) MB\)/, (_, n, mb) => `🚀 Auto-Sync: ${n} Dateien (${mb} MB) werden verarbeitet`)
      .replace(/Sync: Starting pipeline for (\d+) files \((.+) MB\)/, (_, n, mb) => `🚀 Sync: ${n} Dateien (${mb} MB) werden verarbeitet`)
      .replace(/Processed (\d+) files\. (\d+) new uploads\./, (_, total, uploads) => `✅ ${total} Dateien verarbeitet, ${uploads} neue Uploads`)
      .replace(/Auto-sync: Starting pipeline/, "🔄 Automatische Synchronisation läuft")
      .replace(/Bulk check failed: (.+)/, (_, e) => `⚠️ Server-Prüfung fehlgeschlagen: ${e}`)
      .replace(/Upload failed for (.+?): (.+)/, (_, file, err) => `❌ Upload fehlgeschlagen: ${file.split(/[\\/]/).pop()} — ${err}`)
      .replace(/Attempting to connect to (.+)/, (_, url) => `🌐 Verbinde mit ${url}`)
      .replace(/Auto-sync Complete/, "✅ Auto-Sync abgeschlossen")
      .replace(/Sync Complete/, "✅ Sync abgeschlossen");

    return { id: ++logCounter, time, level, message, raw };
  }

  onMount(() => {
    (async () => {
      try {
        isAuthenticated = await invoke("get_auth_status");
        await refreshFolders();
        isAutostartEnabled = await isEnabled();

        const u1 = await listen("sync-progress", (event) => {
        syncStatus = "syncing";
        currentFile = (event.payload as string).split(/[\\/]/).pop() || "";
      });
      const u2 = await listen("sync-progress-percent", (event) => {
        progress = event.payload as number;
      });
      const u3 = await listen("trigger-sync", () => handleStartSync());
      const u4 = await listen("log-message", (event) => {
        const entry = parseLog(event.payload as string);
        logs = [entry, ...logs].slice(0, 150);
      });
      const u5 = await listen("sync-idle", () => {
        syncStatus = "idle";
        progress = 100;
        lastSync = new Date().toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
        setTimeout(() => { progress = 0; }, 2000);
      });
        return () => { u1(); u2(); u3(); u4(); u5(); };
      } catch (e) {
        console.error("Init failed", e);
      }
    })();
  });

  async function handleStartSync() {
    syncStatus = "syncing";
    progress = 0;
    try {
      await invoke("start_sync");
      syncStatus = "idle";
      lastSync = new Date().toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
    } catch (e) {
      console.error("Sync failed", e);
      syncStatus = "error";
    }
  }

  async function refreshFolders() {
    try { watchedFolders = await invoke("get_folders"); } catch {}
  }

  async function handleAddFolder() {
    try {
      const selected = await open({ directory: true, multiple: false, title: "Ordner auswählen" });
      if (selected && typeof selected === "string") {
        await invoke("add_folder", { path: selected });
        await refreshFolders();
      }
    } catch {}
  }

  async function handleRemoveFolder(id: number) {
    try { await invoke("remove_folder", { id }); await refreshFolders(); } catch {}
  }

  async function toggleAutostart() {
    try {
      if (isAutostartEnabled) await disable(); else await enable();
      isAutostartEnabled = await isEnabled();
    } catch {}
  }

  async function handleLogin() {
    if (!serverUrl || !apiKey) return;
    isLoggingIn = true;
    loginError = "";
    try {
      await invoke("login", { serverUrl, apiKey });
      isAuthenticated = true;
    } catch (e) {
      loginError = e as string;
    } finally {
      isLoggingIn = false;
    }
  }

  const statusConfig = {
    idle:    { label: "Bereit",        color: "text-green-500",  bg: "bg-green-500",  ring: "bg-green-500/20" },
    syncing: { label: "Synchronisiert", color: "text-blue-500",  bg: "bg-blue-500",   ring: "bg-blue-500/20" },
    error:   { label: "Fehler",         color: "text-red-500",   bg: "bg-red-500",    ring: "bg-red-500/20" },
  };

  const levelConfig = {
    info:    { icon: "ℹ", cls: "level-info" },
    success: { icon: "✓", cls: "level-success" },
    error:   { icon: "✕", cls: "level-error" },
    warn:    { icon: "!", cls: "level-warn" },
  };
</script>

<div class="min-h-screen flex flex-col font-sans select-none" style="background: rgb(var(--m3-surface));">

  <!-- ===== LOGIN ===== -->
  {#if !isAuthenticated}
    <div class="flex-1 flex items-center justify-center p-6">
      <div class="w-full max-w-sm animate-fade-in-up">
        <!-- Logo -->
        <div class="flex flex-col items-center mb-10">
          <div class="w-20 h-20 rounded-[28px] flex items-center justify-center mb-5 shadow-xl"
               style="background: linear-gradient(135deg, rgb(var(--m3-primary)), rgb(66 99 244));">
            <Cloud class="w-10 h-10 text-white" />
          </div>
          <h1 class="text-2xl font-semibold tracking-tight" style="color: rgb(var(--m3-on-surface));">
            Immich Desktop Sync
          </h1>
          <p class="text-sm mt-1" style="color: rgb(var(--m3-on-surface-variant));">
            Mit deinem Immich-Server verbinden
          </p>
        </div>

        <!-- Card -->
        <div class="m3-card-elevated p-6 space-y-5">
          <div class="space-y-1">
            <label class="text-xs font-medium uppercase tracking-wider" style="color: rgb(var(--m3-on-surface-variant));">
              Server-URL
            </label>
            <input
              type="text"
              placeholder="https://mein-immich.example.com"
              class="m3-input-outlined"
              bind:value={serverUrl}
              onkeydown={(e) => e.key === "Enter" && handleLogin()}
            />
          </div>
          <div class="space-y-1">
            <label class="text-xs font-medium uppercase tracking-wider" style="color: rgb(var(--m3-on-surface-variant));">
              API-Schlüssel
            </label>
            <input
              type="password"
              placeholder="Dein API-Schlüssel"
              class="m3-input-outlined"
              bind:value={apiKey}
              onkeydown={(e) => e.key === "Enter" && handleLogin()}
            />
          </div>

          {#if loginError}
            <div class="flex items-start gap-3 p-3 rounded-xl text-sm"
                 style="background: rgb(var(--m3-error-container)); color: rgb(var(--m3-on-error-container));">
              <AlertCircle class="w-4 h-4 shrink-0 mt-0.5" />
              <span>{loginError}</span>
            </div>
          {/if}

          <button
            class="m3-button-filled w-full h-12 flex items-center justify-center gap-2 text-sm"
            onclick={handleLogin}
            disabled={isLoggingIn || !serverUrl || !apiKey}
          >
            {#if isLoggingIn}
              <RefreshCw class="w-4 h-4 animate-spin" />
              Verbinde...
            {:else}
              Anmelden
            {/if}
          </button>
        </div>
      </div>
    </div>

  <!-- ===== DASHBOARD ===== -->
  {:else}
    <!-- Header -->
    <header class="h-14 px-4 flex items-center justify-between shrink-0"
            style="background: rgb(var(--m3-surface-container)); border-bottom: 1px solid rgb(var(--m3-outline-variant) / 0.5);">
      <div class="flex items-center gap-3">
        <div class="w-8 h-8 rounded-xl flex items-center justify-center"
             style="background: linear-gradient(135deg, rgb(var(--m3-primary)), rgb(66 99 244));">
          <Cloud class="text-white w-4 h-4" />
        </div>
        <span class="font-semibold text-sm" style="color: rgb(var(--m3-on-surface));">Immich Sync</span>
      </div>
      <div class="flex items-center gap-1">
        <button class="m3-icon-button"
                onclick={() => currentView = currentView === "settings" ? "dashboard" : "settings"}
                title={currentView === "settings" ? "Dashboard" : "Einstellungen"}>
          {#if currentView === "settings"}
            <ArrowLeft class="w-5 h-5" />
          {:else}
            <SettingsIcon class="w-5 h-5" />
          {/if}
        </button>
        <button class="m3-icon-button" title="Abmelden"
                onclick={() => { isAuthenticated = false; invoke("logout"); }}
                style="color: rgb(var(--m3-error));">
          <LogOut class="w-5 h-5" />
        </button>
      </div>
    </header>

    <!-- Body: two-column layout (scrollable content + log panel) -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Left: Main Content -->
      <div class="flex-1 overflow-y-auto p-5 space-y-4">

        {#if currentView === "dashboard"}
          <!-- Status Hero Card -->
          <div class="m3-card-elevated p-5 space-y-4">
            <!-- Status row -->
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2.5">
                <div class="relative">
                  <div class="w-3 h-3 rounded-full {statusConfig[syncStatus].bg}
                    {syncStatus === 'syncing' ? 'animate-status-pulse' : ''}">
                  </div>
                  {#if syncStatus === 'syncing'}
                    <div class="absolute inset-0 rounded-full {statusConfig[syncStatus].ring} scale-150 animate-pulse-ring"></div>
                  {/if}
                </div>
                <span class="text-sm font-medium {statusConfig[syncStatus].color}">
                  {statusConfig[syncStatus].label}
                </span>
              </div>
              <div class="flex items-center gap-1.5 text-xs" style="color: rgb(var(--m3-on-surface-variant));">
                <Clock class="w-3.5 h-3.5" />
                <span>Zuletzt: {lastSync}</span>
              </div>
            </div>

            <!-- Progress Bar -->
            <div class="space-y-2">
              <div class="flex items-center justify-between text-xs" style="color: rgb(var(--m3-on-surface-variant));">
                <span>
                  {#if syncStatus === 'syncing'}
                    {currentFile ? `📤 ${currentFile}` : "Vorbereitung..."}
                  {:else if progress === 100}
                    Abgeschlossen
                  {:else}
                    Bereit zum Synchronisieren
                  {/if}
                </span>
                <span class="font-medium tabular-nums" style="color: rgb(var(--m3-primary));">
                  {progress}%
                </span>
              </div>
              <div class="m3-progress-bar {syncStatus === 'syncing' && progress === 0 ? 'm3-progress-bar-indeterminate' : ''}">
                <div class="m3-progress-bar-track" style="width: {progress}%;"></div>
              </div>
            </div>

            <!-- Action Button -->
            <button
              class="m3-button-filled w-full h-11 flex items-center justify-center gap-2"
              onclick={handleStartSync}
              disabled={syncStatus === 'syncing'}
            >
              {#if syncStatus === 'syncing'}
                <RefreshCw class="w-4 h-4 animate-spin" />
                Synchronisiert...
              {:else}
                <CloudUpload class="w-4 h-4" />
                Jetzt synchronisieren
              {/if}
            </button>
          </div>

          <!-- Folders Section -->
          <div class="space-y-3">
            <div class="flex items-center justify-between px-1">
              <h2 class="text-sm font-semibold flex items-center gap-2" style="color: rgb(var(--m3-on-surface));">
                <Folder class="w-4 h-4" style="color: rgb(var(--m3-primary));" />
                Überwachte Ordner
                {#if watchedFolders.length > 0}
                  <span class="text-xs px-2 py-0.5 rounded-full font-medium"
                        style="background: rgb(var(--m3-primary-container)); color: rgb(var(--m3-on-primary-container));">
                    {watchedFolders.length}
                  </span>
                {/if}
              </h2>
              <button
                class="flex items-center gap-1.5 text-xs font-medium px-3 py-1.5 rounded-full transition-colors"
                style="color: rgb(var(--m3-primary)); background: rgb(var(--m3-primary) / 0.08);"
                onclick={handleAddFolder}
              >
                <FolderPlus class="w-3.5 h-3.5" />
                Hinzufügen
              </button>
            </div>

            <div class="space-y-2">
              {#each watchedFolders as folder}
                <div class="m3-card flex items-center gap-3 group cursor-default hover:shadow-sm"
                     style="padding: 0.75rem 1rem;">
                  <div class="w-9 h-9 rounded-xl flex items-center justify-center shrink-0"
                       style="background: rgb(var(--m3-primary-container)); color: rgb(var(--m3-primary));">
                    <Folder class="w-4 h-4" />
                  </div>
                  <div class="flex-1 min-w-0">
                    <p class="text-sm font-medium truncate" style="color: rgb(var(--m3-on-surface));">
                      {folder.path.split(/[\\/]/).pop()}
                    </p>
                    <p class="text-xs truncate" style="color: rgb(var(--m3-on-surface-variant));">
                      {folder.path}
                    </p>
                  </div>
                  <button
                    class="m3-icon-button opacity-0 group-hover:opacity-100 w-8 h-8 shrink-0"
                    style="color: rgb(var(--m3-error));"
                    onclick={() => handleRemoveFolder(folder.id)}
                    title="Ordner entfernen"
                  >
                    <Trash2 class="w-4 h-4" />
                  </button>
                </div>
              {/each}

              {#if watchedFolders.length === 0}
                <div class="flex flex-col items-center gap-3 py-10 rounded-2xl border-2 border-dashed"
                     style="border-color: rgb(var(--m3-outline-variant)); color: rgb(var(--m3-on-surface-variant));">
                  <Folder class="w-10 h-10 opacity-30" />
                  <div class="text-center">
                    <p class="text-sm font-medium">Noch keine Ordner</p>
                    <button class="text-xs mt-1 underline" style="color: rgb(var(--m3-primary));"
                            onclick={handleAddFolder}>
                      Ersten Ordner hinzufügen
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          </div>

        {:else}
          <!-- Settings View -->
          <div class="space-y-4 animate-fade-in-up">
            <h2 class="text-base font-semibold px-1" style="color: rgb(var(--m3-on-surface));">Einstellungen</h2>

            <div class="m3-card-elevated space-y-0 divide-y" style="divide-color: rgb(var(--m3-outline-variant) / 0.4); padding: 0; overflow: hidden; border-radius: 16px;">
              <div class="flex items-center justify-between p-4">
                <div>
                  <p class="text-sm font-medium" style="color: rgb(var(--m3-on-surface));">Autostart</p>
                  <p class="text-xs mt-0.5" style="color: rgb(var(--m3-on-surface-variant));">
                    Beim Systemstart automatisch starten
                  </p>
                </div>
                <button class="m3-switch {isAutostartEnabled ? 'active' : ''}" onclick={toggleAutostart}>
                  <div class="m3-switch-thumb"></div>
                </button>
              </div>
            </div>
          </div>
        {/if}
      </div>

      <!-- Right: Activity Log Panel -->
      <div class="w-72 shrink-0 flex flex-col border-l"
           style="border-color: rgb(var(--m3-outline-variant) / 0.5); background: rgb(var(--m3-surface-container-low));">
        <!-- Log Header -->
        <div class="px-4 py-3 flex items-center justify-between shrink-0 border-b"
             style="border-color: rgb(var(--m3-outline-variant) / 0.4);">
          <div class="flex items-center gap-2">
            {#if syncStatus === 'syncing'}
              <div class="w-2 h-2 rounded-full bg-blue-500 animate-status-pulse"></div>
            {:else}
              <div class="w-2 h-2 rounded-full" style="background: rgb(var(--m3-outline));"></div>
            {/if}
            <span class="text-xs font-semibold uppercase tracking-widest"
                  style="color: rgb(var(--m3-on-surface-variant));">Aktivität</span>
          </div>
          {#if logs.length > 0}
            <button class="m3-icon-button w-7 h-7" onclick={() => logs = []} title="Log leeren">
              <X class="w-3.5 h-3.5" />
            </button>
          {/if}
        </div>

        <!-- Log Entries -->
        <div class="flex-1 overflow-y-auto p-2 space-y-0.5">
          {#if logs.length === 0}
            <div class="flex flex-col items-center justify-center h-full gap-3 py-12"
                 style="color: rgb(var(--m3-on-surface-variant));">
              <Info class="w-8 h-8 opacity-20" />
              <p class="text-xs text-center opacity-50">
                Noch keine Aktivität.<br />Starte eine Synchronisation.
              </p>
            </div>
          {:else}
            {#each logs as log (log.id)}
              <div class="log-item {levelConfig[log.level].cls} animate-fade-in-up">
                <div class="log-item-icon">
                  {levelConfig[log.level].icon}
                </div>
                <div class="flex-1 min-w-0">
                  <p class="text-xs leading-snug break-words" style="color: rgb(var(--m3-on-surface));">
                    {log.message}
                  </p>
                  <p class="text-[10px] mt-0.5 tabular-nums" style="color: rgb(var(--m3-on-surface-variant));">
                    {log.time}
                  </p>
                </div>
              </div>
            {/each}
          {/if}
        </div>

        <!-- Log Footer: Stats -->
        {#if logs.length > 0}
          {@const errorCount = logs.filter(l => l.level === 'error').length}
          {@const successCount = logs.filter(l => l.level === 'success').length}
          <div class="px-4 py-2 border-t shrink-0 flex items-center gap-3"
               style="border-color: rgb(var(--m3-outline-variant) / 0.4);">
            <div class="flex items-center gap-1 text-[10px]" style="color: rgb(var(--m3-success));">
              <CheckCircle2 class="w-3 h-3" />
              {successCount}
            </div>
            {#if errorCount > 0}
              <div class="flex items-center gap-1 text-[10px]" style="color: rgb(var(--m3-error));">
                <AlertCircle class="w-3 h-3" />
                {errorCount} Fehler
              </div>
            {/if}
            <span class="ml-auto text-[10px]" style="color: rgb(var(--m3-on-surface-variant));">
              {logs.length} Einträge
            </span>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
