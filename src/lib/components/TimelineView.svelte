<script lang="ts">
  import { protocolSrc } from "../tauri/protocol-src";
  import { untrack } from "svelte";
  import { Album, Check, Download, Heart, HeartOff, Play, RefreshCw, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";
  import type { AlbumAssignmentResult, AssetDetails, TimelineAsset } from "../contracts";
  import { calculateMasonryLayout } from "./timeline-masonry";
  import Lightbox from "./Lightbox.svelte";
  import AddToAlbumModal from "./AddToAlbumModal.svelte";
  import { createFavoritesLightboxState } from "./favorites-lightbox-state.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

  interface Props {
    timeline: TimelineViewState;
    serverUrl?: string;
    onOpenExternalUrl?(url: string): Promise<void>;
    getAssetDetails?(assetId: string): Promise<AssetDetails | null>;
    setAssetFavorite?(assetId: string, isFavorite: boolean): Promise<void>;
    chooseDownloadDestination?(originalFileName: string): Promise<string | null>;
    downloadAsset?(assetId: string, destinationPath: string): Promise<void>;
    downloadArchive?(assetIds: string[]): Promise<void>;
    albumCatalogue?: TargetAlbumCatalogue;
    addAssetsToAlbum?(assetIds: string[], albumId: string): Promise<AlbumAssignmentResult>;
    showScrubber?: boolean;
    showRefreshButton?: boolean;
    removeUnfavoritedAssets?: boolean;
    loadingMessage?: string;
    emptyMessage?: string;
    loadErrorMessage?: string;
    offlineMessage?: string;
    ariaLabel?: string;
  }

  let {
    timeline,
    serverUrl = "",
    onOpenExternalUrl = async () => {},
    getAssetDetails = async () => null,
    setAssetFavorite = async () => {},
    chooseDownloadDestination = async () => null,
    downloadAsset = async () => {},
    downloadArchive = async () => {},
    albumCatalogue,
    addAssetsToAlbum = async (assetIds) => ({ succeeded: assetIds.length, failed: 0 }),
    showScrubber = true,
    showRefreshButton = true,
    removeUnfavoritedAssets = false,
    loadingMessage = "",
    emptyMessage = "",
    loadErrorMessage = "",
    offlineMessage = "",
    ariaLabel = "",
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
  const favoritesLightbox = createFavoritesLightboxState(() => timeline);
  const assetDetailsById = new Map<string, AssetDetails | null>();
  let favoriteOverrides = $state<Record<string, boolean>>({});
  let updatingFavoriteAssetIds = $state<Record<string, true>>({});
  let isAddToAlbumOpen = $state(false);
  let isAddingToAlbum = $state(false);
  let addToAlbumError = $state("");
  let opener = $state<HTMLElement | null>(null);
  let selectedAssetIds = $state<Set<string>>(new Set());
  let isApplyingSelection = $state(false);
  let isDownloadingSelection = $state(false);
  let selectionResult = $state<{ action: "favorite" | "album" | "download"; succeeded: number; total: number } | null>(null);
  let actionGeneration = 0;
  const selectionCount = $derived(selectedAssetIds.size);
  const isSelectionMode = $derived(selectionCount > 0);
  const selectedTimelineAssets = $derived(lightboxAssets.filter((asset) => selectedAssetIds.has(asset.id)));
  const hasFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => isFavorite(asset)));
  const hasNonFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => !isFavorite(asset)));
  const isSelectionActionBusy = $derived(isApplyingSelection || isAddingToAlbum || isDownloadingSelection);

  function resetSelectionAndModals() {
    actionGeneration += 1;
    isApplyingSelection = false;
    isDownloadingSelection = false;
    isAddingToAlbum = false;
    isAddToAlbumOpen = false;
    addToAlbumError = "";
    selectionResult = null;
    clearSelection();
  }

  function resetLightbox() {
    selectedAsset = null;
    opener = null;
    updatingFavoriteAssetIds = {};
    favoriteOverrides = {};
    favoritesLightbox.reset();
  }

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

  $effect(() => {
    timeline.refreshGeneration;
    untrack(() => {
      assetDetailsById.clear();
      resetSelectionAndModals();
      if (timeline.buckets.length === 0) {
        resetLightbox();
        return;
      }
      favoriteOverrides = Object.fromEntries(
        Object.keys(updatingFavoriteAssetIds)
          .filter((assetId) => assetId in favoriteOverrides)
          .map((assetId) => [assetId, favoriteOverrides[assetId]]),
      );
      favoritesLightbox.clearError();
    });
  });

  $effect(() => {
    timeline;
    untrack(() => {
      resetSelectionAndModals();
      favoritesLightbox.reset();
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

  function isFavorite(asset: TimelineAsset) {
    return favoriteOverrides[asset.id] ?? asset.isFavorite;
  }

  function isAssetSelected(assetId: string) {
    return selectedAssetIds.has(assetId);
  }

  function toggleAssetSelection(assetId: string) {
    selectionResult = null;
    const nextSelectedAssetIds = new Set(selectedAssetIds);
    if (nextSelectedAssetIds.has(assetId)) nextSelectedAssetIds.delete(assetId);
    else nextSelectedAssetIds.add(assetId);
    selectedAssetIds = nextSelectedAssetIds;
  }

  function clearSelection() {
    selectedAssetIds = new Set();
  }

  function updateFavoriteState(assetId: string, isFavorite: boolean) {
    if (!removeUnfavoritedAssets) {
      favoriteOverrides = { ...favoriteOverrides, [assetId]: isFavorite };
    }
    const currentDetails = assetDetailsById.get(assetId);
    if (currentDetails) {
      const updatedDetails = { ...currentDetails, isFavorite };
      assetDetailsById.set(assetId, updatedDetails);
    }
    if (selectedAsset?.id === assetId) selectedAsset = { ...selectedAsset, isFavorite };
  }

  function cleanupRemovedAssetState(assetId: string) {
    const { [assetId]: _removedOverride, ...remainingOverrides } = favoriteOverrides;
    favoriteOverrides = remainingOverrides;
    if (selectedAssetIds.has(assetId)) {
      const nextSelectedAssetIds = new Set(selectedAssetIds);
      nextSelectedAssetIds.delete(assetId);
      selectedAssetIds = nextSelectedAssetIds;
    }
  }

  function updateAssetFavorite(assetId: string, isFavorite: boolean) {
    if (!removeUnfavoritedAssets) return setAssetFavorite(assetId, isFavorite);
    return favoritesLightbox.updateFavorite(
      assetId,
      isFavorite,
      setAssetFavorite,
      () => $t("timeline_favorite_update_error"),
      cleanupRemovedAssetState,
    );
  }

  async function applyFavoriteToSelection(isFavorite: boolean) {
    const assetIds = [...selectedAssetIds];
    if (assetIds.length === 0 || isSelectionActionBusy) return;

    const generation = actionGeneration;
    if (removeUnfavoritedAssets) favoritesLightbox.clearError();
    selectionResult = null;
    isApplyingSelection = true;
    let succeeded = 0;
    for (const assetId of assetIds) {
      try {
        await setAssetFavorite(assetId, isFavorite);
        if (generation !== actionGeneration) return;
        if (removeUnfavoritedAssets) {
          if (!isFavorite) {
            favoritesLightbox.confirmRemoval(assetId);
            cleanupRemovedAssetState(assetId);
          }
        } else {
          updateFavoriteState(assetId, isFavorite);
        }
        succeeded += 1;
      } catch {
        if (generation !== actionGeneration) return;
        // Continue so the result can accurately report a partial update.
      }
    }
    if (generation !== actionGeneration) return;
    isApplyingSelection = false;
    clearSelection();
    selectionResult = { action: "favorite", succeeded, total: assetIds.length };
  }

  async function downloadSelection() {
    const assetIds = [...selectedAssetIds];
    if (assetIds.length === 0 || isSelectionActionBusy) return;

    const generation = actionGeneration;
    selectionResult = null;
    isDownloadingSelection = true;
    try {
      if (assetIds.length === 1) {
        const assetId = assetIds[0];
        let details = assetDetailsById.get(assetId);
        if (!assetDetailsById.has(assetId)) {
          details = await getAssetDetails(assetId);
          if (generation !== actionGeneration) return;
          assetDetailsById.set(assetId, details);
        }
        if (!details?.originalFileName) throw new Error("Original file name is unavailable");
        const destinationPath = await chooseDownloadDestination(details.originalFileName);
        if (!destinationPath || generation !== actionGeneration) return;
        await downloadAsset(assetId, destinationPath);
      } else {
        await downloadArchive(assetIds);
      }
      if (generation !== actionGeneration) return;
      selectionResult = { action: "download", succeeded: assetIds.length, total: assetIds.length };
    } catch {
      if (generation !== actionGeneration) return;
      selectionResult = { action: "download", succeeded: 0, total: assetIds.length };
    } finally {
      if (generation === actionGeneration) isDownloadingSelection = false;
    }
  }

  function openAddSelectionToAlbum() {
    if (selectionCount === 0 || !albumCatalogue || isSelectionActionBusy) return;
    selectionResult = null;
    addToAlbumError = "";
    isAddToAlbumOpen = true;
  }

  async function assignSelectedAssetToAlbum(albumId: string) {
    if (isAddingToAlbum || selectedAssetIds.size === 0) return;
    const assetIds = [...selectedAssetIds];
    const generation = actionGeneration;
    isAddingToAlbum = true;
    let succeeded = 0;
    try { succeeded = (await addAssetsToAlbum(assetIds, albumId)).succeeded; } catch { /* Report failure below. */ }
    if (generation !== actionGeneration) return;
    isAddingToAlbum = false;
    isAddToAlbumOpen = false;
    clearSelection();
    selectionResult = { action: "album", succeeded, total: assetIds.length };
  }

  function openAsset(asset: TimelineAsset, source: HTMLElement) {
    if (isSelectionMode) return;
    if (removeUnfavoritedAssets) favoritesLightbox.open(asset.id);
    selectedAsset = { ...asset, isFavorite: isFavorite(asset) };
    opener = source;
  }
  function closeLightbox() {
    if (removeUnfavoritedAssets) favoritesLightbox.close();
    selectedAsset = null;
    queueMicrotask(() => opener?.focus());
  }
  function navigateLightboxAsset(delta: number, current: TimelineAsset): TimelineAsset | null | Promise<TimelineAsset | null> {
    if (removeUnfavoritedAssets) {
      return favoritesLightbox.navigate(delta, current, isFavorite);
    }
    if (!lightboxAssets.length) return null;
    const index = lightboxAssets.findIndex((candidate) => candidate.id === current.id);
    const next = lightboxAssets[(index + delta + lightboxAssets.length) % lightboxAssets.length];
    return { ...next, isFavorite: isFavorite(next) };
  }
  function handleKeydown(event: KeyboardEvent) {
    if (!selectedAsset && isSelectionMode && event.key === "Escape" && !isAddToAlbumOpen) {
      event.preventDefault();
      clearSelection();
    }
  }

  function handleWindowResize() {
    if (viewport && viewport.clientWidth > 0) {
      viewportWidth = viewport.clientWidth;
    }
  }


</script>

<svelte:window onkeydown={handleKeydown} onresize={handleWindowResize} />

<div class="relative flex h-full min-h-0 flex-col">
  {#if (timeline.hasInitialLoadError && !timeline.isLoading) || (timeline.isOffline && !timeline.isLoading) || showRefreshButton}
    <div class="mb-2 flex shrink-0 items-center justify-end gap-3">
      {#snippet retryButton()}
        {#if showRefreshButton}
          <button
            type="button"
            class="font-semibold underline underline-offset-2"
            aria-label={$t("timeline_retry_connection")}
            onclick={() => void timeline.refresh()}
          >{$t("timeline_retry")}</button>
        {/if}
      {/snippet}
      {#if timeline.hasInitialLoadError && !timeline.isLoading}
        <div class="flex items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-200" role="alert">
          <span>{loadErrorMessage || $t("timeline_offline")}</span>
          {@render retryButton()}
        </div>
      {:else if timeline.isOffline && !timeline.isLoading}
        <div class="flex items-center gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-200" role="status">
          <span>{offlineMessage || $t("timeline_offline")}</span>
          {@render retryButton()}
        </div>
      {/if}
      {#if showRefreshButton}
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
      {/if}
    </div>
  {:else}
    <div class="mb-2 h-8 shrink-0" aria-hidden="true"></div>
  {/if}
  {#if isSelectionMode}
    <div class="mb-3 mr-16 grid shrink-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-lg border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-blue-950 shadow-sm dark:border-blue-900/70 dark:bg-blue-950/30 dark:text-blue-100 sm:flex sm:flex-wrap">
      <span class="min-w-0 truncate font-medium sm:mr-auto">{$t("timeline_selection_count", { values: { count: selectionCount.toLocaleString(numberLocale) } })}</span>
      <div class="col-span-2 flex flex-wrap justify-end gap-1 border-t border-blue-200 pt-2 dark:border-blue-900/70 sm:contents sm:border-0 sm:pt-0">
        <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_selection_download")} aria-label={$t("timeline_selection_download")} disabled={isSelectionActionBusy} onclick={() => void downloadSelection()}><Download class="h-4 w-4 {isDownloadingSelection ? 'animate-spin' : ''}" /></button>
        {#if hasNonFavoriteSelection}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_add_favorite")} aria-label={$t("timeline_add_favorite")} disabled={isSelectionActionBusy} onclick={() => void applyFavoriteToSelection(true)}><Heart class="h-4 w-4" /></button>
        {/if}
        {#if hasFavoriteSelection}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_remove_favorite")} aria-label={$t("timeline_remove_favorite")} disabled={isSelectionActionBusy} onclick={() => void applyFavoriteToSelection(false)}><HeartOff class="h-4 w-4" /></button>
        {/if}
        {#if albumCatalogue}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_add_to_album")} aria-label={$t("timeline_add_to_album")} disabled={isSelectionActionBusy} onclick={openAddSelectionToAlbum}><Album class="h-4 w-4" /></button>
        {/if}
        <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_selection_finish")} aria-label={$t("timeline_selection_finish")} disabled={isSelectionActionBusy} onclick={clearSelection}><X class="h-4 w-4" /></button>
      </div>
    </div>
  {/if}
  {#if removeUnfavoritedAssets && favoritesLightbox.updateError}
    <div class="mb-3 mr-16 flex items-center justify-between gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-200" role="alert">
      <span>{favoritesLightbox.updateError}</span>
      <button
        type="button"
        class="rounded p-1 hover:bg-red-100 dark:hover:bg-red-900/40"
        aria-label={$t("dismiss_error")}
        onclick={() => favoritesLightbox.clearError()}
      ><X class="h-4 w-4" /></button>
    </div>
  {/if}
  {#if selectionResult}
    {@const resultKey = selectionResult.succeeded === selectionResult.total
      ? `timeline_selection_${selectionResult.action}_success`
      : selectionResult.succeeded === 0
        ? `timeline_selection_${selectionResult.action}_failed`
        : `timeline_selection_${selectionResult.action}_partial`}
    <p class="mb-3 mr-16 w-auto shrink-0 break-words rounded-lg px-3 py-2 text-sm leading-5 {selectionResult.succeeded === 0 ? 'bg-red-50 text-red-800 dark:bg-red-950/30 dark:text-red-200' : 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200'}" role={selectionResult.succeeded === 0 ? "alert" : "status"}>
      {$t(resultKey, { values: { count: selectionResult.total.toLocaleString(numberLocale), succeeded: selectionResult.succeeded.toLocaleString(numberLocale) } })}
    </p>
  {/if}
  <div bind:this={viewport} use:measureViewport class="min-h-0 flex-1 overflow-y-auto pr-16 scrollbar-none" aria-label={ariaLabel || $t("timeline_title")} onscroll={syncActiveBucket}>
    {#if timeline.isLoading && loadingMessage}
      <p role="status" class="py-4 text-sm text-slate-500 dark:text-zinc-400">{loadingMessage}</p>
    {:else if timeline.buckets.length === 0 && !timeline.isLoading && !timeline.hasInitialLoadError && emptyMessage}
      <p role="status" class="py-4 text-sm text-slate-500 dark:text-zinc-400">{emptyMessage}</p>
    {/if}
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
                    <div class="group relative overflow-hidden rounded-lg bg-slate-200 dark:bg-zinc-800">
                      <button type="button" class="block w-full text-left" aria-label={item.asset.type === "VIDEO" ? $t("timeline_open_video") : $t("timeline_open_photo")} onclick={(event) => openAsset(item.asset, event.currentTarget)}>
                      <img src={protocolSrc("lymic-thumb", "asset", item.asset.id)} alt="" loading="lazy" width={item.asset.width} height={item.asset.height} style:height={`${item.height}px`} class="w-full object-cover" />
                      {#if isFavorite(item.asset)}
                        <span class="absolute right-2 top-2 rounded-full bg-black/70 p-1.5 text-white" aria-label={$t("timeline_favorite_marker")}><Heart class="h-3.5 w-3.5 fill-current" /></span>
                      {/if}
                      {#if item.asset.type === "VIDEO"}
                        <span class="absolute bottom-2 left-2 inline-flex items-center gap-1 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-white"><Play class="h-3 w-3 fill-current" />{formatDuration(item.asset.duration)}</span>
                      {/if}
                      </button>
                      <label class="absolute left-0 top-0 z-10 flex h-11 w-11 cursor-pointer items-center justify-center opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100 {isSelectionMode || isAssetSelected(item.asset.id) ? 'opacity-100' : ''}">
                        <input type="checkbox" class="peer sr-only" aria-label={$t("timeline_select_asset")} checked={isAssetSelected(item.asset.id)} onchange={() => toggleAssetSelection(item.asset.id)} />
                        <span class="flex h-5 w-5 items-center justify-center rounded-full border-2 border-white bg-black/45 text-white shadow-sm peer-focus-visible:ring-2 peer-focus-visible:ring-blue-500 peer-focus-visible:ring-offset-2 {isAssetSelected(item.asset.id) ? 'border-blue-600 bg-blue-600 dark:border-sky-500 dark:bg-sky-500' : ''}">
                          {#if isAssetSelected(item.asset.id)}<Check class="h-3 w-3 stroke-[3]" />{/if}
                        </span>
                      </label>
                    </div>
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
  <Lightbox asset={selectedAsset} refreshGeneration={timeline.refreshGeneration} detailsCache={assetDetailsById} bind:updatingFavoriteAssetIds navigateAsset={navigateLightboxAsset} onClose={closeLightbox} onFavoriteChange={updateFavoriteState} {serverUrl} {onOpenExternalUrl} {getAssetDetails} setAssetFavorite={updateAssetFavorite} {chooseDownloadDestination} {downloadAsset} {albumCatalogue} {addAssetsToAlbum} />
{/if}
{#if albumCatalogue}
  <AddToAlbumModal
    isOpen={isAddToAlbumOpen}
    catalogue={albumCatalogue}
    isAssigning={isAddingToAlbum}
    assignmentError={addToAlbumError}
    onConfirm={(albumId) => void assignSelectedAssetToAlbum(albumId)}
    onCancel={() => { if (!isAddingToAlbum) { isAddToAlbumOpen = false; addToAlbumError = ""; } }}
  />
{/if}
