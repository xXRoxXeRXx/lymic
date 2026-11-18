import type { AlbumCatalogueItem, FilteredTimeline, RecognizedPerson } from "../contracts";
import type { AlbumTimelineDesktopAdapter, PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import { createAlbumTimelineController } from "./album-timeline-controller.svelte";
import { createPersonTimelineController } from "./person-timeline-controller.svelte";

export const PHOTOS_VIEW = "photos";
export const ALBUMS_VIEW = "albums";
export const ALBUM_TIMELINE_VIEW = "album-timeline";
export const PEOPLE_VIEW = "people";
export const PERSON_TIMELINE_VIEW = "person-timeline";
export const EXPLORE_VIEW = "explore";
export const MAP_VIEW = "map";

export type PersonOrigin = typeof PEOPLE_VIEW | typeof EXPLORE_VIEW;

/** What opening a view loads; the page supplies these from the session's controllers. */
export interface NavigationRefresh {
  photos(): unknown;
  filteredTimeline(view: FilteredTimeline): unknown;
  map(): unknown;
  explore(): unknown;
  exploreIfStale(): unknown;
  people(): unknown;
  albums(): unknown;
}

export interface NavigationOptions {
  adapter: Pick<PeopleDesktopAdapter, "getPersonTimelineBuckets" | "getPersonTimelineBucketAssets"> & AlbumTimelineDesktopAdapter;
  refresh: NavigationRefresh;
  /** The scroll position of the content area, which holds the Album, People and Explore lists. */
  scroll: { read(): number; restore(top: number): void };
}

/**
 * Owns which view is open, the Album and Person being drilled into with their timelines, and the scroll positions
 * of the lists they return to. The page renders from it.
 */
export function createNavigationController(options: NavigationOptions) {
  const { adapter, refresh, scroll } = options;

  let view = $state(PHOTOS_VIEW);
  let mapCreated = $state(false);
  let selectedAlbum = $state<AlbumCatalogueItem | null>(null);
  let albumTimeline = $state<ReturnType<typeof createAlbumTimelineController> | null>(null);
  let selectedPerson = $state<RecognizedPerson | null>(null);
  let personTimeline = $state<ReturnType<typeof createPersonTimelineController> | null>(null);
  let personOrigin = $state<PersonOrigin>(PEOPLE_VIEW);
  let albumsScrollTop = 0;
  let peopleScrollTop = 0;
  let exploreScrollTop = 0;
  // Favorites, Archive and Trash reload every time they open. The Photo Timeline keeps its cache, so a change
  // that affects it (a Restore) marks it stale and the next visit to Photos refreshes it once.
  let photosStale = false;

  function show(next: string) {
    view = next;
  }

  function openPhotos() {
    if (view === PHOTOS_VIEW) return;
    view = PHOTOS_VIEW;
    if (!photosStale) return;
    photosStale = false;
    void refresh.photos();
  }

  function openFilteredTimeline(timeline: FilteredTimeline) {
    if (view === timeline) return;
    view = timeline;
    void refresh.filteredTimeline(timeline);
  }

  function openMap() {
    if (view === MAP_VIEW) return;
    mapCreated = true;
    view = MAP_VIEW;
    void refresh.map();
  }

  function openExplore() {
    view = EXPLORE_VIEW;
    void refresh.explore();
  }

  function openPeople() {
    if (view === PEOPLE_VIEW) return;
    view = PEOPLE_VIEW;
    void refresh.people();
  }

  function openAlbums() {
    if (view === ALBUMS_VIEW) return;
    view = ALBUMS_VIEW;
    void refresh.albums();
  }

  function selectAlbum(album: AlbumCatalogueItem) {
    albumsScrollTop = scroll.read();
    selectedAlbum = album;
    albumTimeline = createAlbumTimelineController(adapter, album.id);
    view = ALBUM_TIMELINE_VIEW;
    void albumTimeline.refresh();
  }

  function closeAlbumTimeline() {
    albumTimeline?.clear();
    albumTimeline = null;
    selectedAlbum = null;
  }

  function returnToAlbums() {
    closeAlbumTimeline();
    view = ALBUMS_VIEW;
    scroll.restore(albumsScrollTop);
  }

  function selectPerson(person: RecognizedPerson) {
    personOrigin = view === EXPLORE_VIEW ? EXPLORE_VIEW : PEOPLE_VIEW;
    if (personOrigin === EXPLORE_VIEW) exploreScrollTop = scroll.read();
    else peopleScrollTop = scroll.read();
    selectedPerson = person;
    personTimeline = createPersonTimelineController(adapter, person.id);
    view = PERSON_TIMELINE_VIEW;
    void personTimeline.refresh();
  }

  function closePersonTimeline() {
    personTimeline?.clear();
    personTimeline = null;
    selectedPerson = null;
  }

  function returnFromPersonTimeline() {
    closePersonTimeline();
    view = personOrigin;
    if (personOrigin === EXPLORE_VIEW) void refresh.exploreIfStale();
    scroll.restore(personOrigin === EXPLORE_VIEW ? exploreScrollTop : peopleScrollTop);
  }

  /** Back to Explore from a view opened out of it, at the position Explore was left. */
  function returnToExplore() {
    view = EXPLORE_VIEW;
    void refresh.exploreIfStale();
    scroll.restore(exploreScrollTop);
  }

  /** Signing out: everything returns to its initial state and the photo timeline is the open view. */
  function reset() {
    mapCreated = false;
    peopleScrollTop = 0;
    exploreScrollTop = 0;
    albumsScrollTop = 0;
    personOrigin = PEOPLE_VIEW;
    photosStale = false;
    closeAlbumTimeline();
    closePersonTimeline();
    view = PHOTOS_VIEW;
  }

  return {
    get view() { return view; },
    get mapCreated() { return mapCreated; },
    get selectedAlbum() { return selectedAlbum; },
    get albumTimeline() { return albumTimeline; },
    get selectedPerson() { return selectedPerson; },
    get personTimeline() { return personTimeline; },
    get personOrigin() { return personOrigin; },
    /** The Explore Overview's scroll position, remembered while a view opened from it is shown. */
    get exploreScrollTop() { return exploreScrollTop; },
    set exploreScrollTop(top: number) { exploreScrollTop = top; },
    show,
    openPhotos,
    markPhotosStale() { photosStale = true; },
    openFilteredTimeline,
    openMap,
    openExplore,
    openPeople,
    openAlbums,
    selectAlbum,
    returnToAlbums,
    selectPerson,
    returnFromPersonTimeline,
    returnToExplore,
    reset,
  };
}

export type NavigationController = ReturnType<typeof createNavigationController>;
