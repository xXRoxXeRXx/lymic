<script lang="ts">
  import { RefreshCw, UserRound } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  let { people }: { people: PeopleViewState } = $props();
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4" aria-label={$t("people_title")}>
  <div class="flex items-center justify-end gap-3">
    {#if people.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("people_offline")}</p>{/if}
    <button type="button" class="rounded-lg p-2" aria-label={$t("people_refresh")} aria-busy={people.isRefreshing} disabled={people.isRefreshing} onclick={() => void people.refresh()}><RefreshCw class={`h-4 w-4 ${people.isRefreshing ? "animate-spin" : ""}`} /></button>
  </div>
  {#if people.isLoading}<p role="status">{$t("people_loading")}</p>
  {:else if people.error === "unavailable"}<div role="alert"><p>{$t("people_unavailable")}</p><button type="button" onclick={() => void people.refresh()}>{$t("people_retry")}</button></div>
  {:else if people.error}<div role="alert"><p>{$t("people_error")}</p><button type="button" onclick={() => void people.refresh()}>{$t("people_retry")}</button></div>
  {:else if people.people.length === 0}<p>{$t("people_empty")}</p>
  {:else}<div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
    {#each people.people as person (person.id)}<article class="rounded-xl border border-slate-200 p-4 dark:border-zinc-700"><div class="mb-3 flex aspect-square items-center justify-center rounded-lg bg-slate-100 dark:bg-zinc-800"><UserRound class="h-12 w-12 text-slate-400" /></div><h2 class="truncate font-semibold">{person.name ?? $t("people_unnamed")}</h2><p class="text-sm text-slate-500">{$t("people_asset_count", { values: { count: person.assetCount } })}</p></article>{/each}
  </div>
  {#if people.hasNextPage}<button type="button" class="self-center rounded-lg border px-4 py-2" onclick={() => void people.loadMore()}>{$t("people_load_more")}</button>{/if}
  {/if}
</div>
