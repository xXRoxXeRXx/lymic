<script lang="ts">
  import {
    Monitor,
    Moon,
    Sun,
    User,
    Sliders,
    Palette,
    RefreshCw,
    Sparkles,
    Power,
    Languages,
    Cpu,
    LogOut
  } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import type { LocaleCatalogEntry } from "../i18n/locale-catalog";
  import type { ThemePreference } from "./types";

  interface Props {
    currentUserName: string;
    serverUrl: string;
    isAutostartEnabled: boolean;
    virtualLibraryEnabled: boolean;
    virtualLibraryPath: string;
    virtualLibraryConnectionStatus: string;
    isClearingVirtualLibraryCache?: boolean;
    isRefreshingVirtualLibrary?: boolean;
    virtualLibraryCacheClearResult?: { bytesFreed: number; filesDehydrated: number; pinnedFilesPreserved: number } | null;
    themePreference: ThemePreference;
    uploadParallelism: number;
    isSavingUploadParallelism: boolean;
    thumbnailCacheCapBytes?: number;
    isClearingThumbnailCache?: boolean;
    updateStatus: "unknown" | "current" | "available";
    isCheckingForUpdates: boolean;
    locales: readonly LocaleCatalogEntry[];
    onToggleAutostart: () => void;
    onToggleVirtualLibrary: () => void;
    onClearVirtualLibraryCache?: () => void;
    onRefreshVirtualLibrary?: () => void;
    onLocaleChange: (locale: string) => void;
    onThemeChange: (preference: ThemePreference) => void;
    onUploadParallelismChange: (event: Event) => void;
    onThumbnailCacheCapChange?: (event: Event) => void;
    onClearThumbnailCache?: () => void;
    onCheckForUpdates: () => void;
    onLogout: () => void;
  }

  let {
    currentUserName,
    serverUrl,
    isAutostartEnabled,
    virtualLibraryEnabled,
    virtualLibraryPath,
    virtualLibraryConnectionStatus,
    isClearingVirtualLibraryCache = false,
    isRefreshingVirtualLibrary = false,
    virtualLibraryCacheClearResult = null,
    themePreference,
    uploadParallelism,
    isSavingUploadParallelism,
    thumbnailCacheCapBytes = 268435456,
    isClearingThumbnailCache = false,
    updateStatus,
    isCheckingForUpdates,
    locales,
    onToggleAutostart,
    onToggleVirtualLibrary,
    onClearVirtualLibraryCache = () => {},
    onRefreshVirtualLibrary = () => {},
    onLocaleChange,
    onThemeChange,
    onUploadParallelismChange,
    onThumbnailCacheCapChange = () => {},
    onClearThumbnailCache = () => {},
    onCheckForUpdates,
    onLogout
  }: Props = $props();
</script>

