<script lang="ts">
  import { tick } from "svelte";
  import { Check, ChevronRight } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { Tag } from "../contracts";
  import type { TagsViewState } from "../controllers/tags-controller.svelte";
  import type { TagRow } from "../controllers/tags-tree";
  import TagColorDot from "./TagColorDot.svelte";

  /**
   * The Tag hierarchy as an accessible tree (arrow keys, roving focus). `onSelect` gets the Tag that was clicked
   * or activated with Enter or Space; expanding is the chevron or the arrow keys. With `selected` the tree is a
   * multi-select: it marks those Tags (and only those) as selected. `labelledBy` is the id of the element that
   * names the tree; without it the tree is named "Tags".
   */
  let { tags, onSelect, selected, labelledBy }: { tags: TagsViewState; onSelect: (tag: Tag) => void; selected?: readonly string[]; labelledBy?: string } = $props();
  const selectedIds = $derived(new Set(selected));

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
        onSelect(row.tag);
        break;
      default: return;
    }
    event.preventDefault();
  }
</script>

<div bind:this={tree} role="tree" aria-label={labelledBy ? undefined : $t("tags_title")} aria-labelledby={labelledBy}
  aria-multiselectable={selected ? true : undefined} class="flex flex-col text-sm text-slate-800 dark:text-zinc-100">
  {#each tags.rows as row, index (row.tag.id)}
    {@const isSelected = selectedIds.has(row.tag.id)}
    <div
      role="treeitem"
      aria-selected={isSelected}
      tabindex={row.tag.id === tabStopId ? 0 : -1}
      aria-level={row.level}
      aria-posinset={row.position}
      aria-setsize={row.siblingCount}
      aria-expanded={row.hasChildren ? row.expanded : undefined}
      data-tag-id={row.tag.id}
      class="flex cursor-default items-center gap-2 rounded-lg py-1.5 pe-3 outline-none hover:bg-slate-200 {isSelected ? 'bg-blue-100 font-medium dark:bg-sky-900/40' : ''} focus-visible:ring-2 focus-visible:ring-blue-500 dark:hover:bg-zinc-800"
      style="padding-inline-start: {0.5 + (row.level - 1) * 1.25}rem"
      onclick={() => { focusedId = row.tag.id; onSelect(row.tag); }}
      onfocus={() => (focusedId = row.tag.id)}
      onkeydown={(event) => onKeyDown(event, row, index)}
    >
      <span
        class="flex h-4 w-4 shrink-0 items-center justify-center text-slate-400 dark:text-zinc-500 {row.hasChildren ? 'cursor-pointer hover:text-slate-700 dark:hover:text-zinc-200' : ''}"
        aria-hidden="true"
        data-testid={row.hasChildren ? "tag-toggle" : undefined}
        onclick={(event) => { event.stopPropagation(); focusedId = row.tag.id; tags.toggle(row.tag.id); }}
      >
        {#if row.hasChildren}<ChevronRight class="h-4 w-4 transition-transform {row.expanded ? 'rotate-90 rtl:-rotate-90' : 'rtl:rotate-180'}" />{/if}
      </span>
      <TagColorDot color={row.tag.color} />
      <span class="truncate">{row.tag.name}</span>
      {#if isSelected}<Check class="ms-auto h-4 w-4 shrink-0 text-blue-600 dark:text-sky-400" aria-hidden="true" />{/if}
    </div>
  {/each}
</div>
