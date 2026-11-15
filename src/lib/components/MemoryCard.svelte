<script lang="ts">
  import { Sparkles } from "lucide-svelte";
  import type { Memory } from "../contracts";
  import { protocolSrc } from "../tauri/protocol-src";

  let { memory, title, onSelect }: { memory: Memory; title: string; onSelect: (memory: Memory) => void } = $props();
  let coverFailed = $state(false);
</script>

<button type="button" class="rounded-xl border border-slate-200 p-3 text-left dark:border-zinc-700" aria-label={title} onclick={() => onSelect(memory)}>
  <div class="mb-2 flex aspect-[4/3] items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
    {#if !coverFailed}
      <img class="h-full w-full object-cover" src={protocolSrc("lymic-thumb", "asset", memory.coverAssetId)} alt="" onerror={() => coverFailed = true} />
    {:else}
      <Sparkles class="h-10 w-10 text-slate-400" />
    {/if}
  </div>
  <span class="block truncate text-sm font-semibold">{title}</span>
</button>
