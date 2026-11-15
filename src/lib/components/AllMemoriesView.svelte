<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import type { Memory } from "../contracts";
  import type { AllMemoriesViewState } from "../controllers/all-memories-controller.svelte";
  import { localizedMemoryTitle } from "../memory-title";
  import MemoryCard from "./MemoryCard.svelte";

  /** "All Memories": every Memory in a grid that loads further pages on scroll. It has no navigation entry; Explore opens it. */
  let { allMemories, onSelect }: { allMemories: AllMemoriesViewState; onSelect: (memory: Memory) => void } = $props();

  function loadMoreWhenVisible(node: HTMLElement) {
    if (typeof IntersectionObserver === "undefined") return {};
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void allMemories.loadMore();
    }, { root: node.closest("main"), rootMargin: "400px 0px" });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4" aria-label={$t("memories_all_title")}>
  {#if allMemories.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("memories_offline")}</p>{/if}
  {#if allMemories.error}
    <div role="alert" class="flex items-center gap-3 text-sm text-red-600 dark:text-red-300">
      <p>{allMemories.error === "permission_denied" ? $t("memories_permission_denied") : allMemories.error === "unavailable" ? $t("memories_unavailable") : $t("memories_error")}</p>
      <button type="button" class="rounded-lg px-2 py-1 font-medium hover:bg-slate-200 dark:hover:bg-zinc-800" onclick={() => void allMemories.refresh()}>{$t("explore_retry")}</button>
    </div>
  {:else if allMemories.isLoading}<p role="status">{$t("memories_loading")}</p>
  {:else if allMemories.memories.length === 0}<p>{$t("memories_all_empty")}</p>
  {:else}
    <div class="grid gap-4" style="grid-template-columns: repeat(auto-fill, minmax(176px, 1fr));">
      {#each allMemories.memories as memory (memory.id)}
        <MemoryCard {memory} title={localizedMemoryTitle(memory, $t, $locale)} {onSelect} />
      {/each}
    </div>
    {#if allMemories.loadMoreFailed}
      <div role="alert" class="flex items-center gap-3 text-sm text-red-600 dark:text-red-300">
        <p>{$t("memories_more_error")}</p>
        <button type="button" class="rounded-lg px-2 py-1 font-medium hover:bg-slate-200 dark:hover:bg-zinc-800" onclick={() => void allMemories.loadMore()}>{$t("explore_retry")}</button>
      </div>
    {:else if allMemories.hasNextPage}
      {#if allMemories.isLoadingMore}<p role="status" class="text-sm text-slate-500 dark:text-zinc-400">{$t("memories_loading_more")}</p>{/if}
      <div use:loadMoreWhenVisible class="h-px" aria-hidden="true"></div>
    {/if}
  {/if}
</div>
