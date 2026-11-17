import type { ContextSearchPage, SearchFeatures, SearchFilters, SearchMode, SearchRating, SearchRequest, SearchResults } from "../contracts";
import { buildRequestFilters, filterErrors as validateFilters } from "../search-filters";
import type { SearchDesktopAdapter } from "../tauri/desktop-adapter";
import { createContextResultsController, type ContextResults } from "./context-results-controller.svelte";
import { classifyRequestFailure, failureMessage } from "./request-failure";
import { createSearchSuggestions, type SearchSuggestions } from "./search-suggestions.svelte";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** Why the latest Search could not be answered. */
export type SearchFailure = "authentication_required" | "permission_denied" | "unavailable" | "context_unavailable" | "error";

/** How the Search Results are shown (ADR-0009): chronologically in Timeline Buckets, or flat in the order of relevance. */
export type SearchPresentation = "chronological" | "relevance";

/** The most results a chronological Search shows (ADR-0009); mirrors the backend constant. */
export const SEARCH_RESULT_LIMIT = 5_000;

/** The Search Mode a session starts with and returns to when the Search is cleared. */
export const DEFAULT_SEARCH_MODE: SearchMode = "context";
/** The Search Mode that replaces one the server cannot serve (Context without Smart Search, OCR without OCR). */
const FALLBACK_SEARCH_MODE: SearchMode = "filename";

/**
 * The Search: its text, Search Mode and Search Filters (held in memory for the session, never persisted)
 * and its chronological Search Results, which satisfy the shared timeline contract so the shared timeline
 * view, Scrubber and Lightbox show them (ADR-0009). `refresh()` re-runs the latest submitted Search, which
 * is what Retry does.
 */
export interface SearchController extends TimelineController {
  /** The text in the search bar, bound to its field. */
  text: string;
  mode: SearchMode;
  /** The Search Filters; each Search Filter is one optional field, edited in place. */
  readonly filters: SearchFilters;
  /** Selects a Rating level; selecting the level that is already selected clears the Rating. */
  toggleRating(level: SearchRating): void;
  /** The locale key of why a set Search Filter cannot be searched, per filter; such a Search does not run. */
  readonly filterErrors: Partial<Record<keyof SearchFilters, string>>;
  /** False while the server is known to have OCR switched off; unknown features count as available. */
  readonly ocrAvailable: boolean;
  /** False while the server is known to have Smart Search switched off; unknown features count as available. */
  readonly smartSearchAvailable: boolean;
  /** True when the server's features could not be read (they are read again with the next Search). */
  readonly featuresFailed: boolean;
  /**
   * How the latest Search shows its results: a Context text is a flat grid in relevance order (`context`),
   * every other Search is chronological (this controller as a timeline).
   */
  readonly presentation: SearchPresentation;
  /** The Context Search Results; empty unless `presentation` is "relevance". */
  readonly context: ContextResults;
  /** The text of the Search whose results are shown (empty before the first successful Search). */
  readonly submittedText: string;
  /** The suggestions Immich offers for the Search Filters chosen from them (camera, later Location). */
  readonly suggestions: SearchSuggestions;
  /** The number of Remote Assets in the results. */
  readonly resultCount: number;
  /** True when the Search was cut at the result limit and only the newest results are shown. */
  readonly truncated: boolean;
  /** Set while the server is older than the Search requires; every other view is unaffected. */
  readonly unsupported: { version: string; requiredVersion: string } | null;
  /** The reason the latest Search failed, or null. */
  readonly failure: SearchFailure | null;
  /** Increases whenever a Search finishes loading, so a view can start at the top of new results. */
  readonly revision: number;
  /** Whether there is a Search to run: a text or a set Search Filter, and every set filter can be searched. */
  readonly canSubmit: boolean;
  /** Reads the server's features once and holds them in memory; a failure leaves them unknown and never throws. */
  loadFeatures(): Promise<void>;
  /** Runs a Search with the current text, mode and filters. Resolves to false, doing nothing, without anything to search for or with a filter that cannot be searched. */
  submit(): Promise<boolean>;
}

const NO_RESULTS: SearchResults = { buckets: [], assetsByBucket: {}, truncated: false };
const NO_CONTEXT_PAGE: ContextSearchPage = { assets: [], nextPage: null };

