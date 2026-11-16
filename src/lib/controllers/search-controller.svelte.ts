import type { SearchFeatures, SearchFilters, SearchMode, SearchQuery, SearchTimeline } from "../contracts";
import { buildRequestFilters, filterErrors as validateFilters } from "../search-filters";
import type { SearchDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyRequestFailure } from "./request-failure";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/** Why the latest Search could not be answered. */
export type SearchFailure = "authentication_required" | "permission_denied" | "unavailable" | "error";

/** The most matches a chronological Search shows (ADR-0009); mirrors the backend constant. */
export const SEARCH_RESULT_LIMIT = 5_000;

/** The Search Mode a session starts with and returns to when the Search is cleared. */
export const DEFAULT_SEARCH_MODE: SearchMode = "filename";

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
  /** The locale key of why a set Search Filter cannot be searched, per filter; such a Search does not run. */
  readonly filterErrors: Partial<Record<keyof SearchFilters, string>>;
  /** False while the server is known to have OCR switched off; unknown features count as available. */
  readonly ocrAvailable: boolean;
  /** True when the server's features could not be read (they are read again with the next Search). */
  readonly featuresFailed: boolean;
  /** The text of the Search whose results are shown (empty before the first successful Search). */
  readonly submittedText: string;
  /** The number of Remote Assets in the results. */
  readonly resultCount: number;
  /** True when the Search was cut at the result limit and only the newest matches are shown. */
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

const NO_RESULTS: SearchTimeline = { buckets: [], assetsByBucket: {}, truncated: false };

function classifyFailure(reason: unknown): SearchFailure {
  const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  if (message.startsWith("search_authentication_required")) return "authentication_required";
  return classifyRequestFailure(reason, "search");
}

export function createSearchController(adapter: SearchDesktopAdapter): SearchController {
  let text = $state("");
  let mode = $state<SearchMode>(DEFAULT_SEARCH_MODE);
  let filters = $state<SearchFilters>({});
  let features = $state<SearchFeatures | null>(null);
  let featuresFailed = $state(false);
  let featuresRead: Promise<void> | null = null;
  let submittedText = $state("");
  let truncated = $state(false);
  let unsupported = $state<{ version: string; requiredVersion: string } | null>(null);
  let failure = $state<SearchFailure | null>(null);
  let revision = $state(0);
  // The Search the shared timeline loads: the latest submitted one, which Retry repeats.
  let pending: SearchQuery | null = null;
  // Only a server that answered "supported" is remembered; an unsupported one is asked again.
  let supported = false;
  // Bumped by every load and by clear(), so a load that finishes late cannot touch newer state.
  let latestLoad = 0;
  // Bumped by clear(), so features that arrive late are not kept for the next session.
  let featuresGeneration = 0;

  const ocrAvailable = $derived(features === null || features.ocr);

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
        if (!read.ocr && mode === "ocr") mode = DEFAULT_SEARCH_MODE;
      } catch {
        // Features only gate optional modes; a failed read must not block a Search.
        if (generation === featuresGeneration) featuresFailed = true;
      } finally {
        if (generation === featuresGeneration) featuresRead = null;
      }
    })();
    return featuresRead;
  }

  function currentQuery(): SearchQuery {
    return { text: text.trim(), mode, filters: buildRequestFilters($state.snapshot(filters)) };
  }

  function hasCriteria(query: SearchQuery) {
    return query.text !== "" || Object.keys(query.filters).length > 0;
  }

  function isSearchable(query: SearchQuery) {
    return hasCriteria(query) && Object.keys(validateFilters($state.snapshot(filters))).length === 0;
  }

  async function load(): Promise<SearchTimeline> {
    const queued = pending;
    if (!queued) return NO_RESULTS;
    const thisLoad = ++latestLoad;
    try {
      if (!supported) {
        const support = await adapter.getSearchSupport();
        if (thisLoad !== latestLoad) return NO_RESULTS;
        if (!support.supported) {
          unsupported = { version: support.version, requiredVersion: support.requiredVersion };
          failure = null;
          truncated = false;
          return NO_RESULTS;
        }
        supported = true;
      }
      await loadFeatures();
      if (thisLoad !== latestLoad) return NO_RESULTS;
      // The features may have turned the mode of the queued Search unavailable.
      const query = queued.mode === "ocr" && !ocrAvailable ? { ...queued, mode: DEFAULT_SEARCH_MODE } : queued;
      const results = await adapter.searchAssets(query);
      if (thisLoad === latestLoad) {
        unsupported = null;
        failure = null;
        truncated = results.truncated;
        submittedText = query.text;
        revision += 1;
      }
      return results;
    } catch (error) {
      if (thisLoad === latestLoad) failure = classifyFailure(error);
      throw error;
    }
  }

  const timeline = createTimelineController({ getTimeline: load });

  function clear() {
    latestLoad += 1;
    pending = null;
    supported = false;
    text = "";
    mode = DEFAULT_SEARCH_MODE;
    filters = {};
    featuresGeneration += 1;
    features = null;
    featuresFailed = false;
    featuresRead = null;
    submittedText = "";
    truncated = false;
    unsupported = null;
    failure = null;
    revision = 0;
    timeline.clear();
  }

  async function submit() {
    const query = currentQuery();
    if (!isSearchable(query)) return false;
    pending = query;
    await timeline.refresh();
    return true;
  }

  return {
    get text() { return text; },
    set text(value: string) { text = value; },
    get mode() { return mode; },
    set mode(value: SearchMode) { if (value !== "ocr" || ocrAvailable) mode = value; },
    get ocrAvailable() { return ocrAvailable; },
    get featuresFailed() { return featuresFailed; },
    get filters() { return filters; },
    get filterErrors() { return validateFilters(filters); },
    get canSubmit() { return isSearchable(currentQuery()); },
    get submittedText() { return submittedText; },
    get resultCount() { return timeline.buckets.reduce((total, bucket) => total + bucket.count, 0); },
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
    loadFeatures,
    submit,
    clear,
  };
}
