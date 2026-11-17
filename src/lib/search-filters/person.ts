import { allIdsFilter } from "./parts";

/** Person: the Recognized People who must all appear in a Remote Asset (several people mean "all of them"). The chip names the first person that is known. */
export const personFilter = allIdsFilter("personIds", (id, context) => context.personName(id));
