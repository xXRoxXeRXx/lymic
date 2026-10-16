<script lang="ts">
  import { onMount } from "svelte";
  import { Settings as SettingsIcon, Info as InfoIcon, AlertCircle, X, FolderSync, ScrollText, User } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import ActivityLog from "#lib/components/ActivityLog.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
  import InfoView from "#lib/components/InfoView.svelte";
  import UpdateDialog from "#lib/components/UpdateDialog.svelte";
  import RemovableVolumePrompt from "#lib/components/RemovableVolumePrompt.svelte";
  import TargetAlbumModal from "#lib/components/TargetAlbumModal.svelte";
  import { createFeedbackController } from "#lib/controllers/feedback-controller.svelte.ts";
  import { createAuthController } from "#lib/controllers/auth-controller.svelte.ts";
  import { createSyncController } from "#lib/controllers/sync-controller.svelte.ts";
  import { createPreferencesController } from "#lib/controllers/preferences-controller.svelte.ts";
  import { createUpdateController } from "#lib/controllers/update-controller.svelte.ts";
  import { createRemovableVolumeController } from "#lib/controllers/removable-volume-controller.svelte.ts";

  let currentView = $state("dashboard");
  const feedback = createFeedbackController(() => $t("unknown_error"));
  const removable = createRemovableVolumeController({
    t: (key, params) => $t(key, params),
  });
  const sync = createSyncController({
    reportError: feedback.report,
    t: (key) => $t(key),
    locale: () => $locale ?? "en",
    onRemovableVolumeAccepted: (volumeId) => removable.dismiss(volumeId),
  });

  const auth = createAuthController({
    formatError: feedback.formatError,
    reportError: feedback.report,
    t: (key) => $t(key),
    onLogin: sync.refreshFailedSyncs,
    onLogout: () => {
      sync.clearUserState();
      currentView = "dashboard";
    },
  });
  const preferences = createPreferencesController({
    reportError: feedback.report,
    addLog: sync.addLog,
    locale: () => $locale ?? "en",
    setLocale: (value) => $locale = value,
  });
  const updater = createUpdateController(() => sync.isActive);

  onMount(() => {
    sync.startLifecycle();
    updater.startLifecycle();
    preferences.startLifecycle();
    void removable.startSubscriptions();

    void auth.hydrate()
      .then(() => auth.isAuthenticated ? sync.refreshFailedSyncs() : undefined)
      .catch((error) => console.error("Failed to hydrate authentication", error));
    void sync.hydrate()
      .then(() => sync.startSubscriptions())
      .catch((error) => console.error("Failed to hydrate sync state", error));
    void preferences.hydrate().catch((error) => console.error("Failed to hydrate preferences", error));
    updater.startup();

    return () => {
      sync.destroy();
      updater.destroy();
      preferences.stopLifecycle();
      removable.destroy();
    };
  });
</script>

