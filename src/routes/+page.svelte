<script lang="ts">
  import { onMount } from "svelte";
  import { Settings as SettingsIcon, AlertCircle, X, LayoutDashboard } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
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
    <!-- Centered Login Layout -->
    <div class="flex-1 flex flex-col items-center justify-center p-8 overflow-y-auto">
      <div class="flex items-center gap-3 mb-8">
        <img src={logo} alt="Lymic Logo" class="w-12 h-12 object-contain" />
        <div>
          <span class="font-bold tracking-tight text-2xl text-slate-900 dark:text-slate-100">Lymic</span>
          <p class="text-xs text-slate-400 dark:text-zinc-500 font-medium uppercase tracking-wider">Immich Desktop Client</p>
        </div>
      </div>
      <div class="w-full max-w-md">
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
      <footer class="mt-8 text-xs text-slate-400 dark:text-zinc-600 font-medium">
        <span>Lymic - Unofficial Immich Desktop Client</span>
        {#if updater.appVersion}
          <span class="ml-2">v{updater.appVersion}</span>
        {/if}
      </footer>
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
            {#if currentView === 'dashboard'}
              <span class="nav-indicator"></span>
            {/if}
            <LayoutDashboard class="w-4 h-4 shrink-0 {currentView === 'dashboard' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('dashboard')}</span>
          </button>

          <button
            type="button"
            class="nav-item {currentView === 'settings' ? 'active' : ''}"
            onclick={() => currentView = "settings"}
            aria-label={$t('settings')}
          >
            {#if currentView === 'settings'}
              <span class="nav-indicator"></span>
            {/if}
            <SettingsIcon class="w-4 h-4 shrink-0 {currentView === 'settings' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('settings')}</span>
          </button>
        </nav>
      </div>

      <!-- Bottom: User & Status info -->
      <div class="pt-3 px-2">
        <div class="flex items-center text-[11px] text-slate-400 dark:text-zinc-500">
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
            {currentView === "dashboard" ? $t('dashboard') : $t('settings')}
          </h1>
        </div>
      </header>

      <main class="flex-1 overflow-y-auto p-8 flex flex-col space-y-6">
        {#if feedback.error}
          <div class="p-4 bg-red-50 dark:bg-red-950/50 text-red-600 dark:text-red-300 text-xs rounded-xl flex items-center gap-3" role="alert">
            <AlertCircle class="w-4 h-4 shrink-0" />
            <span>{feedback.error}</span>
            <button
              type="button"
              class="ml-auto p-1 rounded hover:bg-red-100 dark:hover:bg-red-900/50 transition-colors"
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
        {:else if currentView === "settings"}
          <SettingsView
            currentUserName={auth.currentUserName}
            serverUrl={auth.serverUrl}
            isAutostartEnabled={preferences.isAutostartEnabled}
            themePreference={preferences.themePreference}
            uploadParallelism={preferences.uploadParallelism}
            isSavingUploadParallelism={preferences.isSavingUploadParallelism}
            updateStatus={updater.updateStatus}
            isCheckingForUpdates={updater.isCheckingForUpdates}
            locales={preferences.locales}
            onToggleAutostart={preferences.toggleAutostart}
            onLocaleChange={preferences.setLocale}
            onThemeChange={preferences.setThemePreference}
            onUploadParallelismChange={preferences.changeUploadParallelism}
            onCheckForUpdates={updater.checkForUpdates}
            onLogout={auth.logout}
          />
        {/if}
      </main>

      <!-- Global Footer -->
      <footer class="h-10 px-8 flex items-center justify-end text-[10px] font-medium text-slate-400 dark:text-zinc-500 uppercase tracking-wider shrink-0">
        <span>Lymic - Unofficial Immich Desktop Client</span>
        {#if updater.appVersion}
          <span class="ml-2">v{updater.appVersion}</span>
        {/if}
      </footer>
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
