import type { SearchFilters, SearchRequestFilters } from "../contracts";

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
}
