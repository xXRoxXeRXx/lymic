<script lang="ts">
  import type { ComponentProps } from "svelte";
  import { AlertCircle } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import { SEARCH_RESULT_LIMIT, type SearchController } from "../controllers/search-controller.svelte";
  import BackBarView from "./BackBarView.svelte";
  import TimelineView from "./TimelineView.svelte";

  /**
   * The Search Results (ADR-0009): chronologically in the shared timeline view, or, for a Context text, as a
   * flat grid in the order of relevance that loads further pages while the user scrolls. With the Back control
   * to the view the Search was started from and the Search's own states: unsupported server, failures,
   * nothing matched and the result limit notice.
   */
  let { search, backLabel, onBack, timelineProps }: {
    search: SearchController;
    backLabel: string;
    onBack: () => void;
    /** What every timeline shares: server URL, Lightbox actions and album catalogue. */
    timelineProps: Partial<ComponentProps<typeof TimelineView>>;
  } = $props();

  const numberLocale = $derived(toBcp47Locale($locale));
  const relevance = $derived(search.presentation === "relevance");
  // The relevance presentation is its own timeline-shaped list; the chronological one is the search itself.
  const results = $derived(relevance ? search.context : search);
  // A further page is requested only while one can be loaded and the last request did not fail.
  const loadMore = $derived(relevance && search.context.hasNextPage && !search.context.loadMoreFailed ? () => void search.context.loadMore() : undefined);
  // An unusable API key, a missing permission or an unreadable response is an error over cached results too;
  // an unreachable server keeps the previous results with an offline status.
  const failureError = $derived.by(() => {
    switch (search.failure) {
      case "permission_denied": return $t("search_permission_denied");
      case "authentication_required": return $t("search_authentication_required");
      case "context_unavailable": return $t("search_context_unavailable");
      case "error": return $t("search_invalid_response");
      default: return null;
    }
  });
  const failureIsError = $derived(failureError !== null);
  const loadErrorMessage = $derived(failureError ?? $t("search_unavailable"));
  const hintMessage = $derived(
    search.truncated ? $t("search_truncated", { values: { count: SEARCH_RESULT_LIMIT.toLocaleString(numberLocale) } }) : "",
  );
</script>

<BackBarView label={backLabel} onBack={onBack}>
  {#if search.unsupported}
    <div class="flex items-start gap-3 rounded-xl border border-amber-200 bg-amber-50 p-4 text-sm text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-200" role="alert">
      <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
      <span>{$t("search_unsupported", { values: { required: search.unsupported.requiredVersion, version: search.unsupported.version } })}</span>
    </div>
  {:else}
    <div class="sr-only" aria-live="polite">
      {#if search.submittedText && !results.isLoading}{$t("search_results_count", { values: { count: search.resultCount.toLocaleString(numberLocale) } })}{/if}
    </div>
    <!-- A new result set starts at the top; a failed Search keeps the previous results where they are. -->
    {#key search.revision}
      <TimelineView
        timeline={results}
        {...timelineProps}
        ariaLabel={$t("search_title")}
        loadingMessage={$t("search_loading")}
        emptyMessage={$t("search_empty")}
        {loadErrorMessage}
        offlineMessage={$t("search_offline")}
        {failureIsError}
        {hintMessage}
        flat={relevance}
        onLoadMore={loadMore}
      >
        {#snippet footer()}
          {#if relevance && search.context.loadMoreFailed}
            <div role="alert" class="flex items-center gap-3 py-3 text-sm text-red-600 dark:text-red-300">
              <p>{$t("search_more_error")}</p>
              <button type="button" class="rounded-lg px-2 py-1 font-medium hover:bg-slate-200 dark:hover:bg-zinc-800" onclick={() => void search.context.loadMore()}>{$t("timeline_retry")}</button>
            </div>
          {:else if relevance && search.context.isLoadingMore}
            <p role="status" class="py-3 text-sm text-slate-500 dark:text-zinc-400">{$t("search_loading_more")}</p>
          {/if}
        {/snippet}
      </TimelineView>
    {/key}
  {/if}
</BackBarView>
