<script lang="ts">
  import { X } from "lucide-svelte";
  import { t } from "svelte-i18n";

  /**
   * A Filter Chip: the entry to the editor of one Search Filter (or the Search Mode). It shows the name and,
   * when the filter is set, its value, and removes the filter with its own button. A chip is a plain
   * button: `aria-expanded` says whether its editor is open.
   */
  let { editor, label, summary, expanded, controls, invalid = false, unavailable = null, onToggle, onRemove }: {
    /** The id of the editor the chip opens; the chip's own id is derived from it. */
    editor: string;
    label: string;
    /** The value of the chip, or null when its filter is not set. */
    summary: string | null;
    expanded: boolean;
    /** The element the chip controls: the editor, or the panel while it is closed. */
    controls: string;
    /** True when the value cannot be searched, so the chip stands out. */
    invalid?: boolean;
    /** Why the filter is not available (shown as the tooltip), or null. */
    unavailable?: string | null;
    onToggle: () => void;
    /** Present for a chip that can be emptied; shown only while the filter is set. */
    onRemove?: () => void;
  } = $props();

  const text = $derived(summary === null ? label : `${label}: ${summary}`);
  const removeLabel = $derived($t("search_chip_remove", { values: { filter: label } }));
</script>

<span
  class="inline-flex max-w-full items-center rounded-lg border text-sm {invalid ? 'border-red-500 text-red-700 dark:border-red-400 dark:text-red-300' : summary !== null ? 'border-blue-500 bg-blue-50 text-blue-800 dark:border-sky-500 dark:bg-sky-950/40 dark:text-sky-200' : 'border-slate-200/80 text-slate-600 dark:border-zinc-700 dark:text-zinc-300'} {unavailable ? 'opacity-60' : ''} {expanded ? 'ring-2 ring-blue-600 dark:ring-sky-400' : ''}"
>
  <button
    type="button"
    id="search-chip-{editor}"
    class="min-w-0 truncate rounded-lg px-2.5 py-1 hover:bg-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 dark:hover:bg-zinc-800"
    aria-expanded={expanded}
    aria-controls={controls}
    title={unavailable ?? text}
    onclick={onToggle}
  >{text}</button>
  {#if onRemove && summary !== null}
    <button
      type="button"
      class="mr-1 rounded p-0.5 hover:bg-slate-200 focus-visible:outline-2 focus-visible:outline-blue-600 dark:hover:bg-zinc-700"
      aria-label={removeLabel}
      title={removeLabel}
      onclick={onRemove}
    >
      <X class="h-3.5 w-3.5" aria-hidden="true" />
    </button>
  {/if}
</span>
