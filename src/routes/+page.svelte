<script lang="ts">
  import { onMount, untrack, type ComponentProps } from "svelte";
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
  import {
    ALBUMS_VIEW,
    ALBUM_TIMELINE_VIEW,
    ALL_MEMORIES_VIEW,
    DASHBOARD_VIEW,
    EXPLORE_VIEW,
    INFO_VIEW,
    LOGS_VIEW,
    MAP_VIEW,
    MEMORY_TIMELINE_VIEW,
    PEOPLE_VIEW,
    PERSON_TIMELINE_VIEW,
    PHOTOS_VIEW,
    PLACES_VIEW,
    PLACE_TIMELINE_VIEW,
    SEARCH_VIEW,
    SETTINGS_VIEW,
    TRASH_VIEW,
    createNavigationController,
    type ViewId,
  } from "#lib/controllers/navigation-controller.svelte.ts";
  import { createPlaceTimelineController } from "#lib/controllers/place-timeline-controller.svelte.ts";
  import { createMemoryTimelineController } from "#lib/controllers/memory-timeline-controller.svelte.ts";
  import { createTagTimelineController } from "#lib/controllers/tag-timeline-controller.svelte.ts";
  import { defaultTagOrigin, recordTagOrigin } from "#lib/controllers/tag-origin.ts";
  import { TAG_TIMELINE_VIEW, TAGS_VIEW, type ViewPosition } from "#lib/controllers/view-position.ts";
  import { localizedMemoryTitle } from "#lib/memory-title.ts";
  import { FILTERED_TIMELINES, type AssetTag, type FilteredTimeline, type Memory, type Place } from "#lib/contracts.ts";
  import { tauriDesktopAdapter } from "#lib/tauri/desktop-adapter.ts";

  /** Whether `view` is a Filtered Timeline: a main view that shows its own filtered timeline and reloads it every time it opens. */
  function isFilteredTimeline(view: ViewId): view is FilteredTimeline {
    return (FILTERED_TIMELINES as readonly string[]).includes(view);
  }
  const desktop = tauriDesktopAdapter;
  /** Scrolls the content area to `top` once the view that was just shown has rendered. */
  const restoreScroll = (top: number) => queueMicrotask(() => { if (content) content.scrollTop = top; });
  const navigation = createNavigationController({
    adapter: desktop,
    refresh: {
      photos: () => timeline.refresh(),
      filteredTimeline: (view) => filteredTimelines[view].refresh(),
      map: () => map.refresh(),
      explore: () => explore.refresh(),
      exploreIfStale: () => explore.refreshIfStale(),
      people: () => people.refresh(),
      albums: () => albumCatalogue.refresh(),
    },
    scroll: {
      read: () => content?.scrollTop ?? 0,
      restore: restoreScroll,
    },
  });
  const currentView = $derived(navigation.view);
  let selectedMemory = $state<Memory | null>(null);
  let memoryTimeline = $state<ReturnType<typeof createMemoryTimelineController> | null>(null);
  let memoryOrigin = $state<typeof EXPLORE_VIEW | typeof ALL_MEMORIES_VIEW>(EXPLORE_VIEW);
  let selectedPlace = $state<Place | null>(null);
  let placeTimeline = $state<ReturnType<typeof createPlaceTimelineController> | null>(null);
  let placeOrigin = $state<typeof EXPLORE_VIEW | typeof PLACES_VIEW>(EXPLORE_VIEW);
  let placesScrollTop = 0;
  let allMemoriesScrollTop = 0;
  /** The Tags view's own scroll position, kept while another view is open. */
  let tagsScrollTop = 0;
  let selectedTag = $state<AssetTag | null>(null);
  let tagTimeline = $state<ReturnType<typeof createTagTimelineController> | null>(null);
  /** The one view (and scroll position) Back returns to from a Tag Timeline, whichever view it was opened from. */
  let tagOrigin = $state<ViewPosition>(defaultTagOrigin);
  let content = $state<HTMLElement>();
  /** The view a Search was started from and where it was scrolled to, so Back returns there (even to a sub-view). */
  let searchOrigin = $state<ViewPosition | null>(null);
  /** The scroll position the open shared timeline view last reported; its scrolling area is inside the view, not `main`. */
  let timelineScrollTop = 0;
  /** Where the timeline view of `view` opens, set only while returning from a Search. */
  let scrollRestore = $state<ViewPosition | null>(null);
  /** Views that show the shared timeline view, which scrolls inside itself. */
  const timelineScrollViews = new Set<string>([PHOTOS_VIEW, ...FILTERED_TIMELINES, PERSON_TIMELINE_VIEW, ALBUM_TIMELINE_VIEW, PLACE_TIMELINE_VIEW, MEMORY_TIMELINE_VIEW, TAG_TIMELINE_VIEW, SEARCH_VIEW]);

  /** The open view and its scroll position; a timeline view scrolls inside itself, any other view in `main`. */
  function currentPosition(): ViewPosition {
    return { view: currentView, top: timelineScrollViews.has(currentView) ? timelineScrollTop : (content?.scrollTop ?? 0) };
  }

  /** Opens a Tag's Tag Timeline from any view; a Tag Timeline opened from another Tag Timeline keeps its origin. */
  function selectTag(tag: AssetTag) {
    tagOrigin = recordTagOrigin(tagOrigin, currentPosition());
    tagTimeline?.clear();
    selectedTag = tag;
    tagTimeline = createTagTimelineController(desktop, tag.id);
    navigation.show(TAG_TIMELINE_VIEW);
    void tagTimeline.refresh();
  }

  function selectPlace(place: Place) {
    placeOrigin = currentView === PLACES_VIEW ? PLACES_VIEW : EXPLORE_VIEW;
    if (placeOrigin === PLACES_VIEW) placesScrollTop = content?.scrollTop ?? 0;
    selectedPlace = place;
    placeTimeline = createPlaceTimelineController(desktop, place.city);
    navigation.show(PLACE_TIMELINE_VIEW);
    void placeTimeline.refresh();
  }

  function selectMemory(memory: Memory) {
    memoryOrigin = currentView === ALL_MEMORIES_VIEW ? ALL_MEMORIES_VIEW : EXPLORE_VIEW;
    if (memoryOrigin === ALL_MEMORIES_VIEW) allMemoriesScrollTop = content?.scrollTop ?? 0;
    selectedMemory = memory;
    memoryTimeline = createMemoryTimelineController(memory);
    navigation.show(MEMORY_TIMELINE_VIEW);
    void memoryTimeline.refresh();
  }

  function viewTitle(view: ViewId) {
    // A main view's title is its navigation label.
    const mainView = navigationEntries.find((entry) => entry.view === view);
    if (mainView) return $t(mainView.label);
    switch (view) {
      case SEARCH_VIEW: return $t("search_title");
      case PLACES_VIEW: return $t("places_all_title");
      case ALL_MEMORIES_VIEW: return $t("memories_all_title");
      case ALBUM_TIMELINE_VIEW: return navigation.selectedAlbum?.albumName ?? $t("albums_title");
      case PLACE_TIMELINE_VIEW: return selectedPlace?.city ?? $t("places_title");
      case TAG_TIMELINE_VIEW: return selectedTag?.value ?? $t("tags_title");
      case PERSON_TIMELINE_VIEW: return navigation.selectedPerson?.name ?? $t("people_unnamed");
      case MEMORY_TIMELINE_VIEW: return selectedMemory ? localizedMemoryTitle(selectedMemory, $t, $locale) : $t("memories_title");
      default: return $t("info");
    }
  }
  const pageTitle = $derived(viewTitle(currentView));

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

  async function restoreTrashedAssets(assetIds: string[]) {
    const result = await desktop.restoreTrashedAssets(assetIds);
    if (result.restoredAssetIds.length > 0) navigation.markPhotosStale();
    return result;
  }

  /**
   * Move to Trash for every view that offers it (selection actions, the Lightbox and the Map View all receive this).
   * The one place that runs after a move: cached views that may still show a moved asset reload when they next open.
   * The Photo Timeline already dropped it when the move ran inside it, and Explore and Tags only mark themselves
   * stale while they are not the open view.
   */
  async function trashAssets(assetIds: string[]) {
    const origin = currentView;
    const result = await desktop.trashAssets(assetIds);
    if (result.movedAssetIds.length > 0) {
      if (origin !== PHOTOS_VIEW) navigation.markPhotosStale();
      void explore.refreshIfActive();
      void tags.refreshIfActive();
    }
    return result;
  }

  function openTags() {
    if (currentView === TAGS_VIEW) return;
    navigation.show(TAGS_VIEW);
    void tags.refresh();
    restoreScroll(tagsScrollTop);
  }

  /** "All Places" has no navigation entry; it is only opened from the Explore Overview. */
  function openAllPlaces() {
    placesScrollTop = 0;
    navigation.show(PLACES_VIEW);
  }

  /** "All Memories" has no navigation entry; it is only opened from the Explore Overview. */
  function openAllMemories() {
    allMemoriesScrollTop = 0;
    navigation.show(ALL_MEMORIES_VIEW);
    void allMemories.refresh();
  }

  function returnToListOrExplore(origin: typeof EXPLORE_VIEW | typeof PLACES_VIEW | typeof ALL_MEMORIES_VIEW, listScrollTop: number) {
    if (origin === EXPLORE_VIEW) {
      navigation.returnToExplore();
      return;
    }
    navigation.show(origin);
    restoreScroll(listScrollTop);
  }

  function returnFromPlaceTimeline() {
    closePlaceTimeline();
    returnToListOrExplore(placeOrigin, placesScrollTop);
  }

  function returnFromMemoryTimeline() {
    closeMemoryTimeline();
    returnToListOrExplore(memoryOrigin, allMemoriesScrollTop);
  }

  function submitSearch() {
    if (!search.canSubmit) return;
    // Refining a Search from its results keeps the view the user first searched from.
    if (currentView !== SEARCH_VIEW) {
      searchOrigin = currentPosition();
      navigation.show(SEARCH_VIEW);
    }
    void search.submit();
  }

  /** Shows `origin.view` again at its scroll position, reloading what was dropped or went stale while it was left. */
  function returnTo(origin: ViewPosition) {
    navigation.show(origin.view);
    if (timelineScrollViews.has(origin.view)) scrollRestore = { view: origin.view, top: origin.top };
    else restoreScroll(origin.top);
    // The Trash Timeline is dropped when it is left, and Photos, Explore and Tags may have gone stale.
    if (origin.view === TRASH_VIEW) void filteredTimelines.trash.refresh();
    if (origin.view === PHOTOS_VIEW) navigation.refreshPhotosIfStale();
    if (origin.view === EXPLORE_VIEW) void explore.refreshIfStale();
    if (origin.view === TAGS_VIEW) void tags.refreshIfStale();
  }

  function returnFromSearch() {
    returnTo(searchOrigin ?? { view: PHOTOS_VIEW, top: 0 });
  }

  function returnFromTagTimeline() {
    closeTagTimeline();
    returnTo(tagOrigin);
  }

  function clearSearch() {
    const isOpen = currentView === SEARCH_VIEW;
    search.clear();
    if (isOpen) returnFromSearch();
  }

  const session = createSessionController({
    adapter: desktop,
    t: (key, params) => $t(key, params),
    locale: () => $locale ?? "en",
    resetView: () => {
      navigation.reset();
      allMemoriesScrollTop = 0;
      memoryOrigin = EXPLORE_VIEW;
      placeOrigin = EXPLORE_VIEW;
      placesScrollTop = 0;
      tagsScrollTop = 0;
      tagOrigin = defaultTagOrigin;
      closeTagTimeline();
      closePlaceTimeline();
      closeMemoryTimeline();
      searchOrigin = null;
      timelineScrollTop = 0;
      scrollRestore = null;
    },
    isExploreActive: () => currentView === EXPLORE_VIEW,
    isTagsActive: () => currentView === TAGS_VIEW,
  });
  const {
    auth,
    feedback,
    targetAlbumCatalogue,
    removable,
    watchedFolders,
    sync,
    timeline,
    filteredTimelines,
    people,
    places,
    tags,
    memories,
    allMemories,
    explore,
    map,
    albumCatalogue,
    search,
  } = session;
  /**
   * What each Filtered Timeline adds to the shared timeline: its navigation icon and its own TimelineView options.
   * Its texts are the `<timeline>_title`, `_loading`, `_empty`, `_error` and `_offline` locale keys, plus
   * `_permission_error` when its controller reports a missing permission (only Archive does).
   */
  const filteredTimelineViews: Record<FilteredTimeline, { icon: typeof Heart; options(): Partial<ComponentProps<typeof TimelineView>> }> = {
    favorites: { icon: Heart, options: () => ({ removeUnfavoritedAssets: true, showRefreshButton: false }) },
    archive: { icon: Archive, options: () => ({}) },
    trash: { icon: Trash2, options: () => ({ allowFavoriteAndAlbum: false, restoreTrashedAssets, trashAssets: undefined, hintMessage: $t("trash_hint") }) },
  };
  function filteredTimelineEntry(view: FilteredTimeline) {
    return { view, icon: filteredTimelineViews[view].icon, label: `${view}_title`, open: () => navigation.openFilteredTimeline(view) };
  }
  /** The main views, grouped in navigation order. A main view's navigation label is also its page title. */
  const navigationGroups = [
    [
      { view: PHOTOS_VIEW, icon: Images, label: "timeline_title", open: navigation.openPhotos },
      { view: EXPLORE_VIEW, icon: Compass, label: "explore_title", open: navigation.openExplore },
      { view: MAP_VIEW, icon: MapIcon, label: "map_title", open: navigation.openMap },
      { view: PEOPLE_VIEW, icon: Users, label: "people_title", open: navigation.openPeople },
      filteredTimelineEntry("favorites"),
      { view: ALBUMS_VIEW, icon: Album, label: "albums_title", open: navigation.openAlbums },
      { view: TAGS_VIEW, icon: TagIcon, label: "tags_title", open: openTags },
      filteredTimelineEntry("archive"),
      filteredTimelineEntry("trash"),
    ],
    [
      { view: DASHBOARD_VIEW, icon: FolderSync, label: "dashboard", open: () => navigation.show(DASHBOARD_VIEW) },
      { view: LOGS_VIEW, icon: ScrollText, label: "logs", open: () => navigation.show(LOGS_VIEW) },
    ],
    [
      { view: SETTINGS_VIEW, icon: SettingsIcon, label: "settings", open: () => navigation.show(SETTINGS_VIEW) },
      { view: INFO_VIEW, icon: InfoIcon, label: "info", open: () => navigation.show(INFO_VIEW) },
    ],
  ];
  const navigationEntries = navigationGroups.flat();
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
    const trashIsOpen = currentView === TRASH_VIEW;
    if (trashWasOpen && !trashIsOpen) untrack(() => filteredTimelines.trash.clear());
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
    session.startLifecycle();
    updater.startLifecycle();
    preferences.startLifecycle();

    void auth.hydrate()
      .catch((error) => console.error("Failed to hydrate authentication", error));
    void preferences.hydrate().catch((error) => console.error("Failed to hydrate preferences", error));
    updater.startup();

    return () => {
      session.destroy();
      updater.destroy();
      preferences.stopLifecycle();
    };
  });
  const sharedTimelineProps = $derived({
    serverUrl: auth.serverUrl,
    desktop,
    albumCatalogue: targetAlbumCatalogue,
    initialScrollTop: scrollRestore?.view === currentView ? scrollRestore.top : 0,
    onScrollPosition: (scrollTop: number) => { timelineScrollTop = scrollTop; },
    onSelectTag: selectTag,
    // Move to Trash is offered by every hosting view through this one prop; only the Trash Timeline opts out.
    trashAssets,
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
      <div class="space-y-6 min-h-0 overflow-y-auto">
        <!-- Brand / Header (padded for desktop window controls) -->
        <div class="flex items-center gap-3 px-3 py-1">
          <img src={logo} alt={$t('lymic_logo')} class="w-8 h-8 object-contain shrink-0" />
          <div class="leading-tight">
            <span class="font-bold text-base text-slate-900 dark:text-slate-100">Lymic</span>
            <p class="text-[10px] text-slate-400 dark:text-zinc-500 uppercase tracking-wider font-semibold">{$t('desktop_client')}</p>
          </div>
        </div>

        <!-- Navigation items -->
        <nav class="space-y-4" aria-label={$t('main_navigation')}>
          {#each navigationGroups as group, index (index)}
            <div class="space-y-1">
              {#each group as entry (entry.view)}
                {@const active = currentView === entry.view}
                <button type="button" class="nav-item {active ? 'active' : ''}" onclick={entry.open} aria-label={$t(entry.label)}>
                  <entry.icon class="w-4 h-4 shrink-0 {active ? 'text-blue-600 dark:text-sky-400' : 'text-slate-400 dark:text-zinc-500'}" />
                  <span>{$t(entry.label)}</span>
                </button>
              {/each}
            </div>
          {/each}
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
        <SearchBar {search} {people} {tags} onSubmit={submitSearch} onClear={clearSearch} onOpenArchive={() => navigation.openFilteredTimeline("archive")} />
      </div>

      <main bind:this={content} onscroll={() => { if (currentView === TAGS_VIEW && content) tagsScrollTop = content.scrollTop; }} class="flex-1 {currentView === MAP_VIEW || timelineScrollViews.has(currentView) ? 'overflow-hidden px-8 pt-2 pb-8' : 'overflow-y-auto px-8 pt-4 pb-8'} flex flex-col space-y-6">
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
              navigation.show(DASHBOARD_VIEW);
              await watchedFolders.acceptRemovableFolder(vol);
            }}
            onDismiss={(volId) => removable.dismiss(volId)}
          />
        {/if}

        {#if navigation.mapCreated}<MapView mapState={map} active={currentView === MAP_VIEW} serverUrl={auth.serverUrl} {desktop} albumCatalogue={targetAlbumCatalogue} onSelectTag={selectTag} {trashAssets} />{/if}
        {#if currentView === PHOTOS_VIEW}
          <TimelineView {timeline} {...sharedTimelineProps} />
        {:else if isFilteredTimeline(currentView)}
          {@const view = currentView}
          {@const filtered = filteredTimelines[view]}
          <!-- Keyed so switching between Filtered Timelines mounts a fresh TimelineView, as separate views would. -->
          {#key view}
            <TimelineView
              timeline={filtered}
              {...sharedTimelineProps}
              {...filteredTimelineViews[view].options()}
              ariaLabel={$t(`${view}_title`)}
              loadingMessage={$t(`${view}_loading`)}
              emptyMessage={$t(`${view}_empty`)}
              loadErrorMessage={filtered.permissionDenied ? $t(`${view}_permission_error`) : $t(`${view}_error`)}
              offlineMessage={$t(`${view}_offline`)}
              failureIsError={filtered.permissionDenied}
            />
          {/key}
        {:else if currentView === EXPLORE_VIEW}
          <ExploreView {explore} {people} {places} {memories} onViewAllPeople={navigation.openPeople} onSelectPerson={navigation.selectPerson} onViewAllPlaces={openAllPlaces} onSelectPlace={selectPlace} onViewAllMemories={openAllMemories} onSelectMemory={selectMemory} />
        {:else if currentView === ALL_MEMORIES_VIEW}
          <BackBarView label={$t('explore_return')} onBack={navigation.returnToExplore}>
            <AllMemoriesView {allMemories} onSelect={selectMemory} />
          </BackBarView>
        {:else if currentView === PLACES_VIEW}
          <BackBarView label={$t('explore_return')} onBack={navigation.returnToExplore}>
            <PlacesView {places} onSelect={selectPlace} />
          </BackBarView>
        {:else if currentView === PEOPLE_VIEW}
          <PeopleView {people} onSelect={navigation.selectPerson} />
        {:else if currentView === TAGS_VIEW}
          <TagsView {tags} onSelect={selectTag} />
        {:else if currentView === ALBUMS_VIEW}
          <AlbumCatalogueView catalogue={albumCatalogue} onSelect={navigation.selectAlbum} />
        {:else if currentView === ALBUM_TIMELINE_VIEW && navigation.albumTimeline}
          <BackBarView label={$t('albums_return')} onBack={navigation.returnToAlbums}>
            <TimelineView timeline={navigation.albumTimeline} {...sharedTimelineProps} emptyMessage={$t('albums_timeline_empty')} loadErrorMessage={$t('albums_error')} />
          </BackBarView>
        {:else if currentView === PLACE_TIMELINE_VIEW && placeTimeline}
          <BackBarView label={$t(placeOrigin === PLACES_VIEW ? 'places_return' : 'explore_return')} onBack={returnFromPlaceTimeline}>
            <TimelineView timeline={placeTimeline} {...sharedTimelineProps} emptyMessage={$t('places_timeline_empty')} loadErrorMessage={$t('places_timeline_error')} />
          </BackBarView>
        {:else if currentView === PERSON_TIMELINE_VIEW && navigation.personTimeline}
          <BackBarView label={$t(navigation.personOrigin === EXPLORE_VIEW ? 'explore_return' : 'people_return')} onBack={navigation.returnFromPersonTimeline}>
            <TimelineView timeline={navigation.personTimeline} {...sharedTimelineProps} emptyMessage={$t('people_timeline_empty')} />
          </BackBarView>
        {:else if currentView === MEMORY_TIMELINE_VIEW && memoryTimeline}
          <BackBarView label={$t(memoryOrigin === ALL_MEMORIES_VIEW ? 'memories_all_return' : 'explore_return')} onBack={returnFromMemoryTimeline}>
            <TimelineView timeline={memoryTimeline} {...sharedTimelineProps} emptyMessage={$t('memories_timeline_empty')} />
          </BackBarView>
        {:else if currentView === TAG_TIMELINE_VIEW && tagTimeline}
          <BackBarView label={$t("tags_timeline_return", { values: { view: viewTitle(tagOrigin.view) } })} onBack={returnFromTagTimeline}>
            <!-- A Tag chip in the Lightbox opens another Tag Timeline in this same place: start it afresh, at its top. -->
            {#key tagTimeline}
              <TimelineView timeline={tagTimeline} {...sharedTimelineProps} emptyMessage={$t('tags_timeline_empty')} loadErrorMessage={$t('tags_timeline_error')} />
            {/key}
          </BackBarView>
        {:else if currentView === SEARCH_VIEW}
          <SearchResultsView
            {search}
            backLabel={$t("search_return", { values: { view: viewTitle(searchOrigin?.view ?? PHOTOS_VIEW) } })}
            onBack={returnFromSearch}
            timelineProps={sharedTimelineProps}
          />
        {:else if currentView === DASHBOARD_VIEW}
          <DashboardView
            status={sync.status}
            folders={watchedFolders.folders}
            catalogue={targetAlbumCatalogue}
          />
        {:else if currentView === LOGS_VIEW}
          <ActivityLog logs={sync.logs} />
        {:else if currentView === SETTINGS_VIEW}
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
        {:else if currentView === INFO_VIEW}
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
