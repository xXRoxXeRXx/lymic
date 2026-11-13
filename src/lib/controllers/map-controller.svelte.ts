import type { MapMarker } from "../contracts";
import type { MapDesktopAdapter } from "../tauri/desktop-adapter";

export function createMapController(adapter: MapDesktopAdapter) {
  let markers = $state.raw<MapMarker[]>([]);
  let loading = $state(false);
  let loaded = $state(false);
  let stale = $state(false);
  let error = $state<"permission" | "server" | null>(null);
  let generation = 0;
  async function refresh() {
    const current = ++generation;
    loading = true;
    try {
      const result = await adapter.getMapMarkers();
      if (current !== generation) return;
      markers = result;
      loaded = true;
      stale = false;
      error = null;
    } catch (reason) {
      if (current !== generation) return;
      stale = loaded;
      error = String(reason).startsWith("map_permission_denied") ? "permission" : "server";
    } finally {
      if (current === generation) loading = false;
    }
  }
  function leave() {
    generation++;
    loading = false;
  }
  function clear() {
    leave();
    markers = [];
    loaded = false;
    stale = false;
    error = null;
  }
  return {
    get markers() { return markers; },
    get loading() { return loading; },
    get loaded() { return loaded; },
    get stale() { return stale; },
    get error() { return error; },
    refresh, leave, clear,
  };
}
export type MapViewState = ReturnType<typeof createMapController>;
