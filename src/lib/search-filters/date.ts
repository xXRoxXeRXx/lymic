import type { SearchFilterDefinition } from "./types";

const DAY = /^(\d{4})-(\d{2})-(\d{2})$/;

/** The chosen day as `YYYY-MM-DD` (what a date field holds), or null when the field is empty or malformed. */
function parseDay(day: string | undefined): [number, number, number] | null {
  const match = day ? DAY.exec(day) : null;
  return match ? [Number(match[1]), Number(match[2]) - 1, Number(match[3])] : null;
}

/** The first instant of the chosen day in the user's local time zone, as the UTC instant Immich's `takenAt` filter compares. */
export function startOfLocalDay(day: string): string | null {
  const parts = parseDay(day);
  return parts ? new Date(parts[0], parts[1], parts[2], 0, 0, 0, 0).toISOString() : null;
}

/** The last instant of the chosen day in the user's local time zone, so the day is included whole. */
export function endOfLocalDay(day: string): string | null {
  const parts = parseDay(day);
  return parts ? new Date(parts[0], parts[1], parts[2], 23, 59, 59, 999).toISOString() : null;
}

/** Date: from the start of the chosen local day to the end of the chosen local day; either bound is optional. */
export const dateFilter: SearchFilterDefinition = {
  key: "date",
  error({ date }) {
    const from = parseDay(date?.from) ? date?.from : undefined;
    const to = parseDay(date?.to) ? date?.to : undefined;
    // `YYYY-MM-DD` strings sort like the days they name. A reversed range is rejected, never swapped.
    return from && to && from > to ? "search_filter_date_invalid_range" : null;
  },
  apply({ date }, request) {
    const from = date?.from ? startOfLocalDay(date.from) : null;
    const to = date?.to ? endOfLocalDay(date.to) : null;
    if (from || to) request.date = { ...(from && { from }), ...(to && { to }) };
  },
};
