import type { Album } from "../contracts";
import type { AlbumsDesktopAdapter } from "../tauri/desktop-adapter";

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

function errorMessage(error: unknown) {
  return typeof error === "string" ? error : (error as Error)?.message || "Failed to load albums";
}

function sorted(albums: Album[]) {
  return [...albums].sort((a, b) => a.albumName.toLowerCase().localeCompare(b.albumName.toLowerCase()));
}

export function createTargetAlbumCatalogue(adapter: AlbumsDesktopAdapter): TargetAlbumCatalogue {
  let albums = $state<Album[]>([]);
  let isLoading = $state(false);
  let error = $state<string | null>(null);
  let hasRequested = $state(false);
  let latestRequest = 0;
  let pendingCreated = new Map<string, Album>();

  async function refresh() {
    const request = ++latestRequest;
    hasRequested = true;
    isLoading = true;
    error = null;

    try {
      const loadedAlbums = await adapter.getAlbums();
      if (request === latestRequest) {
        for (const album of loadedAlbums) pendingCreated.delete(album.id);
        albums = sorted([...loadedAlbums, ...[...pendingCreated.values()].filter((album) => !loadedAlbums.some((loaded) => loaded.id === album.id))]);
      }
    } catch (caught) {
      if (request === latestRequest) error = errorMessage(caught);
    } finally {
      if (request === latestRequest) isLoading = false;
    }
  }

  async function create(name: string) {
    const created = await adapter.createAlbum(name);
    pendingCreated.set(created.id, created);
    albums = sorted([...albums.filter((album) => album.id !== created.id), created]);
    await refresh();
    return created;
  }

  function clear() {
    latestRequest += 1;
    albums = [];
    pendingCreated.clear();
    error = null;
    hasRequested = false;
    isLoading = false;
  }

  return {
    get albums() { return albums; },
    get isLoading() { return isLoading; },
    get error() { return error; },
    get hasRequested() { return hasRequested; },
    refresh,
    clear,
    create,
    nameFor(id) { return albums.find((album) => album.id === id)?.albumName; },
  };
}
