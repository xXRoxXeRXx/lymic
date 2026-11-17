import type { SearchSuggestionsRequest } from "../contracts";
import type { SearchDesktopAdapter } from "../tauri/desktop-adapter";
import { createSuggestionsCache, type SuggestionsState } from "./suggestions-cache.svelte";

export type { SuggestionsState };

/**
 * The suggestions Immich offers for Search Filters whose values Immich compares exactly (camera make, model
 * and lens model, and the Location country and state). Each distinct request (type plus narrowing) is read once and held in memory
 * for the session; a failure is kept per request and retried by the next `load`. A failed request never
 * throws, so it cannot break the rest of Search.
 */
export interface SearchSuggestions {
  /** The current state of a request; reactive. */
  stateFor(request: SearchSuggestionsRequest): SuggestionsState;
  /** Reads a request unless it is already known or in flight. Never rejects. */
  load(request: SearchSuggestionsRequest): Promise<void>;
  clear(): void;
}

/** The same request, written the same way, so equal requests share one entry. */
function keyOf(request: SearchSuggestionsRequest): string {
  const { type, make, country, state } = request;
  return JSON.stringify([type, make ?? null, country ?? null, state ?? null]);
}

export function createSearchSuggestions(adapter: Pick<SearchDesktopAdapter, "getSearchSuggestions">): SearchSuggestions {
  return createSuggestionsCache<SearchSuggestionsRequest>({
    keyOf,
    read: (request) => adapter.getSearchSuggestions({ ...request }),
    prefix: "search",
  });
}
