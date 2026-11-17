<script lang="ts">
  import { t } from "svelte-i18n";
  import type { RecognizedPerson } from "../contracts";
  import type { PeopleViewState } from "../controllers/people-controller.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import RecognizedPersonTile from "./RecognizedPersonTile.svelte";

  /**
   * The Person Search Filter: choose one or more Recognized People, named ones first; a Search returns the
   * Remote Assets in which all of them appear. The people come from the same source as the People view. A
   * key without `person.read` disables only this filter and says which permission is missing.
   */
  let { search, people }: { search: SearchController; people: PeopleViewState } = $props();

  // Named people first, unnamed ones after; each group keeps the order the server gave.
  const ordered = $derived([
    ...people.people.filter((person) => person.name),
    ...people.people.filter((person) => !person.name),
  ]);
  const chosen = $derived(search.filters.personIds ?? []);

  $effect(() => {
    if (people.people.length === 0 && !people.isLoading && !people.error) void people.refresh();
  });

  function toggle(person: RecognizedPerson) {
    const next = chosen.includes(person.id) ? chosen.filter((id) => id !== person.id) : [...chosen, person.id];
    search.filters.personIds = next.length > 0 ? next : undefined;
  }
</script>

<div class="space-y-2">
  <span id="search-filter-person-label" class="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{$t("search_filter_person")}</span>
  <p class="text-xs text-slate-500 dark:text-zinc-400">{$t("search_filter_person_hint")}</p>
  {#if people.isLoading}
    <p role="status" class="text-sm text-slate-600 dark:text-zinc-300">{$t("people_loading")}</p>
  {:else if people.error}
    <div role="alert" class="space-y-1 text-sm text-slate-700 dark:text-zinc-300">
      <p>{$t(people.error === "permission_denied" ? "search_filter_person_permission_denied" : people.error === "unavailable" ? "people_unavailable" : "people_error")}</p>
      <button type="button" class="rounded text-xs font-medium text-blue-600 hover:underline dark:text-sky-400" onclick={() => void people.refresh()}>{$t("people_retry")}</button>
    </div>
  {:else if ordered.length === 0}
    <p class="text-sm text-slate-600 dark:text-zinc-300">{$t("people_empty")}</p>
  {:else}
    <div role="group" aria-labelledby="search-filter-person-label" class="grid max-h-64 grid-cols-3 gap-2 overflow-y-auto p-1 sm:grid-cols-4 lg:grid-cols-6">
      {#each ordered as person (person.id)}
        <RecognizedPersonTile {person} compact selected={chosen.includes(person.id)} onSelect={toggle} />
      {/each}
    </div>
    {#if people.hasNextPage}
      <button type="button" class="rounded text-xs font-medium text-blue-600 hover:underline dark:text-sky-400" onclick={() => void people.loadMore()}>{$t("people_load_more")}</button>
    {/if}
  {/if}
</div>
