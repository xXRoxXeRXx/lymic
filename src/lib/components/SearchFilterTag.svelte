<script lang="ts">
  import { t } from "svelte-i18n";
  import type { Tag } from "../contracts";
  import { messageForFailure } from "../controllers/request-failure";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import SearchFilterHeading from "./SearchFilterHeading.svelte";
  import TagTree from "./TagTree.svelte";

  /**
   * The Tag Search Filter: choose one or more Tags from the Tag hierarchy; a Search returns the Remote Assets
   * that carry all of them. Only the chosen Tags count, not their children. The Tags come from the same
   * source as the Tags view. A key without `tag.read` disables only this filter and says which permission
   * is missing.
   */
  let { search, tags }: { search: SearchController; tags: TagsViewState } = $props();

  const chosen = $derived(search.filters.tagIds ?? []);
  let requested = false;

  // Reads the Tags once per opening of the panel, and only when the Tags view has not loaded them already.
  $effect(() => {
    if (requested || !tags.isEmpty || tags.isLoading || tags.error) return;
    requested = true;
    void tags.refresh();
  });

  function toggle(tag: Tag) {
    const next = chosen.includes(tag.id) ? chosen.filter((id) => id !== tag.id) : [...chosen, tag.id];
    search.filters.tagIds = next.length > 0 ? next : undefined;
  }
</script>

<div class="space-y-2">
  <SearchFilterHeading id="search-filter-tag-label" text={$t("search_filter_tag")} />
  <p class="text-xs text-slate-500 dark:text-zinc-400">{$t("search_filter_tag_hint")}</p>
  {#if tags.isLoading}
    <p role="status" class="text-sm text-slate-600 dark:text-zinc-300">{$t("tags_loading")}</p>
  {:else if tags.error}
    <div role="alert" class="space-y-1 text-sm text-slate-700 dark:text-zinc-300">
      <p>{messageForFailure(tags.error, { permission_denied: $t("search_filter_tag_permission_denied"), unavailable: $t("tags_unavailable"), error: $t("tags_error") })}</p>
      <button type="button" class="rounded text-xs font-medium text-blue-600 hover:underline dark:text-sky-400" onclick={() => void tags.refresh()}>{$t("tags_retry")}</button>
    </div>
  {:else if tags.isEmpty}
    <p class="text-sm text-slate-600 dark:text-zinc-300">{$t("tags_empty")}</p>
  {:else}
    <div class="max-h-64 overflow-y-auto p-1">
      <TagTree {tags} onSelect={toggle} selected={chosen} />
    </div>
  {/if}
</div>
