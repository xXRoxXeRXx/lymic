import type { MemoriesViewState } from "./memories-controller.svelte";
import type { PeopleViewState } from "./people-controller.svelte";
import type { PlacesViewState } from "./places-controller.svelte";
import type { ExploreError } from "./explore-error";

/**
 * The state every Explore Overview section exposes. Each section owns its own
 * loading, error, offline and empty state so one failing section never blocks
 * the others. `createExploreController` adapts the People, Places and
 * Memories controllers to this interface.
 */
export interface ExploreSectionState {
  /** True while the first load has no data to show yet. */
  readonly isLoading: boolean;
  /** True when a refresh failed but previously loaded data stays visible. */
  readonly isOffline: boolean;
  /** Failure of a load that has no cached data to fall back on. */
  readonly error: ExploreError | null;
  /** Number of items the section would show; 0 means empty. */
  readonly itemCount: number;
  /** Reloads this section only; also serves as the retry action. */
  refresh(): Promise<void>;
}

/** An empty section without an error is hidden; loading and failed sections stay visible. */
export function isExploreSectionVisible(section: ExploreSectionState): boolean {
  return section.isLoading || section.error !== null || section.itemCount > 0;
}

export interface ExploreViewState {
  readonly people: ExploreSectionState;
  readonly places: ExploreSectionState;
  readonly memories: ExploreSectionState;
  /** True when no section has anything to show, so the placeholder replaces them. */
  readonly isAllEmpty: boolean;
  /** Refreshes every section independently; one failure does not stop the others. */
  refresh(): Promise<void>;
  /**
   * Refreshes while the Explore Overview is the open view (e.g. after Synchronization settles).
   * Otherwise it only remembers that the sections are stale, so `refreshIfStale` can catch up.
   */
  refreshIfActive(): Promise<void>;
  /** Refreshes only when Synchronization settled while Explore was not open; call when returning to it. */
  refreshIfStale(): Promise<void>;
}

export function createExploreController(options: {
  people: PeopleViewState;
  places: PlacesViewState;
  memories: MemoriesViewState;
  isActive(): boolean;
}): ExploreViewState {
  const { people: peopleController } = options;
  const people: ExploreSectionState = {
    get isLoading() { return peopleController.isLoading; },
    get isOffline() { return peopleController.isOffline; },
    get error() { return peopleController.error; },
    get itemCount() { return peopleController.people.length; },
    refresh: () => peopleController.refresh(),
  };
  const { places: placesController } = options;
  const places: ExploreSectionState = {
    get isLoading() { return placesController.isLoading; },
    get isOffline() { return placesController.isOffline; },
    get error() { return placesController.error; },
    get itemCount() { return placesController.places.length; },
    refresh: () => placesController.refresh(),
  };
  const { memories: memoriesController } = options;
  const memories: ExploreSectionState = {
    get isLoading() { return memoriesController.isLoading; },
    get isOffline() { return memoriesController.isOffline; },
    get error() { return memoriesController.error; },
    get itemCount() { return memoriesController.memories.length; },
    refresh: () => memoriesController.refresh(),
  };
  const sections = [people, places, memories] as const;

  let stale = false;

  function refresh() {
    stale = false;
    return Promise.all(sections.map((section) => section.refresh())).then(() => undefined);
  }

  return {
    people,
    places,
    memories,
    get isAllEmpty() { return !sections.some(isExploreSectionVisible); },
    refresh,
    refreshIfActive() {
      if (options.isActive()) return refresh();
      stale = true;
      return Promise.resolve();
    },
    refreshIfStale: () => (stale ? refresh() : Promise.resolve()),
  };
}
