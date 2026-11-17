import { untrack } from "svelte";

/** Why a load failed. Boxed so an `undefined` rejection still counts as an error. */
export interface RemoteCollectionError {
  readonly reason: unknown;
}

/** One page of a paged list and whether Immich has a further one. */
export interface RemoteCollectionPage<T> {
  readonly items: T[];
  readonly hasNextPage: boolean;
}

/**
 * The state of one list loaded from Immich: what is shown, whether it is
 * loading, and what a failed load means.
 */
export interface RemoteCollection<T> {
  readonly items: T[];
  /** The first load: a request is in flight and nothing is cached yet. */
  readonly isLoading: boolean;
  /** A refresh is in flight, including one that replaces cached items; loading a further page does not count. */
  readonly isRefreshing: boolean;
  /** The last request failed but cached items stay visible. */
  readonly isOffline: boolean;
  /** The last request failed and nothing is cached. */
  readonly error: RemoteCollectionError | null;
  /** Why the last request failed, whether or not items are cached; null after a success. Lets a caller classify an offline failure. */
  readonly failure: RemoteCollectionError | null;
  /** A load has succeeded since the last `clear`, so an empty list is a real answer. */
  readonly hasLoaded: boolean;
  /** Immich has a page after the last one loaded. Always false for a list that loads as one array. */
  readonly hasNextPage: boolean;
  /** Replaces the items with a fresh load of the first page. Concurrent refreshes share one request. */
  refresh(): Promise<void>;
  /** Appends the next page, and does nothing when there is none. Concurrent calls share one request. */
  loadMore(): Promise<void>;
  /** Stops waiting for a request in flight, which is ignored when it settles; cached items and any earlier failure stay. */
  abandon(): void;
  /** Drops items and state; a request still in flight is ignored when it settles. */
  clear(): void;
}

/**
 * Owns the shared behaviour of a remote list: a response that arrives after
 * `abandon`, `clear` or after a newer request is ignored, concurrent requests
 * share one request, and a failure keeps cached items (offline) or reports an
 * error when there are none. `load` receives the 1-based page and returns the
 * whole list, or a page when the list is paged.
 */
export function createRemoteCollection<T>(options: {
  load(page: number): Promise<T[] | RemoteCollectionPage<T>>;
}): RemoteCollection<T> {
  let items = $state.raw<T[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let failure = $state.raw<RemoteCollectionError | null>(null);
  let hasLoaded = $state(false);
  let hasNextPage = $state(false);
  let page = 0;
  // Identifies the current request; a settled request that no longer matches is stale.
  let generation = 0;
  let pending: { request: Promise<void>; appends: boolean } | null = null;

  // The public methods are called from effects; they must not subscribe the caller to the state they write.
  const refresh = () => untrack(startRefresh);
  const loadMore = () => untrack(startLoadMore);
  const abandon = () => untrack(stopWaiting);
  const clear = () => untrack(reset);

  function startRefresh() {
    if (pending && !pending.appends) return pending.request;
    return start(false);
  }

  function startLoadMore() {
    if (pending) return pending.request;
    return hasNextPage ? start(true) : Promise.resolve();
  }

  function start(appends: boolean) {
    const requestGeneration = ++generation;
    const requestPage = appends ? page + 1 : 1;
    isLoading = items.length === 0;
    isRefreshing = !appends;
    failure = null;
    const request = new Promise<T[] | RemoteCollectionPage<T>>((resolve) => resolve(options.load(requestPage)))
      .then((result) => {
        if (requestGeneration !== generation) return;
        const loaded = Array.isArray(result) ? { items: result, hasNextPage: false } : result;
        items = appends ? [...items, ...loaded.items] : loaded.items;
        page = requestPage;
        hasNextPage = loaded.hasNextPage;
        hasLoaded = true;
        isOffline = false;
      })
      .catch((reason: unknown) => {
        if (requestGeneration !== generation) return;
        failure = { reason };
        isOffline = items.length > 0;
      })
      .finally(() => {
        if (requestGeneration !== generation) return;
        pending = null;
        isLoading = false;
        isRefreshing = false;
      });
    pending = { request, appends };
    return request;
  }

  function stopWaiting() {
    generation += 1;
    pending = null;
    isLoading = false;
    isRefreshing = false;
  }

  function reset() {
    stopWaiting();
    items = [];
    page = 0;
    hasNextPage = false;
    hasLoaded = false;
    isOffline = false;
    failure = null;
  }

  return {
    get items() { return items; },
    get isLoading() { return isLoading; },
    get isRefreshing() { return isRefreshing; },
    get isOffline() { return isOffline; },
    get error() { return items.length === 0 ? failure : null; },
    get failure() { return failure; },
    get hasLoaded() { return hasLoaded; },
    get hasNextPage() { return hasNextPage; },
    refresh,
    loadMore,
    abandon,
    clear,
  };
}
