<script lang="ts">
  import { t } from "svelte-i18n";
  import type { SearchController } from "../controllers/search-controller.svelte";

  /** The Date Search Filter: an optional first and last local day, both inclusive. */
  let { search }: { search: SearchController } = $props();

  const date = $derived(search.filters.date);
  const error = $derived(search.filterErrors.date);

  function setBound(bound: "from" | "to", value: string) {
    const next = { ...search.filters.date, [bound]: value || undefined };
    // A range with neither bound is no filter at all.
    search.filters.date = next.from || next.to ? next : undefined;
  }
</script>

<div role="group" aria-labelledby="search-filter-date-label" class="space-y-2">
  <div class="flex items-center justify-between">
    <span id="search-filter-date-label" class="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{$t("search_filter_date")}</span>
    {#if date}
      <button
        type="button"
        class="rounded text-xs font-medium text-blue-600 hover:underline dark:text-sky-400"
        onclick={() => { search.filters.date = undefined; }}
      >{$t("search_filter_date_clear")}</button>
    {/if}
  </div>
  <div class="flex flex-wrap items-center gap-3">
    <label class="flex items-center gap-2 text-sm text-slate-700 dark:text-zinc-300">
      <span>{$t("search_filter_date_from")}</span>
      <input
        type="date"
        class="glass-input !py-1.5 text-sm"
        value={date?.from ?? ""}
        aria-invalid={error ? "true" : undefined}
        aria-describedby={error ? "search-filter-date-error" : undefined}
        oninput={(event) => setBound("from", event.currentTarget.value)}
      />
    </label>
    <label class="flex items-center gap-2 text-sm text-slate-700 dark:text-zinc-300">
      <span>{$t("search_filter_date_to")}</span>
      <input
        type="date"
        class="glass-input !py-1.5 text-sm"
        value={date?.to ?? ""}
        aria-invalid={error ? "true" : undefined}
        aria-describedby={error ? "search-filter-date-error" : undefined}
        oninput={(event) => setBound("to", event.currentTarget.value)}
      />
    </label>
  </div>
  {#if error}
    <p id="search-filter-date-error" role="alert" class="text-sm text-red-600 dark:text-red-400">{$t(error)}</p>
  {/if}
</div>
