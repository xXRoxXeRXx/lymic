import type { SearchFilterDefinition } from "./types";

/** Person: the Recognized People who must all appear in a Remote Asset (several people mean "all of them"). */
export const personFilter: SearchFilterDefinition = {
  key: "personIds",
  apply({ personIds }, request) {
    if (personIds?.length) request.personIds = [...personIds];
  },
};
