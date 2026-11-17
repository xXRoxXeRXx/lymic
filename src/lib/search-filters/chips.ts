import type { SearchFilters } from "../contracts";
import { filterSummary } from "./index";
import type { SearchChipContext } from "./types";

/** The editors of the search panel: the Search Mode, and one per Filter Chip. */
export type SearchEditorId = "mode" | "person" | "tag" | "date" | "mediaType" | "camera" | "location" | "rating" | "display";

/** One Filter Chip: the entry to the editor of one or more Search Filters, which shows their value. */
export interface SearchChip {
  id: Exclude<SearchEditorId, "mode">;
  /** The locale key of the chip's name. */
  labelKey: string;
  /** The Search Filters the chip edits; Display is the only chip with more than one. */
  keys: (keyof SearchFilters)[];
}

/** The Filter Chips in the order of the panel. The Search Mode is the chip that comes before them and is no Search Filter. */
export const searchChips: SearchChip[] = [
  { id: "person", labelKey: "search_filter_person", keys: ["personIds"] },
  { id: "tag", labelKey: "search_filter_tag", keys: ["tagIds"] },
  { id: "date", labelKey: "search_filter_date", keys: ["date"] },
  { id: "mediaType", labelKey: "search_filter_media_type", keys: ["mediaType"] },
  { id: "camera", labelKey: "search_filter_camera", keys: ["camera"] },
  { id: "location", labelKey: "search_filter_location", keys: ["location"] },
  { id: "rating", labelKey: "search_filter_rating", keys: ["rating"] },
  { id: "display", labelKey: "search_filter_display", keys: ["favorite", "archived", "unfiled"] },
];

/** The value of a chip ("Anna +2", "Favorites, Archived"), or null when none of its Search Filters is set. */
export function chipSummary(chip: SearchChip, filters: SearchFilters, context: SearchChipContext): string | null {
  const parts = chip.keys.map((key) => filterSummary(key, filters, context)).filter((part): part is string => part !== null);
  return parts.length > 0 ? parts.join(", ") : null;
}

/** The chips with a value: the active Search Filters. */
export function activeChips(filters: SearchFilters, context: SearchChipContext): { chip: SearchChip; summary: string }[] {
  const active: { chip: SearchChip; summary: string }[] = [];
  for (const chip of searchChips) {
    const summary = chipSummary(chip, filters, context);
    if (summary !== null) active.push({ chip, summary });
  }
  return active;
}

/** Unsets every Search Filter of the chip. */
export function clearChip(chip: SearchChip, filters: SearchFilters): void {
  for (const key of chip.keys) filters[key] = undefined as never;
}

/** The editor that holds a Search Filter, to open it when the filter blocks a Search. */
export function editorOfFilter(key: keyof SearchFilters): SearchEditorId | null {
  return searchChips.find((chip) => chip.keys.includes(key))?.id ?? null;
}
