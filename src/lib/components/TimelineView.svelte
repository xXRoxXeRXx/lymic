<script lang="ts">
  import { ExternalLink, Play, RefreshCw, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";
  import type { TimelineAsset } from "../contracts";
  import { calculateMasonryLayout } from "./timeline-masonry";

  interface Props {
    timeline: TimelineViewState;
    serverUrl?: string;
    onOpenExternalUrl?(url: string): Promise<void>;
  }

  let {
    timeline,
    serverUrl = "",
    onOpenExternalUrl = async () => {},
  }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  const scrubberIndex = $derived(buildTimelineScrubberIndex(timeline.buckets));
  const scrubberMonths = $derived(scrubberIndex.flatMap((entry) => entry.months));
  let viewport: HTMLDivElement;
  let activeTimeBucket = $state<string | null>(null);
  let activeBuckets = $state<Record<string, boolean>>({});
  let viewportWidth = $state(616);
  let isDragging = false;
  let selectedAsset = $state<TimelineAsset | null>(null);
  let selectedIndex = $state(-1);
  let lightboxAssets = $state<TimelineAsset[]>([]);
  let opener = $state<HTMLElement | null>(null);

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
    document.getElementById(bucketId(timeBucket))?.scrollIntoView({ behavior: "auto", block: "start" });
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
    (event.currentTarget as HTMLElement | null)?.setPointerCapture(event.pointerId);
    jumpFromPointer(event.clientY);
  }

  function drag(event: PointerEvent) {
    if (isDragging) jumpFromPointer(event.clientY);
  }

  function endDrag() {
    isDragging = false;
  }

  function measureViewport(node: HTMLElement) {
    const resizeObserver = new ResizeObserver(() => {
      viewportWidth = node.clientWidth;
    });
    resizeObserver.observe(node);
    viewportWidth = node.clientWidth;
    return { destroy: () => resizeObserver.disconnect() };
  }

  function observeBucket(node: HTMLElement, timeBucket: string) {
    const observer = new IntersectionObserver((entries) => {
      const isActive = entries.some((entry) => entry.isIntersecting);
      activeBuckets = { ...activeBuckets, [timeBucket]: isActive };
      if (isActive) void timeline.activateBucket(timeBucket).catch(() => {});
    }, { root: viewport, rootMargin: "400px 0px" });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  function masonryLayout(timeBucket: string) {
    return calculateMasonryLayout(timeline.assetsFor(timeBucket), viewportWidth);
  }

  function placeholderHeight(assetCount: number) {
    const layout = calculateMasonryLayout([], viewportWidth);
    const rows = Math.ceil(assetCount / layout.columnCount);
    return Math.max(128, rows * (layout.columnWidth * 0.75 + 8));
  }

  function formatDuration(duration: number | null) {
    if (duration === null) return "";
    const hours = Math.floor(duration / 3600);
    const minutes = Math.floor((duration % 3600) / 60);
    const seconds = Math.floor(duration % 60).toString().padStart(2, "0");
    return hours > 0 ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds}` : `${minutes}:${seconds}`;
  }

  function openLightbox(asset: TimelineAsset, source: HTMLElement) {
    lightboxAssets = timeline.buckets.flatMap((bucket) => timeline.assetsFor(bucket.timeBucket));
    selectedIndex = lightboxAssets.findIndex((candidate) => candidate.id === asset.id);
    selectedAsset = asset;
    opener = source;
  }

  function closeLightbox() {
    selectedAsset = null;
    selectedIndex = -1;
    queueMicrotask(() => opener?.focus());
  }

  function navigate(delta: number) {
    if (!selectedAsset || lightboxAssets.length === 0) return;
    selectedIndex = (selectedIndex + delta + lightboxAssets.length) % lightboxAssets.length;
    selectedAsset = lightboxAssets[selectedIndex];
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!selectedAsset) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closeLightbox();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      navigate(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      navigate(1);
    }
  }

  function assetUrl(asset: TimelineAsset) {
    return `${serverUrl.replace(/\/$/, "")}/photos/${encodeURIComponent(asset.id)}`;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="relative h-full min-h-0">
  <div class="mb-4 flex items-center justify-end gap-3">
    {#if timeline.isOffline}
      <div class="flex items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-200" role="status">
        <span>{$t("timeline_offline")}</span>
        <button
          type="button"
          class="font-semibold underline underline-offset-2"
          aria-label={$t("timeline_retry_connection")}
          onclick={() => void timeline.refresh()}
        >{$t("timeline_retry")}</button>
      </div>
    {/if}
    <button
      type="button"
      class="rounded-lg p-2 text-slate-500 hover:bg-slate-200 hover:text-slate-900 disabled:cursor-wait disabled:opacity-60 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-100"
      aria-label={$t("timeline_refresh")}
      aria-busy={timeline.isRefreshing}
      disabled={timeline.isRefreshing}
      onclick={() => void timeline.refresh()}
    >
      <RefreshCw class="h-4 w-4 {timeline.isRefreshing ? 'animate-spin' : ''}" />
    </button>
  </div>
  <div bind:this={viewport} use:measureViewport class="h-full overflow-y-auto pr-16" aria-label={$t("timeline_title")} onscroll={syncActiveBucket}>
    <div class="space-y-8">
      {#each timeline.buckets as bucket (bucket.timeBucket)}
        {@const layout = masonryLayout(bucket.timeBucket)}
        <section
          id={bucketId(bucket.timeBucket)}
          use:observeBucket={bucket.timeBucket}
          class="space-y-3 scroll-mt-4"
          style:min-height={`${layout.height + 52}px`}
        >
          <div class="flex items-baseline justify-between">
            <h2 class="text-xl font-bold text-slate-900 dark:text-slate-100">{monthLabel(bucket.timeBucket)}</h2>
            <p class="text-sm text-slate-500 dark:text-zinc-400">{$t("timeline_asset_count", { values: { count: bucket.count.toLocaleString(numberLocale) } })}</p>
          </div>
          {#if activeBuckets[bucket.timeBucket] && timeline.assetsFor(bucket.timeBucket).length > 0}
            <div class="flex gap-2" style:height={`${layout.height}px`} aria-label="Month masonry grid">
              {#each layout.columns as column, columnIndex (columnIndex)}
                <div class="flex min-w-0 flex-1 flex-col gap-2">
                  {#each column as item (item.asset.id)}
                    <button type="button" class="relative overflow-hidden rounded-lg bg-slate-200 text-left dark:bg-zinc-800" aria-label={`Open ${item.asset.type === "VIDEO" ? "video" : "photo"}`} onclick={(event) => openLightbox(item.asset, event.currentTarget)}>
                      <img src={`lymic-thumb://asset/${item.asset.id}`} alt="" loading="lazy" width={item.asset.width} height={item.asset.height} style:height={`${item.height}px`} class="w-full object-cover" />
                      {#if item.asset.type === "VIDEO"}
                        <span class="absolute bottom-2 left-2 inline-flex items-center gap-1 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-white"><Play class="h-3 w-3 fill-current" />{formatDuration(item.asset.duration)}</span>
                      {/if}
                    </button>
                  {/each}
                </div>
              {/each}
            </div>
          {:else}
            <div style:height={`${layout.height || placeholderHeight(bucket.count)}px`} class="rounded-xl bg-slate-200/70 dark:bg-zinc-800/70 animate-pulse" aria-label={$t("timeline_placeholders")}></div>
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

{#if selectedAsset}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/90 p-4" role="presentation" onclick={(event) => event.currentTarget === event.target && closeLightbox()}>
    <dialog open role="dialog" aria-label="Media lightbox" aria-modal="true" class="m-0 grid h-full w-full max-w-7xl grid-rows-[minmax(0,1fr)_auto] border-0 bg-transparent p-0 text-white">
      <div class="relative flex min-h-0 items-center justify-center">
        <button type="button" class="absolute right-2 top-2 z-10 rounded-full bg-black/60 p-3" aria-label="Close lightbox" onclick={closeLightbox}><X class="h-5 w-5" /></button>
        {#if selectedAsset.type === "VIDEO"}
          <video class="max-h-full max-w-full" controls autoplay aria-label="Video preview" src={`lymic-asset://video/${selectedAsset.id}`}></video>
        {:else}
          <img class="max-h-full max-w-full object-contain" alt="Photo preview" src={`lymic-asset://asset/${selectedAsset.id}`} />
        {/if}
      </div>
      <div class="flex flex-wrap items-center justify-between gap-3 bg-black/70 px-4 py-3 text-sm">
        <div class="space-y-1">
          <p>{selectedAsset.takenAt ? new Intl.DateTimeFormat(numberLocale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(selectedAsset.takenAt)) : "Unknown date"}</p>
          <p>{[selectedAsset.cameraMake, selectedAsset.cameraModel].filter(Boolean).join(" ") || "Unknown camera"}</p>
          <p>{selectedAsset.width.toLocaleString(numberLocale)} × {selectedAsset.height.toLocaleString(numberLocale)}{selectedAsset.fileSize === null ? "" : ` · ${(selectedAsset.fileSize / 1024 / 1024).toFixed(1)} MB`}</p>
        </div>
        <div class="flex gap-2">
          <button type="button" class="rounded bg-white/15 px-3 py-2" aria-label="Previous asset" onclick={() => navigate(-1)}>Previous</button>
          <button type="button" class="rounded bg-white/15 px-3 py-2" aria-label="Next asset" onclick={() => navigate(1)}>Next</button>
          <button type="button" class="inline-flex items-center gap-2 rounded bg-blue-600 px-3 py-2" aria-label="Open in Immich" onclick={() => selectedAsset && onOpenExternalUrl(assetUrl(selectedAsset))}>Open in Immich <ExternalLink class="h-4 w-4" /></button>
        </div>
      </div>
    </dialog>
  </div>
{/if}
