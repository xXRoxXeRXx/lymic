import type { AlbumCatalogueItem } from "../contracts";
import type { AlbumCatalogueDesktopAdapter } from "../tauri/desktop-adapter";

export interface AlbumCatalogueViewState {
  readonly albums: AlbumCatalogueItem[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  readonly error: "error" | null;
  refresh(): Promise<void>;
  clear(): void;
}

export function createAlbumCatalogueController(adapter: AlbumCatalogueDesktopAdapter): AlbumCatalogueViewState {
  let albums = $state<AlbumCatalogueItem[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let error = $state<"error" | null>(null);
  let generation = 0;
  let pending: Promise<void> | null = null;

  function refresh() {
    if (pending) return pending;
    const requestGeneration = generation;
    isLoading = albums.length === 0;
    isRefreshing = true;
    const request = adapter.getAlbumCatalogue()
      .then((result) => {
        if (requestGeneration !== generation) return;
        albums = result;
        error = null;
        isOffline = false;
      })
      .catch(() => {
        if (requestGeneration !== generation) return;
        error = albums.length === 0 ? "error" : null;
        isOffline = albums.length > 0;
      })
      .finally(() => {
        if (pending === request) pending = null;
        if (requestGeneration === generation) {
          isLoading = false;
          isRefreshing = false;
        }
      });
    pending = request;
    return request;
  }

  function clear() {
    generation += 1;
    pending = null;
    albums = [];
    isLoading = false;
    isRefreshing = false;
    isOffline = false;
    error = null;
  }

  return {
    get albums() { return albums; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    get error() { return error; },
    refresh,
    clear,
  };
}
