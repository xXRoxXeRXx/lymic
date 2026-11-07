<script lang="ts">
  import { locale } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";

  interface Props {
    timeline: TimelineViewState;
  }

  let { timeline }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));

  function monthLabel(timeBucket: string) {
    return new Intl.DateTimeFormat(numberLocale, { month: "long", year: "numeric", timeZone: "UTC" })
      .format(new Date(timeBucket));
  }
</script>

<div class="space-y-8" aria-label="Photo Timeline">
  {#each timeline.buckets as bucket (bucket.timeBucket)}
    <section class="space-y-3" onmouseenter={() => timeline.activateBucket(bucket.timeBucket)} onfocusin={() => timeline.activateBucket(bucket.timeBucket)}>
      <div class="flex items-baseline justify-between">
        <h2 class="text-xl font-bold text-slate-900 dark:text-slate-100">{monthLabel(bucket.timeBucket)}</h2>
        <p class="text-sm text-slate-500 dark:text-zinc-400">{bucket.count.toLocaleString(numberLocale)} assets</p>
      </div>
      {#if timeline.assetsFor(bucket.timeBucket).length > 0}
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-6">
          {#each timeline.assetsFor(bucket.timeBucket) as asset (asset.id)}
            <img
              src={`lymic-thumb://asset/${asset.id}`}
              alt=""
              loading="lazy"
              width={asset.width}
              height={asset.height}
              class="aspect-square w-full rounded-lg object-cover bg-slate-200 dark:bg-zinc-800"
            />
          {/each}
        </div>
      {:else}
        <div class="h-32 rounded-xl bg-slate-200/70 dark:bg-zinc-800/70 animate-pulse" aria-label="Media placeholders"></div>
      {/if}
    </section>
  {/each}
</div>
