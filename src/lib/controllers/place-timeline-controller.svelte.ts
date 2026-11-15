import type { PlaceTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/**
 * Adapts one complete city-filtered search result to the shared timeline
 * interface, exactly as the Album Timeline does (ADR-0006, ADR-0007): the
 * bucket index is published only after every result page loaded.
 */
export function createPlaceTimelineController(
  adapter: PlaceTimelineDesktopAdapter,
  city: string,
): TimelineController {
  return createTimelineController({
    getTimeline: () => adapter.getPlaceTimeline(city),
  });
}
