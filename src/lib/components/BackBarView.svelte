<script lang="ts">
  import type { Snippet } from "svelte";
  import { ArrowLeft } from "lucide-svelte";

  /** A sub-view with a Back button above its content (All Places, All Memories and the album, place, person and memory timelines). */
  let { label, onBack, children }: { label: string; onBack: () => void; children: Snippet } = $props();
</script>

<div class="flex h-full min-h-0 flex-col gap-2">
  <button type="button" class="inline-flex self-start items-center gap-2 rounded-lg px-2 py-1 text-sm font-medium text-slate-600 hover:bg-slate-200 hover:text-slate-900 dark:text-zinc-300 dark:hover:bg-zinc-800 dark:hover:text-zinc-100" onclick={onBack}>
    <ArrowLeft class="h-4 w-4" />
    {label}
  </button>
  {@render children()}
</div>
