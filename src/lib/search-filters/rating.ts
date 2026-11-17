import type { SearchFilterDefinition } from "./types";

/** Rating: one exact star level, one to five. */
export const ratingFilter: SearchFilterDefinition = {
  key: "rating",
  apply({ rating }, request) {
    if (rating) request.rating = rating;
  },
  summary: ({ rating }) => (rating ? `${rating} ★` : null),
};
