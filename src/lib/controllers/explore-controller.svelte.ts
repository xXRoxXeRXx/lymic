import type { MemoriesViewState } from "./memories-controller.svelte";
import type { PeopleViewState } from "./people-controller.svelte";

/**
 * The state every Explore Overview section exposes. Each section owns its own
 * loading, error, offline and empty state so one failing section never blocks
 * the others. Later sections (Places, Memories) add an entry to
 * `createExploreController` that satisfies this interface.
 */
export interface ExploreSectionState {
  /** True while the first load has no data to show yet. */
  readonly isLoading: boolean;
  /** True when a refresh failed but previously loaded data stays visible. */
  readonly isOffline: boolean;
  /** Failure of a load that has no cached data to fall back on. */
  readonly error: "unavailable" | "error" | null;
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
  readonly memories: ExploreSectionState;
  readonly sections: readonly ExploreSectionState[];
  /** True when no section has anything to show, so the placeholder replaces them. */
  readonly isAllEmpty: boolean;
  /** Refreshes every section independently; one failure does not stop the others. */
  refresh(): Promise<void>;
  /** Refreshes only while the Explore Overview is the open view (e.g. after Synchronization settles). */
  refreshIfActive(): Promise<void>;
}

export function createExploreController(options: {
  people: PeopleViewState;
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
  const { memories: memoriesController } = options;
  const memories: ExploreSectionState = {
    get isLoading() { return memoriesController.isLoading; },
    get isOffline() { return memoriesController.isOffline; },
    get error() { return memoriesController.error; },
    get itemCount() { return memoriesController.memories.length; },
    refresh: () => memoriesController.refresh(),
  };
  const sections = [people, memories] as const;

  function refresh() {
    return Promise.all(sections.map((section) => section.refresh())).then(() => undefined);
  }

  return {
    people,
    memories,
    sections,
    get isAllEmpty() { return !sections.some(isExploreSectionVisible); },
    refresh,
    refreshIfActive: () => (options.isActive() ? refresh() : Promise.resolve()),
  };
}
