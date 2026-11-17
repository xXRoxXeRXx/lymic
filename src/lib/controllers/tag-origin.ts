import { TAG_TIMELINE_VIEW, TAGS_VIEW, type ViewPosition } from "./view-position";

/** Where Back goes from a Tag Timeline opened from the Tags tree. */
export const defaultTagOrigin: ViewPosition = { view: TAGS_VIEW, top: 0 };

/**
 * The origin after a Tag Timeline is opened from `from`: that view, unless it is itself a Tag Timeline, so a
 * chain of Tag Timelines (Lightbox chip to chip) keeps the origin where the chain started.
 */
export function recordTagOrigin(current: ViewPosition, from: ViewPosition): ViewPosition {
  return from.view === TAG_TIMELINE_VIEW ? current : from;
}
