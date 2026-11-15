import type { Memory } from "../contracts";
import type { MemoriesDesktopAdapter } from "../tauri/desktop-adapter";
import type { ExploreError } from "./explore-error";
import { createExploreListController } from "./explore-list-controller.svelte";

export interface MemoriesViewState {
  readonly memories: Memory[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: ExploreError | null;
  refresh(): Promise<void>;
  clear(): void;
}

/** The Explore Overview shows at most this many Memories, newest first. */
export const MEMORIES_SECTION_SIZE = 12;

function memoryTime(memory: Memory): number {
  const time = memory.memoryAt ? Date.parse(memory.memoryAt) : Number.NaN;
  return Number.isNaN(time) ? Number.NEGATIVE_INFINITY : time;
}

/** Newest first and capped, in case the server ignores `size` and `order`. */
function newestMemories(result: unknown): Memory[] {
  if (!Array.isArray(result)) return [];
  return [...result].sort((a, b) => memoryTime(b) - memoryTime(a)).slice(0, MEMORIES_SECTION_SIZE);
}

export function createMemoriesController(adapter: MemoriesDesktopAdapter): MemoriesViewState {
  const list = createExploreListController({
    prefix: "memories",
    load: async () => newestMemories(await adapter.getMemories(MEMORIES_SECTION_SIZE)),
  });
  return {
    get memories() { return list.items; },
    get isLoading() { return list.isLoading; },
    get isOffline() { return list.isOffline; },
    get error() { return list.error; },
    refresh: list.refresh,
    clear: list.clear,
  };
}