<div class="h-screen flex selection:bg-blue-600 selection:text-white overflow-hidden bg-slate-100/70 dark:bg-zinc-900/60">
  {#if !auth.isAuthenticated}
    <!-- Left Side: Hero Image (Login Screen) -->
    <aside class="w-[38%] h-full shrink-0 relative overflow-hidden select-none">
      <div class="hero-container">
        <img
          src={hero}
          alt="Hero Landscape"
          class="hero-image"
        />
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

    <!-- Right Side: Login Content Column -->
    <div class="flex-1 flex flex-col h-full relative overflow-hidden bg-slate-100/70 dark:bg-zinc-900/60">
      <div class="flex-1 overflow-y-auto p-10 flex flex-col justify-center">
        <LoginView
          bind:serverUrl={auth.serverUrl}
          bind:apiKey={auth.apiKey}
          bind:allowLocalHttp={auth.allowLocalHttp}
          bind:cloudflareClientId={auth.cloudflareClientId}
          bind:cloudflareClientSecret={auth.cloudflareClientSecret}
          isLoggingIn={auth.isLoggingIn}
          loginError={auth.loginError}
          loginErrorCode={auth.loginErrorCode}
          onLogin={auth.login}
        />
      </div>

    </div>
  {:else}
    <!-- Authenticated Layout: Left Sidebar + Right Content Area -->
    <aside class="w-60 shrink-0 h-full flex flex-col justify-between border-r border-black/5 dark:border-white/10 bg-slate-100/70 dark:bg-zinc-900/60 pt-6 pb-4 px-3 select-none">
      <div class="space-y-6">
        <!-- Brand / Header (padded for desktop window controls) -->
        <div class="flex items-center gap-3 px-3 py-1">
          <img src={logo} alt="Lymic Logo" class="w-8 h-8 object-contain shrink-0" />
          <div class="leading-tight">
            <span class="font-bold text-base text-slate-900 dark:text-slate-100">Lymic</span>
            <p class="text-[10px] text-slate-400 dark:text-zinc-500 uppercase tracking-wider font-semibold">Desktop Client</p>
          </div>
        </div>

        <!-- Navigation items -->
        <nav class="space-y-1" aria-label="Main Navigation">
          <button
            type="button"
            class="nav-item {currentView === 'dashboard' ? 'active' : ''}"
            onclick={() => currentView = "dashboard"}
            aria-label={$t('dashboard')}
          >
            <FolderSync class="w-4 h-4 shrink-0 {currentView === 'dashboard' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('dashboard')}</span>
          </button>

          <button
            type="button"
            class="nav-item {currentView === 'logs' ? 'active' : ''}"
            onclick={() => currentView = "logs"}
            aria-label={$t('logs')}
          >
            <ScrollText class="w-4 h-4 shrink-0 {currentView === 'logs' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('logs')}</span>
          </button>

          <button
            type="button"
            class="nav-item {currentView === 'settings' ? 'active' : ''}"
            onclick={() => currentView = "settings"}
            aria-label={$t('settings')}
          >
            <SettingsIcon class="w-4 h-4 shrink-0 {currentView === 'settings' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('settings')}</span>
          </button>

          <button
            type="button"
            class="nav-item {currentView === 'info' ? 'active' : ''}"
            onclick={() => currentView = "info"}
            aria-label={$t('info')}
          >
            <InfoIcon class="w-4 h-4 shrink-0 {currentView === 'info' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('info')}</span>
          </button>
        </nav>
      </div>

      <!-- Bottom: User & Status info -->
      <div class="pt-3 px-2">
        <div class="flex items-center gap-2 text-[11px] text-slate-400 dark:text-zinc-500">
          <User class="w-3.5 h-3.5 shrink-0" />
          <span class="truncate font-medium" title={auth.currentUserName || auth.serverUrl}>
            {auth.currentUserName || "Connected"}
          </span>
        </div>
      </div>
    </aside>

    <!-- Right Side: Content Column -->
    <div class="flex-1 flex flex-col h-full relative overflow-hidden bg-slate-100/70 dark:bg-zinc-900/60">
      <!-- Header -->
      <header class="h-16 px-8 flex items-center justify-between shrink-0 z-10">
        <div class="flex items-center gap-4">
          <h1 class="text-lg font-bold text-slate-900 dark:text-slate-100">
            {currentView === "dashboard" ? $t('dashboard') : currentView === "logs" ? $t('logs') : currentView === "settings" ? $t('settings') : $t('info')}
          </h1>
        </div>
      </header>

      <main class="flex-1 overflow-y-auto p-8 flex flex-col space-y-6">
        {#if feedback.error}
          <div class="p-4 bg-red-50/70 dark:bg-red-950/30 border border-red-200/80 dark:border-red-900/50 text-red-600 dark:text-red-300 text-xs rounded-xl flex items-center gap-3" role="alert">
            <AlertCircle class="w-4 h-4 shrink-0" />
            <span class="flex-1">{feedback.error}</span>
            <button
              type="button"
              class="ml-auto p-1 rounded text-red-400 hover:text-red-600 hover:bg-red-100 dark:text-red-400 dark:hover:text-red-200 dark:hover:bg-red-900/50 transition-colors cursor-pointer shrink-0"
              aria-label={$t('dismiss_error')}
              onclick={feedback.clear}
            >
              <X class="w-4 h-4" />
            </button>
          </div>
        {/if}
        {#if removable.currentPrompt}
          <RemovableVolumePrompt
            volume={removable.currentPrompt}
            onAccept={async (vol) => {
              currentView = "dashboard";
              await sync.acceptRemovableFolder(vol);
            }}
            onDismiss={(volId) => removable.dismiss(volId)}
          />
        {/if}

        {#if currentView === "dashboard"}
          <DashboardView
            dashboard={sync.dashboard}
            serverUrl={auth.serverUrl}
          />
        {:else if currentView === "logs"}
          <ActivityLog logs={sync.dashboard.logs} />
        {:else if currentView === "settings"}
          <SettingsView
            currentUserName={auth.currentUserName}
            serverUrl={auth.serverUrl}
            isAutostartEnabled={preferences.isAutostartEnabled}
            virtualLibraryEnabled={preferences.virtualLibraryEnabled}
            virtualLibraryPath={preferences.virtualLibraryPath}
            virtualLibraryConnectionStatus={preferences.virtualLibraryConnectionStatus}
            themePreference={preferences.themePreference}
            uploadParallelism={preferences.uploadParallelism}
            isSavingUploadParallelism={preferences.isSavingUploadParallelism}
            thumbnailCacheCapBytes={preferences.thumbnailCacheCapBytes}
            isClearingThumbnailCache={preferences.isClearingThumbnailCache}
            isClearingVirtualLibraryCache={preferences.isClearingVirtualLibraryCache}
            virtualLibraryCacheClearResult={preferences.virtualLibraryCacheClearResult}
            isRefreshingVirtualLibrary={preferences.isRefreshingVirtualLibrary}
            updateStatus={updater.updateStatus}
            isCheckingForUpdates={updater.isCheckingForUpdates}
            locales={preferences.locales}
            onToggleAutostart={preferences.toggleAutostart}
            onToggleVirtualLibrary={preferences.toggleVirtualLibrary}
            onLocaleChange={preferences.setLocale}
            onThemeChange={preferences.setThemePreference}
            onUploadParallelismChange={preferences.changeUploadParallelism}
            onThumbnailCacheCapChange={preferences.changeThumbnailCacheCap}
            onClearThumbnailCache={preferences.clearThumbnailCache}
            onClearVirtualLibraryCache={preferences.clearVirtualLibraryCache}
            onRefreshVirtualLibrary={preferences.refreshVirtualLibrary}
            onCheckForUpdates={updater.checkForUpdates}
            onLogout={auth.logout}
          />
        {:else if currentView === "info"}
          <InfoView
            appVersion={updater.appVersion}
          />
        {/if}
      </main>
    </div>
  {/if}

  {#if updater.isUpdateDialogOpen && updater.availableUpdate}
    <UpdateDialog
      version={updater.availableUpdate.version}
      notes={updater.availableUpdate.body ?? ""}
      isSyncActive={sync.isActive}
      installState={updater.updateInstallState}
      downloadedBytes={updater.downloadedUpdateBytes}
      contentLength={updater.updateContentLength}
      onLater={() => updater.isUpdateDialogOpen = false}
      onInstall={updater.installUpdate}
    />
  {/if}
  {#if !auth.isAuthenticated && sync.isTargetAlbumModalOpen}
    <TargetAlbumModal
      isOpen={sync.isTargetAlbumModalOpen}
      folderPath={sync.pendingFolderPath ?? ""}
      onConfirm={sync.confirmAddFolder}
      onCancel={sync.cancelAddFolder}
    />
  {/if}
</div>
