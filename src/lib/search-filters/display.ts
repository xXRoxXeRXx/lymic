import type { SearchFilterDefinition } from "./types";

/** Favorite: only Favorites. A switched-off option adds no condition. */
export const favoriteFilter: SearchFilterDefinition = {
  key: "favorite",
  apply({ favorite }, request) {
    if (favorite) request.favorite = true;
  },
};

/** Archived: only Archived Assets. Without it the backend leaves Archived Assets out, so unset sends nothing. */
export const archivedFilter: SearchFilterDefinition = {
  key: "archived",
  apply({ archived }, request) {
    if (archived) request.archived = true;
  },
};

/** Unfiled Asset: only Remote Assets in no Album. */
export const unfiledFilter: SearchFilterDefinition = {
  key: "unfiled",
  apply({ unfiled }, request) {
    if (unfiled) request.unfiled = true;
  },
};
