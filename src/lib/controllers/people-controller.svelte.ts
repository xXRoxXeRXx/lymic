import type { RecognizedPerson } from "../contracts";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";

export interface PeopleViewState {
  readonly people: RecognizedPerson[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  readonly error: "unavailable" | "error" | null;
  onSelect?: (person: RecognizedPerson) => void;
  refresh(): Promise<void>;
  clear(): void;
}

const PAGE_SIZE = 100;

export function createPeopleController(adapter: PeopleDesktopAdapter): PeopleViewState {
  let people = $state<RecognizedPerson[]>([]);
  let isLoading = $state(false);
  let isRefreshing = $state(false);
  let isOffline = $state(false);
  let error = $state<"unavailable" | "error" | null>(null);
  let generation = 0;
  let pending: Promise<void> | null = null;

  async function refresh() {
    if (pending) return pending;
    const requestGeneration = generation;
    isLoading = people.length === 0;
    isRefreshing = true;
    const request = (async () => {
      let page = 1;
      let hasNextPage = true;
      const loaded: RecognizedPerson[] = [];
      while (hasNextPage) {
        const result = await adapter.getPeople(page, PAGE_SIZE);
        if (requestGeneration !== generation) return;
        loaded.push(...result.people);
        hasNextPage = result.hasNextPage;
        page += 1;
      }
      people = loaded;
      error = null;
      isOffline = false;
    })().catch((reason: unknown) => {
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

  function startRefresh() { generation += 1; pending = null; return refresh(); }
  function clear() { generation += 1; pending = null; people = []; isLoading = false; isRefreshing = false; isOffline = false; error = null; }
  return { get people() { return people; }, get isLoading() { return isLoading; }, get isRefreshing() { return isRefreshing; }, get isOffline() { return isOffline; }, get error() { return error; }, refresh: startRefresh, clear };
}
