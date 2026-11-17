import type { SearchSuggestionsRequest } from "../contracts";
import type { SearchDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure, type RequestFailure } from "./request-failure";

/** What a Search Filter shows for one suggestions request. */
export interface SuggestionsState {
  status: "idle" | "loading" | "ready" | "error";
  items: string[];
  /** Why the suggestions could not be read; set only with `status: "error"`. */
  failure: RequestFailure | null;
}

/**
 * The suggestions Immich offers for Search Filters whose values Immich compares exactly (camera make, model
 * and lens model; Location later). Each distinct request (type plus narrowing) is read once and held in memory
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

const IDLE: SuggestionsState = { status: "idle", items: [], failure: null };

/** The same request, written the same way, so equal requests share one entry. */
function keyOf(request: SearchSuggestionsRequest): string {
  const { type, make, country, state } = request;
  return JSON.stringify([type, make ?? null, country ?? null, state ?? null]);
}

export function createSearchSuggestions(adapter: Pick<SearchDesktopAdapter, "getSearchSuggestions">): SearchSuggestions {
  let entries = $state<Record<string, SuggestionsState>>({});
  const inFlight = new Map<string, Promise<void>>();
  // Bumped by clear(), so an answer that arrives late is not kept for the next session.
  let generation = 0;

  function load(request: SearchSuggestionsRequest): Promise<void> {
    const key = keyOf(request);
    if (entries[key]?.status === "ready") return Promise.resolve();
    const running = inFlight.get(key);
    if (running) return running;

    const thisGeneration = generation;
    entries[key] = { status: "loading", items: [], failure: null };
    const read = (async () => {
      try {
        const items = await adapter.getSearchSuggestions({ ...request });
        if (thisGeneration === generation) entries[key] = { status: "ready", items, failure: null };
      } catch (error) {
        if (thisGeneration === generation) {
          entries[key] = { status: "error", items: [], failure: classifyRequestFailure(error, "search") };
        }
      } finally {
        if (thisGeneration === generation) inFlight.delete(key);
      }
    })();
    inFlight.set(key, read);
    return read;
  }

  return {
    stateFor: (request) => entries[keyOf(request)] ?? IDLE,
    load,
    clear() {
      generation += 1;
      entries = {};
      inFlight.clear();
    },
  };
}
