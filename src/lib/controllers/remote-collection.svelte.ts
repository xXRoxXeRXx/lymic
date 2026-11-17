import { untrack } from "svelte";

/** Why a load failed while nothing was cached. Boxed so an `undefined` rejection still counts as an error. */
export interface RemoteCollectionError {
  readonly reason: unknown;
}

/**
 * The state of one list loaded from Immich: what is shown, whether it is
 * loading, and what a failed load means.
 */
export interface RemoteCollection<T> {
  readonly items: T[];
  /** The first load: a request is in flight and nothing is cached yet. */
  readonly isLoading: boolean;
  /** Any request is in flight, including one that refreshes cached items. */
  readonly isRefreshing: boolean;
  /** The last refresh failed but cached items stay visible. */
  readonly isOffline: boolean;
  /** The last refresh failed and nothing is cached. */
  readonly error: RemoteCollectionError | null;
  /** Replaces the items with a fresh load. Concurrent calls share one request. */
  refresh(): Promise<void>;
  /** Drops items and state; a request still in flight is ignored when it settles. */
  clear(): void;
}

/**
 * Owns the shared behaviour of a remote list: a response that arrives after
 * `clear` or after a newer request is ignored, concurrent refreshes share one
 * request, and a failure keeps cached items (offline) or reports an error when
 * there are none.
 */
export function createRemoteCollection<T>(options: { load(): Promise<T[]> }): RemoteCollection<T> {
  let items = $state<T[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let error = $state<RemoteCollectionError | null>(null);
  // Identifies the current request; a settled request that no longer matches is stale.
  let generation = 0;
  let pending: Promise<void> | null = null;

  // `refresh` and `clear` are called from effects; they must not subscribe the caller to the state they write.
  const refresh = () => untrack(startRefresh);
  const clear = () => untrack(reset);

  function startRefresh() {
    if (pending) return pending;
    const requestGeneration = ++generation;
    isLoading = items.length === 0;
    isRefreshing = true;
    error = null;
    const request = new Promise<T[]>((resolve) => resolve(options.load()))
      .then((result) => {
        if (requestGeneration !== generation) return;
        items = result;
        isOffline = false;
      })
      .catch((reason: unknown) => {
        if (requestGeneration !== generation) return;
        error = items.length === 0 ? { reason } : null;
        isOffline = items.length > 0;
      })
      .finally(() => {
        if (requestGeneration !== generation) return;
        pending = null;
        isLoading = false;
        isRefreshing = false;
      });
    pending = request;
    return request;
  }

  function reset() {
    generation += 1;
    pending = null;
    items = [];
    isLoading = false;
    isRefreshing = false;
    isOffline = false;
    error = null;
  }

  return {
    get items() { return items; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    get error() { return error; },
    refresh,
    clear,
  };
}
