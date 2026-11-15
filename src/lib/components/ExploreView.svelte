<script lang="ts">
  import { t, locale } from "svelte-i18n";
  import type { Memory, RecognizedPerson } from "../contracts";
  import type { MemoriesViewState } from "../controllers/memories-controller.svelte";
  import { memoryTitle } from "../memory-title";
  import MemoryCard from "./MemoryCard.svelte";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { ExploreViewState } from "../controllers/explore-controller.svelte";
  import ExploreSection from "./ExploreSection.svelte";
  import RecognizedPersonTile from "./RecognizedPersonTile.svelte";

  let { explore, people, memories, onViewAllPeople, onSelectPerson, onViewAllMemories = () => {}, onSelectMemory }: {
    explore: ExploreViewState;
    people: PeopleViewState;
    memories: MemoriesViewState;
    onViewAllPeople: () => void;
    onSelectPerson: (person: RecognizedPerson) => void;
    /** Inert until the All Memories view exists. */
    onViewAllMemories?: () => void;
    onSelectMemory: (memory: Memory) => void;
  } = $props();

  const translate = (key: string, values?: Record<string, string | number>) => $t(key, { values });

  // One row of tiles: as many as fit the measured width, with a fixed fallback when unmeasured.
  const TILE_MIN_WIDTH = 144;
  const TILE_GAP = 16;
  const FALLBACK_COLUMNS = 6;
  let peopleColumns = $state(FALLBACK_COLUMNS);

  function measureColumns(node: HTMLElement) {
    if (typeof ResizeObserver === "undefined") return {};
    const observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0;
      if (width > 0) peopleColumns = Math.max(1, Math.floor((width + TILE_GAP) / (TILE_MIN_WIDTH + TILE_GAP)));
    });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<div class="flex flex-col gap-8" aria-label={$t("explore_title")}>
  <ExploreSection
    id="people"
    title={$t("people_title")}
    section={explore.people}
    loadingMessage={$t("people_loading")}
    errorMessage={$t("people_error")}
    unavailableMessage={$t("people_unavailable")}
    offlineMessage={$t("people_offline")}
    onViewAll={onViewAllPeople}
  >
    <div use:measureColumns class="grid gap-4" style="grid-template-columns: repeat({peopleColumns}, minmax(0, 1fr));">
      {#each people.people.slice(0, peopleColumns) as person (person.id)}
        <RecognizedPersonTile {person} onSelect={onSelectPerson} />
      {/each}
    </div>
  </ExploreSection>

  <ExploreSection
    id="memories"
    title={$t("memories_title")}
    section={explore.memories}
    loadingMessage={$t("memories_loading")}
    errorMessage={$t("memories_error")}
    unavailableMessage={$t("memories_unavailable")}
    offlineMessage={$t("memories_offline")}
    onViewAll={onViewAllMemories}
  >
    <div class="grid gap-4" style="grid-template-columns: repeat(auto-fill, minmax(176px, 1fr));">
      {#each memories.memories as memory (memory.id)}
        <MemoryCard {memory} title={memoryTitle(memory, translate, { locale: $locale ?? "en" })} onSelect={onSelectMemory} />
      {/each}
    </div>
  </ExploreSection>

  {#if explore.isAllEmpty}
    <p class="py-12 text-center text-sm text-slate-500 dark:text-zinc-400">{$t("explore_empty")}</p>
  {/if}
</div>
