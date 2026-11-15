import type { Place } from "../contracts";
import type { PlacesDesktopAdapter } from "../tauri/desktop-adapter";

export type PlacesError = "permission_denied" | "unavailable" | "error";

export interface PlacesViewState {
  readonly places: Place[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: PlacesError | null;
  refresh(): Promise<void>;
  clear(): void;
}

function classifyError(reason: unknown): PlacesError {
  const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  if (message.startsWith("places_permission_denied:")) return "permission_denied";
  if (message.startsWith("places_authentication_required") || message.startsWith("places_unavailable")) return "unavailable";
  return "error";
}

export function createPlacesController(adapter: PlacesDesktopAdapter): PlacesViewState {
  let places = $state<Place[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let error = $state<PlacesError | null>(null);
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
      error = places.length === 0 ? classifyError(reason) : null;
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
