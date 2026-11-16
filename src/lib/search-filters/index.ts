import type { SearchFilters, SearchRequestFilters } from "../contracts";
import { dateFilter } from "./date";
import { mediaTypeFilter } from "./media-type";
import type { SearchFilterDefinition } from "./types";

/** Every Search Filter. A new Search Filter adds one entry here. */
const searchFilters: SearchFilterDefinition[] = [dateFilter, mediaTypeFilter];

/** The Search request's filters: only the set Search Filters, which Immich combines with AND. */
export function buildRequestFilters(filters: SearchFilters): SearchRequestFilters {
  const request: SearchRequestFilters = {};
  for (const filter of searchFilters) filter.apply(filters, request);
  return request;
}

/** The locale key of the problem with each Search Filter whose value cannot be searched. */
export function filterErrors(filters: SearchFilters): Partial<Record<keyof SearchFilters, string>> {
  const errors: Partial<Record<keyof SearchFilters, string>> = {};
  for (const filter of searchFilters) {
    const error = filter.error?.(filters);
    if (error) errors[filter.key] = error;
  }
  return errors;
}
