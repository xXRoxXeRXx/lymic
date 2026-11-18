import type { AlbumCatalogueItem, FilteredTimeline, RecognizedPerson } from "../contracts";
import type { AlbumTimelineDesktopAdapter, PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import { createAlbumTimelineController } from "./album-timeline-controller.svelte";
import { createPersonTimelineController } from "./person-timeline-controller.svelte";
import { TAGS_VIEW, TAG_TIMELINE_VIEW } from "./view-position";

export const PHOTOS_VIEW = "photos";
export const ALBUMS_VIEW = "albums";
export const ALBUM_TIMELINE_VIEW = "album-timeline";
export const PEOPLE_VIEW = "people";
export const PERSON_TIMELINE_VIEW = "person-timeline";
export const EXPLORE_VIEW = "explore";
export const MAP_VIEW = "map";
export const TRASH_VIEW = "trash";
// Views the page shows through `show`: the page owns their state and Back origins.
export const PLACES_VIEW = "places";
export const PLACE_TIMELINE_VIEW = "place-timeline";
export const ALL_MEMORIES_VIEW = "all-memories";
export const MEMORY_TIMELINE_VIEW = "memory-timeline";
export const SEARCH_VIEW = "search";
export const DASHBOARD_VIEW = "dashboard";
export const LOGS_VIEW = "logs";
export const SETTINGS_VIEW = "settings";
export const INFO_VIEW = "info";

export type ViewId =
  | FilteredTimeline
  | typeof PHOTOS_VIEW
  | typeof ALBUMS_VIEW
  | typeof ALBUM_TIMELINE_VIEW
  | typeof PEOPLE_VIEW
  | typeof PERSON_TIMELINE_VIEW
  | typeof EXPLORE_VIEW
  | typeof MAP_VIEW
  | typeof PLACES_VIEW
  | typeof PLACE_TIMELINE_VIEW
  | typeof ALL_MEMORIES_VIEW
  | typeof MEMORY_TIMELINE_VIEW
  | typeof TAGS_VIEW
  | typeof TAG_TIMELINE_VIEW
  | typeof SEARCH_VIEW
  | typeof DASHBOARD_VIEW
  | typeof LOGS_VIEW
  | typeof SETTINGS_VIEW
  | typeof INFO_VIEW;

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

  let view = $state<ViewId>(PHOTOS_VIEW);
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

  function show(next: ViewId) {
    if (view === EXPLORE_VIEW) exploreScrollTop = scroll.read();
    view = next;
  }

  function openAndRefresh(target: ViewId, load: () => unknown) {
    if (view === target) return;
    view = target;
    void load();
  }

  function openPhotos() {
    if (view === PHOTOS_VIEW) return;
    view = PHOTOS_VIEW;
    if (!photosStale) return;
    photosStale = false;
    void refresh.photos();
  }

  function openFilteredTimeline(timeline: FilteredTimeline) {
    openAndRefresh(timeline, () => refresh.filteredTimeline(timeline));
  }

  function openMap() {
    if (view === MAP_VIEW) return;
    mapCreated = true;
    view = MAP_VIEW;
    void refresh.map();
  }

  const openExplore = () => openAndRefresh(EXPLORE_VIEW, refresh.explore);
  const openPeople = () => openAndRefresh(PEOPLE_VIEW, refresh.people);
  const openAlbums = () => openAndRefresh(ALBUMS_VIEW, refresh.albums);

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
    if (personOrigin === PEOPLE_VIEW) peopleScrollTop = scroll.read();
    selectedPerson = person;
    personTimeline = createPersonTimelineController(adapter, person.id);
    show(PERSON_TIMELINE_VIEW);
    void personTimeline.refresh();
  }

  function closePersonTimeline() {
    personTimeline?.clear();
    personTimeline = null;
    selectedPerson = null;
  }

  /** Back to Explore from a view opened out of it, at the position Explore was left. */
  function returnToExplore() {
    view = EXPLORE_VIEW;
    void refresh.exploreIfStale();
    scroll.restore(exploreScrollTop);
  }

  function returnFromPersonTimeline() {
    closePersonTimeline();
    if (personOrigin === EXPLORE_VIEW) {
      returnToExplore();
      return;
    }
    view = PEOPLE_VIEW;
    scroll.restore(peopleScrollTop);
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
