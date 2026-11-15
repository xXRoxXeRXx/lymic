<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "svelte-i18n";
  import { isExploreSectionVisible, type ExploreSectionState } from "../controllers/explore-controller.svelte";

  /**
   * The independent-section pattern of the Explore Overview. The section renders
   * its own loading, error (with retry), offline indication and content, and
   * renders nothing at all while it is empty without an error.
   */
  let {
    id,
    title,
    section,
    loadingMessage,
    errorMessage,
    unavailableMessage = errorMessage,
    offlineMessage,
    onViewAll,
    children,
  }: {
    id: string;
    title: string;
    section: ExploreSectionState;
    loadingMessage: string;
    errorMessage: string;
    unavailableMessage?: string;
    offlineMessage: string;
    onViewAll: () => void;
    children: Snippet;
  } = $props();
</script>

{#if isExploreSectionVisible(section)}
  <section class="flex flex-col gap-3" aria-labelledby="explore-{id}-title">
    <div class="flex items-center justify-between gap-3">
      <h2 id="explore-{id}-title" class="text-base font-semibold text-slate-900 dark:text-slate-100">{title}</h2>
      <div class="flex items-center gap-3">
        {#if section.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{offlineMessage}</p>{/if}
        <button type="button" class="rounded-lg px-2 py-1 text-sm font-medium text-blue-600 hover:bg-slate-200 dark:text-sky-400 dark:hover:bg-zinc-800" onclick={onViewAll}>{$t("explore_view_all")}</button>
      </div>
    </div>
    {#if section.error}
      <div role="alert" class="flex items-center gap-3 text-sm text-red-600 dark:text-red-300">
        <p>{section.error === "unavailable" ? unavailableMessage : errorMessage}</p>
        <button type="button" class="rounded-lg px-2 py-1 font-medium hover:bg-slate-200 dark:hover:bg-zinc-800" onclick={() => void section.refresh()}>{$t("explore_retry")}</button>
      </div>
    {:else if section.isLoading}
      <p role="status" class="text-sm text-slate-500 dark:text-zinc-400">{loadingMessage}</p>
    {:else}
      {@render children()}
    {/if}
  </section>
{/if}
