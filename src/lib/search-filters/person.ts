import { allIdsFilter } from "./parts";

/** Person: the Recognized People who must all appear in a Remote Asset (several people mean "all of them"). */
export const personFilter = allIdsFilter("personIds");
