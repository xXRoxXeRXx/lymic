<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { LogOut, Settings as SettingsIcon, Play, Pause, FolderPlus, Folder, Trash2, ArrowLeft, ShieldCheck, Zap } from "lucide-svelte";

  let isAuthenticated = $state(false);
  let currentView = $state("dashboard"); // "dashboard" or "settings"
  let serverUrl = $state("");
  let apiKey = $state("");
  let isLoggingIn = $state(false);
  let loginError = $state("");
  let syncStatus = $state("Idle");
  let lastSync = $state("Never");
  let progress = $state(0);
  let currentFile = $state("");
  let watchedFolders = $state<any[]>([]);
  let isAutostartEnabled = $state(false);
  let logs = $state<string[]>([]);
  let showLogs = $state(false);

  onMount(async () => {
    try {
      isAuthenticated = await invoke("get_auth_status");
      await refreshFolders();
      isAutostartEnabled = await isEnabled();

      const unlisten = await listen("sync-progress", (event) => {
        syncStatus = "Syncing";
        currentFile = (event.payload as string).split(/[\\/]/).pop() || "";
      });

      const unlistenPct = await listen("sync-progress-percent", (event) => {
        progress = event.payload as number;
      });

      const unlistenTrigger = await listen("trigger-sync", () => {
        handleStartSync();
      });

      const unlistenLogs = await listen("log-message", (event) => {
        logs = [event.payload as string, ...logs].slice(0, 100);
      });

      const unlistenIdle = await listen("sync-idle", () => {
        syncStatus = "Idle";
        lastSync = new Date().toLocaleTimeString();
      });

      return () => {
        unlisten();
        unlistenPct();
        unlistenTrigger();
        unlistenLogs();
        unlistenIdle();
      };
    } catch (e) {
      console.error("Failed to initialize dashboard", e);
    }
  });

  async function handleStartSync() {
    syncStatus = "Syncing";
    try {
      await invoke("start_sync");
      syncStatus = "Idle";
      lastSync = new Date().toLocaleTimeString();
    } catch (e) {
      console.error("Sync failed", e);
      syncStatus = "Error";
    }
  }

  async function refreshFolders() {
    try {
      watchedFolders = await invoke("get_folders");
    } catch (e) {
      console.error("Failed to fetch folders", e);
    }
  }

  async function handleAddFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Folder to Sync",
      });

      if (selected && typeof selected === "string") {
        await invoke("add_folder", { path: selected });
        await refreshFolders();
      }
    } catch (e) {
      console.error("Failed to add folder", e);
    }
  }

  async function handleRemoveFolder(id: number) {
    try {
      await invoke("remove_folder", { id });
      await refreshFolders();
    } catch (e) {
      console.error("Failed to remove folder", e);
    }
  }

  async function toggleAutostart() {
    try {
      if (isAutostartEnabled) {
        await disable();
      } else {
        await enable();
      }
      isAutostartEnabled = await isEnabled();
    } catch (e) {
      console.error("Failed to toggle autostart", e);
    }
  }

  async function handleLogin() {
    if (!serverUrl || !apiKey) return;
    isLoggingIn = true;
    loginError = "";
    try {
      await invoke("login", { serverUrl, apiKey });
      isAuthenticated = true;
    } catch (e) {
      console.error("Login failed", e);
      loginError = e as string;
    } finally {
      isLoggingIn = false;
    }
  }
</script>

