import type { Place } from "../contracts";
import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";
import type { ExploreError } from "./explore-error";
import { createExploreListController } from "./explore-list-controller.svelte";

export interface PlacesViewState {
  readonly places: Place[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: ExploreError | null;
  refresh(): Promise<void>;
  clear(): void;
}

export function createPlacesController(adapter: PlacesDesktopAdapter): PlacesViewState {
  const list = createExploreListController({ prefix: "places", load: () => adapter.getPlaces() });
  return {
    get places() { return list.items; },
    get isLoading() { return list.isLoading; },
    get isOffline() { return list.isOffline; },
    get error() { return list.error; },
    refresh: list.refresh,
    clear: list.clear,
  };
}
