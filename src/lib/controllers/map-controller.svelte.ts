import type { MapMarker } from "../contracts";
import type { MapDesktopAdapter } from "../tauri/desktop-adapter";
import { createRemoteCollection } from "./remote-collection.svelte";
import { classifyRequestFailure, type RequestFailure } from "./request-failure";

export interface MapViewState {
  readonly markers: MapMarker[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  /** A load has succeeded, so an empty marker list means no assets have locations. */
  readonly hasLoaded: boolean;
  /** The last refresh failed and the previous markers stay shown. */
  readonly isOffline: boolean;
  /** Why the last refresh failed, also while previous markers stay shown. */
  readonly error: RequestFailure | null;
  refresh(): Promise<void>;
  /** Drops the marker of an asset that left the library (moved to the trash) until the next refresh. */
  removeMarker(assetId: string): void;
  /** Stops waiting for a marker load in flight, for when the view is left. */
  abandon(): void;
  clear(): void;
}

export function createMapController(adapter: MapDesktopAdapter): MapViewState {
  const collection = createRemoteCollection({ load: () => adapter.getMapMarkers() });

  return {
    get markers() { return collection.items; },
    get isLoading() { return collection.isLoading; },
    get isRefreshing() { return collection.isRefreshing; },
    get hasLoaded() { return collection.hasLoaded; },
    get isOffline() { return collection.isOffline; },
    get error() { return collection.failure ? classifyRequestFailure(collection.failure.reason, "map") : null; },
    refresh: collection.refresh,
    removeMarker: (assetId) => collection.remove((marker) => marker.id === assetId),
    abandon: collection.abandon,
    clear: collection.clear,
  };
}
