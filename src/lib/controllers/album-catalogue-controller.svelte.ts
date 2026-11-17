import type { AlbumCatalogueItem } from "../contracts";
import type { AlbumCatalogueDesktopAdapter } from "../tauri/desktop-adapter";
import { createRemoteCollection } from "./remote-collection.svelte";

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
  const collection = createRemoteCollection({ load: () => adapter.getAlbumCatalogue() });

  return {
    get albums() { return collection.items; },
    get isLoading() { return collection.isLoading; },
    get isRefreshing() { return collection.isRefreshing; },
    get isOffline() { return collection.isOffline; },
    get error() { return collection.error ? "error" : null; },
    refresh: collection.refresh,
    clear: collection.clear,
  };
}
