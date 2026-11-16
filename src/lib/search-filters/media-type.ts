import type { SearchFilterDefinition } from "./types";

/** Media type: Images or Videos; unset means all. */
export const mediaTypeFilter: SearchFilterDefinition = {
  key: "mediaType",
  apply({ mediaType }, request) {
    if (mediaType) request.mediaType = mediaType;
  },
};
