import type { Memory } from "../contracts";
import type { MemoriesDesktopAdapter } from "../tauri/desktop-adapter";

export interface MemoriesViewState {
  readonly memories: Memory[];
  readonly isLoading: boolean;
  readonly isOffline: boolean;
  readonly error: "unavailable" | "error" | null;
  refresh(): Promise<void>;
  clear(): void;
}

/** The Explore Overview shows at most this many Memories, newest first. */
export const MEMORIES_SECTION_SIZE = 12;

function classifyError(reason: unknown): "unavailable" | "error" {
  const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  return message.startsWith("memories_permission_denied:") ? "unavailable" : "error";
}

export function createMemoriesController(adapter: MemoriesDesktopAdapter): MemoriesViewState {
  let memories = $state<Memory[]>([]);
  let isLoading = $state(false);
  let isOffline = $state(false);
  let error = $state<"unavailable" | "error" | null>(null);
  let generation = 0;

  async function refresh() {
    const requestGeneration = ++generation;
    isLoading = memories.length === 0;
    try {
      const result = await adapter.getMemories(MEMORIES_SECTION_SIZE);
      if (requestGeneration !== generation) return;
      memories = Array.isArray(result) ? result : [];
      error = null;
      isOffline = false;
    } catch (reason) {
      if (requestGeneration !== generation) return;
      error = memories.length === 0 ? classifyError(reason) : null;
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
