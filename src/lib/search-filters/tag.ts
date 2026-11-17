import type { SearchFilterDefinition } from "./types";

/**
 * Tag: the Tags that must all be on a Remote Asset (several Tags mean "all of them"). Only the Tags that
 * were chosen count; the children of a chosen Tag do not.
 */
export const tagFilter: SearchFilterDefinition = {
  key: "tagIds",
  apply({ tagIds }, request) {
    if (tagIds?.length) request.tagIds = [...tagIds];
  },
};
