import type { TimelineBucket } from "../contracts";
import type { TimelineDesktopAdapter } from "../tauri/desktop-adapter";

export interface TimelineViewState {
  readonly buckets: TimelineBucket[];
  readonly isLoading: boolean;
  refresh(): Promise<void>;
  clear(): void;
}

export function createTimelineController(adapter: TimelineDesktopAdapter): TimelineViewState {
  let buckets = $state<TimelineBucket[]>([]);
  let isLoading = $state(false);
  let latestRequest = 0;

  async function refresh() {
    const request = ++latestRequest;
    isLoading = true;
    try {
      const loaded = await adapter.getTimelineBuckets();
      if (request === latestRequest) {
        buckets = [...loaded].sort((a, b) => b.timeBucket.localeCompare(a.timeBucket));
      }
    } finally {
      if (request === latestRequest) isLoading = false;
    }
  }

  function clear() {
    latestRequest += 1;
    buckets = [];
    isLoading = false;
  }

  return {
    get buckets() { return buckets; },
    get isLoading() { return isLoading; },
    refresh,
    clear,
  };
}