function classifyFailure(reason: unknown): SearchFailure {
  // Unlike the other sections, a rejected API key is its own failure here: the view asks to sign in again
  // instead of saying the server is unavailable (which `classifyRequestFailure` maps it to).
  const message = failureMessage(reason);
  if (message.startsWith("search_authentication_required")) return "authentication_required";
  if (message.startsWith("search_context_unavailable")) return "context_unavailable";
  return classifyRequestFailure(reason, "search");
}

export function createSearchController(adapter: SearchDesktopAdapter): SearchController {
  let text = $state("");
  let mode = $state<SearchMode>(DEFAULT_SEARCH_MODE);
  let filters = $state<SearchFilters>({});
  let features = $state<SearchFeatures | null>(null);
  let featuresFailed = $state(false);
  let featuresRead: Promise<void> | null = null;
  let presentation = $state<SearchPresentation>("chronological");
  let submittedText = $state("");
  let truncated = $state(false);
  let unsupported = $state<{ version: string; requiredVersion: string } | null>(null);
  let failure = $state<SearchFailure | null>(null);
  let revision = $state(0);
  // The Search the shared timeline loads: the latest submitted one, which Retry repeats.
  let pending: SearchRequest | null = null;
  // The Context Search whose results are shown; further pages belong to it, not to a newer Search that failed.
  let shownContextRequest: SearchRequest | null = null;
  // Only a server that answered "supported" is remembered; an unsupported one is asked again.
  let supported = false;
  // Bumped by every load and by clear(), so a load that finishes late cannot touch newer state.
  let latestLoad = 0;
  // Bumped by clear(), so features that arrive late are not kept for the next session.
  let featuresGeneration = 0;

  const suggestions = createSearchSuggestions(adapter);
  const ocrAvailable = $derived(features === null || features.ocr);
  const smartSearchAvailable = $derived(features === null || features.smartSearch);

  function loadFeatures(): Promise<void> {
    if (features) return Promise.resolve();
    featuresRead ??= (async () => {
      const generation = featuresGeneration;
      try {
        const read = await adapter.getSearchFeatures();
        if (generation !== featuresGeneration) return;
        features = read;
        featuresFailed = false;
        // OCR can no longer be answered, so a Search must not keep asking for it.
        if (!isAvailable(mode, read)) mode = FALLBACK_SEARCH_MODE;
      } catch {
        // Features only gate optional modes; a failed read must not block a Search.
        if (generation === featuresGeneration) featuresFailed = true;
      } finally {
        if (generation === featuresGeneration) featuresRead = null;
      }
    })();
    return featuresRead;
  }

  function isAvailable(candidate: SearchMode, read: SearchFeatures | null) {
    if (read === null) return true;
    return candidate === "ocr" ? read.ocr : candidate === "context" ? read.smartSearch : true;
  }

  function currentRequest(): SearchRequest {
    return { text: text.trim(), mode, filters: buildRequestFilters($state.snapshot(filters)) };
  }

  function hasCriteria(request: SearchRequest) {
    return request.text !== "" || Object.keys(request.filters).length > 0;
  }

  function isSearchable(request: SearchRequest) {
    return hasCriteria(request) && Object.keys(validateFilters($state.snapshot(filters))).length === 0;
  }

  /** Reads the server's support once; false (with `unsupported` set) when the server is older than a Search requires. */
  async function ensureSupported(): Promise<boolean> {
    if (supported) return true;
    const generation = featuresGeneration;
    const support = await adapter.getSearchSupport();
    if (generation !== featuresGeneration) return false;
    if (!support.supported) {
      unsupported = { version: support.version, requiredVersion: support.requiredVersion };
      failure = null;
      truncated = false;
      return false;
    }
    supported = true;
    return true;
  }

  async function load(): Promise<SearchResults> {
    const queued = pending;
    if (!queued) return NO_RESULTS;
    const thisLoad = ++latestLoad;
    try {
      if (!(await ensureSupported()) || thisLoad !== latestLoad) return NO_RESULTS;
      await loadFeatures();
      if (thisLoad !== latestLoad) return NO_RESULTS;
      // The features may have turned the mode of the queued Search unavailable.
      const request = isAvailable(queued.mode, features) ? queued : { ...queued, mode: FALLBACK_SEARCH_MODE };
      const results = await adapter.searchAssets(request);
      if (thisLoad === latestLoad) {
        unsupported = null;
        failure = null;
        truncated = results.truncated;
        submittedText = request.text;
        revision += 1;
      }
      return results;
    } catch (error) {
      if (thisLoad === latestLoad) failure = classifyFailure(error);
      throw error;
    }
  }

  const timeline = createTimelineController({ getTimeline: load });

  /** Loads one page of the Context Search: the pending Search for the first page, the shown one for further pages. */
  async function loadContextPage(page: number): Promise<ContextSearchPage> {
    const first = page === 1;
    const request = first ? pending : shownContextRequest;
    if (!request) return NO_CONTEXT_PAGE;
    const thisLoad = first ? ++latestLoad : latestLoad;
    try {
      if (!(await ensureSupported()) || thisLoad !== latestLoad) return NO_CONTEXT_PAGE;
      const result = await adapter.searchContextPage(request, page);
      if (thisLoad === latestLoad && first) {
        unsupported = null;
        failure = null;
        truncated = false;
        shownContextRequest = request;
        submittedText = request.text;
        revision += 1;
      }
      return result;
    } catch (error) {
      // A failed further page keeps what is shown and offers Retry; only the first page is a failed Search.
      if (first && thisLoad === latestLoad) failure = classifyFailure(error);
      throw error;
    }
  }

  const context = createContextResultsController({ loadPage: loadContextPage });

  function clear() {
    latestLoad += 1;
    pending = null;
    shownContextRequest = null;
    supported = false;
    text = "";
    mode = DEFAULT_SEARCH_MODE;
    filters = {};
    featuresGeneration += 1;
    suggestions.clear();
    features = null;
    featuresFailed = false;
    featuresRead = null;
    presentation = "chronological";
    submittedText = "";
    truncated = false;
    unsupported = null;
    failure = null;
    revision = 0;
    timeline.clear();
    context.clear();
  }

  async function submit() {
    let request = currentRequest();
    if (!isSearchable(request)) return false;
    if (request.mode === "context" && features === null) {
      // Whether Smart Search is on decides how a Context text is searched, so it is known before the Search.
      const generation = featuresGeneration;
      await loadFeatures();
      if (generation !== featuresGeneration) return false;
      request = currentRequest();
    }
    if (!isAvailable(request.mode, features)) request = { ...request, mode: FALLBACK_SEARCH_MODE };
    pending = request;
    const next: SearchPresentation = request.mode === "context" && request.text !== "" ? "relevance" : "chronological";
    if (next !== presentation) {
      // The results of the other kind of Search no longer belong to the Search being shown.
      (next === "relevance" ? timeline : context).clear();
      presentation = next;
      failure = null;
      truncated = false;
    }
    await (next === "relevance" ? context : timeline).refresh();
    return true;
  }

  return {
    get text() { return text; },
    set text(value: string) { text = value; },
    get mode() { return mode; },
    set mode(value: SearchMode) { if (isAvailable(value, features)) mode = value; },
    get ocrAvailable() { return ocrAvailable; },
    get smartSearchAvailable() { return smartSearchAvailable; },
    get featuresFailed() { return featuresFailed; },
    get filters() { return filters; },
    get filterErrors() { return validateFilters(filters); },
    get canSubmit() { return isSearchable(currentRequest()); },
    get presentation() { return presentation; },
    context,
    suggestions,
    get submittedText() { return submittedText; },
    get resultCount() {
      return presentation === "relevance"
        ? context.buckets.reduce((total, bucket) => total + bucket.count, 0)
        : timeline.buckets.reduce((total, bucket) => total + bucket.count, 0);
    },
    get truncated() { return truncated; },
    get unsupported() { return unsupported; },
    get failure() { return failure; },
    get revision() { return revision; },
    get buckets() { return timeline.buckets; },
    get refreshGeneration() { return timeline.refreshGeneration; },
    get isLoading() { return timeline.isLoading; },
    get isRefreshing() { return timeline.isRefreshing; },
    get isOffline() { return timeline.isOffline; },
    get hasInitialLoadError() { return timeline.hasInitialLoadError; },
    assetsFor: timeline.assetsFor,
    activateBucket: timeline.activateBucket,
    refresh: timeline.refresh,
    removeAsset: timeline.removeAsset,
    locateAsset: timeline.locateAsset,
    stepAsset: timeline.stepAsset,
    toggleRating(level) { filters.rating = filters.rating === level ? undefined : level; },
    loadFeatures,
    submit,
    clear,
  };
}
