<script lang="ts">
  import { protocolSrc } from "../tauri/protocol-src";
  import { tick, untrack, type Snippet } from "svelte";
  import { Album, ArchiveRestore, ArchiveX, Check, Download, Heart, HeartOff, Play, RefreshCw, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineController } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";
  import type { AssetTag, AssetVisibility, AssetVisibilityResult, TimelineAsset, TrashRestoreResult } from "../contracts";
  import type { RemoteAssetDesktopAdapter } from "../tauri/desktop-adapter";
  import { calculateMasonryLayout } from "./timeline-masonry";
  import Lightbox from "./Lightbox.svelte";
  import AddToAlbumModal from "./AddToAlbumModal.svelte";
  import { createRemoteAssetActions } from "./remote-asset-actions.svelte";
  import { createRemoteAssetFavorites } from "./remote-asset-favorites.svelte";
  import { archiveFeedbackMessage, createRemoteAssetArchive, type ArchiveAction } from "./remote-asset-archive";
  import { createRemoteAssetRestore, restoreFeedbackMessage } from "./remote-asset-restore";
  import { createTimelineSelectionActions } from "./timeline-selection-actions.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";

  interface Props {
    timeline: TimelineController;
    serverUrl?: string;
    /** The Remote Asset operations of the Lightbox and multi-selection. */
    desktop: RemoteAssetDesktopAdapter;
    albumCatalogue?: TargetAlbumCatalogue;
    showScrubber?: boolean;
    showRefreshButton?: boolean;
    removeUnfavoritedAssets?: boolean;
    loadingMessage?: string;
    emptyMessage?: string;
    loadErrorMessage?: string;
    offlineMessage?: string;
    /** Shows a failed load as an error alert with Retry, even over cached assets, instead of the offline status. */
    failureIsError?: boolean;
    ariaLabel?: string;
    /** Fixed explanatory text shown above the assets. */
    hintMessage?: string;
    /** When false, the Lightbox and multi-selection offer no Favorite and no Add to Album (viewing, Download and Restore only). */
    allowFavoriteAndAlbum?: boolean;
    /** Offers Restore in the Lightbox and in multi-selection: Trashed Assets only. */
    restoreTrashedAssets?(assetIds: string[]): Promise<TrashRestoreResult>;
    /** Writes the visibility for an Archive or Unarchive; needed together with `archiveAction`. */
    setAssetsVisibility?(assetIds: string[], visibility: AssetVisibility): Promise<AssetVisibilityResult>;
    /** The one Archive action the multi-selection offers (Unarchive in the Archive Timeline); leave it out where there is none. */
    archiveAction?: ArchiveAction;
    /** Where to scroll to once the first buckets are shown, for a view that is reopened where the user left it. */
    initialScrollTop?: number;
    /** Reports the scroll position when the view opens and whenever the user scrolls. */
    onScrollPosition?(scrollTop: number): void;
    /** Opens the Tag Timeline of a Tag chip selected in the Lightbox, which has been closed by then. */
    onSelectTag?(tag: AssetTag): void;
    /** Shows one ordered list instead of month sections: no month headings and no Scrubber (Context Search Results, ADR-0009). */
    flat?: boolean;
    /** Called while the end of the list is near the viewport, to load a further page. Pass it only while a page can be loaded. */
    onLoadMore?(): void;
    /** Shown below the list, inside the scrolling area: a further page loading or failing. */
    footer?: Snippet;
  }

  let {
    timeline,
    serverUrl = "",
    desktop,
    albumCatalogue,
    showScrubber = true,
    showRefreshButton = true,
    removeUnfavoritedAssets = false,
    loadingMessage = "",
    emptyMessage = "",
    loadErrorMessage = "",
    offlineMessage = "",
    failureIsError = false,
    ariaLabel = "",
    hintMessage = "",
    allowFavoriteAndAlbum = true,
    restoreTrashedAssets,
    setAssetsVisibility,
    archiveAction,
    initialScrollTop = 0,
    onScrollPosition,
    onSelectTag,
    flat = false,
    onLoadMore,
    footer,
  }: Props = $props();
  const targetAlbumCatalogue = $derived(allowFavoriteAndAlbum ? albumCatalogue : undefined);
  const showLoadError = $derived(!timeline.isLoading && (timeline.hasInitialLoadError || (failureIsError && timeline.isOffline)));
  const numberLocale = $derived(toBcp47Locale($locale));
  const scrubberIndex = $derived(flat ? [] : buildTimelineScrubberIndex(timeline.buckets));
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
  let updatingFavoriteAssetIds = $state<Record<string, true>>({});
  let isAddToAlbumOpen = $state(false);
  let opener = $state<HTMLElement | null>(null);
  const assetActions = createRemoteAssetActions({
    get getAssetDetails() { return desktop.getAssetDetails; },
    get chooseDownloadDestination() { return desktop.chooseDownloadDestination; },
    get downloadAsset() { return desktop.downloadAsset; },
    get downloadArchive() { return desktop.downloadArchive; },
  });
  const favorites = createRemoteAssetFavorites({
    get timeline() { return timeline; },
    get setAssetFavorite() { return desktop.setAssetFavorite; },
    get removeUnfavorited() { return removeUnfavoritedAssets; },
    get errorMessage() { return $t("timeline_favorite_update_error"); },
    isFavorite: (asset) => assetActions.isFavorite(asset),
    onReflect: reflectFavoriteState,
    onRemoved: (assetId) => selection.forget(assetId),
  });
  const restore = createRemoteAssetRestore({
    get timeline() { return timeline; },
    restoreTrashedAssets: (assetIds) => restoreTrashedAssets!(assetIds),
    onRemoved: (assetId) => selection.forget(assetId),
  });
  const archive = createRemoteAssetArchive({
    get timeline() { return timeline; },
    setAssetsVisibility: (assetIds, visibility) => setAssetsVisibility!(assetIds, visibility),
    onRemoved: (assetId) => selection.forget(assetId),
  });
  const offersUnarchive = $derived(archiveAction === "unarchive" && setAssetsVisibility !== undefined);
  const selection = createTimelineSelectionActions({
    updateFavorites: (assetIds, isFavorite, isCurrent) => favorites.updateMany(assetIds, isFavorite, isCurrent),
    clearFavoriteError: () => favorites.clearError(),
    download: (assetIds, isCurrent) => assetActions.download(assetIds, isCurrent),
    addAssetsToAlbum: (assetIds, albumId) => desktop.addAssetsToAlbum(assetIds, albumId),
    get restore() { return restoreTrashedAssets ? (assetIds: string[]) => restore.restore(assetIds) : undefined; },
    get unarchive() { return offersUnarchive ? (assetIds: string[]) => archive.apply("unarchive", assetIds) : undefined; },
  });
  const selectedTimelineAssets = $derived(lightboxAssets.filter((asset) => selection.isSelected(asset.id)));
  const hasFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => favorites.isFavorite(asset)));
  const hasNonFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => !favorites.isFavorite(asset)));

  function resetSelectionAndModals() {
    selection.reset();
    isAddToAlbumOpen = false;
  }

  function resetLightbox() {
    selectedAsset = null;
    opener = null;
    updatingFavoriteAssetIds = {};
    assetActions.invalidate();
    favorites.reset();
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
      assetActions.invalidate(Object.keys(updatingFavoriteAssetIds));
      resetSelectionAndModals();
      if (timeline.buckets.length === 0) {
        resetLightbox();
        return;
      }
      favorites.clearError();
    });
  });

  $effect(() => {
    timeline;
    untrack(() => {
      assetActions.invalidate();
      resetSelectionAndModals();
      favorites.reset();
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

  let scrollRestored = false;
  $effect(() => {
    const hasBuckets = timeline.buckets.length > 0;
    if (scrollRestored || !viewport) return;
    if (initialScrollTop <= 0) {
      scrollRestored = true;
      untrack(() => onScrollPosition?.(0));
      return;
    }
    // The month sections reserve their height, so the position exists as soon as the buckets do.
    if (!hasBuckets) return;
    scrollRestored = true;
    void tick().then(() => {
      viewport.scrollTop = initialScrollTop;
      onScrollPosition?.(viewport.scrollTop);
    });
  });

  function syncActiveBucket() {
    onScrollPosition?.(viewport.scrollTop);
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

  function observeEnd(node: HTMLElement) {
    if (typeof IntersectionObserver === "undefined") return {};
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) onLoadMore?.();
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

  function reflectFavoriteState(assetId: string, isFavorite: boolean) {
    assetActions.reflectFavorite(assetId, isFavorite);
    if (selectedAsset?.id === assetId) selectedAsset = { ...selectedAsset, isFavorite };
  }

  /** Restores the Lightbox's asset; on success the Lightbox closes because the asset left the Trash Timeline. */
  async function restoreFromLightbox(assetId: string) {
    const { outcome, applied } = await selection.restoreOne(assetId);
    if (applied) {
      favorites.close();
      selectedAsset = null;
      opener = null;
    }
    return outcome;
  }

  function openAddSelectionToAlbum() {
    if (selection.count === 0 || !targetAlbumCatalogue || selection.isBusy) return;
    selection.clearResult();
    isAddToAlbumOpen = true;
  }

  async function assignSelectedAssetToAlbum(albumId: string) {
    if (await selection.addToAlbum(albumId)) isAddToAlbumOpen = false;
  }

  function openAsset(asset: TimelineAsset, source: HTMLElement) {
    if (selection.isSelectionMode) return;
    favorites.open(asset.id);
    selectedAsset = { ...asset, isFavorite: favorites.isFavorite(asset) };
    opener = source;
  }
  function closeLightbox() {
    favorites.close();
    selectedAsset = null;
    queueMicrotask(() => opener?.focus());
  }
  function handleKeydown(event: KeyboardEvent) {
    if (!selectedAsset && selection.isSelectionMode && event.key === "Escape" && !isAddToAlbumOpen) {
      event.preventDefault();
      selection.clear();
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
  {#if (showLoadError || (timeline.isOffline && !timeline.isLoading)) || showRefreshButton}
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
      {#if showLoadError}
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
  {#if selection.isSelectionMode}
    <div class="mb-3 mr-16 grid shrink-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-lg border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-blue-950 shadow-sm dark:border-blue-900/70 dark:bg-blue-950/30 dark:text-blue-100 sm:flex sm:flex-wrap">
      <span class="min-w-0 truncate font-medium sm:mr-auto">{$t("timeline_selection_count", { values: { count: selection.count.toLocaleString(numberLocale) } })}</span>
      <div class="col-span-2 flex flex-wrap justify-end gap-1 border-t border-blue-200 pt-2 dark:border-blue-900/70 sm:contents sm:border-0 sm:pt-0">
        <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_selection_download")} aria-label={$t("timeline_selection_download")} disabled={selection.isBusy} onclick={() => void selection.download()}><Download class="h-4 w-4 {selection.busyAction === 'download' ? 'animate-spin' : ''}" /></button>
        {#if restoreTrashedAssets}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("trash_restore")} aria-label={$t("trash_restore")} aria-busy={selection.busyAction === 'restore'} disabled={selection.isBusy} onclick={() => void selection.restore()}><ArchiveRestore class="h-4 w-4 {selection.busyAction === 'restore' ? 'animate-pulse' : ''}" /></button>
        {/if}
        {#if offersUnarchive}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("archive_unarchive")} aria-label={$t("archive_unarchive")} aria-busy={selection.busyAction === 'unarchive'} disabled={selection.isBusy} onclick={() => void selection.unarchive()}><ArchiveX class="h-4 w-4 {selection.busyAction === 'unarchive' ? 'animate-pulse' : ''}" /></button>
        {/if}
        {#if allowFavoriteAndAlbum && hasNonFavoriteSelection}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_add_favorite")} aria-label={$t("timeline_add_favorite")} disabled={selection.isBusy} onclick={() => void selection.setFavorite(true)}><Heart class="h-4 w-4" /></button>
        {/if}
        {#if allowFavoriteAndAlbum && hasFavoriteSelection}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_remove_favorite")} aria-label={$t("timeline_remove_favorite")} disabled={selection.isBusy} onclick={() => void selection.setFavorite(false)}><HeartOff class="h-4 w-4" /></button>
        {/if}
        {#if targetAlbumCatalogue}
          <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_add_to_album")} aria-label={$t("timeline_add_to_album")} disabled={selection.isBusy} onclick={openAddSelectionToAlbum}><Album class="h-4 w-4" /></button>
        {/if}
        <button type="button" class="rounded-md p-2 text-blue-700 hover:bg-blue-100 disabled:opacity-50 dark:text-blue-200 dark:hover:bg-blue-950/70" title={$t("timeline_selection_finish")} aria-label={$t("timeline_selection_finish")} disabled={selection.isBusy} onclick={() => selection.clear()}><X class="h-4 w-4" /></button>
      </div>
    </div>
  {/if}
  {#if favorites.updateError}
    <div class="mb-3 mr-16 flex items-center justify-between gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-900/60 dark:bg-red-950/30 dark:text-red-200" role="alert">
      <span>{favorites.updateError}</span>
      <button
        type="button"
        class="rounded p-1 hover:bg-red-100 dark:hover:bg-red-900/40"
        aria-label={$t("dismiss_error")}
        onclick={() => favorites.clearError()}
      ><X class="h-4 w-4" /></button>
    </div>
  {/if}
  {#if selection.result}
    {@const selectionResult = selection.result}
    {@const resultKey = selectionResult.succeeded === selectionResult.total
      ? `timeline_selection_${selectionResult.action}_success`
      : selectionResult.succeeded === 0
        ? `timeline_selection_${selectionResult.action}_failed`
        : `timeline_selection_${selectionResult.action}_partial`}
    <p class="mb-3 mr-16 w-auto shrink-0 break-words rounded-lg px-3 py-2 text-sm leading-5 {selectionResult.succeeded === 0 ? 'bg-red-50 text-red-800 dark:bg-red-950/30 dark:text-red-200' : 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200'}" role={selectionResult.succeeded === 0 ? "alert" : "status"}>
      {$t(resultKey, { values: { count: selectionResult.total.toLocaleString(numberLocale), succeeded: selectionResult.succeeded.toLocaleString(numberLocale) } })}
    </p>
  {/if}
  {#if selection.restoreFeedback}
    {@const restoreFeedback = selection.restoreFeedback}
    {@const isSuccess = restoreFeedback.kind === "success"}
    <p class="mb-3 mr-16 w-auto shrink-0 break-words rounded-lg px-3 py-2 text-sm leading-5 {isSuccess ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200' : 'bg-red-50 text-red-800 dark:bg-red-950/30 dark:text-red-200'}" role={isSuccess ? "status" : "alert"}>
      {restoreFeedbackMessage(restoreFeedback, $t, numberLocale)}
    </p>
  {/if}
  {#if selection.archiveFeedback}
    {@const archiveFeedback = selection.archiveFeedback}
    {@const isSuccess = archiveFeedback.feedback.kind === "success"}
    <p class="mb-3 mr-16 w-auto shrink-0 break-words rounded-lg px-3 py-2 text-sm leading-5 {isSuccess ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200' : 'bg-red-50 text-red-800 dark:bg-red-950/30 dark:text-red-200'}" role={isSuccess ? "status" : "alert"}>
      {archiveFeedbackMessage(archiveFeedback.action, archiveFeedback.feedback, $t, numberLocale)}
    </p>
  {/if}
  {#if hintMessage}
    <p class="mb-3 mr-16 shrink-0 text-sm text-slate-500 dark:text-zinc-400">{hintMessage}</p>
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
          style:min-height={`${layout.height + (flat ? 0 : 52)}px`}
        >
          {#if !flat}
            <div class="flex items-baseline justify-between">
              <h2 class="text-xl font-bold text-slate-900 dark:text-slate-100">{monthLabel(bucket.timeBucket)}</h2>
              <p class="text-sm text-slate-500 dark:text-zinc-400">{$t("timeline_asset_count", { values: { count: bucket.count.toLocaleString(numberLocale) } })}</p>
            </div>
          {/if}
          {#if activeBuckets[bucket.timeBucket] && timeline.assetsFor(bucket.timeBucket).length > 0}
            <div class="flex gap-2" style:height={`${layout.height}px`} aria-label={$t("timeline_masonry_grid")}>
              {#each layout.columns as column, columnIndex (columnIndex)}
                <div class="flex min-w-0 flex-1 flex-col gap-2">
                  {#each column as item (item.asset.id)}
                    <div class="group relative overflow-hidden rounded-lg bg-slate-200 dark:bg-zinc-800">
                      <button type="button" class="block w-full text-left" aria-label={item.asset.type === "VIDEO" ? $t("timeline_open_video") : $t("timeline_open_photo")} onclick={(event) => openAsset(item.asset, event.currentTarget)}>
                      <img src={protocolSrc("lymic-thumb", "asset", item.asset.id)} alt="" loading="lazy" width={item.asset.width} height={item.asset.height} style:height={`${item.height}px`} class="w-full object-cover" />
                      {#if favorites.isFavorite(item.asset)}
                        <span class="absolute right-2 top-2 rounded-full bg-black/70 p-1.5 text-white" aria-label={$t("timeline_favorite_marker")}><Heart class="h-3.5 w-3.5 fill-current" /></span>
                      {/if}
                      {#if item.asset.type === "VIDEO"}
                        <span class="absolute bottom-2 left-2 inline-flex items-center gap-1 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-white"><Play class="h-3 w-3 fill-current" />{formatDuration(item.asset.duration)}</span>
                      {/if}
                      </button>
                      <label class="absolute left-0 top-0 z-10 flex h-11 w-11 cursor-pointer items-center justify-center opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100 {selection.isSelectionMode || selection.isSelected(item.asset.id) ? 'opacity-100' : ''}">
                        <input type="checkbox" class="peer sr-only" aria-label={$t("timeline_select_asset")} checked={selection.isSelected(item.asset.id)} onchange={() => selection.toggle(item.asset.id)} />
                        <span class="flex h-5 w-5 items-center justify-center rounded-full border-2 border-white bg-black/45 text-white shadow-sm peer-focus-visible:ring-2 peer-focus-visible:ring-blue-500 peer-focus-visible:ring-offset-2 {selection.isSelected(item.asset.id) ? 'border-blue-600 bg-blue-600 dark:border-sky-500 dark:bg-sky-500' : ''}">
                          {#if selection.isSelected(item.asset.id)}<Check class="h-3 w-3 stroke-[3]" />{/if}
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
    {#if onLoadMore}
      <!-- A new sentinel after every page: one that is still in view re-triggers, so a tall window keeps loading. -->
      {#key timeline.buckets.reduce((total, bucket) => total + bucket.count, 0)}
        <div use:observeEnd class="h-px" aria-hidden="true"></div>
      {/key}
    {/if}
    {@render footer?.()}
  </div>
  {#if showScrubber && !flat && scrubberMonths.length > 0}
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
  <Lightbox asset={selectedAsset} {assetActions} bind:updatingFavoriteAssetIds navigateAsset={favorites.navigate} onClose={closeLightbox} onFavoriteChange={favorites.reflect} {serverUrl} onOpenExternalUrl={desktop.openExternalUrl} setAssetFavorite={favorites.updateOne} albumCatalogue={targetAlbumCatalogue} addAssetsToAlbum={desktop.addAssetsToAlbum} allowFavorite={allowFavoriteAndAlbum} onRestore={restoreTrashedAssets ? restoreFromLightbox : undefined} {onSelectTag} />
{/if}
{#if targetAlbumCatalogue}
  <AddToAlbumModal
    isOpen={isAddToAlbumOpen}
    catalogue={targetAlbumCatalogue}
    isAssigning={selection.busyAction === 'album'}
    assignmentError=""
    onConfirm={(albumId) => void assignSelectedAssetToAlbum(albumId)}
    onCancel={() => { if (selection.busyAction !== 'album') { isAddToAlbumOpen = false; } }}
  />
{/if}
