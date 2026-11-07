<script lang="ts">
  import { locale } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";

  interface Props {
    timeline: TimelineViewState;
  }

  let { timeline }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  const scrubberIndex = $derived(buildTimelineScrubberIndex(timeline.buckets));
  const scrubberMonths = $derived(scrubberIndex.flatMap((entry) => entry.months));
  let viewport: HTMLDivElement;
  let activeTimeBucket = $state<string | null>(null);
  let isDragging = false;

  $effect(() => {
    if (!activeTimeBucket || !scrubberMonths.includes(activeTimeBucket)) {
      activeTimeBucket = scrubberMonths[0] ?? null;
    }
  });

  function monthLabel(timeBucket: string) {
    return new Intl.DateTimeFormat(numberLocale, { month: "long", year: "numeric", timeZone: "UTC" })
      .format(new Date(timeBucket));
  }

  function monthShortLabel(timeBucket: string) {
    return new Intl.DateTimeFormat(numberLocale, { month: "short", timeZone: "UTC" })
      .format(new Date(timeBucket));
  }

  function bucketId(timeBucket: string) {
    return `timeline-bucket-${timeBucket.replace(/[^a-zA-Z0-9]/g, "-")}`;
  }

  function scrollToBucket(timeBucket: string) {
    document.getElementById(bucketId(timeBucket))?.scrollIntoView({ behavior: "smooth", block: "start" });
    activeTimeBucket = timeBucket;
  }

  function syncActiveBucket() {
    let visibleBucket = scrubberMonths[0] ?? null;
    for (const timeBucket of scrubberMonths) {
      const section = document.getElementById(bucketId(timeBucket));
      if (section && section.offsetTop <= viewport.scrollTop + 1) visibleBucket = timeBucket;
    }
    activeTimeBucket = visibleBucket;
  }

  function jumpFromPointer(clientY: number) {
    const bounds = viewport.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientY - bounds.top) / bounds.height));
    const index = Math.round(ratio * (scrubberMonths.length - 1));
    const timeBucket = scrubberMonths[index];
    if (timeBucket) scrollToBucket(timeBucket);
  }

  function startDrag(event: PointerEvent) {
    isDragging = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    jumpFromPointer(event.clientY);
  }

  function drag(event: PointerEvent) {
    if (isDragging) jumpFromPointer(event.clientY);
  }

  function endDrag() {
    isDragging = false;
  }
</script>

<div class="relative h-full min-h-0">
  <div bind:this={viewport} class="h-full overflow-y-auto pr-16" aria-label="Photo Timeline" onscroll={syncActiveBucket}>
    <div class="space-y-8">
      {#each timeline.buckets as bucket (bucket.timeBucket)}
        <section id={bucketId(bucket.timeBucket)} class="space-y-3 scroll-mt-4" onmouseenter={() => timeline.activateBucket(bucket.timeBucket)} onfocusin={() => timeline.activateBucket(bucket.timeBucket)}>
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
  </div>

  {#if scrubberMonths.length > 0}
    <nav class="absolute inset-y-0 right-0 w-14 border-l border-slate-200/80 bg-slate-100/85 py-2 pl-1 backdrop-blur dark:border-zinc-700/80 dark:bg-zinc-900/85" aria-label="Timeline scrubber">
      <div class="relative h-full">
        <div class="h-full overflow-y-auto pr-2">
          {#each scrubberIndex as entry (entry.year)}
            <div class="mb-1 flex flex-col items-end gap-0.5">
              <button type="button" class="min-h-6 rounded px-1 text-xs font-bold text-slate-700 hover:bg-slate-200 dark:text-zinc-200 dark:hover:bg-zinc-700" onclick={() => scrollToBucket(entry.months[0])}>{entry.year}</button>
              {#each entry.months as timeBucket (timeBucket)}
                <button
                  type="button"
                  class="min-h-6 rounded px-1 text-[10px] leading-none {activeTimeBucket === timeBucket ? 'bg-blue-600 text-white' : 'text-slate-500 hover:bg-slate-200 dark:text-zinc-400 dark:hover:bg-zinc-700'}"
                  aria-current={activeTimeBucket === timeBucket ? "true" : undefined}
                  aria-label={monthLabel(timeBucket)}
                  onclick={() => scrollToBucket(timeBucket)}
                >{monthShortLabel(timeBucket)}</button>
              {/each}
            </div>
          {/each}
        </div>
        <button
          type="button"
          class="absolute right-0 h-6 w-2 rounded-full bg-blue-600 shadow transition-transform"
          style:top={`${scrubberMonths.length > 1 && activeTimeBucket ? `${(Math.max(0, scrubberMonths.indexOf(activeTimeBucket)) / (scrubberMonths.length - 1)) * 100}%` : "0"}`}
          aria-label="Drag timeline scrubber"
          onpointerdown={startDrag}
          onpointermove={drag}
          onpointerup={endDrag}
          onpointercancel={endDrag}
        ></button>
      </div>
    </nav>
  {/if}
</div>
