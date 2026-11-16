import type { Memory } from "../contracts";
import type { AllMemoriesDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure, type RequestFailure } from "./request-failure";

export interface AllMemoriesViewState {
  readonly memories: Memory[];
  /** The first page is loading and nothing is shown yet. */
  readonly isLoading: boolean;
  readonly isLoadingMore: boolean;
  /** A refresh failed while earlier results are still shown. */
  readonly isOffline: boolean;
  readonly hasNextPage: boolean;
  /** The first page failed and nothing can be shown. */
  readonly error: RequestFailure | null;
  /** A further page failed; the pages already shown are kept. */
  readonly loadMoreFailed: boolean;
  /** Reloads from the first page. */
  refresh(): Promise<void>;
  /** Loads the next page, or retries the one that failed. */
  loadMore(): Promise<void>;
  clear(): void;
}

/** Memories carry all their assets, so a page stays small. */
export const ALL_MEMORIES_PAGE_SIZE = 24;

export function createAllMemoriesController(adapter: AllMemoriesDesktopAdapter): AllMemoriesViewState {
  let memories = $state<Memory[]>([]);
  let isLoading = $state(false);
  let isLoadingMore = $state(false);
  let isOffline = $state(false);
  let hasNextPage = $state(false);
  let error = $state<RequestFailure | null>(null);
  let loadMoreFailed = $state(false);
  let page = 0;
  let generation = 0;
  let pending: Promise<void> | null = null;

  async function load(nextPage: number, replace: boolean) {
    if (pending) return pending;
    const requestGeneration = generation;
    if (replace) isLoading = memories.length === 0;
    else isLoadingMore = true;
    const request = adapter
      .getMemoriesPage(nextPage, ALL_MEMORIES_PAGE_SIZE)
      .then((result) => {
        if (requestGeneration !== generation) return;
        const loaded = Array.isArray(result?.memories) ? result.memories : [];
        const known = new Set(replace ? [] : memories.map((memory) => memory.id));
        memories = replace ? loaded : [...memories, ...loaded.filter((memory) => !known.has(memory.id))];
        page = nextPage;
        hasNextPage = result?.hasNextPage === true;
        error = null;
        isOffline = false;
        loadMoreFailed = false;
      })
      .catch((reason: unknown) => {
        if (requestGeneration !== generation) return;
        if (replace) {
          error = memories.length === 0 ? classifyRequestFailure(reason, "memories") : null;
          isOffline = memories.length > 0;
        } else {
          loadMoreFailed = true;
        }
      })
      .finally(() => {
        if (pending === request) pending = null;
        if (requestGeneration === generation) {
          isLoading = false;
          isLoadingMore = false;
        }
      });
    pending = request;
    return request;
  }

  function refresh() {
    generation += 1;
    pending = null;
    loadMoreFailed = false;
    return load(1, true);
  }

  function loadMore() {
    if (!hasNextPage) return Promise.resolve();
    return load(page + 1, false);
  }

  function clear() {
    generation += 1;
    pending = null;
    memories = [];
    page = 0;
    hasNextPage = false;
    isLoading = false;
    isLoadingMore = false;
    isOffline = false;
    error = null;
    loadMoreFailed = false;
  }

  return {
    get memories() { return memories; },
    get isLoading() { return isLoading; },
    get isLoadingMore() { return isLoadingMore; },
    get isOffline() { return isOffline; },
    get hasNextPage() { return hasNextPage; },
    get error() { return error; },
    get loadMoreFailed() { return loadMoreFailed; },
    refresh,
    loadMore,
    clear,
  };
}
