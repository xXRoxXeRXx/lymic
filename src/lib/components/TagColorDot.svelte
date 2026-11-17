<script lang="ts">
  import { validTagColor } from "../tag-color";

  /** The Tag's colour as Immich returns it; nothing is drawn unless it is exactly `#RRGGBB`. */
  let { color }: { color: string | null | undefined } = $props();
  const valid = $derived(validTagColor(color));
</script>

{#if valid}<span class="h-2.5 w-2.5 shrink-0 rounded-full" style="background-color: {valid}" data-testid="tag-color-dot" aria-hidden="true"></span>{/if}
