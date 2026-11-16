import { classifyRequestFailure, type RequestFailure } from "./request-failure";

/** The load state shared by the Places and Memories overview controllers. */
export interface ExploreListState<T> {
  readonly items: T[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: RequestFailure | null;
  refresh(): Promise<void>;
  clear(): void;
}

/**
 * One list that loads independently: a stale response never overwrites a newer
 * one (generation guard), a failure keeps cached items visible as offline, and
 * a failure with nothing cached is classified by the backend error `prefix`.
 */
export function createExploreListController<T>(options: { prefix: string; load(): Promise<T[]> }): ExploreListState<T> {
  let items = $state<T[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let error = $state<RequestFailure | null>(null);
  let generation = 0;

  async function refresh() {
    const requestGeneration = ++generation;
    isLoading = items.length === 0;
    try {
      const result = await options.load();
      if (requestGeneration !== generation) return;
      items = result;
      error = null;
      isOffline = false;
    } catch (reason) {
      if (requestGeneration !== generation) return;
      error = items.length === 0 ? classifyRequestFailure(reason, options.prefix) : null;
      isOffline = items.length > 0;
    } finally {
      if (requestGeneration === generation) isLoading = false;
    }
  }

  function clear() {
    generation += 1;
    items = [];
    isLoading = false;
    isOffline = false;
    error = null;
  }

  return {
    get items() { return items; },
    get isLoading() { return isLoading; },
    get isOffline() { return isOffline; },
    get error() { return error; },
    refresh,
    clear,
  };
}
