<script lang="ts">
  import { MapPin } from "lucide-svelte";
  import type { Place } from "../contracts";
  import { protocolSrc } from "../tauri/protocol-src";

  let { place, onSelect }: { place: Place; onSelect: (place: Place) => void } = $props();
  let thumbnailFailed = $state(false);
</script>

<button type="button" class="rounded-xl border border-slate-200 p-4 text-left dark:border-zinc-700" aria-label={place.city} onclick={() => onSelect(place)}>
  <div class="mb-3 flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
    {#if !thumbnailFailed}
      <img class="h-full w-full object-cover" src={protocolSrc("lymic-thumb", "asset", place.thumbnailAssetId)} alt={place.city} onerror={() => thumbnailFailed = true} />
    {:else}
      <MapPin class="h-12 w-12 text-slate-400" />
    {/if}
  </div>
  <span class="block truncate font-semibold">{place.city}</span>
</button>
