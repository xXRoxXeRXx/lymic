import type { RecognizedPerson } from "../contracts";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

export interface PeopleViewState {
  readonly people: RecognizedPerson[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  readonly hasNextPage: boolean;
  readonly error: "unavailable" | "error" | null;
  onSelect?: (person: RecognizedPerson) => void;
  refresh(): Promise<void>;
  loadMore(): Promise<void>;
  clear(): void;
}

export function createPeopleController(adapter: PeopleDesktopAdapter): PeopleViewState {
  let people = $state<RecognizedPerson[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let hasNextPage = $state(false);
  let error = $state<"unavailable" | "error" | null>(null);
  let page = 0;
  let generation = 0;
  let pending: Promise<void> | null = null;

  async function load(nextPage: number, replace: boolean) {
    if (pending) return pending;
    const requestGeneration = generation;
    isLoading = people.length === 0;
    isRefreshing = replace;
    const request = adapter.getPeople(nextPage, 50).then((result) => {
      if (requestGeneration !== generation) return;
      people = replace ? result.people : [...people, ...result.people];
      page = nextPage;
      hasNextPage = result.hasNextPage;
      error = null;
      isOffline = false;
    }).catch((reason: unknown) => {
      if (requestGeneration !== generation) return;
      const message = (typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "").toLowerCase();
      error = people.length === 0 ? (message.includes("permission") || message.includes("unavailable") ? "unavailable" : "error") : null;
      isOffline = people.length > 0;
    }).finally(() => {
      if (pending === request) pending = null;
      if (requestGeneration === generation) { isLoading = false; isRefreshing = false; }
    });
    pending = request;
    return request;
  }

  function refresh() { generation += 1; pending = null; return load(1, true); }
  function loadMore() { return hasNextPage ? load(page + 1, false) : Promise.resolve(); }
  function clear() { generation += 1; pending = null; people = []; page = 0; hasNextPage = false; isLoading = false; isRefreshing = false; isOffline = false; error = null; }
  return { get people() { return people; }, get isLoading() { return isLoading; }, get isRefreshing() { return isRefreshing; }, get isOffline() { return isOffline; }, get hasNextPage() { return hasNextPage; }, get error() { return error; }, refresh, loadMore, clear };
}
