<script lang="ts">
  import { tick } from "svelte";
  import { ChevronRight } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import { validTagColor, type TagRow } from "../controllers/tags-tree";

  let { tags }: { tags: TagsViewState } = $props();

  let tree = $state<HTMLElement>();
  /** The Tag that holds the tree's single tab stop (roving focus). */
  let focusedId = $state<string | null>(null);
  const tabStopId = $derived(tags.rows.some((row) => row.tag.id === focusedId) ? focusedId : (tags.rows[0]?.tag.id ?? null));

  async function focusRow(row: TagRow | undefined) {
    if (!row) return;
    focusedId = row.tag.id;
    await tick();
    const items = [...(tree?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? [])];
    items.find((element) => element.dataset.tagId === row.tag.id)?.focus();
  }

  function onKeyDown(event: KeyboardEvent, row: TagRow, index: number) {
    const rows = tags.rows;
    switch (event.key) {
      case "ArrowDown": void focusRow(rows[index + 1]); break;
      case "ArrowUp": void focusRow(rows[index - 1]); break;
      case "Home": void focusRow(rows[0]); break;
      case "End": void focusRow(rows[rows.length - 1]); break;
      case "ArrowRight":
        if (row.hasChildren && !row.expanded) tags.setExpanded(row.tag.id, true);
        else if (row.expanded) void focusRow(rows[index + 1]);
        break;
      case "ArrowLeft":
        if (row.expanded) tags.setExpanded(row.tag.id, false);
        else void focusRow(rows.find((candidate) => candidate.tag.id === row.parentId));
        break;
      case "Enter":
      case " ":
        tags.toggle(row.tag.id);
        break;
      default: return;
    }
    event.preventDefault();
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4">
  {#if tags.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("tags_offline")}</p>{/if}
  {#if tags.isLoading}<p role="status">{$t("tags_loading")}</p>
  {:else if tags.error}
    <div role="alert">
      <p>{$t(tags.error === "permission_denied" ? "tags_permission_denied" : tags.error === "unavailable" ? "tags_unavailable" : "tags_error")}</p>
      <button type="button" onclick={() => void tags.refresh()}>{$t("tags_retry")}</button>
    </div>
  {:else if tags.isEmpty}<p>{$t("tags_empty")}</p>
  {:else}
    <div bind:this={tree} role="tree" aria-label={$t("tags_title")} class="flex flex-col text-sm text-slate-800 dark:text-zinc-100">
      {#each tags.rows as row, index (row.tag.id)}
        {@const color = validTagColor(row.tag.color)}
        <div
          role="treeitem"
          aria-selected="false"
          tabindex={row.tag.id === tabStopId ? 0 : -1}
          aria-level={row.level}
          aria-posinset={row.position}
          aria-setsize={row.siblingCount}
          aria-expanded={row.hasChildren ? row.expanded : undefined}
          data-tag-id={row.tag.id}
          class="flex cursor-default items-center gap-2 rounded-lg py-1.5 pe-3 outline-none hover:bg-slate-200 focus-visible:ring-2 focus-visible:ring-blue-500 dark:hover:bg-zinc-800"
          style="padding-inline-start: {0.5 + (row.level - 1) * 1.25}rem"
          onclick={() => { focusedId = row.tag.id; tags.toggle(row.tag.id); }}
          onfocus={() => (focusedId = row.tag.id)}
          onkeydown={(event) => onKeyDown(event, row, index)}
        >
          <span class="flex h-4 w-4 shrink-0 items-center justify-center text-slate-400 dark:text-zinc-500" aria-hidden="true">
            {#if row.hasChildren}<ChevronRight class="h-4 w-4 transition-transform {row.expanded ? 'rotate-90 rtl:-rotate-90' : 'rtl:rotate-180'}" />{/if}
          </span>
          {#if color}<span class="h-2.5 w-2.5 shrink-0 rounded-full" style="background-color: {color}" data-testid="tag-color-dot" aria-hidden="true"></span>{/if}
          <span class="truncate">{row.tag.name}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>