<div class="max-w-3xl w-full space-y-8 animate-in pb-10">
  <!-- Category 1: General (Autostart & Language) -->
  <section class="space-y-3">
    <h2 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2">
      <Sliders class="w-3.5 h-3.5" />
      <span>{$t('autostart')} & {$t('language')}</span>
    </h2>

    <!-- Autostart Card -->
    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0 mt-0.5">
          <Power class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <p class="font-semibold text-slate-900 dark:text-slate-100 text-sm">{$t('autostart')}</p>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">{$t('autostart_subtitle')}</p>
        </div>
      </div>

      <button
        type="button"
        class="desktop-toggle {isAutostartEnabled ? 'enabled' : ''}"
        onclick={onToggleAutostart}
        aria-label={isAutostartEnabled ? $t('autostart_disable') : $t('autostart_enable')}
        aria-pressed={isAutostartEnabled}
      >
        <div class="desktop-toggle-knob"></div>
      </button>
    </div>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-sky-50 dark:bg-sky-950/40 text-sky-600 dark:text-sky-400 shrink-0 mt-0.5">
          <Monitor class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <p class="font-semibold text-slate-900 dark:text-slate-100 text-sm">Virtual Library (Windows Explorer)</p>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">Read-only library at <span class="font-mono">{virtualLibraryPath}</span> · {virtualLibraryConnectionStatus === 'connected' ? 'Connected' : virtualLibraryConnectionStatus === 'error' ? 'Error' : 'Disconnected'}</p>
        </div>
      </div>

      <div class="flex items-center gap-2">
        {#if virtualLibraryEnabled}
          <button type="button" class="rounded-lg border border-slate-200 bg-slate-100 px-3.5 py-1.5 text-xs font-semibold text-slate-700 transition-colors hover:bg-slate-200 disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-slate-200 dark:hover:bg-zinc-700 cursor-pointer" onclick={onRefreshVirtualLibrary} disabled={isRefreshingVirtualLibrary}>
            {isRefreshingVirtualLibrary ? "Refreshing…" : "Refresh Library"}
          </button>
          <button type="button" class="rounded-lg border border-slate-200 bg-slate-100 px-3.5 py-1.5 text-xs font-semibold text-slate-700 transition-colors hover:bg-slate-200 disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-slate-200 dark:hover:bg-zinc-700 cursor-pointer" onclick={onClearVirtualLibraryCache} disabled={isClearingVirtualLibraryCache}>
            {isClearingVirtualLibraryCache ? "Clearing…" : "Clear Local Cache"}
          </button>
        {/if}
        <button
          type="button"
          class="desktop-toggle {virtualLibraryEnabled ? 'enabled' : ''}"
          onclick={onToggleVirtualLibrary}
          aria-label={virtualLibraryEnabled ? 'Disable Virtual Library' : 'Enable Virtual Library'}
          aria-pressed={virtualLibraryEnabled}
        >
          <div class="desktop-toggle-knob"></div>
        </button>
      </div>
    </div>
    {#if virtualLibraryCacheClearResult}
      <p class="text-xs text-slate-500 dark:text-slate-400">Freed {virtualLibraryCacheClearResult.bytesFreed} bytes from {virtualLibraryCacheClearResult.filesDehydrated} file(s); kept {virtualLibraryCacheClearResult.pinnedFilesPreserved} pinned file(s).</p>
    {/if}

    <!-- Language Card -->
    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400 shrink-0 mt-0.5">
          <Languages class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <label for="locale" class="font-semibold text-slate-900 dark:text-slate-100 text-sm cursor-pointer">{$t('language')}</label>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">{$t('language_subtitle')}</p>
        </div>
      </div>

      <select
        id="locale"
        class="glass-input !w-auto !py-1.5 !pl-3 !pr-8 text-sm font-medium rounded-lg cursor-pointer"
        value={$locale}
        onchange={(event) => onLocaleChange((event.currentTarget as HTMLSelectElement).value)}
      >
        {#each locales as { code, name }}
          <option value={code}>{name}</option>
        {/each}
      </select>
    </div>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-amber-50 dark:bg-amber-950/40 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5">
          <Cpu class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <label for="thumbnail-cache-cap" class="font-semibold text-slate-900 dark:text-slate-100 text-sm cursor-pointer">Thumbnail cache</label>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">Keeps Virtual Library previews in local app data.</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <select id="thumbnail-cache-cap" class="glass-input !w-24 !py-1.5 !pl-3 !pr-8 text-sm font-medium rounded-lg cursor-pointer" value={thumbnailCacheCapBytes} onchange={onThumbnailCacheCapChange}>
          <option value={67108864}>64 MB</option>
          <option value={268435456}>256 MB</option>
          <option value={536870912}>512 MB</option>
          <option value={1073741824}>1 GB</option>
        </select>
        <button type="button" class="rounded-lg border border-slate-200 bg-slate-100 px-3.5 py-1.5 text-xs font-semibold text-slate-700 transition-colors hover:bg-slate-200 disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-slate-200 dark:hover:bg-zinc-700 cursor-pointer" onclick={onClearThumbnailCache} disabled={isClearingThumbnailCache}>
          {isClearingThumbnailCache ? "Clearing…" : "Clear Thumbnail Cache"}
        </button>
      </div>
    </div>
  </section>

  <!-- Category 2: Appearance -->
  <section class="space-y-3">
    <h2 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2">
      <Palette class="w-3.5 h-3.5" />
      <span>{$t('appearance')}</span>
    </h2>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-purple-50 dark:bg-purple-950/40 text-purple-600 dark:text-purple-400 shrink-0 mt-0.5">
          <Palette class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <p class="font-semibold text-slate-900 dark:text-slate-100 text-sm">{$t('appearance')}</p>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">{$t('appearance_subtitle')}</p>
        </div>
      </div>

      <div class="flex gap-1.5 p-1 rounded-lg bg-slate-100 dark:bg-zinc-800/80 border border-slate-200/60 dark:border-zinc-700/60" role="group" aria-label={$t('appearance')}>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-all cursor-pointer {themePreference === 'system' ? 'bg-white dark:bg-zinc-700 text-slate-900 dark:text-white shadow-sm' : 'text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-white'}"
          aria-pressed={themePreference === 'system'}
          title={$t('theme_system')}
          onclick={() => onThemeChange('system')}
        >
          <Monitor class="w-3.5 h-3.5" />
          <span>{$t('theme_system')}</span>
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-all cursor-pointer {themePreference === 'light' ? 'bg-white dark:bg-zinc-700 text-slate-900 dark:text-white shadow-sm' : 'text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-white'}"
          aria-pressed={themePreference === 'light'}
          title={$t('theme_light')}
          onclick={() => onThemeChange('light')}>
          <Sun class="w-3.5 h-3.5" />
          <span>{$t('theme_light')}</span>
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-all cursor-pointer {themePreference === 'dark' ? 'bg-white dark:bg-zinc-700 text-slate-900 dark:text-white shadow-sm' : 'text-slate-600 dark:text-zinc-400 hover:text-slate-900 dark:hover:text-white'}"
          aria-pressed={themePreference === 'dark'}
          title={$t('theme_dark')}
          onclick={() => onThemeChange('dark')}
        >
          <Moon class="w-3.5 h-3.5" />
          <span>{$t('theme_dark')}</span>
        </button>
      </div>
    </div>
  </section>

  <!-- Category 3: Sync Performance -->
  <section class="space-y-3">
    <h2 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2">
      <RefreshCw class="w-3.5 h-3.5" />
      <span>{$t('upload_parallelism')}</span>
    </h2>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-amber-50 dark:bg-amber-950/40 text-amber-600 dark:text-amber-400 shrink-0 mt-0.5">
          <Cpu class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <label for="upload-parallelism" class="font-semibold text-slate-900 dark:text-slate-100 text-sm cursor-pointer">{$t('upload_parallelism')}</label>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">{$t('upload_parallelism_subtitle')}</p>
        </div>
      </div>

      <select
        id="upload-parallelism"
        class="glass-input !w-20 !min-w-[5rem] !py-1.5 !pl-3.5 !pr-8 text-sm font-medium rounded-lg cursor-pointer text-left"
        value={uploadParallelism}
        onchange={onUploadParallelismChange}
        disabled={isSavingUploadParallelism}
      >
        {#each [1, 2, 3, 4, 5, 6, 7, 8] as value}
          <option value={value}>{value}</option>
        {/each}
      </select>
    </div>
  </section>

  <!-- Category 4: Updates -->
  <section class="space-y-3">
    <h2 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2">
      <Sparkles class="w-3.5 h-3.5" />
      <span>{$t('updates')}</span>
    </h2>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-blue-50 dark:bg-blue-950/40 text-blue-600 dark:text-sky-400 shrink-0 mt-0.5">
          <Sparkles class="w-4 h-4" />
        </div>
        <div class="space-y-0.5">
          <p class="font-semibold text-slate-900 dark:text-slate-100 text-sm">{$t('updates')}</p>
          <p class="text-xs text-slate-500 dark:text-slate-400 leading-relaxed max-w-lg">
            {#if updateStatus === 'current'}
              {$t('update_current')}
            {:else if updateStatus === 'available'}
              {$t('update_available_short')}
            {:else}
              {$t('update_check_subtitle')}
            {/if}
          </p>
        </div>
      </div>

      <button
        type="button"
        class="rounded-lg border border-slate-200 bg-slate-100 px-3.5 py-1.5 text-xs font-semibold text-slate-700 transition-colors hover:bg-slate-200 disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-slate-200 dark:hover:bg-zinc-700 cursor-pointer"
        onclick={onCheckForUpdates}
        disabled={isCheckingForUpdates}
      >
        {isCheckingForUpdates ? $t('update_checking') : $t('check_for_updates')}
      </button>
    </div>
  </section>

  <!-- Category 5: Account (Moved to the bottom) -->
  <section class="space-y-3">
    <h2 class="text-xs font-bold uppercase tracking-wider text-slate-400 dark:text-zinc-500 flex items-center gap-2">
      <User class="w-3.5 h-3.5" />
      <span>{$t('account')}</span>
    </h2>

    <div class="settings-card flex items-center justify-between gap-4">
      <div class="flex items-start gap-3">
        <div class="p-2 rounded-lg bg-sky-50 dark:bg-sky-950/40 text-sky-600 dark:text-sky-400 shrink-0 mt-0.5">
          <User class="w-4 h-4" />
        </div>
        <div class="space-y-1">
          <p class="font-semibold text-slate-900 dark:text-slate-100 text-sm">{$t('account')}</p>
          {#if currentUserName}
            <p class="text-xs font-semibold text-slate-700 dark:text-slate-300">
              {$t('logged_in_as')} {currentUserName}
            </p>
          {/if}
          <p class="text-xs text-slate-500 dark:text-slate-400">
            {$t('logged_in_at')} <span class="font-mono text-[11px]">{serverUrl}</span>
          </p>
        </div>
      </div>

      <button
        type="button"
        class="flex items-center gap-1.5 rounded-lg border border-red-200 bg-red-50 px-3.5 py-1.5 text-xs font-semibold text-red-600 transition-colors hover:bg-red-100 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-400 dark:hover:bg-red-950/60 cursor-pointer"
        onclick={onLogout}
      >
        <LogOut class="w-3.5 h-3.5" />
        <span>{$t('logout')}</span>
      </button>
    </div>
  </section>
</div>
