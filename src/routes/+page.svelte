<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import {
    Settings as SettingsIcon, FolderPlus, Folder,
    Trash2, ArrowLeft, Cloud, CloudUpload,
    AlertCircle, Info, RefreshCw, Clock, X, User,
    ChevronRight, ExternalLink, Minus, Activity, Power
  } from "lucide-svelte";
  import hero from "$lib/assets/hero.png";
  import logo from "$lib/assets/logo.png";

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
          logs = [entry, ...logs].slice(0, 50);
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
    
    <div class="absolute top-12 left-12 flex items-center gap-4 text-white">
      <div class="w-12 h-12 overflow-hidden rounded-xl bg-white/10 backdrop-blur-md flex items-center justify-center p-2">
        <img src={logo} alt="Lymic Logo" class="w-full h-full object-contain" />
      </div>
      <span class="font-bold tracking-tight text-xl">Lymic</span>
    </div>

    <div class="absolute bottom-12 left-12 right-12 text-white">
      <p class="text-xs font-semibold uppercase tracking-[0.3em] opacity-60 mb-2">Sync Manager</p>
      <h1 class="text-4xl font-bold tracking-tight">Deine Fotos.<br/>Sicher lokal.</h1>
    </div>
  </aside>

  <!-- Right Side: Modern Glass Content -->
  <div class="flex-1 flex flex-col h-full relative overflow-hidden">
    
    <!-- Header (Simple & Glass) -->
    <header class="h-20 px-10 flex items-center justify-between shrink-0 z-10">
      {#if isAuthenticated}
        <div class="flex items-center gap-4">
          <h2 class="text-xl font-bold text-slate-800">
            {currentView === "dashboard" ? "Dashboard" : "Einstellungen"}
          </h2>
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

    <main class="flex-1 overflow-y-auto p-10 space-y-10">
      
      <!-- ===== LOGIN VIEW ===== -->
      {#if !isAuthenticated}
        <div class="max-w-md mx-auto py-20 space-y-10 animate-in">
          <div class="text-center space-y-6">
            <div class="w-20 h-20 mx-auto bg-white rounded-2xl shadow-sm border border-slate-100 flex items-center justify-center p-4 mb-4">
              <img src={logo} alt="Lymic Logo" class="w-full h-full object-contain" />
            </div>
            <h2 class="text-3xl font-bold text-slate-900">Anmelden</h2>
            <p class="text-slate-500 text-sm">Verbinde deinen Desktop mit deinem Immich-Server.</p>
          </div>

          <div class="glass-pane space-y-8">
            <div class="space-y-6">
              <div class="space-y-2">
                <label class="text-xs font-bold text-slate-400 uppercase tracking-wider ml-1">Server URL</label>
                <input
                  type="text"
                  placeholder="https://deine-immich-url.de"
                  class="glass-input"
                  bind:value={serverUrl}
                />
              </div>
              <div class="space-y-2">
                <label class="text-xs font-bold text-slate-400 uppercase tracking-wider ml-1">API Key</label>
                <input
                  type="password"
                  placeholder="Dein API-Schlüssel"
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
              {isLoggingIn ? 'Verbinde...' : 'Anmelden'}
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
                 <div class="w-2 h-2 rounded-full {syncStatus === 'syncing' ? 'bg-blue-500 animate-pulse' : 'bg-emerald-500'}"></div>
                 <span class="text-xs font-bold text-slate-400 uppercase tracking-wider">Status</span>
              </div>
              <h3 class="text-3xl font-bold text-slate-900">
                {syncStatus === 'syncing' ? 'Synchronisierung läuft' : 'Alles aktuell'}
              </h3>
            </div>
            <button
              class="btn-action"
              onclick={handleStartSync}
              disabled={syncStatus === 'syncing'}
            >
              {syncStatus === 'syncing' ? 'Synchronisiere...' : 'Jetzt synchronisieren'}
            </button>
          </div>

          {#if syncStatus === 'syncing'}
            <div class="space-y-3">
              <div class="glass-progress">
                <div class="glass-progress-fill" style="width: {progress}%;"></div>
              </div>
              <div class="flex justify-between text-xs font-medium text-slate-400">
                <span>{progress}% abgeschlossen</span>
                <span class="truncate max-w-[250px]">{currentFile}</span>
              </div>
            </div>
          {:else}
            <div class="flex gap-10 pt-2 border-t border-black/5">
              <div class="space-y-1">
                <p class="text-xs font-bold text-slate-300 uppercase tracking-wider">Letzter Sync</p>
                <p class="font-semibold text-slate-600">{lastSync}</p>
              </div>
              <div class="space-y-1">
                <p class="text-xs font-bold text-slate-300 uppercase tracking-wider">Server</p>
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
              <h4 class="text-sm font-bold text-slate-800 uppercase tracking-widest">Synchronisierte Ordner</h4>
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
                      <p class="text-sm font-bold text-slate-900 truncate">{folder.path.split(/[\\/]/).pop()}</p>
                      <p class="text-[10px] font-mono text-slate-400 truncate">{folder.path}</p>
                    </div>
                  </div>
                  <button class="p-2 text-slate-300 hover:text-red-500 opacity-0 group-hover:opacity-100 transition-all" onclick={() => handleRemoveFolder(folder.id)}>
                    <Trash2 class="w-4 h-4" />
                  </button>
                </div>
              {/each}
              {#if watchedFolders.length === 0}
                 <div class="glass-pane !p-12 text-center border-dashed border-2">
                   <p class="text-sm text-slate-400 italic">Noch keine Ordner hinzugefügt.</p>
                 </div>
              {/if}
            </div>
          </div>

          <!-- Activity Log -->
          <div class="space-y-6 animate-in" style="animation-delay: 200ms">
             <h4 class="text-sm font-bold text-slate-800 uppercase tracking-widest px-2">Aktivitätsverlauf</h4>
             <div class="glass-pane !p-6 h-full max-h-[400px] overflow-y-auto space-y-4">
               {#if logs.length === 0}
                 <p class="text-xs text-slate-300 italic">Keine aktuellen Aktivitäten.</p>
               {:else}
                 {#each logs.slice(0, 10) as log}
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
        <div class="max-w-2xl mx-auto space-y-10 animate-in">
          <h2 class="text-3xl font-bold text-slate-900">Einstellungen</h2>
          
          <div class="glass-pane space-y-2 !p-0 overflow-hidden">
            <div class="p-8 flex items-center justify-between border-b border-black/5">
              <div class="space-y-1">
                <p class="font-bold text-slate-900">Autostart</p>
                <p class="text-xs text-slate-500">Immich beim Systemstart automatisch öffnen.</p>
              </div>
              <button class="w-12 h-6 border rounded-full relative transition-all {isAutostartEnabled ? 'bg-blue-600 border-blue-600' : 'bg-slate-200 border-slate-200'}" 
                      onclick={toggleAutostart}>
                <div class="absolute top-1 left-1 w-4 h-4 rounded-full bg-white shadow-sm transition-all {isAutostartEnabled ? 'translate-x-6' : 'translate-x-0'}"></div>
              </button>
            </div>

            <div class="p-8 flex items-center justify-between">
              <div class="space-y-1">
                <p class="font-bold text-slate-900">Account</p>
                <p class="text-xs text-slate-500">Angemeldet bei {serverUrl}</p>
              </div>
              <button class="text-xs font-bold text-red-500 hover:bg-red-50 px-4 py-2 rounded-lg transition-colors" 
                      onclick={() => { isAuthenticated = false; invoke("logout"); }}>
                Abmelden
              </button>
            </div>
          </div>
        </div>
      {/if}

    </main>

    <!-- Global Footer -->
    <footer class="h-16 px-10 border-t border-black/5 flex items-center justify-between text-[10px] font-bold text-slate-400 uppercase tracking-widest shrink-0">
      <div class="flex items-center gap-6">
        <span>© 2024 Lymic</span>
        <div class="w-1.5 h-1.5 rounded-full {syncStatus === 'syncing' ? 'bg-blue-500 animate-pulse' : 'bg-emerald-500'}"></div>
      </div>
      <div class="flex items-center gap-6">
        <span>Lymic Engine v.2.4</span>
      </div>
    </footer>

  </div>
</div>
