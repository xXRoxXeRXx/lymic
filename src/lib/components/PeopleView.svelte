<script lang="ts">
  import { RefreshCw } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { RecognizedPerson } from "../contracts";
  import RecognizedPersonTile from "./RecognizedPersonTile.svelte";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import { messageForFailure } from "../controllers/request-failure";
  let { people, onSelect }: { people: PeopleViewState; onSelect: (person: RecognizedPerson) => void } = $props();
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
  {:else if people.error}<div role="alert"><p>{messageForFailure(people.error, { permission_denied: $t("people_permission_denied"), unavailable: $t("people_unavailable"), error: $t("people_error") })}</p><button type="button" onclick={() => void people.refresh()}>{$t("people_retry")}</button></div>
  {:else if people.people.length === 0}<p>{$t("people_empty")}</p>
  {:else}<div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
    {#each people.people as person (person.id)}
      <RecognizedPersonTile {person} {onSelect} />
    {/each}
  </div>
  {#if people.hasNextPage}<div use:loadMoreWhenVisible class="h-px" aria-hidden="true"></div>{/if}
  {/if}
</div>
