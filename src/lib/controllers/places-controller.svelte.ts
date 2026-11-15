import type { Place } from "../contracts";
import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyExploreError, type ExploreError } from "./explore-error";

export interface PlacesViewState {
  readonly places: Place[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: ExploreError | null;
  refresh(): Promise<void>;
  clear(): void;
}

export function createPlacesController(adapter: PlacesDesktopAdapter): PlacesViewState {
  let places = $state<Place[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let error = $state<ExploreError | null>(null);
  let generation = 0;

  async function refresh() {
    const requestGeneration = ++generation;
    isLoading = places.length === 0;
    try {
      const result = await adapter.getPlaces();
      if (requestGeneration !== generation) return;
      places = result;
      error = null;
      isOffline = false;
    } catch (reason) {
      if (requestGeneration !== generation) return;
      error = places.length === 0 ? classifyExploreError(reason, "places") : null;
      isOffline = places.length > 0;
    } finally {
      if (requestGeneration === generation) isLoading = false;
    }
  }

  function clear() {
    generation += 1;
    places = [];
    isLoading = false;
    isOffline = false;
    error = null;
  }

  return {
    get places() { return places; },
    get isLoading() { return isLoading; },
    get isOffline() { return isOffline; },
    get error() { return error; },
    refresh,
    clear,
  };
}
