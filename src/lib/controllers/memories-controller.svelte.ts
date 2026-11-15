import type { Memory } from "../contracts";
import type { MemoriesDesktopAdapter } from "../tauri/desktop-adapter";
import { classifyExploreError, type ExploreError } from "./explore-error";

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
  let memories = $state<Memory[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let error = $state<ExploreError | null>(null);
  let generation = 0;

  async function refresh() {
    const requestGeneration = ++generation;
    isLoading = memories.length === 0;
    try {
      const result = await adapter.getMemories(MEMORIES_SECTION_SIZE);
      if (requestGeneration !== generation) return;
      memories = newestMemories(result);
      error = null;
      isOffline = false;
    } catch (reason) {
      if (requestGeneration !== generation) return;
      error = memories.length === 0 ? classifyExploreError(reason, "memories") : null;
      isOffline = memories.length > 0;
    } finally {
      if (requestGeneration === generation) isLoading = false;
    }
  }

  function clear() {
    generation += 1;
    memories = [];
    isLoading = false;
    isOffline = false;
    error = null;
  }

  return {
    get memories() { return memories; },
    get isLoading() { return isLoading; },
    get isOffline() { return isOffline; },
    get error() { return error; },
    refresh,
    clear,
  };
}
