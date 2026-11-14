import type { AlbumTimelineDesktopAdapter } from "../tauri/desktop-adapter";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/**
 * Adapts one complete album search result to the shared timeline interface.
 * The album request completes before its bucket index is published; activated
 * months then read from that already complete result rather than the network.
 */
export function createAlbumTimelineController(
  adapter: AlbumTimelineDesktopAdapter,
  albumId: string,
): TimelineController {
  return createTimelineController({
    getTimeline: () => adapter.getAlbumTimeline(albumId),
  });
}
