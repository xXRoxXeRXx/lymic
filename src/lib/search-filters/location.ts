import type { SearchLocation } from "../contracts";
import type { SearchFilterDefinition } from "./types";

/**
 * Location: country, state or region and city, each optional and each chosen from what Immich offers (country and
 * state from its suggestions, the city from the city listing of the Explore Places). The parts combine.
 */
export const locationFilter: SearchFilterDefinition = {
  key: "location",
  apply({ location }, request) {
    const chosen: SearchLocation = {};
    if (location?.country) chosen.country = location.country;
    if (location?.state) chosen.state = location.state;
    if (location?.city) chosen.city = location.city;
    if (Object.keys(chosen).length > 0) request.location = chosen;
  },
};

/**
 * The Location filter with one part set (or cleared, with `undefined`); unset when no part is left. A different
 * country drops the chosen state, which may not exist there (state suggestions are limited by the country).
 */
export function setLocationPart(location: SearchLocation | undefined, part: keyof SearchLocation, value: string | undefined): SearchLocation | undefined {
  const next: SearchLocation = { ...location, [part]: value };
  if (part === "country" && value !== undefined && value !== location?.country) next.state = undefined;
  for (const key of Object.keys(next) as (keyof SearchLocation)[]) if (next[key] === undefined) delete next[key];
  return Object.keys(next).length > 0 ? next : undefined;
}
