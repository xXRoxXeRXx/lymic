import type { Album } from "../components/types";
import type { DesktopAdapter } from "../tauri/desktop-adapter";

export type TargetAlbumCatalogueGateway = Pick<DesktopAdapter, "getAlbums" | "createAlbum">;

export interface TargetAlbumCatalogue {
  readonly albums: Album[];
  readonly isLoading: boolean;
  readonly error: string | null;
  readonly hasRequested: boolean;
  refresh(): Promise<void>;
  create(name: string): Promise<Album>;
  nameFor(id: string): string | undefined;
}

function errorMessage(error: unknown) {
  return typeof error === "string" ? error : (error as Error)?.message || "Failed to load albums";
}

function sorted(albums: Album[]) {
  return [...albums].sort((a, b) => a.albumName.toLowerCase().localeCompare(b.albumName.toLowerCase()));
}

export function createTargetAlbumCatalogue(gateway: TargetAlbumCatalogueGateway): TargetAlbumCatalogue {
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
      const loadedAlbums = await gateway.getAlbums();
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
    const created = await gateway.createAlbum(name);
    pendingCreated.set(created.id, created);
    albums = sorted([...albums.filter((album) => album.id !== created.id), created]);
    await refresh();
    return created;
  }

  return {
    get albums() { return albums; },
    get isLoading() { return isLoading; },
    get error() { return error; },
    get hasRequested() { return hasRequested; },
    refresh,
    create,
    nameFor(id) { return albums.find((album) => album.id === id)?.albumName; },
  };
}
