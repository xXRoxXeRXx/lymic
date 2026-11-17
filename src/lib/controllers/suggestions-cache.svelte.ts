import { classifyRequestFailure, type RequestFailure } from "./request-failure";

/** What a Search Filter shows for one suggestions request. */
export interface SuggestionsState {
  status: "idle" | "loading" | "ready" | "error";
  items: string[];
  /** Why the suggestions could not be read; set only with `status: "error"`. */
  failure: RequestFailure | null;
}

export interface SuggestionsCache<R> {
  /** The current state of a request; reactive. */
  stateFor(request: R): SuggestionsState;
  /** Reads a request unless it is already known or in flight. Never rejects. */
  load(request: R): Promise<void>;
  clear(): void;
}

const IDLE: SuggestionsState = { status: "idle", items: [], failure: null };

/**
 * The read-once cache behind the Search Filter value lists: each distinct request (identified by `keyOf`) is
 * read once and held in memory for the session, concurrent loads share one read, a failure is kept per request
 * and retried by the next `load`, and `clear` bumps a generation so an answer that arrives late is not kept for
 * the next session. A failed read never throws; its failure is classified by the backend error `prefix`.
 */
export function createSuggestionsCache<R>(options: {
  keyOf(request: R): string;
  read(request: R): Promise<string[]>;
  prefix: string;
}): SuggestionsCache<R> {
  let entries = $state<Record<string, SuggestionsState>>({});
  const inFlight = new Map<string, Promise<void>>();
  let generation = 0;

  function load(request: R): Promise<void> {
    const key = options.keyOf(request);
    if (entries[key]?.status === "ready") return Promise.resolve();
    const running = inFlight.get(key);
    if (running) return running;

    const thisGeneration = generation;
    entries[key] = { status: "loading", items: [], failure: null };
    const read = (async () => {
      try {
        const items = await options.read(request);
        if (thisGeneration === generation) entries[key] = { status: "ready", items, failure: null };
      } catch (error) {
        if (thisGeneration === generation) {
          entries[key] = { status: "error", items: [], failure: classifyRequestFailure(error, options.prefix) };
        }
      } finally {
        if (thisGeneration === generation) inFlight.delete(key);
      }
    })();
    inFlight.set(key, read);
    return read;
  }

  return {
    stateFor: (request) => entries[options.keyOf(request)] ?? IDLE,
    load,
    clear() {
      generation += 1;
      entries = {};
      inFlight.clear();
    },
  };
}

