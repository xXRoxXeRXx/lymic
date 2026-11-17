/** A view and the scroll position to return to; the page records one for the open Tag Timeline. */
export interface TagOrigin {
  view: string;
  top: number;
}

/** Where Back goes from a Tag Timeline opened from the Tags tree. */
export const defaultTagOrigin: TagOrigin = { view: "tags", top: 0 };

/**
 * The origin after a Tag Timeline is opened from `from`: that view, unless it is itself a Tag Timeline, so a
 * chain of Tag Timelines (Lightbox chip to chip) keeps the origin where the chain started.
 */
export function recordTagOrigin(current: TagOrigin, from: TagOrigin): TagOrigin {
  return from.view === "tag-timeline" ? current : from;
}
