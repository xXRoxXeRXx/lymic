import type { RecognizedPerson } from "../contracts";
import type { PeopleDesktopAdapter } from "../tauri/desktop-adapter";
import { createRemoteCollection } from "./remote-collection.svelte";
import { classifyRequestFailure, type RequestFailure } from "./request-failure";

export interface PeopleViewState {
  readonly people: RecognizedPerson[];
  readonly isLoading: boolean;
  readonly isRefreshing: boolean;
  readonly isOffline: boolean;
  readonly hasNextPage: boolean;
  readonly error: RequestFailure | null;
  refresh(): Promise<void>;
  loadMore(): Promise<void>;
  clear(): void;
}

const PAGE_SIZE = 50;
export function createPeopleController(adapter: PeopleDesktopAdapter): PeopleViewState {
  const collection = createRemoteCollection({
    async load(page) {
      const result = await adapter.getPeople(page, PAGE_SIZE);
      return { items: result.people, hasNextPage: result.hasNextPage };
    },
  });

  return {
    get people() { return collection.items; },
    get isLoading() { return collection.isLoading; },
    get isRefreshing() { return collection.isRefreshing; },
    get isOffline() { return collection.isOffline; },
    get hasNextPage() { return collection.hasNextPage; },
    get error() { return collection.error ? classifyRequestFailure(collection.error.reason, "recognized_people") : null; },
    refresh: collection.refresh,
    loadMore: collection.loadMore,
    clear: collection.clear,
  };
}
