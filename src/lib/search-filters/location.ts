import type { SearchLocation } from "../contracts";
import { chosenParts, setPart } from "./parts";
import type { SearchFilterDefinition } from "./types";

/**
 * Location: country, state or region and city, each optional and each chosen from what Immich offers (country and
 * state from its suggestions, the city from the city listing of the Explore Places). The parts combine.
 */
export const locationFilter: SearchFilterDefinition = {
  key: "location",
  apply({ location }, request) {
    const chosen = chosenParts(location, ["country", "state", "city"]);
    if (chosen) request.location = chosen;
  },
};

/**
 * The Location filter with one part set (or cleared, with `undefined`); unset when no part is left. A different
 * country drops the chosen state, which may not exist there (state suggestions are limited by the country).
 */
export function setLocationPart(location: SearchLocation | undefined, part: keyof SearchLocation, value: string | undefined): SearchLocation | undefined {
  return setPart(location, part, value, { parent: "country", dependent: "state" });
}
