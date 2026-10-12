<script lang="ts">
  import { onMount } from "svelte";
  import { Settings as SettingsIcon, AlertCircle, X } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
  import UpdateDialog from "#lib/components/UpdateDialog.svelte";
  import { createFeedbackController } from "#lib/controllers/feedback-controller.svelte.ts";
  import { createAuthController } from "#lib/controllers/auth-controller.svelte.ts";
  import { createSyncController } from "#lib/controllers/sync-controller.svelte.ts";
  import { createPreferencesController } from "#lib/controllers/preferences-controller.svelte.ts";
  import { createUpdateController } from "#lib/controllers/update-controller.svelte.ts";

  let currentView = $state("dashboard");
  const feedback = createFeedbackController(() => $t("unknown_error"));
  const sync = createSyncController({
    reportError: feedback.report,
    t: (key) => $t(key),
    locale: () => $locale ?? "en",
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
    };
  });
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
      {#if auth.isAuthenticated}
        <div class="flex items-center gap-4">
          <!-- h2 → h1 for correct heading hierarchy in the content column -->
          <h1 class="text-xl font-bold text-slate-800 dark:text-slate-100">
            {currentView === "dashboard" ? $t('dashboard') : $t('settings')}
          </h1>
        </div>
        <div class="flex items-center gap-2">
          <button
            class="w-10 h-10 rounded-full flex items-center justify-center hover:bg-white/50 dark:hover:bg-white/10 transition-colors"
            onclick={() => currentView = currentView === "settings" ? "dashboard" : "settings"}
          >
            {#if currentView === "settings"}
              <X class="w-5 h-5 text-slate-900 dark:text-slate-100" />
            {:else}
              <SettingsIcon class="w-5 h-5 text-slate-400 dark:text-slate-400" />
            {/if}
          </button>
        </div>
      {/if}
    </header>

    <main class="flex-1 overflow-y-auto p-10 flex flex-col {!auth.isAuthenticated ? 'justify-center' : 'space-y-10'}">
      {#if auth.isAuthenticated && feedback.error}
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
      {#if !auth.isAuthenticated}
        <LoginView
          bind:serverUrl={auth.serverUrl}
          bind:apiKey={auth.apiKey}
          bind:cloudflareClientId={auth.cloudflareClientId}
          bind:cloudflareClientSecret={auth.cloudflareClientSecret}
          isLoggingIn={auth.isLoggingIn}
          loginError={auth.loginError}
          onLogin={auth.login}
        />
      <!-- ===== DASHBOARD VIEW ===== -->
      {:else if currentView === "dashboard"}
        <DashboardView
          syncStatus={sync.syncStatus}
          lastSync={sync.lastSync}
          progress={sync.progress}
          processed={sync.processed}
          syncTotal={sync.syncTotal}
          currentFile={sync.currentFile}
          transferredBytes={sync.transferredBytes}
          transferRateBytesPerSecond={sync.transferRateBytesPerSecond}
          estimatedSecondsRemaining={sync.estimatedSecondsRemaining}
          serverUrl={auth.serverUrl}
          watchedFolders={sync.watchedFolders}
          failedSyncs={sync.failedSyncs}
          logs={sync.logs}
          isSyncActionPending={sync.isSyncActionPending}
          onSyncAction={sync.syncAction}
          onRetryFailedSyncs={sync.retryFailedSyncs}
          onAddFolder={sync.addFolder}
          onRemoveFolder={sync.removeFolder}
        />
      <!-- ===== SETTINGS VIEW ===== -->
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
    <footer class="h-16 px-10 border-t border-black/5 dark:border-white/10 flex items-center justify-end text-[10px] font-bold text-slate-400 dark:text-slate-400 uppercase tracking-widest shrink-0">
      <span>Lymic - Unofficial Immich Desktop Client</span>
      {#if updater.appVersion}
        <span class="ml-2">v{updater.appVersion}</span>
      {/if}
    </footer>
  </div>
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
</div>
