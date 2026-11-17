import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";
import { createSuggestionsCache, type SuggestionsState } from "./suggestions-cache.svelte";

/**
 * The cities a Search can be limited to. Immich's suggestions have no city type, so they come from the
 * city listing the Explore Places view already uses (`getPlaces`); the Place's own state is not shared, so a
 * failure here stays in the city filter. The listing is read lazily, once, the first time the city control
 * is used, and held in memory for the session. A failure is kept and retried by the next `load`. Names are
 * kept exactly as Immich returned them, because its city filter compares them exactly.
 */
export interface SearchCities {
  /** The current state of the listing; reactive. Shaped like suggestions so the same combobox shows both. */
  readonly state: SuggestionsState;
  /** Reads the listing unless it is already known or in flight. Never rejects. */
  load(): Promise<void>;
  clear(): void;
}

export function createSearchCities(adapter: Pick<PlacesDesktopAdapter, "getPlaces">): SearchCities {
  // The listing is one request, so the shared cache holds a single entry.
  const cache = createSuggestionsCache<void>({
    keyOf: () => "cities",
    read: async () => {
      const places = await adapter.getPlaces();
      return [...new Set(places.map((place) => place.city))].sort((a, b) => a.localeCompare(b));
    },
    prefix: "places",
  });

  return {
    get state() { return cache.stateFor(); },
    load: () => cache.load(),
    clear: () => cache.clear(),
  };
}
