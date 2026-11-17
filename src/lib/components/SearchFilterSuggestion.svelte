<script lang="ts">
  import { X } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { SearchSuggestionsRequest } from "../contracts";
  import type { SuggestionsState } from "../controllers/search-suggestions.svelte";
  import type { SearchController } from "../controllers/search-controller.svelte";

  /**
   * One value chosen only from Immich's suggestions. Immich compares these values exactly, so the field
   * filters the suggestion list as the user types but never applies the typed text: only a suggestion that is
   * clicked, or highlighted with the arrow keys and chosen with Enter, becomes the value. A failure to read
   * the suggestions is shown here, in this filter, with Retry; nothing else in Search is affected.
   * Location filters reuse this component with their own suggestion `request`; the city takes its names from a
   * `source` of its own (the city listing of the Explore Places) instead of Immich's suggestions.
   */
  let { search, request, source, label, value, onChange, id }: {
    search: SearchController;
    /** What to suggest from Immich's suggestions, including narrowing (for example the chosen make for models). Not used with a `source`. */
    request?: SearchSuggestionsRequest;
    /** Another list to choose from, read lazily the first time the field is used; used instead of `request`. */
    source?: { readonly state: SuggestionsState; load(): Promise<void> };
    /** The translated name of the filter. */
    label: string;
    /** The chosen suggestion, if any. */
    value: string | undefined;
    onChange: (value: string | undefined) => void;
    /** A unique prefix for the element ids of this field. */
    id: string;
  } = $props();

  let query = $state("");
  let open = $state(false);
  let active = $state(-1);
  let input = $state<HTMLInputElement | null>(null);

  const suggestions = $derived(source ? source.state : search.suggestions.stateFor(request ?? { type: "country" }));
  const shown = $derived.by(() => {
    const typed = query.trim().toLowerCase();
    return typed === "" ? suggestions.items : suggestions.items.filter((item) => item.toLowerCase().includes(typed));
  });
  const listId = $derived(`${id}-list`);
  const errorId = $derived(`${id}-error`);
  const hasList = $derived(open && suggestions.status === "ready" && shown.length > 0);

  function openList() {
    open = true;
    if (source) void source.load();
    else if (request) void search.suggestions.load(request);
  }

  function close() {
    open = false;
    active = -1;
  }

  function choose(suggestion: string) {
    onChange(suggestion);
    query = "";
    close();
  }

  function retry() {
    // Focusing the field opens the list and reads the suggestions again.
    input?.focus();
    openList();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) openList();
      if (shown.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      active = (active + step + shown.length) % shown.length;
    } else if (event.key === "Enter") {
      // The field sits inside the search form; Enter must never submit the Search from here.
      event.preventDefault();
      if (hasList && active >= 0 && shown[active] !== undefined) choose(shown[active]);
    } else if (event.key === "Escape" && open) {
      event.stopPropagation();
      close();
    }
  }
</script>

<div class="space-y-1">
  {#if value !== undefined}
    <div class="flex items-center justify-between gap-2 rounded-lg border border-slate-200/80 px-3 py-1.5 text-sm text-slate-800 dark:border-zinc-700 dark:text-zinc-100">
      <span class="min-w-0 truncate"><span class="text-slate-500 dark:text-zinc-400">{label}:</span> {value}</span>
      <button
        type="button"
        class="shrink-0 rounded p-0.5 text-slate-400 hover:text-slate-600 dark:hover:text-zinc-200"
        aria-label={$t("search_suggestions_clear", { values: { filter: label } })}
        title={$t("search_suggestions_clear", { values: { filter: label } })}
        onclick={() => onChange(undefined)}
      >
        <X class="h-4 w-4" aria-hidden="true" />
      </button>
    </div>
  {:else}
    <input
      bind:this={input}
      type="text"
      role="combobox"
      class="glass-input !py-1.5 text-sm"
      aria-label={label}
      placeholder={label}
      aria-autocomplete="list"
      aria-expanded={hasList}
      aria-controls={listId}
      aria-activedescendant={hasList && active >= 0 ? `${id}-option-${active}` : undefined}
      aria-invalid={suggestions.status === "error" ? "true" : undefined}
      aria-describedby={suggestions.status === "error" ? errorId : undefined}
      autocomplete="off"
      spellcheck="false"
      value={query}
      onfocus={openList}
      onblur={() => { query = ""; close(); }}
      oninput={(event) => { query = event.currentTarget.value; active = -1; if (!open) openList(); }}
      onkeydown={onKeydown}
    />
    {#if open}
      {#if suggestions.status === "loading"}
        <p role="status" class="text-sm text-slate-500 dark:text-zinc-400">{$t("search_suggestions_loading")}</p>
      {:else if suggestions.status === "ready"}
        {#if shown.length > 0}
          <ul id={listId} role="listbox" aria-label={label} class="max-h-40 overflow-y-auto rounded-lg border border-slate-200/80 bg-white py-1 dark:border-zinc-700 dark:bg-zinc-900">
            {#each shown as suggestion, index (suggestion)}
              <!-- Mouse down must not blur the field before the click chooses the suggestion. The keyboard operates the options through the combobox field (aria-activedescendant), not through focus on the options. -->
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
              <li
                id="{id}-option-{index}"
                role="option"
                aria-selected={index === active}
                class="cursor-pointer px-3 py-1 text-sm text-slate-800 dark:text-zinc-100 {index === active ? 'bg-blue-50 dark:bg-zinc-800' : 'hover:bg-slate-100 dark:hover:bg-zinc-800'}"
                onmousedown={(event) => event.preventDefault()}
                onclick={() => choose(suggestion)}
              >{suggestion}</li>
            {/each}
          </ul>
        {:else if suggestions.items.length > 0}
          <p class="text-sm text-slate-500 dark:text-zinc-400">{$t("search_suggestions_no_match")}</p>
        {:else}
          <p class="text-sm text-slate-500 dark:text-zinc-400">{$t("search_suggestions_empty")}</p>
        {/if}
      {/if}
    {/if}
    {#if suggestions.status === "error"}
      <div id={errorId} role="alert" class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-red-600 dark:text-red-400">
        <span>{$t(suggestions.failure === "permission_denied" ? "search_suggestions_permission_denied" : "search_suggestions_failed")}</span>
        <button type="button" class="rounded font-medium underline" onclick={retry}>{$t("search_suggestions_retry")}</button>
      </div>
    {/if}
  {/if}
</div>
