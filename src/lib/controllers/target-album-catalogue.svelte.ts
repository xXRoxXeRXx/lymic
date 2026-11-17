import type { Album } from "../contracts";
import { formatError } from "./feedback-controller.svelte";
import type { AlbumsDesktopAdapter } from "../tauri/desktop-adapter";
import { createRemoteCollection } from "./remote-collection.svelte";

export interface TargetAlbumCatalogue {
  readonly albums: Album[];
  readonly isLoading: boolean;
  readonly error: string | null;
  readonly hasRequested: boolean;
  refresh(): Promise<void>;
  clear(): void;
  create(name: string): Promise<Album>;
  nameFor(id: string): string | undefined;
}

function sorted(albums: Album[]) {
  return [...albums].sort((a, b) => a.albumName.toLowerCase().localeCompare(b.albumName.toLowerCase()));
}

function notIn(albums: Album[], loaded: Album[]) {
  return albums.filter((album) => !loaded.some((known) => known.id === album.id));
}

export function createTargetAlbumCatalogue(adapter: AlbumsDesktopAdapter): TargetAlbumCatalogue {
  let hasRequested = $state(false);
  // Created Target Albums stay listed until a load from Immich contains them.
  let created = $state<Album[]>([]);
  let epoch = 0;

  const collection = createRemoteCollection({
    async load() {
      const loaded = await adapter.getAlbums();
      created = notIn(created, loaded);
      return loaded;
    },
  });

  const albums = $derived(sorted([...collection.items, ...notIn(created, collection.items)]));

  function refresh() {
    hasRequested = true;
    return collection.refresh();
  }

  async function create(name: string) {
    const request = epoch;
    const album = await adapter.createAlbum(name);
    if (request !== epoch) return album;
    created = [...created.filter((known) => known.id !== album.id), album];
    await refresh();
    return album;
  }

  function clear() {
    epoch += 1;
    created = [];
    hasRequested = false;
    collection.clear();
  }

  return {
    get albums() { return albums; },
    // Views treat any in-flight request, first load or refresh, as loading.
    get isLoading() { return collection.isRefreshing; },
    get error() { return collection.error ? formatError(collection.error.reason, "Failed to load albums") : null; },
    get hasRequested() { return hasRequested; },
    refresh,
    clear,
    create,
    nameFor(id) { return albums.find((album) => album.id === id)?.albumName; },
  };
}
