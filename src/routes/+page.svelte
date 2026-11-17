<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { Settings as SettingsIcon, Info as InfoIcon, AlertCircle, X, FolderSync, ScrollText, User, Images, Heart, Users, Album, Map as MapIcon, Compass, Archive, Trash2, Tag as TagIcon } from "lucide-svelte";
  import { t, locale } from "svelte-i18n";
  import hero from "#lib/assets/hero.png";
  import logo from "#lib/assets/logo.png";
  import LoginView from "#lib/components/LoginView.svelte";
  import DashboardView from "#lib/components/DashboardView.svelte";
  import TimelineView from "#lib/components/TimelineView.svelte";
  import MapView from "#lib/components/MapView.svelte";
  import ExploreView from "#lib/components/ExploreView.svelte";
  import BackBarView from "#lib/components/BackBarView.svelte";
  import AllMemoriesView from "#lib/components/AllMemoriesView.svelte";
  import SearchBar from "#lib/components/SearchBar.svelte";
  import SearchResultsView from "#lib/components/SearchResultsView.svelte";
  import PeopleView from "#lib/components/PeopleView.svelte";
  import PlacesView from "#lib/components/PlacesView.svelte";
  import TagsView from "#lib/components/TagsView.svelte";
  import AlbumCatalogueView from "#lib/components/AlbumCatalogueView.svelte";
  import ActivityLog from "#lib/components/ActivityLog.svelte";
  import SettingsView from "#lib/components/SettingsView.svelte";
  import InfoView from "#lib/components/InfoView.svelte";
  import UpdateDialog from "#lib/components/UpdateDialog.svelte";
  import RemovableVolumePrompt from "#lib/components/RemovableVolumePrompt.svelte";
  import TargetAlbumModal from "#lib/components/TargetAlbumModal.svelte";
  import { createSessionController } from "#lib/controllers/session-controller.svelte.ts";
  import { createPreferencesController } from "#lib/controllers/preferences-controller.svelte.ts";
  import { createUpdateController } from "#lib/controllers/update-controller.svelte.ts";
  import { createAlbumTimelineController } from "#lib/controllers/album-timeline-controller.svelte.ts";
  import { createPersonTimelineController } from "#lib/controllers/person-timeline-controller.svelte.ts";
  import { createPlaceTimelineController } from "#lib/controllers/place-timeline-controller.svelte.ts";
  import { createMemoryTimelineController } from "#lib/controllers/memory-timeline-controller.svelte.ts";
  import { createTagTimelineController } from "#lib/controllers/tag-timeline-controller.svelte.ts";
  import { defaultTagOrigin, recordTagOrigin } from "#lib/controllers/tag-origin.ts";
  import { localizedMemoryTitle } from "#lib/memory-title.ts";
  import type { AlbumCatalogueItem, Memory, Place, RecognizedPerson, Tag } from "#lib/contracts.ts";
  import { tauriDesktopAdapter } from "#lib/tauri/desktop-adapter.ts";

  let currentView = $state("photos");
  const desktop = tauriDesktopAdapter;
  let mapCreated = $state(false);
  let selectedAlbum = $state<AlbumCatalogueItem | null>(null);
  let albumTimeline = $state<ReturnType<typeof createAlbumTimelineController> | null>(null);
  let selectedPerson = $state<RecognizedPerson | null>(null);
  let personTimeline = $state<ReturnType<typeof createPersonTimelineController> | null>(null);
  let selectedMemory = $state<Memory | null>(null);
  let memoryTimeline = $state<ReturnType<typeof createMemoryTimelineController> | null>(null);
  let personOrigin = $state<"people" | "explore">("people");
  let memoryOrigin = $state<"explore" | "all-memories">("explore");
  let selectedPlace = $state<Place | null>(null);
  let placeTimeline = $state<ReturnType<typeof createPlaceTimelineController> | null>(null);
  let placeOrigin = $state<"explore" | "places">("explore");
  let placesScrollTop = 0;
  let peopleScrollTop = 0;
  let exploreScrollTop = 0;
  let allMemoriesScrollTop = 0;
  let albumsScrollTop = 0;
  /** The Tags view's own scroll position, kept while another view is open. */
  let tagsScrollTop = 0;
  let selectedTag = $state<Tag | null>(null);
  let tagTimeline = $state<ReturnType<typeof createTagTimelineController> | null>(null);
  /** The one view (and scroll position) Back returns to from a Tag Timeline, whichever view it was opened from. */
  let tagOrigin = $state(defaultTagOrigin);
  let content = $state<HTMLElement>();
  /** The view a Search was started from and where it was scrolled to, so Back returns there (even to a sub-view). */
  let searchOrigin = $state<{ view: string; top: number } | null>(null);
  /** The scroll position the open shared timeline view last reported; its scrolling area is inside the view, not `main`. */
  let timelineScrollTop = 0;
  /** Where the timeline view of `view` opens, set only while returning from a Search. */
  let scrollRestore = $state<{ view: string; top: number } | null>(null);
  /** Views that show the shared timeline view, which scrolls inside itself. */
  const timelineScrollViews = new Set(["photos", "favorites", "archive", "trash", "person-timeline", "album-timeline", "place-timeline", "memory-timeline", "tag-timeline", "search"]);

  function selectPerson(person: RecognizedPerson) {
    personOrigin = currentView === "explore" ? "explore" : "people";
    if (personOrigin === "explore") exploreScrollTop = content?.scrollTop ?? 0;
    else peopleScrollTop = content?.scrollTop ?? 0;
    selectedPerson = person;
    personTimeline = createPersonTimelineController(desktop, person.id);
    currentView = "person-timeline";
    void personTimeline.refresh();
  }

  /** The open view and its scroll position; a timeline view scrolls inside itself, any other view in `main`. */
  function currentPosition() {
    return { view: currentView, top: timelineScrollViews.has(currentView) ? timelineScrollTop : (content?.scrollTop ?? 0) };
  }

  /** Opens a Tag's Tag Timeline from any view; a Tag Timeline opened from another Tag Timeline keeps its origin. */
  function selectTag(tag: Tag) {
    tagOrigin = recordTagOrigin(tagOrigin, currentPosition());
    tagTimeline?.clear();
    selectedTag = tag;
    tagTimeline = createTagTimelineController(desktop, tag.id);
    currentView = "tag-timeline";
    void tagTimeline.refresh();
  }

  function selectPlace(place: Place) {
    placeOrigin = currentView === "places" ? "places" : "explore";
    if (placeOrigin === "places") placesScrollTop = content?.scrollTop ?? 0;
    else exploreScrollTop = content?.scrollTop ?? 0;
    selectedPlace = place;
    placeTimeline = createPlaceTimelineController(desktop, place.city);
    currentView = "place-timeline";
    void placeTimeline.refresh();
  }

  function selectMemory(memory: Memory) {
    memoryOrigin = currentView === "all-memories" ? "all-memories" : "explore";
    if (memoryOrigin === "all-memories") allMemoriesScrollTop = content?.scrollTop ?? 0;
    else exploreScrollTop = content?.scrollTop ?? 0;
    selectedMemory = memory;
    memoryTimeline = createMemoryTimelineController(memory);
    currentView = "memory-timeline";
    void memoryTimeline.refresh();
  }

  function selectAlbum(album: AlbumCatalogueItem) {
    albumsScrollTop = content?.scrollTop ?? 0;
    selectedAlbum = album;
    albumTimeline = createAlbumTimelineController(desktop, album.id);
    currentView = "album-timeline";
    void albumTimeline.refresh();
  }

  function viewTitle(view: string) {
    switch (view) {
      case "search": return $t("search_title");
      case "photos": return $t("timeline_title");
      case "explore": return $t("explore_title");
      case "favorites": return $t("favorites_title");
      case "archive": return $t("archive_title");
      case "trash": return $t("trash_title");
      case "map": return $t("map_title");
      case "people": return $t("people_title");
      case "tags": return $t("tags_title");
      case "places": return $t("places_all_title");
      case "all-memories": return $t("memories_all_title");
      case "albums": return $t("albums_title");
      case "album-timeline": return selectedAlbum?.albumName ?? $t("albums_title");
      case "place-timeline": return selectedPlace?.city ?? $t("places_title");
      case "tag-timeline": return selectedTag?.value ?? $t("tags_title");
      case "person-timeline": return selectedPerson?.name ?? $t("people_unnamed");
      case "memory-timeline": return selectedMemory ? localizedMemoryTitle(selectedMemory, $t, $locale) : $t("memories_title");
      case "dashboard": return $t("dashboard");
      case "logs": return $t("logs");
      case "settings": return $t("settings");
      default: return $t("info");
    }
  }
  const pageTitle = $derived(viewTitle(currentView));

  function closePersonTimeline() {
    personTimeline?.clear();
    personTimeline = null;
    selectedPerson = null;
  }

  function closeTagTimeline() {
    tagTimeline?.clear();
    tagTimeline = null;
    selectedTag = null;
  }

  function closePlaceTimeline() {
    placeTimeline?.clear();
    placeTimeline = null;
    selectedPlace = null;
  }

  function closeMemoryTimeline() {
    memoryTimeline?.clear();
    memoryTimeline = null;
    selectedMemory = null;
  }

  function closeAlbumTimeline() {
    albumTimeline?.clear();
    albumTimeline = null;
    selectedAlbum = null;
  }

  // Favorites, Archive and Trash reload every time they open. The Photo Timeline keeps its cache, so a Restore
  // marks it stale and the next visit to Photos refreshes it once.
  let photosStale = false;

  function openPhotos() {
    if (currentView === "photos") return;
    currentView = "photos";
    if (!photosStale) return;
    photosStale = false;
    void timeline.refresh();
  }

  async function restoreTrashedAssets(assetIds: string[]) {
    const result = await desktop.restoreTrashedAssets(assetIds);
    if (result.restoredAssetIds.length > 0) photosStale = true;
    return result;
  }

  function openFavorites() {
    if (currentView === "favorites") return;
    currentView = "favorites";
    void favorites.refresh();
  }

  function openArchive() {
    if (currentView === "archive") return;
    currentView = "archive";
    void archive.refresh();
  }

  function openTrash() {
    if (currentView === "trash") return;
    currentView = "trash";
    void trash.refresh();
  }

  function openMap() {
    if (currentView === "map") return;
    mapCreated = true;
    currentView = "map";
    void map.refresh();
  }

  function openExplore() {
    currentView = "explore";
    void explore.refresh();
  }

  function openPeople() {
    currentView = "people";
    void people.refresh();
  }

  function openTags() {
    if (currentView === "tags") return;
    currentView = "tags";
    void tags.refresh();
    void tick().then(() => { if (content) content.scrollTop = tagsScrollTop; });
  }

  function openAlbums() {
    currentView = "albums";
    void albumCatalogue.refresh();
  }

  /** "All Places" has no navigation entry; it is only opened from the Explore Overview. */
  function openAllPlaces() {
    placesScrollTop = 0;
    exploreScrollTop = content?.scrollTop ?? 0;
    currentView = "places";
  }

  /** "All Memories" has no navigation entry; it is only opened from the Explore Overview. */
  function openAllMemories() {
    allMemoriesScrollTop = 0;
    exploreScrollTop = content?.scrollTop ?? 0;
    currentView = "all-memories";
    void allMemories.refresh();
  }

  function returnToExplore() {
    currentView = "explore";
    void explore.refreshIfStale();
    queueMicrotask(() => { if (content) content.scrollTop = exploreScrollTop; });
  }

  function returnFromPersonTimeline() {
    closePersonTimeline();
    currentView = personOrigin;
    if (personOrigin === "explore") void explore.refreshIfStale();
    const scrollTop = personOrigin === "explore" ? exploreScrollTop : peopleScrollTop;
    queueMicrotask(() => { if (content) content.scrollTop = scrollTop; });
  }

  function returnFromPlaceTimeline() {
    closePlaceTimeline();
    currentView = placeOrigin;
    if (placeOrigin === "explore") void explore.refreshIfStale();
    const scrollTop = placeOrigin === "places" ? placesScrollTop : exploreScrollTop;
    queueMicrotask(() => { if (content) content.scrollTop = scrollTop; });
  }

  function returnFromMemoryTimeline() {
    closeMemoryTimeline();
    if (memoryOrigin === "all-memories") {
      currentView = "all-memories";
      queueMicrotask(() => { if (content) content.scrollTop = allMemoriesScrollTop; });
    } else returnToExplore();
  }

  function returnToAlbums() {
    closeAlbumTimeline();
    currentView = "albums";
    queueMicrotask(() => { if (content) content.scrollTop = albumsScrollTop; });
  }

  function submitSearch() {
    if (!search.canSubmit) return;
    // Refining a Search from its results keeps the view the user first searched from.
    if (currentView !== "search") {
      searchOrigin = currentPosition();
      currentView = "search";
    }
    void search.submit();
  }

  /** Shows `origin.view` again at its scroll position, reloading what was dropped or went stale while it was left. */
  function returnTo(origin: { view: string; top: number }) {
    currentView = origin.view;
    if (timelineScrollViews.has(origin.view)) scrollRestore = { view: origin.view, top: origin.top };
    else queueMicrotask(() => { if (content) content.scrollTop = origin.top; });
    // The Trash Timeline is dropped when it is left, and Explore and Tags may have gone stale.
    if (origin.view === "trash") void trash.refresh();
    if (origin.view === "explore") void explore.refreshIfStale();
    if (origin.view === "tags") void tags.refreshIfStale();
  }

  function returnFromSearch() {
    returnTo(searchOrigin ?? { view: "photos", top: 0 });
  }

  function returnFromTagTimeline() {
    closeTagTimeline();
    returnTo(tagOrigin);
  }

  function clearSearch() {
    const isOpen = currentView === "search";
    search.clear();
    if (isOpen) returnFromSearch();
  }

  const auth = createSessionController({
    adapter: desktop,
    t: (key, params) => $t(key, params),
    locale: () => $locale ?? "en",
    resetView: () => {
      mapCreated = false;
      peopleScrollTop = 0;
      exploreScrollTop = 0;
      allMemoriesScrollTop = 0;
      personOrigin = "people";
      memoryOrigin = "explore";
      placeOrigin = "explore";
      placesScrollTop = 0;
      albumsScrollTop = 0;
      tagsScrollTop = 0;
      tagOrigin = defaultTagOrigin;
      closeTagTimeline();
      closeAlbumTimeline();
      closePersonTimeline();
      closePlaceTimeline();
      closeMemoryTimeline();
      searchOrigin = null;
      timelineScrollTop = 0;
      scrollRestore = null;
      currentView = "photos";
    },
    isExploreActive: () => currentView === "explore",
    isTagsActive: () => currentView === "tags",
  });
  const {
    feedback,
    targetAlbumCatalogue,
    removable,
    watchedFolders,
    sync,
    timeline,
    favorites,
    archive,
    trash,
    people,
    places,
    tags,
    memories,
    allMemories,
    explore,
    map,
    albumCatalogue,
    search,
  } = auth;
  const preferences = createPreferencesController({
    adapter: desktop,
    reportError: feedback.report,
    addLog: sync.addLog,
    locale: () => $locale ?? "en",
    setLocale: (value) => $locale = value,
  });
  const updater = createUpdateController(() => sync.isActive, desktop);
  const targetAlbumDialog = $derived(watchedFolders.targetAlbumDialog);

  // Leaving the Trash Timeline drops its assets and selection; returning shows a fresh load.
  let trashWasOpen = false;
  $effect(() => {
    const trashIsOpen = currentView === "trash";
    if (trashWasOpen && !trashIsOpen) untrack(() => trash.clear());
    trashWasOpen = trashIsOpen;
  });

  // A scroll position is restored once, when returning from a Search; later visits open at the top.
  $effect(() => {
    if (scrollRestore && currentView !== scrollRestore.view) untrack(() => scrollRestore = null);
  });

  let profileImageLoadFailed = $state(false);

  $effect(() => {
    if (auth.profileImage) {
      profileImageLoadFailed = false;
    }
  });

  onMount(() => {
    auth.startLifecycle();
    updater.startLifecycle();
    preferences.startLifecycle();

    void auth.hydrate()
      .catch((error) => console.error("Failed to hydrate authentication", error));
    void preferences.hydrate().catch((error) => console.error("Failed to hydrate preferences", error));
    updater.startup();

    return () => {
      auth.destroy();
      updater.destroy();
      preferences.stopLifecycle();
    };
  });
  const sharedTimelineProps = $derived({
    serverUrl: auth.serverUrl,
    onOpenExternalUrl: desktop.openExternalUrl,
    getAssetDetails: desktop.getAssetDetails,
    setAssetFavorite: desktop.setAssetFavorite,
    chooseDownloadDestination: desktop.chooseDownloadDestination,
    downloadAsset: desktop.downloadAsset,
    downloadArchive: desktop.downloadArchive,
    albumCatalogue: targetAlbumCatalogue,
    addAssetsToAlbum: desktop.addAssetsToAlbum,
    initialScrollTop: scrollRestore?.view === currentView ? scrollRestore.top : 0,
    onScrollPosition: (scrollTop: number) => { timelineScrollTop = scrollTop; },
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
            onclick={openPhotos}
            aria-label={$t('timeline_title')}
          >
            <Images class="w-4 h-4 shrink-0 {currentView === 'photos' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('timeline_title')}</span>
          </button>
          <button
            type="button"
            class="nav-item {currentView === 'explore' ? 'active' : ''}"
            onclick={openExplore}
            aria-label={$t('explore_title')}
          >
            <Compass class="w-4 h-4 shrink-0 {currentView === 'explore' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('explore_title')}</span>
          </button>
          <button
            type="button"
            class="nav-item {currentView === 'favorites' ? 'active' : ''}"
            onclick={openFavorites}
            aria-label={$t('favorites_title')}
          >
            <Heart class="w-4 h-4 shrink-0 {currentView === 'favorites' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('favorites_title')}</span>
          </button>
          <button
            type="button"
            class="nav-item {currentView === 'archive' ? 'active' : ''}"
            onclick={openArchive}
            aria-label={$t('archive_title')}
          >
            <Archive class="w-4 h-4 shrink-0 {currentView === 'archive' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('archive_title')}</span>
          </button>
          <button
            type="button"
            class="nav-item {currentView === 'trash' ? 'active' : ''}"
            onclick={openTrash}
            aria-label={$t('trash_title')}
          >
            <Trash2 class="w-4 h-4 shrink-0 {currentView === 'trash' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('trash_title')}</span>
          </button>
          <button type="button" class="nav-item {currentView === 'map' ? 'active' : ''}" onclick={openMap} aria-label={$t('map_title')}>
            <MapIcon class="w-4 h-4 shrink-0 {currentView === 'map' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('map_title')}</span>
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

          <button type="button" class="nav-item {currentView === 'tags' ? 'active' : ''}" onclick={openTags} aria-label={$t('tags_title')}>
            <TagIcon class="w-4 h-4 shrink-0 {currentView === 'tags' ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
            <span>{$t('tags_title')}</span>
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

      <div class="shrink-0 px-8 pb-2">
        <SearchBar {search} {people} onSubmit={submitSearch} onClear={clearSearch} onOpenArchive={openArchive} />
      </div>

      <main bind:this={content} onscroll={() => { if (currentView === "tags" && content) tagsScrollTop = content.scrollTop; }} class="flex-1 {currentView === 'map' || timelineScrollViews.has(currentView) ? 'overflow-hidden px-8 pt-2 pb-8' : 'overflow-y-auto px-8 pt-4 pb-8'} flex flex-col space-y-6">
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

        {#if mapCreated}<MapView mapState={map} active={currentView === "map"} serverUrl={auth.serverUrl} {desktop} albumCatalogue={targetAlbumCatalogue} />{/if}
        {#if currentView === "photos"}
          <TimelineView {timeline} {...sharedTimelineProps} />
        {:else if currentView === "favorites"}
          <TimelineView
            timeline={favorites}
            {...sharedTimelineProps}
            removeUnfavoritedAssets={true}
            showRefreshButton={false}
            ariaLabel={$t("favorites_title")}
            loadingMessage={$t("favorites_loading")}
            emptyMessage={$t("favorites_empty")}
            loadErrorMessage={$t("favorites_error")}
            offlineMessage={$t("favorites_offline")}
          />
        {:else if currentView === "archive"}
          <TimelineView
            timeline={archive}
            {...sharedTimelineProps}
            ariaLabel={$t("archive_title")}
            loadingMessage={$t("archive_loading")}
            emptyMessage={$t("archive_empty")}
            loadErrorMessage={archive.permissionDenied ? $t("archive_permission_error") : $t("archive_error")}
            offlineMessage={$t("archive_offline")}
            failureIsError={archive.permissionDenied}
          />
        {:else if currentView === "trash"}
          <TimelineView
            timeline={trash}
            {...sharedTimelineProps}
            allowFavoriteAndAlbum={false}
            {restoreTrashedAssets}
            ariaLabel={$t("trash_title")}
            loadingMessage={$t("trash_loading")}
            emptyMessage={$t("trash_empty")}
            loadErrorMessage={$t("trash_error")}
            offlineMessage={$t("trash_offline")}
            hintMessage={$t("trash_hint")}
          />
        {:else if currentView === "explore"}
          <ExploreView {explore} {people} {places} {memories} onViewAllPeople={openPeople} onSelectPerson={selectPerson} onViewAllPlaces={openAllPlaces} onSelectPlace={selectPlace} onViewAllMemories={openAllMemories} onSelectMemory={selectMemory} />
        {:else if currentView === "all-memories"}
          <BackBarView label={$t('explore_return')} onBack={returnToExplore}>
            <AllMemoriesView {allMemories} onSelect={selectMemory} />
          </BackBarView>
        {:else if currentView === "places"}
          <BackBarView label={$t('explore_return')} onBack={returnToExplore}>
            <PlacesView {places} onSelect={selectPlace} />
          </BackBarView>
        {:else if currentView === "people"}
          <PeopleView {people} onSelect={selectPerson} />
        {:else if currentView === "tags"}
          <TagsView {tags} onSelect={selectTag} />
        {:else if currentView === "albums"}
          <AlbumCatalogueView catalogue={albumCatalogue} onSelect={selectAlbum} />
        {:else if currentView === "album-timeline" && albumTimeline}
          <BackBarView label={$t('albums_return')} onBack={returnToAlbums}>
            <TimelineView timeline={albumTimeline} {...sharedTimelineProps} emptyMessage={$t('albums_timeline_empty')} loadErrorMessage={$t('albums_error')} />
          </BackBarView>
        {:else if currentView === "place-timeline" && placeTimeline}
          <BackBarView label={$t(placeOrigin === "places" ? 'places_return' : 'explore_return')} onBack={returnFromPlaceTimeline}>
            <TimelineView timeline={placeTimeline} {...sharedTimelineProps} emptyMessage={$t('places_timeline_empty')} loadErrorMessage={$t('places_timeline_error')} />
          </BackBarView>
        {:else if currentView === "person-timeline" && personTimeline}
          <BackBarView label={$t(personOrigin === "explore" ? 'explore_return' : 'people_return')} onBack={returnFromPersonTimeline}>
            <TimelineView timeline={personTimeline} {...sharedTimelineProps} emptyMessage={$t('people_timeline_empty')} />
          </BackBarView>
        {:else if currentView === "memory-timeline" && memoryTimeline}
          <BackBarView label={$t(memoryOrigin === "all-memories" ? 'memories_all_return' : 'explore_return')} onBack={returnFromMemoryTimeline}>
            <TimelineView timeline={memoryTimeline} {...sharedTimelineProps} emptyMessage={$t('memories_timeline_empty')} />
          </BackBarView>
        {:else if currentView === "tag-timeline" && tagTimeline}
          <BackBarView label={$t("tags_timeline_return", { values: { view: viewTitle(tagOrigin.view) } })} onBack={returnFromTagTimeline}>
            <TimelineView timeline={tagTimeline} {...sharedTimelineProps} emptyMessage={$t('tags_timeline_empty')} loadErrorMessage={$t('tags_timeline_error')} />
          </BackBarView>
        {:else if currentView === "search"}
          <SearchResultsView
            {search}
            backLabel={$t("search_return", { values: { view: viewTitle(searchOrigin?.view ?? "photos") } })}
            onBack={returnFromSearch}
            timelineProps={sharedTimelineProps}
          />
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
