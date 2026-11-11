<script lang="ts">
  import { onMount } from "svelte";
  import { Settings as SettingsIcon, Info as InfoIcon, AlertCircle, ArrowLeft, X, FolderSync, ScrollText, User, Images, Users, Album } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import TimelineView from "#lib/components/TimelineView.svelte";
  import PeopleView from "#lib/components/PeopleView.svelte";
  import AlbumCatalogueView from "#lib/components/AlbumCatalogueView.svelte";
  import ActivityLog from "#lib/components/ActivityLog.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
  import InfoView from "#lib/components/InfoView.svelte";
  import UpdateDialog from "#lib/components/UpdateDialog.svelte";
  import RemovableVolumePrompt from "#lib/components/RemovableVolumePrompt.svelte";
  import TargetAlbumModal from "#lib/components/TargetAlbumModal.svelte";
  import { createFeedbackController } from "#lib/controllers/feedback-controller.svelte.ts";
  import { createAuthController } from "#lib/controllers/auth-controller.svelte.ts";
  import { createSyncController } from "#lib/controllers/sync-controller.svelte.ts";
  import { createWatchedFoldersController } from "#lib/controllers/watched-folders-controller.svelte.ts";
  import { createPreferencesController } from "#lib/controllers/preferences-controller.svelte.ts";
  import { createUpdateController } from "#lib/controllers/update-controller.svelte.ts";
  import { createRemovableVolumeController } from "#lib/controllers/removable-volume-controller.svelte.ts";
  import { createTargetAlbumCatalogue } from "#lib/controllers/target-album-catalogue.svelte.ts";
  import { createTimelineController } from "#lib/controllers/timeline-controller.svelte.ts";
  import { createPeopleController } from "#lib/controllers/people-controller.svelte.ts";
  import { createAlbumCatalogueController } from "#lib/controllers/album-catalogue-controller.svelte.ts";
  import { createAlbumTimelineController } from "#lib/controllers/album-timeline-controller.svelte.ts";
  import { createPersonTimelineController } from "#lib/controllers/person-timeline-controller.svelte.ts";
  import type { AlbumCatalogueItem, RecognizedPerson } from "#lib/contracts.ts";
  import { tauriDesktopAdapter } from "#lib/tauri/desktop-adapter.ts";

  let currentView = $state("photos");
  const desktop = tauriDesktopAdapter;
  const feedback = createFeedbackController(() => $t("unknown_error"));
  const targetAlbumCatalogue = createTargetAlbumCatalogue(desktop);
  const removable = createRemovableVolumeController({
    adapter: desktop,
    t: (key, params) => $t(key, params),
    onVolumeRemounted: () => void watchedFolders.refresh(),
  });
  const watchedFolders = createWatchedFoldersController({
    adapter: desktop,
    reportError: feedback.report,
    t: (key) => $t(key),
    targetAlbumCatalogue,
    onRemovableVolumeAccepted: (volumeId) => removable.dismiss(volumeId),
    onFolderRemoved: () => sync.refreshFailedSyncs(),
  });
  const sync = createSyncController({
    adapter: desktop,
    reportError: feedback.report,
    locale: () => $locale ?? "en",
    canStartSync: () => watchedFolders.hasFolders,
    onSyncSettled: () => {
      void timeline.refresh();
      return watchedFolders.refresh();
    },
  });
  const timeline = createTimelineController(desktop);
  const people = createPeopleController(desktop);
  const albumCatalogue = createAlbumCatalogueController(desktop);
  let selectedAlbum = $state<AlbumCatalogueItem | null>(null);
  let albumTimeline = $state<ReturnType<typeof createAlbumTimelineController> | null>(null);
  let selectedPerson = $state<RecognizedPerson | null>(null);
  let personTimeline = $state<ReturnType<typeof createPersonTimelineController> | null>(null);
  let peopleScrollTop = 0;
  let albumsScrollTop = 0;
  let content = $state<HTMLElement>();

  function selectPerson(person: RecognizedPerson) {
    peopleScrollTop = content?.scrollTop ?? 0;
    selectedPerson = person;
    personTimeline = createPersonTimelineController(desktop, person.id);
    currentView = "person-timeline";
    void personTimeline.refresh();
  }

  function selectAlbum(album: AlbumCatalogueItem) {
    albumsScrollTop = content?.scrollTop ?? 0;
    selectedAlbum = album;
    albumTimeline = createAlbumTimelineController(desktop, album.id);
    currentView = "album-timeline";
    void albumTimeline.refresh();
  }

  const pageTitle = $derived.by(() => {
    switch (currentView) {
      case "photos": return $t("timeline_title");
      case "people": return $t("people_title");
      case "albums": return $t("albums_title");
      case "album-timeline": return selectedAlbum?.albumName ?? $t("albums_title");
      case "person-timeline": return selectedPerson?.name ?? $t("people_unnamed");
      case "dashboard": return $t("dashboard");
      case "logs": return $t("logs");
      case "settings": return $t("settings");
      default: return $t("info");
    }
  });

  function closePersonTimeline() {
    personTimeline?.clear();
    personTimeline = null;
    selectedPerson = null;
  }

  function closeAlbumTimeline() {
    albumTimeline?.clear();
    albumTimeline = null;
    selectedAlbum = null;
  }

  function openPeople() {
    currentView = "people";
    void people.refresh();
  }

  function openAlbums() {
    currentView = "albums";
    void albumCatalogue.refresh();
  }

  function returnToPeople() {
    closePersonTimeline();
    currentView = "people";
    queueMicrotask(() => { if (content) content.scrollTop = peopleScrollTop; });
  }

  function returnToAlbums() {
    closeAlbumTimeline();
    currentView = "albums";
    queueMicrotask(() => { if (content) content.scrollTop = albumsScrollTop; });
  }

  const auth = createAuthController({
    adapter: desktop,
    formatError: feedback.formatError,
    reportError: feedback.report,
    t: (key) => $t(key),
    onLogin: async () => {
      currentView = "photos";
      await Promise.all([timeline.refresh(), sync.refreshFailedSyncs(), watchedFolders.refresh()]);
    },
    onLogout: () => {
      targetAlbumCatalogue.clear();
      feedback.clear();
      removable.clear();
      sync.clearUserState();
      watchedFolders.clear();
      timeline.clear();
      people.clear();
      albumCatalogue.clear();
      closeAlbumTimeline();
      closePersonTimeline();
      currentView = "photos";
    },
  });
  const preferences = createPreferencesController({
    adapter: desktop,
    reportError: feedback.report,
    addLog: sync.addLog,
    locale: () => $locale ?? "en",
    setLocale: (value) => $locale = value,
  });
  const updater = createUpdateController(() => sync.isActive, desktop);
  const targetAlbumDialog = $derived(watchedFolders.targetAlbumDialog);

  let profileImageLoadFailed = $state(false);

  $effect(() => {
    if (auth.profileImage) {
      profileImageLoadFailed = false;
    }
  });

  onMount(() => {
    sync.startLifecycle();
    watchedFolders.startLifecycle();
    updater.startLifecycle();
    preferences.startLifecycle();
    void removable.startSubscriptions();

    void auth.hydrate()
      .then(() => {
        if (!auth.isAuthenticated) return;
        return Promise.all([sync.refreshFailedSyncs(), timeline.refresh()]);
      })
      .catch((error) => console.error("Failed to hydrate authentication", error));
    void watchedFolders.refresh()
      .catch((error) => console.error("Failed to hydrate watched folders", error));
    void sync.hydrate()
      .then(() => sync.startSubscriptions())
      .catch((error) => console.error("Failed to hydrate sync state", error));
    void preferences.hydrate().catch((error) => console.error("Failed to hydrate preferences", error));
    updater.startup();

    return () => {
      sync.destroy();
      watchedFolders.destroy();
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
          alt={$t('hero_landscape')}
          class="hero-image"
        />
        <div class="absolute inset-0 bg-gradient-to-t from-black/50 to-transparent"></div>
      </div>

      <div class="absolute top-12 left-12 flex items-center gap-3 text-white">
        <img src={logo} alt={$t('lymic_logo')} class="w-16 h-16 object-contain" />
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
          <img src={logo} alt={$t('lymic_logo')} class="w-8 h-8 object-contain shrink-0" />
          <div class="leading-tight">
            <span class="font-bold text-base text-slate-900 dark:text-slate-100">Lymic</span>
            <p class="text-[10px] text-slate-400 dark:text-zinc-500 uppercase tracking-wider font-semibold">{$t('desktop_client')}</p>
          </div>
        </div>

        <!-- Navigation items -->
        <nav class="space-y-1" aria-label={$t('main_navigation')}>
          <button
            type="button"
            class="nav-item {currentView === 'photos' ? 'active' : ''}"
            onclick={() => currentView = "photos"}
            aria-label={$t('timeline_title')}
          >
            <Images class="w-4 h-4 shrink-0 {currentView === 'photos' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('timeline_title')}</span>
          </button>
          <button type="button" class="nav-item {currentView === 'albums' ? 'active' : ''}" onclick={openAlbums} aria-label={$t('albums_title')}>
            <Album class="w-4 h-4 shrink-0 {currentView === 'albums' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('albums_title')}</span>
          </button>
          <button
            type="button"
            class="nav-item {currentView === 'people' ? 'active' : ''}"
            onclick={openPeople}
            aria-label={$t('people_title')}
          >
            <Users class="w-4 h-4 shrink-0 {currentView === 'people' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('people_title')}</span>
          </button>

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
          {#if auth.profileImage && !profileImageLoadFailed}
            <img
              src={auth.profileImage}
              alt={auth.currentUserName || $t('user_profile')}
              class="w-3.5 h-3.5 rounded-full object-cover shrink-0"
              onerror={() => profileImageLoadFailed = true}
              data-testid="sidebar-profile-image"
            />
          {:else}
            <User class="w-3.5 h-3.5 shrink-0" data-testid="sidebar-user-icon" />
          {/if}
          <span class="truncate font-medium" title={auth.currentUserName || auth.serverUrl}>
            {auth.currentUserName || $t('connected')}
          </span>
        </div>
      </div>
    </aside>

    <!-- Right Side: Content Column -->
    <div class="flex-1 flex flex-col h-full relative overflow-hidden bg-slate-100/70 dark:bg-zinc-900/60">
      <!-- Header -->
      <header class="h-12 px-8 flex items-center justify-between shrink-0 z-10">
        <div class="flex items-center gap-4">
          <h1 class="text-lg font-bold text-slate-900 dark:text-slate-100">
            {pageTitle}
          </h1>
        </div>
      </header>

      <main bind:this={content} class="flex-1 {currentView === 'photos' || currentView === 'person-timeline' || currentView === 'album-timeline' ? 'overflow-hidden px-8 pt-2 pb-8' : 'overflow-y-auto px-8 pt-4 pb-8'} flex flex-col space-y-6">
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
              await watchedFolders.acceptRemovableFolder(vol);
            }}
            onDismiss={(volId) => removable.dismiss(volId)}
          />
        {/if}

        {#if currentView === "photos"}
          <TimelineView {timeline} serverUrl={auth.serverUrl} onOpenExternalUrl={desktop.openExternalUrl} getAssetDetails={desktop.getAssetDetails} />
        {:else if currentView === "people"}
          <PeopleView {people} onSelect={selectPerson} />
        {:else if currentView === "albums"}
          <AlbumCatalogueView catalogue={albumCatalogue} onSelect={selectAlbum} />
        {:else if currentView === "album-timeline" && albumTimeline}
          <div class="flex h-full min-h-0 flex-col gap-2">
            <button type="button" class="inline-flex self-start items-center gap-2 rounded-lg px-2 py-1 text-sm font-medium text-slate-600 hover:bg-slate-200 hover:text-slate-900 dark:text-zinc-300 dark:hover:bg-zinc-800 dark:hover:text-zinc-100" onclick={returnToAlbums}>
              <ArrowLeft class="h-4 w-4" />
              {$t('albums_return')}
            </button>
            <TimelineView timeline={albumTimeline} serverUrl={auth.serverUrl} onOpenExternalUrl={desktop.openExternalUrl} getAssetDetails={desktop.getAssetDetails} emptyMessage={$t('albums_timeline_empty')} loadErrorMessage={$t('albums_error')} />
          </div>
        {:else if currentView === "person-timeline" && personTimeline}
          <div class="flex h-full min-h-0 flex-col gap-2">
            <button type="button" class="inline-flex self-start items-center gap-2 rounded-lg px-2 py-1 text-sm font-medium text-slate-600 hover:bg-slate-200 hover:text-slate-900 dark:text-zinc-300 dark:hover:bg-zinc-800 dark:hover:text-zinc-100" onclick={returnToPeople}>
              <ArrowLeft class="h-4 w-4" />
              {$t('people_return')}
            </button>
            <TimelineView timeline={personTimeline} serverUrl={auth.serverUrl} onOpenExternalUrl={desktop.openExternalUrl} getAssetDetails={desktop.getAssetDetails} emptyMessage={$t('people_timeline_empty')} />
          </div>
        {:else if currentView === "dashboard"}
          <DashboardView
            status={sync.status}
            folders={watchedFolders.folders}
            catalogue={targetAlbumCatalogue}
          />
        {:else if currentView === "logs"}
          <ActivityLog logs={sync.logs} />
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
            isUpdatesSupported={updater.isUpdatesSupported}
            locales={preferences.locales}
            onToggleAutostart={preferences.toggleAutostart}
            onLocaleChange={preferences.setLocale}
            onThemeChange={preferences.setThemePreference}
            onUploadParallelismChange={preferences.changeUploadParallelism}
            onCheckForUpdates={updater.checkForUpdates}
            onLogout={auth.logout}
          />
        {:else if currentView === "info"}
          <InfoView
            appVersion={updater.appVersion}
            onOpenExternalUrl={desktop.openExternalUrl}
          />
        {/if}
      </main>
    </div>
  {/if}

  {#if updater.isUpdatesSupported && updater.isUpdateDialogOpen && updater.availableUpdate}
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
  {#if auth.isAuthenticated && targetAlbumDialog.isOpen}
    <TargetAlbumModal
      isOpen={targetAlbumDialog.isOpen}
      folderPath={targetAlbumDialog.folderPath ?? ""}
      onConfirm={targetAlbumDialog.confirm}
      onCancel={targetAlbumDialog.cancel}
      catalogue={targetAlbumCatalogue}
    />
  {/if}
</div>
