<script lang="ts">
  import { untrack } from "svelte";
  import { ChevronLeft, ChevronRight, ExternalLink, Play, RefreshCw, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";
  import type { AssetDetails, TimelineAsset } from "../contracts";
  import { calculateMasonryLayout } from "./timeline-masonry";

  interface Props {
    timeline: TimelineViewState;
    serverUrl?: string;
    onOpenExternalUrl?(url: string): Promise<void>;
    getAssetDetails?(assetId: string): Promise<AssetDetails | null>;
    showScrubber?: boolean;
    emptyMessage?: string;
    loadErrorMessage?: string;
  }

  let {
    timeline,
    serverUrl = "",
    onOpenExternalUrl = async () => {},
    getAssetDetails = async () => null,
    showScrubber = true,
    emptyMessage = "",
    loadErrorMessage = "",
  }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  const scrubberIndex = $derived(buildTimelineScrubberIndex(timeline.buckets));
  const scrubberMonths = $derived(scrubberIndex.flatMap((entry) => entry.months));
  const lightboxAssets = $derived(
    timeline.buckets.flatMap((bucket) => timeline.assetsFor(bucket.timeBucket)),
  );
  let viewport: HTMLDivElement;
  let activeTimeBucket = $state<string | null>(null);
  let activeBuckets = $state<Record<string, boolean>>({});
  let viewportWidth = $state(616);
  let isDragging = false;
  let scrollRaf: number | null = null;
  let selectedAsset = $state<TimelineAsset | null>(null);
  let assetDetails = $state<AssetDetails | null>(null);
  const assetDetailsById = new Map<string, AssetDetails | null>();
  let selectedIndex = $state(-1);
  let opener = $state<HTMLElement | null>(null);

  $effect(() => {
    if (!activeTimeBucket || !scrubberMonths.includes(activeTimeBucket)) {
      activeTimeBucket = scrubberMonths[0] ?? null;
    }
  });

  $effect(() => {
    const currentBuckets = timeline.buckets;
    untrack(() => {
      for (const bucket of currentBuckets) {
        if (activeBuckets[bucket.timeBucket]) {
          void Promise.resolve(timeline.activateBucket(bucket.timeBucket)).catch(() => {});
        }
      }
    });
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

  function updateActiveBucket() {
    if (!viewport) return;
    let visibleBucket = scrubberMonths[0] ?? null;
    for (const timeBucket of scrubberMonths) {
      const section = document.getElementById(bucketId(timeBucket));
      if (section && section.offsetTop <= viewport.scrollTop + 1) visibleBucket = timeBucket;
    }
    activeTimeBucket = visibleBucket;
  }

  function syncActiveBucket() {
    if (scrollRaf !== null) return;
    updateActiveBucket();
    if (typeof requestAnimationFrame === "function") {
      scrollRaf = requestAnimationFrame(() => {
        scrollRaf = null;
        updateActiveBucket();
      });
    }
  }

  function jumpFromPointer(clientY: number, trackElement?: HTMLElement | null) {
    const trackBounds = trackElement?.getBoundingClientRect();
    const bounds =
      trackBounds && trackBounds.height > 0 ? trackBounds : viewport.getBoundingClientRect();
    if (bounds.height <= 0) return;
    const ratio = Math.max(0, Math.min(1, (clientY - bounds.top) / bounds.height));
    const index = Math.round(ratio * (scrubberMonths.length - 1));
    const timeBucket = scrubberMonths[index];
    if (timeBucket) scrollToBucket(timeBucket);
  }

  function startDrag(event: PointerEvent) {
    isDragging = true;
    const thumb = event.currentTarget as HTMLElement | null;
    thumb?.setPointerCapture(event.pointerId);
    jumpFromPointer(event.clientY, thumb?.parentElement);
  }

  function drag(event: PointerEvent) {
    if (isDragging) {
      const thumb = event.currentTarget as HTMLElement | null;
      jumpFromPointer(event.clientY, thumb?.parentElement);
    }
  }

  function endDrag() {
    isDragging = false;
  }

  function measureViewport(node: HTMLElement) {
    if (typeof ResizeObserver === "undefined") {
      if (node.clientWidth > 0) viewportWidth = node.clientWidth;
      return {};
    }
    const resizeObserver = new ResizeObserver(() => {
      if (node.clientWidth > 0) viewportWidth = node.clientWidth;
    });
    resizeObserver.observe(node);
    if (node.clientWidth > 0) viewportWidth = node.clientWidth;
    return { destroy: () => resizeObserver.disconnect() };
  }

  function observeBucket(node: HTMLElement, timeBucket: string) {
    if (typeof IntersectionObserver === "undefined") {
      return {};
    }
    const observer = new IntersectionObserver((entries) => {
      const isActive = entries.some((entry) => entry.isIntersecting);
      activeBuckets = { ...activeBuckets, [timeBucket]: isActive };
      if (isActive) void Promise.resolve(timeline.activateBucket(timeBucket)).catch(() => {});
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

  const exifDetails = $derived.by(() => {
    if (!assetDetails) return [];
    const details: string[] = [];
    if (assetDetails.fNumber !== null) details.push(`f/${assetDetails.fNumber.toLocaleString(numberLocale, { maximumFractionDigits: 2 })}`);
    if (assetDetails.exposureTime) details.push(assetDetails.exposureTime.endsWith("s") ? assetDetails.exposureTime : `${assetDetails.exposureTime}s`);
    if (assetDetails.iso !== null) details.push(`ISO ${assetDetails.iso.toLocaleString(numberLocale)}`);
    if (assetDetails.focalLength !== null) details.push(`${assetDetails.focalLength.toLocaleString(numberLocale, { maximumFractionDigits: 2 })}mm`);
    return details;
  });

  function loadAssetDetails(asset: TimelineAsset) {
    if (assetDetailsById.has(asset.id)) {
      assetDetails = assetDetailsById.get(asset.id) ?? null;
      return;
    }
    assetDetails = null;
    void getAssetDetails(asset.id)
      .then((details) => {
        assetDetailsById.set(asset.id, details);
        if (selectedAsset?.id === asset.id) assetDetails = details;
      })
      // Details enhance the footer; a failure must not disrupt asset viewing.
      .catch(() => {});
  }

  function openLightbox(asset: TimelineAsset, source: HTMLElement) {
    selectedIndex = lightboxAssets.findIndex((candidate) => candidate.id === asset.id);
    selectedAsset = asset;
    opener = source;
    loadAssetDetails(asset);
  }

  function hasRealDimensions(asset: TimelineAsset) {
    if (asset.width <= 0 || asset.height <= 0) return false;
    if (
      asset.height === 1000 &&
      asset.fileSize === null &&
      !asset.cameraMake &&
      !asset.cameraModel
    ) {
      return false;
    }
    return true;
  }

  function closeLightbox() {
    selectedAsset = null;
    assetDetails = null;
    selectedIndex = -1;
    queueMicrotask(() => opener?.focus());
  }

  function navigate(delta: number) {
    if (!selectedAsset || lightboxAssets.length === 0) return;
    selectedIndex = (selectedIndex + delta + lightboxAssets.length) % lightboxAssets.length;
    selectedAsset = lightboxAssets[selectedIndex];
    loadAssetDetails(selectedAsset);
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

  function handleWindowResize() {
    if (viewport && viewport.clientWidth > 0) {
      viewportWidth = viewport.clientWidth;
    }
  }

  function assetUrl(asset: TimelineAsset) {
    return `${serverUrl.replace(/\/$/, "")}/photos/${encodeURIComponent(asset.id)}`;
  }

  function protocolSrc(protocol: "lymic-thumb" | "lymic-asset", kind: "asset" | "video", id: string) {
    const encodedId = encodeURIComponent(id);
    const convertFileSrc = (
      globalThis as typeof globalThis & {
        __TAURI_INTERNALS__?: { convertFileSrc?: (filePath: string, protocol?: string) => string };
      }
    ).__TAURI_INTERNALS__?.convertFileSrc;
    if (typeof convertFileSrc === "function") {
      const base = convertFileSrc("", protocol).replace(/\/$/, "");
      return `${base}/${kind}/${encodedId}`;
    }
    return `${protocol}://${kind}/${encodedId}`;
  }
</script>

<svelte:window onkeydown={handleKeydown} onresize={handleWindowResize} />

<div class="relative flex h-full min-h-0 flex-col">
  <div class="mb-2 flex shrink-0 items-center justify-end gap-3">
    {#if timeline.hasInitialLoadError}
      <div class="flex items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-200" role="alert">
        <span>{loadErrorMessage || $t("timeline_offline")}</span>
        <button
          type="button"
          class="font-semibold underline underline-offset-2"
          aria-label={$t("timeline_retry_connection")}
          onclick={() => void timeline.refresh()}
        >{$t("timeline_retry")}</button>
      </div>
    {:else if timeline.isOffline}
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
  <div bind:this={viewport} use:measureViewport class="min-h-0 flex-1 overflow-y-auto pr-16 scrollbar-none" aria-label={$t("timeline_title")} onscroll={syncActiveBucket}>
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
            <div class="flex gap-2" style:height={`${layout.height}px`} aria-label={$t("timeline_masonry_grid")}>
              {#each layout.columns as column, columnIndex (columnIndex)}
                <div class="flex min-w-0 flex-1 flex-col gap-2">
                  {#each column as item (item.asset.id)}
                    <button type="button" class="relative overflow-hidden rounded-lg bg-slate-200 text-left dark:bg-zinc-800" aria-label={item.asset.type === "VIDEO" ? $t("timeline_open_video") : $t("timeline_open_photo")} onclick={(event) => openLightbox(item.asset, event.currentTarget)}>
                      <img src={protocolSrc("lymic-thumb", "asset", item.asset.id)} alt="" loading="lazy" width={item.asset.width} height={item.asset.height} style:height={`${item.height}px`} class="w-full object-cover" />
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

  {#if timeline.buckets.length === 0 && !timeline.isLoading && !timeline.hasInitialLoadError && emptyMessage}<p role="status">{emptyMessage}</p>{/if}
  {#if showScrubber && scrubberMonths.length > 0}
    {@const thumbRatio = scrubberMonths.length > 1 && activeTimeBucket ? Math.max(0, scrubberMonths.indexOf(activeTimeBucket)) / (scrubberMonths.length - 1) : 0}
    <nav class="absolute inset-y-0 -right-4 w-16 border-l border-slate-200/80 bg-slate-100/85 py-2 pl-1 pr-1 backdrop-blur dark:border-zinc-700/80 dark:bg-zinc-900/85" aria-label={$t("timeline_scrubber")}>
      <div class="relative h-full">
        <div class="h-full overflow-y-auto pr-5 scrollbar-none">
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
          style:top={`calc(${thumbRatio * 100}% - ${thumbRatio * 1.5}rem)`}
          aria-label={$t("timeline_scrubber_drag")}
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
    <dialog open aria-label={$t("timeline_lightbox_label")} aria-modal="true" class="m-0 grid h-full w-full max-h-none max-w-none grid-rows-[minmax(0,1fr)_auto] border-0 bg-transparent p-0 text-white">
      <div class="relative flex h-full w-full min-h-0 min-w-0 items-center justify-center overflow-hidden">
        <button type="button" class="absolute right-2 top-2 z-10 rounded-full bg-black/60 p-3 text-white hover:bg-black/80" aria-label={$t("timeline_close_lightbox")} onclick={closeLightbox}><X class="h-5 w-5" /></button>
        <button type="button" class="absolute left-2 top-1/2 z-10 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80" aria-label={$t("timeline_previous_asset")} onclick={() => navigate(-1)}><ChevronLeft class="h-5 w-5" /></button>
        <button type="button" class="absolute right-2 top-1/2 z-10 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80" aria-label={$t("timeline_next_asset")} onclick={() => navigate(1)}><ChevronRight class="h-5 w-5" /></button>
        {#if selectedAsset.type === "VIDEO"}
          <video class="h-full w-full max-h-full max-w-full object-contain" controls autoplay aria-label={$t("timeline_video_preview")} src={protocolSrc("lymic-asset", "video", selectedAsset.id)}>
            <track kind="captions" />
          </video>
        {:else}
          <img class="h-full w-full max-h-full max-w-full object-contain" alt={$t("timeline_photo_preview")} src={protocolSrc("lymic-asset", "asset", selectedAsset.id)} />
        {/if}
      </div>
      <div class="flex flex-wrap items-center justify-between gap-3 bg-black/70 px-4 py-3 text-sm">
        <div class="space-y-1">
          <p>{selectedAsset.takenAt ? new Intl.DateTimeFormat(numberLocale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(selectedAsset.takenAt)) : $t("timeline_unknown_date")}</p>
          <p>{[selectedAsset.cameraMake, selectedAsset.cameraModel].filter(Boolean).join(" ") || $t("timeline_unknown_camera")}</p>
          {#if assetDetails?.originalFileName}
            <p>{assetDetails.originalFileName}</p>
          {/if}
          {#if exifDetails.length > 0}
            <p>{exifDetails.join(" · ")}</p>
          {/if}
          {#if assetDetails?.lensModel}
            <p>{assetDetails.lensModel}</p>
          {/if}
          <p>
            {#if hasRealDimensions(selectedAsset)}
              <span>{selectedAsset.width.toLocaleString(numberLocale)} × {selectedAsset.height.toLocaleString(numberLocale)}</span>
            {/if}
            {#if selectedAsset.fileSize !== null}
              <span>{hasRealDimensions(selectedAsset) ? " · " : ""}{(selectedAsset.fileSize / 1024 / 1024).toFixed(1)} MB</span>
            {/if}
          </p>
        </div>
        <div class="flex gap-2">
          <button type="button" class="inline-flex items-center gap-2 rounded bg-blue-600 px-3 py-2 disabled:opacity-50" aria-label={$t("timeline_open_in_immich")} disabled={!serverUrl.trim()} onclick={() => selectedAsset && serverUrl.trim() && onOpenExternalUrl(assetUrl(selectedAsset))}>{$t("timeline_open_in_immich")} <ExternalLink class="h-4 w-4" /></button>
        </div>
      </div>
    </dialog>
  </div>
{/if}