<div class="min-h-screen flex flex-col font-sans">
  {#if !isAuthenticated}
    <!-- Login View -->
    <div class="flex-1 flex items-center justify-center p-6 bg-[rgb(var(--m3-surface))]">
      <div class="w-full max-w-md m3-card bg-white dark:bg-zinc-900 shadow-xl border border-zinc-200 dark:border-zinc-800">
        <div class="flex flex-col items-center mb-8">
          <div class="w-16 h-16 bg-[rgb(var(--m3-primary))] rounded-2xl flex items-center justify-center mb-4 shadow-lg shadow-blue-500/20">
            <Zap class="text-white w-8 h-8" />
          </div>
          <h1 class="text-2xl font-bold tracking-tight">Welcome to Immich</h1>
          <p class="text-[rgb(var(--m3-on-surface-variant))] text-sm">Sign in to your server</p>
        </div>

        <div class="space-y-4">
          <div>
            <label class="text-xs font-medium uppercase tracking-wider ml-1 mb-1 block text-zinc-500">Server URL</label>
            <input 
              type="text" 
              placeholder="https://your-immich-instance.com" 
              class="w-full m3-input bg-zinc-50 dark:bg-zinc-800" 
              bind:value={serverUrl}
            />
          </div>
          <div>
            <label class="text-xs font-medium uppercase tracking-wider ml-1 mb-1 block text-zinc-500">API Key</label>
            <input 
              type="password" 
              placeholder="Your secret API key" 
              class="w-full m3-input bg-zinc-50 dark:bg-zinc-800" 
              bind:value={apiKey}
            />
          </div>

          {#if loginError}
            <div class="p-3 rounded-xl bg-red-50 text-red-600 text-xs flex items-center gap-2 border border-red-100">
              <Trash2 class="w-4 h-4" />
              {loginError}
            </div>
          {/if}

          <button 
            class="w-full m3-button-filled mt-4 flex items-center justify-center gap-2 h-12"
            onclick={handleLogin}
            disabled={isLoggingIn}
          >
            {#if isLoggingIn}
              <div class="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin"></div>
            {:else}
              Get Started
            {/if}
          </button>
        </div>
      </div>
    </div>
  {:else}
    <!-- Dashboard View -->
    <header class="h-16 px-6 flex items-center justify-between border-b border-[rgb(var(--m3-surface-variant))] sticky top-0 bg-[rgb(var(--m3-surface))] z-10">
      <div class="flex items-center gap-3">
        <div class="w-8 h-8 bg-[rgb(var(--m3-primary))] rounded-lg flex items-center justify-center">
          <Zap class="text-white w-4 h-4" />
        </div>
        <h1 class="text-lg font-semibold tracking-tight">Immich Sync</h1>
      </div>
      
      <div class="flex items-center gap-2">
        <button 
          class="p-2 hover:bg-[rgb(var(--m3-surface-variant))] rounded-full transition-colors"
          onclick={() => currentView = currentView === "settings" ? "dashboard" : "settings"}
        >
          {#if currentView === "settings"}
            <ArrowLeft class="w-5 h-5" />
          {:else}
            <SettingsIcon class="w-5 h-5 text-[rgb(var(--m3-on-surface-variant))]" />
          {/if}
        </button>
        <button 
          class="p-2 hover:bg-red-50 text-red-500 rounded-full transition-colors"
          onclick={() => { isAuthenticated = false; invoke("logout"); }}
        >
          <LogOut class="w-5 h-5" />
        </button>
      </div>
    </header>

    <main class="flex-1 overflow-y-auto p-6 max-w-4xl mx-auto w-full space-y-6 pb-24">
      {#if currentView === "dashboard"}
        <!-- Status Card -->
        <div class="m3-card relative overflow-hidden group">
          <div class="absolute top-0 right-0 p-8 opacity-5 group-hover:opacity-10 transition-opacity">
            <ShieldCheck class="w-32 h-32 text-[rgb(var(--m3-primary))]" />
          </div>
          
          <div class="relative z-10">
            <div class="flex items-center gap-2 mb-1">
              <span class="flex h-2 w-2 rounded-full {syncStatus === 'Syncing' ? 'bg-blue-500 animate-pulse' : 'bg-green-500'}"></span>
              <span class="text-xs font-medium uppercase tracking-widest text-[rgb(var(--m3-on-surface-variant))]">
                System Status: {syncStatus}
              </span>
            </div>
            <h2 class="text-3xl font-bold mb-4">Ready to Sync</h2>
            
            <div class="grid grid-cols-2 gap-4 mb-6">
              <div class="bg-white/50 dark:bg-black/20 p-4 rounded-2xl">
                <p class="text-xs text-[rgb(var(--m3-on-surface-variant))] mb-1">Last Update</p>
                <p class="font-semibold">{lastSync}</p>
              </div>
              <div class="bg-white/50 dark:bg-black/20 p-4 rounded-2xl">
                <p class="text-xs text-[rgb(var(--m3-on-surface-variant))] mb-1">Backup Progress</p>
                <p class="font-semibold">{progress}%</p>
              </div>
            </div>

            <div class="flex gap-3">
              <button 
                class="m3-button-filled flex-1 flex items-center justify-center gap-2 h-12 shadow-lg shadow-blue-500/20"
                onclick={handleStartSync}
                disabled={syncStatus === 'Syncing'}
              >
                {#if syncStatus === 'Syncing'}
                  <div class="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin"></div>
                  Syncing...
                {:else}
                  <Play class="w-5 h-5 fill-current" />
                  Sync Now
                {/if}
              </button>
              <button 
                class="m3-button-tonal w-12 h-12 p-0 flex items-center justify-center"
                onclick={() => showLogs = !showLogs}
              >
                <div class="flex flex-col gap-0.5 items-center">
                  <div class="w-4 h-0.5 bg-current rounded-full"></div>
                  <div class="w-2.5 h-0.5 bg-current rounded-full"></div>
                  <div class="w-4 h-0.5 bg-current rounded-full"></div>
                </div>
              </button>
            </div>
          </div>
        </div>

        <!-- Folders Section -->
        <div class="space-y-4">
          <div class="flex items-center justify-between px-2">
            <h3 class="text-lg font-semibold flex items-center gap-2">
              <Folder class="w-5 h-5 text-[rgb(var(--m3-primary))]" />
              Watched Folders
            </h3>
            <button 
              class="text-sm font-medium text-[rgb(var(--m3-primary))] flex items-center gap-1 hover:underline"
              onclick={handleAddFolder}
            >
              <FolderPlus class="w-4 h-4" />
              Add Folder
            </button>
          </div>

          <div class="grid gap-3">
            {#each watchedFolders as folder}
              <div class="flex items-center justify-between p-4 bg-white dark:bg-zinc-900 border border-zinc-100 dark:border-zinc-800 rounded-2xl hover:border-[rgb(var(--m3-primary))] transition-all group">
                <div class="flex items-center gap-4 overflow-hidden">
                  <div class="w-10 h-10 rounded-xl bg-blue-50 dark:bg-blue-900/20 flex items-center justify-center text-blue-600 dark:text-blue-400 shrink-0">
                    <Folder class="w-5 h-5" />
                  </div>
                  <div class="overflow-hidden">
                    <p class="font-medium text-sm truncate">{folder.path.split(/[\\/]/).pop()}</p>
                    <p class="text-xs text-zinc-500 truncate">{folder.path}</p>
                  </div>
                </div>
                <button 
                  class="p-2 text-zinc-400 hover:text-red-500 hover:bg-red-50 rounded-lg opacity-0 group-hover:opacity-100 transition-all shrink-0"
                  onclick={() => handleRemoveFolder(folder.id)}
                >
                  <Trash2 class="w-4 h-4" />
                </button>
              </div>
            {/each}
            
            {#if watchedFolders.length === 0}
              <div class="text-center py-12 m3-card border-dashed border-2 border-zinc-200 dark:border-zinc-800 bg-transparent">
                <Folder class="w-12 h-12 text-zinc-300 mx-auto mb-3" />
                <p class="text-zinc-500 font-medium">No folders added yet</p>
                <button class="text-[rgb(var(--m3-primary))] text-sm hover:underline mt-1" onclick={handleAddFolder}>Add your first folder</button>
              </div>
            {/if}
          </div>
        </div>
      {:else}
        <!-- Settings View -->
        <div class="space-y-6">
          <div class="m3-card">
            <h3 class="text-lg font-semibold mb-4 flex items-center gap-2">
              <SettingsIcon class="w-5 h-5 text-[rgb(var(--m3-primary))]" />
              General Preferences
            </h3>
            <div class="space-y-4">
              <div class="flex items-center justify-between py-2 border-b border-[rgb(var(--m3-surface))] last:border-0">
                <div>
                  <p class="font-medium text-sm text-[rgb(var(--m3-on-surface))]">Launch on Startup</p>
                  <p class="text-xs text-[rgb(var(--m3-on-surface-variant))]">Automatically start sync client when you login</p>
                </div>
                <button 
                  class="w-12 h-6 rounded-full transition-colors relative {isAutostartEnabled ? 'bg-[rgb(var(--m3-primary))]' : 'bg-zinc-300'}"
                  onclick={toggleAutostart}
                >
                  <div class="absolute top-1 left-1 w-4 h-4 bg-white rounded-full shadow-sm transition-transform {isAutostartEnabled ? 'translate-x-6' : ''}"></div>
                </button>
              </div>
            </div>
          </div>
          <button 
            class="w-full flex items-center justify-center gap-2 text-sm text-zinc-500 hover:text-[rgb(var(--m3-primary))] transition-colors py-4"
            onclick={() => currentView = "dashboard"}
          >
            <ArrowLeft class="w-4 h-4" /> Back to Dashboard
          </button>
        </div>
      {/if}
    </main>

    <!-- Logs Drawer -->
    {#if showLogs}
      <div class="fixed inset-x-0 bottom-0 z-20 h-2/3 bg-[rgb(var(--m3-surface))] border-t border-[rgb(var(--m3-surface-variant))] shadow-2xl flex flex-col transform transition-transform animate-in slide-in-from-bottom duration-300">
        <div class="flex items-center justify-between px-6 py-4 border-b border-[rgb(var(--m3-surface-variant))] bg-white/50 backdrop-blur-md">
          <div class="flex items-center gap-2">
            <div class="w-2 h-2 rounded-full bg-blue-500 animate-pulse"></div>
            <h3 class="font-semibold text-xs uppercase tracking-widest text-[rgb(var(--m3-on-surface-variant))]">Real-time Activity</h3>
          </div>
          <button 
            class="m3-button-tonal !px-4 !py-1 text-xs"
            onclick={() => showLogs = false}
          >
            Close
          </button>
        </div>
        <div class="flex-1 overflow-y-auto p-4 bg-zinc-950 text-zinc-300 font-mono text-[11px] space-y-1">
          {#each logs as log}
            <div class="flex gap-4 border-l-2 {log.includes('[ERROR]') ? 'border-red-500' : log.includes('[SUCCESS]') ? 'border-green-500' : 'border-blue-500'} pl-4 py-1 hover:bg-white/5 transition-colors">
              <span class="text-zinc-600 shrink-0">{new Date().toLocaleTimeString()}</span>
              <span class="break-all whitespace-pre-wrap">{log}</span>
            </div>
          {/each}
          {#if logs.length === 0}
            <div class="text-zinc-700 text-center py-20 flex flex-col items-center gap-2 italic">
              <Zap class="w-8 h-8 opacity-20" />
              Waiting for activity...
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</div>
