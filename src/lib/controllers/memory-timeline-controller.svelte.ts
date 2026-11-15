import type { Memory } from "../contracts";
import { createTimelineController, type TimelineController } from "./timeline-controller.svelte";

/**
 * Adapts one Memory to the shared timeline interface. Immich returns a Memory's
 * assets with the Memory itself, so the timeline reads that already complete
 * grouping rather than the network.
 */
export function createMemoryTimelineController(memory: Memory): TimelineController {
  return createTimelineController({
    getTimeline: () => Promise.resolve(memory.timeline),
  });
}
