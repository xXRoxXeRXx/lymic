import { allIdsFilter } from "./parts";

/**
 * Tag: the Tags that must all be on a Remote Asset (several Tags mean "all of them"). Only the Tags that
 * were chosen count; the children of a chosen Tag do not. The chip shows how many are chosen.
 */
export const tagFilter = allIdsFilter("tagIds");
