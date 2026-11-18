import type { ViewId } from "./navigation-controller.svelte";

/** An open view and the scroll position to return to; Back from a Search or a Tag Timeline returns to one. */
export interface ViewPosition {
  view: ViewId;
  top: number;
}

/** The ids of the Tags views, shared by the page and the Tag origin so neither repeats the literal. */
export const TAGS_VIEW = "tags";
export const TAG_TIMELINE_VIEW = "tag-timeline";
