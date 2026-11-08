<script lang="ts">
  import { RefreshCw, UserRound } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  let { people }: { people: PeopleViewState } = $props();
  let failedThumbnailIds = $state(new Set<string>());

  function thumbnailSrc(personId: string) {
    const encodedId = encodeURIComponent(personId);
    const convertFileSrc = (
      globalThis as typeof globalThis & {
        __TAURI_INTERNALS__?: { convertFileSrc?: (filePath: string, protocol?: string) => string };
      }
    ).__TAURI_INTERNALS__?.convertFileSrc;
    if (typeof convertFileSrc === "function") {
      return `${convertFileSrc("", "lymic-thumb").replace(/\/$/, "")}/person/${encodedId}`;
    }
    return `lymic-thumb://person/${encodedId}`;
  }

  function hideThumbnail(personId: string) {
    failedThumbnailIds = new Set(failedThumbnailIds).add(personId);
  }

  function loadMoreWhenVisible(node: HTMLElement) {
    if (typeof IntersectionObserver === "undefined") return {};
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void people.loadMore();
    }, { root: node.closest("main"), rootMargin: "400px 0px" });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4" aria-label={$t("people_title")}>
  <div class="flex items-center justify-end gap-3">
    {#if people.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("people_offline")}</p>{/if}
    <button type="button" class="rounded-lg p-2 text-slate-500 hover:bg-slate-200 hover:text-slate-900 disabled:cursor-wait disabled:opacity-60 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-100" aria-label={$t("people_refresh")} aria-busy={people.isRefreshing} disabled={people.isRefreshing} onclick={() => void people.refresh()}><RefreshCw class={`h-4 w-4 ${people.isRefreshing ? "animate-spin" : ""}`} /></button>
  </div>
  {#if people.isLoading}<p role="status">{$t("people_loading")}</p>
  {:else if people.error === "unavailable"}<div role="alert"><p>{$t("people_unavailable")}</p><button type="button" onclick={() => void people.refresh()}>{$t("people_retry")}</button></div>
  {:else if people.error}<div role="alert"><p>{$t("people_error")}</p><button type="button" onclick={() => void people.refresh()}>{$t("people_retry")}</button></div>
  {:else if people.people.length === 0}<p>{$t("people_empty")}</p>
  {:else}<div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
    {#each people.people as person (person.id)}
      <button type="button" class="rounded-xl border border-slate-200 p-4 text-left dark:border-zinc-700" aria-label={person.name ?? $t("people_unnamed")} onclick={() => people.onSelect?.(person)}>
        <div class="mb-3 flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
          {#if person.thumbnailPath && !failedThumbnailIds.has(person.id)}
            <img class="h-full w-full object-cover" src={thumbnailSrc(person.id)} alt={person.name ?? $t("people_unnamed")} onerror={() => hideThumbnail(person.id)} />
          {:else}
            <UserRound class="h-12 w-12 text-slate-400" />
          {/if}
        </div>
        <h2 class="truncate font-semibold">{person.name ?? $t("people_unnamed")}</h2>
      </button>
    {/each}
  </div>
  {#if people.hasNextPage}<div use:loadMoreWhenVisible class="h-px" aria-hidden="true"></div>{/if}
  {/if}
</div>
