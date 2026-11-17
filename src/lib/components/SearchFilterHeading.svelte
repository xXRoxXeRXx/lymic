<script lang="ts">
  /** The small uppercase heading of one Search Filter; the filter's group or control points at it with `aria-labelledby`. */
  let { id, text }: { id: string; text: string } = $props();
</script>

<span {id} class="text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-zinc-400">{text}</span>
