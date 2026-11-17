import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure } from "./request-failure";
import type { SuggestionsState } from "./search-suggestions.svelte";

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

const IDLE: SuggestionsState = { status: "idle", items: [], failure: null };

export function createSearchCities(adapter: Pick<PlacesDesktopAdapter, "getPlaces">): SearchCities {
  let state = $state<SuggestionsState>(IDLE);
  let inFlight: Promise<void> | null = null;
  // Bumped by clear(), so an answer that arrives late is not kept for the next session.
  let generation = 0;

  function load(): Promise<void> {
    if (state.status === "ready") return Promise.resolve();
    if (inFlight) return inFlight;

    const thisGeneration = generation;
    state = { status: "loading", items: [], failure: null };
    const read = (async () => {
      try {
        const places = await adapter.getPlaces();
        if (thisGeneration === generation) {
          const names = [...new Set(places.map((place) => place.city))];
          state = { status: "ready", items: names.sort((a, b) => a.localeCompare(b)), failure: null };
        }
      } catch (error) {
        if (thisGeneration === generation) state = { status: "error", items: [], failure: classifyRequestFailure(error, "places") };
      } finally {
        if (thisGeneration === generation) inFlight = null;
      }
    })();
    inFlight = read;
    return read;
  }

  return {
    get state() { return state; },
    load,
    clear() {
      generation += 1;
      state = IDLE;
      inFlight = null;
    },
  };
}
