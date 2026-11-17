import type { SearchFilters, SearchRequestFilters } from "../contracts";

/** What a chip summary needs from the outside: translation, the names of chosen ids and the user's date format. */
export interface SearchChipContext {
  t(key: string, values?: Record<string, string | number>): string;
  /** The name of a Recognized Person, or null when the person is unnamed or not loaded (yet). */
  personName(id: string): string | null;
  /** A local day `YYYY-MM-DD` in the user's locale. */
  formatDay(day: string): string;
}

/**
 * One Search Filter. A Search Filter is one optional field of `SearchFilters`, one definition like this in
 * its own file, one entry in `index.ts`, and one panel component that edits `search.filters.<key>`.
 */
export interface SearchFilterDefinition {
  key: keyof SearchFilters;
  /** The locale key of why the filter's current value cannot be searched, or null when it can. */
  error?(filters: SearchFilters): string | null;
  /** Writes this filter's part of the Search request; writes nothing when the filter is not set. */
  apply(filters: SearchFilters, request: SearchRequestFilters): void;
  /** The value a Filter Chip shows for this filter ("Anna +2"), or null when the filter is not set. */
  summary?(filters: SearchFilters, context: SearchChipContext): string | null;
}
