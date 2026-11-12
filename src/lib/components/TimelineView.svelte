<script lang="ts">
  import { flushSync, onDestroy, untrack } from "svelte";
  import { Album, Check, ChevronLeft, ChevronRight, Download, ExternalLink, Heart, HeartOff, Info, Play, RefreshCw, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { TimelineViewState } from "../controllers/timeline-controller.svelte";
  import { buildTimelineScrubberIndex } from "./timeline-scrubber";
  import type { AlbumAssignmentResult, AssetDetails, TimelineAsset } from "../contracts";
  import { calculateMasonryLayout } from "./timeline-masonry";
  import AddToAlbumModal from "./AddToAlbumModal.svelte";
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
    emptyMessage?: string;
    loadErrorMessage?: string;
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
  let isAssetDetailsLoading = $state(false);
  let isInfoOpen = $state(false);
  let favoriteOverrides = $state<Record<string, boolean>>({});
  let updatingFavoriteAssetIds = $state<Record<string, true>>({});
  let favoriteError = $state("");
  let downloadingAssetId = $state<string | null>(null);
  let downloadMessage = $state("");
  let downloadError = $state("");
  let isAddToAlbumOpen = $state(false);
  let isAddingToAlbum = $state(false);
  let addToAlbumMessage = $state("");
  let addToAlbumError = $state("");
  let addToAlbumMessageTimeout: ReturnType<typeof setTimeout> | null = null;
  let selectedIndex = $state(-1);
  let opener = $state<HTMLElement | null>(null);
  let detailsGeneration = 0;
  let selectedAssetIds = $state<Set<string>>(new Set());
  let isApplyingSelection = $state(false);
  let isDownloadingSelection = $state(false);
  let selectionResult = $state<{ action: "favorite" | "album" | "download"; succeeded: number; total: number } | null>(null);
  let addToAlbumSource = $state<"lightbox" | "selection" | null>(null);
  const selectionCount = $derived(selectedAssetIds.size);
  const isSelectionMode = $derived(selectionCount > 0);
  const selectedTimelineAssets = $derived(lightboxAssets.filter((asset) => selectedAssetIds.has(asset.id)));
  const hasFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => isFavorite(asset)));
  const hasNonFavoriteSelection = $derived(selectedTimelineAssets.some((asset) => !isFavorite(asset)));
  const isSelectionActionBusy = $derived(isApplyingSelection || isAddingToAlbum || isDownloadingSelection);

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
      detailsGeneration += 1;
      assetDetailsById.clear();
      favoriteOverrides = Object.fromEntries(
        Object.keys(updatingFavoriteAssetIds).map((assetId) => [assetId, favoriteOverrides[assetId]]),
      );
      clearSelection();
      if (selectedAsset) loadAssetDetails(selectedAsset);
    });
  });

  $effect(() => {
    timeline;
    untrack(clearSelection);
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

  function cameraLabel(make: string | null | undefined, model: string | null | undefined) {
    if (!model) return make ?? "";
    if (!make || model.toLocaleLowerCase().startsWith(make.toLocaleLowerCase())) return model;
    return `${make} ${model}`;
  }

  function locationLabel(...values: Array<string | null | undefined>) {
    const seen = new Set<string>();
    return values.filter((value): value is string => {
      if (!value || seen.has(value.toLocaleLowerCase())) return false;
      seen.add(value.toLocaleLowerCase());
      return true;
    }).join(", ");
  }

  const cameraName = $derived(cameraLabel(assetDetails?.cameraMake, assetDetails?.cameraModel));
  const locationName = $derived(locationLabel(assetDetails?.city, assetDetails?.state, assetDetails?.country));
  const imageWidth = $derived(assetDetails?.imageWidth ?? selectedAsset?.width ?? null);
  const imageHeight = $derived(assetDetails?.imageHeight ?? selectedAsset?.height ?? null);
  const fileSize = $derived(assetDetails?.fileSize ?? selectedAsset?.fileSize ?? null);

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
    favoriteOverrides = { ...favoriteOverrides, [assetId]: isFavorite };
    const currentDetails = assetDetailsById.get(assetId);
    if (currentDetails) {
      const updatedDetails = { ...currentDetails, isFavorite };
      assetDetailsById.set(assetId, updatedDetails);
      if (selectedAsset?.id === assetId) assetDetails = updatedDetails;
    }
    if (selectedAsset?.id === assetId) selectedAsset = { ...selectedAsset, isFavorite };
  }

  async function applyFavoriteToSelection(isFavorite: boolean) {
    const assetIds = [...selectedAssetIds];
    if (assetIds.length === 0 || isSelectionActionBusy) return;

    selectionResult = null;
    isApplyingSelection = true;
    let succeeded = 0;
    for (const assetId of assetIds) {
      try {
        await setAssetFavorite(assetId, isFavorite);
        updateFavoriteState(assetId, isFavorite);
        succeeded += 1;
      } catch {
        // Continue so the result can accurately report a partial update.
      }
    }
    isApplyingSelection = false;
    clearSelection();
    selectionResult = { action: "favorite", succeeded, total: assetIds.length };
  }

  async function downloadSelection() {
    const assetIds = [...selectedAssetIds];
    if (assetIds.length === 0 || isSelectionActionBusy) return;

    selectionResult = null;
    isDownloadingSelection = true;
    try {
      if (assetIds.length === 1) {
        const assetId = assetIds[0];
        let details = assetDetailsById.get(assetId);
        if (!assetDetailsById.has(assetId)) {
          details = await getAssetDetails(assetId);
          assetDetailsById.set(assetId, details);
        }
        if (!details?.originalFileName) throw new Error("Original file name is unavailable");
        const destinationPath = await chooseDownloadDestination(details.originalFileName);
        if (!destinationPath) return;
        await downloadAsset(assetId, destinationPath);
      } else {
        await downloadArchive(assetIds);
      }
      selectionResult = { action: "download", succeeded: assetIds.length, total: assetIds.length };
    } catch {
      selectionResult = { action: "download", succeeded: 0, total: assetIds.length };
    } finally {
      isDownloadingSelection = false;
    }
  }

  function loadAssetDetails(asset: TimelineAsset) {
    favoriteError = "";
    if (assetDetailsById.has(asset.id)) {
      assetDetails = assetDetailsById.get(asset.id) ?? null;
      isAssetDetailsLoading = false;
      return;
    }
    assetDetails = null;
    isAssetDetailsLoading = true;
    const requestGeneration = detailsGeneration;
    void getAssetDetails(asset.id)
      .then((details) => {
        if (requestGeneration !== detailsGeneration) return;
        const favorite = favoriteOverrides[asset.id];
        const resolvedDetails = details && favorite === undefined ? details : details && { ...details, isFavorite: favorite };
        assetDetailsById.set(asset.id, resolvedDetails);
        if (selectedAsset?.id === asset.id) {
          assetDetails = resolvedDetails;
          isAssetDetailsLoading = false;
        }
      })
      // Details enhance the Info drawer; a failure must not disrupt asset viewing.
      .catch(() => {
        if (requestGeneration === detailsGeneration && selectedAsset?.id === asset.id) isAssetDetailsLoading = false;
      });
  }

  async function toggleFavorite() {
    if (!selectedAsset) return;

    const assetId = selectedAsset.id;
    if (updatingFavoriteAssetIds[assetId]) return;
    const previousFavorite = isFavorite(selectedAsset);
    const nextFavorite = !previousFavorite;
    updateFavoriteState(assetId, nextFavorite);
    favoriteError = "";
    updatingFavoriteAssetIds = { ...updatingFavoriteAssetIds, [assetId]: true };

    try {
      await setAssetFavorite(assetId, nextFavorite);
    } catch {
      updateFavoriteState(assetId, previousFavorite);
      if (selectedAsset?.id === assetId) {
        favoriteError = $t("timeline_favorite_update_error");
      }
    } finally {
      const { [assetId]: _completed, ...remainingUpdates } = updatingFavoriteAssetIds;
      updatingFavoriteAssetIds = remainingUpdates;
    }
  }

  async function downloadOriginal() {
    if (!selectedAsset || !assetDetails?.originalFileName || downloadingAssetId) return;

    const assetId = selectedAsset.id;
    const originalFileName = assetDetails.originalFileName;
    downloadMessage = "";
    downloadError = "";
    downloadingAssetId = assetId;
    try {
      const destinationPath = await chooseDownloadDestination(originalFileName);
      if (!destinationPath) return;
      await downloadAsset(assetId, destinationPath);
      if (selectedAsset?.id === assetId) downloadMessage = $t("timeline_download_saved");
    } catch {
      if (selectedAsset?.id === assetId) downloadError = $t("timeline_download_failed");
    } finally {
      downloadingAssetId = null;
    }
  }

  function openAddToAlbum() {
    if (!selectedAsset || !albumCatalogue) return;
    clearAddToAlbumMessage();
    addToAlbumError = "";
    addToAlbumSource = "lightbox";
    isAddToAlbumOpen = true;
  }

  function openAddSelectionToAlbum() {
    if (selectionCount === 0 || !albumCatalogue || isSelectionActionBusy) return;
    selectionResult = null;
    addToAlbumError = "";
    addToAlbumSource = "selection";
    isAddToAlbumOpen = true;
  }

  async function assignSelectedAssetToAlbum(albumId: string) {
    if (isAddingToAlbum || !addToAlbumSource) return;

    if (addToAlbumSource === "selection") {
      const assetIds = [...selectedAssetIds];
      if (assetIds.length === 0) return;

      isAddingToAlbum = true;
      let succeeded = 0;
      try {
        const result = await addAssetsToAlbum(assetIds, albumId);
        succeeded = result.succeeded;
      } catch {
        // Transport failures leave every selected asset unassigned.
      }
      isAddingToAlbum = false;
      isAddToAlbumOpen = false;
      addToAlbumSource = null;
      clearSelection();
      selectionResult = { action: "album", succeeded, total: assetIds.length };
      return;
    }

    if (!selectedAsset) return;

    const assetId = selectedAsset.id;
    addToAlbumError = "";
    isAddingToAlbum = true;
    try {
      const result = await addAssetsToAlbum([assetId], albumId);
      if (result.succeeded !== 1 || result.failed !== 0) throw new Error("Album assignment was not completed");
      if (selectedAsset?.id === assetId) {
        isAddToAlbumOpen = false;
        addToAlbumSource = null;
        showAddToAlbumMessage();
      }
    } catch {
      if (selectedAsset?.id === assetId) addToAlbumError = $t("timeline_add_to_album_failed");
    } finally {
      isAddingToAlbum = false;
    }
  }

  function clearAddToAlbumMessage() {
    if (addToAlbumMessageTimeout !== null) clearTimeout(addToAlbumMessageTimeout);
    addToAlbumMessageTimeout = null;
    addToAlbumMessage = "";
  }

  function showAddToAlbumMessage() {
    clearAddToAlbumMessage();
    addToAlbumMessage = $t("timeline_add_to_album_success");
    addToAlbumMessageTimeout = setTimeout(clearAddToAlbumMessage, 3_000);
  }

  onDestroy(clearAddToAlbumMessage);

  function openLightbox(asset: TimelineAsset, source: HTMLElement) {
    selectedIndex = lightboxAssets.findIndex((candidate) => candidate.id === asset.id);
    selectedAsset = asset;
    opener = source;
    downloadMessage = "";
    downloadError = "";
    isAddToAlbumOpen = false;
    isInfoOpen = false;
    clearAddToAlbumMessage();
    addToAlbumError = "";
    loadAssetDetails(asset);
  }

  function openAsset(asset: TimelineAsset, source: HTMLElement) {
    if (isSelectionMode) return;
    openLightbox(asset, source);
  }

  function hasRealDimensions(asset: TimelineAsset, width: number | null, height: number | null) {
    if (width === null || height === null || width <= 0 || height <= 0) return false;
    if (assetDetails?.imageWidth !== null && assetDetails?.imageHeight !== null) return true;
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
    if (downloadingAssetId || isAddingToAlbum) return;
    selectedAsset = null;
    assetDetails = null;
    isAssetDetailsLoading = false;
    isInfoOpen = false;
    favoriteError = "";
    downloadMessage = "";
    downloadError = "";
    isAddToAlbumOpen = false;
    addToAlbumSource = null;
    clearAddToAlbumMessage();
    addToAlbumError = "";
    selectedIndex = -1;
    queueMicrotask(() => opener?.focus());
  }

  function closeFromLightboxBackdrop() {
    if (isInfoOpen) isInfoOpen = false;
    else closeLightbox();
  }

  function navigate(delta: number) {
    if (!selectedAsset || downloadingAssetId || isAddingToAlbum || lightboxAssets.length === 0) return;
    const nextIndex = (selectedIndex + delta + lightboxAssets.length) % lightboxAssets.length;
    const nextAsset = lightboxAssets[nextIndex];
    flushSync(() => {
      selectedIndex = nextIndex;
      selectedAsset = nextAsset;
      assetDetails = null;
      isAssetDetailsLoading = true;
      downloadMessage = "";
      downloadError = "";
      isAddToAlbumOpen = false;
      clearAddToAlbumMessage();
      addToAlbumError = "";
    });
    loadAssetDetails(nextAsset);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (isAddToAlbumOpen) return;
    if (isSelectionMode && event.key === "Escape") {
      event.preventDefault();
      clearSelection();
      return;
    }
    if (!selectedAsset) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (isInfoOpen) isInfoOpen = false;
      else closeLightbox();
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
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/90 p-4" role="presentation" onclick={(event) => event.currentTarget === event.target && closeFromLightboxBackdrop()}>
    <dialog open aria-label={$t("timeline_lightbox_label")} aria-modal="true" class="m-0 h-full w-full max-h-none max-w-none border-0 bg-transparent p-0 text-white">
      <div class="relative flex h-full w-full min-h-0 min-w-0 items-center justify-center overflow-hidden">
        {#if isInfoOpen}
          <button type="button" tabindex="-1" class="absolute inset-0 z-10 cursor-default" aria-label={$t("timeline_close_info")} onclick={() => isInfoOpen = false}></button>
        {/if}
        <div class="absolute right-2 top-2 z-30 flex max-w-[calc(100%-1rem)] flex-wrap justify-end gap-2">
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:cursor-not-allowed" title={$t("timeline_download_original")} aria-label={$t("timeline_download_original")} aria-busy={downloadingAssetId === selectedAsset.id} disabled={!assetDetails?.originalFileName || downloadingAssetId !== null} onclick={() => void downloadOriginal()}><Download class="h-5 w-5 {downloadingAssetId === selectedAsset.id ? 'animate-spin' : ''}" /></button>
          {#if albumCatalogue}
            <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("timeline_add_to_album")} aria-label={$t("timeline_add_to_album")} disabled={isAddingToAlbum} onclick={openAddToAlbum}><Album class="h-5 w-5" /></button>
          {/if}
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={isFavorite(selectedAsset) ? $t("timeline_remove_favorite") : $t("timeline_add_favorite")} aria-label={isFavorite(selectedAsset) ? $t("timeline_remove_favorite") : $t("timeline_add_favorite")} aria-pressed={isFavorite(selectedAsset)} disabled={Boolean(updatingFavoriteAssetIds[selectedAsset.id])} onclick={() => void toggleFavorite()}><Heart class="h-5 w-5 {isFavorite(selectedAsset) ? 'fill-current' : ''}" /></button>
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80" title={$t("timeline_info")} aria-label={$t("timeline_info")} aria-pressed={isInfoOpen} onclick={() => isInfoOpen = !isInfoOpen}><Info class="h-5 w-5" /></button>
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("timeline_close_lightbox")} aria-label={$t("timeline_close_lightbox")} disabled={downloadingAssetId !== null || isAddingToAlbum} onclick={closeLightbox}><X class="h-5 w-5" /></button>
        </div>
        <button type="button" class="absolute left-2 top-1/2 z-30 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" aria-label={$t("timeline_previous_asset")} disabled={downloadingAssetId !== null || isAddingToAlbum} onclick={() => navigate(-1)}><ChevronLeft class="h-5 w-5" /></button>
        <button type="button" class="absolute right-2 top-1/2 z-30 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" aria-label={$t("timeline_next_asset")} disabled={downloadingAssetId !== null || isAddingToAlbum} onclick={() => navigate(1)}><ChevronRight class="h-5 w-5" /></button>
        {#if selectedAsset.type === "VIDEO"}
          <video class="relative z-20 h-full w-full max-h-full max-w-full object-contain" controls autoplay aria-label={$t("timeline_video_preview")} src={protocolSrc("lymic-asset", "video", selectedAsset.id)}>
            <track kind="captions" />
          </video>
        {:else}
          <img class="h-full w-full max-h-full max-w-full object-contain" alt={$t("timeline_photo_preview")} src={protocolSrc("lymic-asset", "asset", selectedAsset.id)} />
        {/if}
        {#if isInfoOpen}
          <aside class="absolute inset-y-0 right-0 z-20 w-[min(400px,calc(100vw-2rem))] overflow-y-auto bg-black/75 p-6 pt-20 text-sm shadow-2xl backdrop-blur-sm" aria-label={$t("timeline_info")}>
            {#if isAssetDetailsLoading}
              <p role="status">{$t("timeline_info_loading")}</p>
            {:else if assetDetails}
              <div class="space-y-6">
                {#if selectedAsset.takenAt || cameraName || locationName || exifDetails.length > 0 || assetDetails.lensModel}
                  <section class="space-y-3">
                    <h2 class="font-semibold text-white">{$t("timeline_info_capture")}</h2>
                    {#if selectedAsset.takenAt}<p><span class="text-white/60">{$t("timeline_info_date")}</span><br />{new Intl.DateTimeFormat(numberLocale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(selectedAsset.takenAt))}</p>{/if}
                    {#if cameraName}<p><span class="text-white/60">{$t("timeline_info_camera")}</span><br />{cameraName}</p>{/if}
                    {#if locationName}<p><span class="text-white/60">{$t("timeline_info_location")}</span><br />{locationName}</p>{/if}
                    {#if assetDetails.lensModel}<p><span class="text-white/60">{$t("timeline_info_lens")}</span><br />{assetDetails.lensModel}</p>{/if}
                    {#if exifDetails.length > 0}<p><span class="text-white/60">{$t("timeline_info_exposure")}</span><br />{exifDetails.join(" · ")}</p>{/if}
                  </section>
                {/if}
                {#if assetDetails.originalFileName || hasRealDimensions(selectedAsset, imageWidth, imageHeight) || fileSize !== null}
                  <section class="space-y-3">
                    <h2 class="font-semibold text-white">{$t("timeline_info_file")}</h2>
                    {#if assetDetails.originalFileName}<p><span class="text-white/60">{$t("timeline_info_filename")}</span><br />{assetDetails.originalFileName}</p>{/if}
                    {#if hasRealDimensions(selectedAsset, imageWidth, imageHeight)}<p><span class="text-white/60">{$t("timeline_info_dimensions")}</span><br />{imageWidth?.toLocaleString(numberLocale)} × {imageHeight?.toLocaleString(numberLocale)}</p>{/if}
                    {#if fileSize !== null}<p><span class="text-white/60">{$t("timeline_info_size")}</span><br />{(fileSize / 1024 / 1024).toFixed(1)} MB</p>{/if}
                  </section>
                {/if}
              </div>
            {:else}
              <p role="status">{$t("timeline_info_unavailable")}</p>
            {/if}
            <section class="mt-6 space-y-3">
              <h2 class="font-semibold text-white">Immich</h2>
              <button type="button" class="inline-flex w-full items-center justify-center gap-2 rounded bg-blue-600 px-3 py-2 font-medium hover:bg-blue-500 disabled:opacity-50" disabled={!serverUrl.trim()} onclick={() => selectedAsset && serverUrl.trim() && onOpenExternalUrl(assetUrl(selectedAsset))}>{$t("timeline_open_in_immich")} <ExternalLink class="h-4 w-4" /></button>
            </section>
          </aside>
        {/if}
        <div class="absolute bottom-4 left-1/2 z-30 -translate-x-1/2 space-y-2 text-center text-sm">
          {#if favoriteError}<p class="rounded bg-red-950/90 px-3 py-2 text-red-200" role="alert">{favoriteError}</p>{/if}
          {#if downloadMessage}<p class="rounded bg-emerald-950/90 px-3 py-2 text-emerald-200" role="status">{downloadMessage}</p>{/if}
          {#if downloadError}<p class="rounded bg-red-950/90 px-3 py-2 text-red-200" role="alert">{downloadError}</p>{/if}
          {#if addToAlbumMessage}<p class="rounded bg-emerald-950/90 px-3 py-2 text-emerald-200" role="status">{addToAlbumMessage}</p>{/if}
        </div>
      </div>
    </dialog>
  </div>
{/if}

{#if albumCatalogue}
  <AddToAlbumModal
    isOpen={isAddToAlbumOpen}
    catalogue={albumCatalogue}
    isAssigning={isAddingToAlbum}
    assignmentError={addToAlbumError}
    onConfirm={(albumId) => void assignSelectedAssetToAlbum(albumId)}
    onCancel={() => { if (!isAddingToAlbum) { isAddToAlbumOpen = false; addToAlbumSource = null; addToAlbumError = ""; } }}
  />
{/if}
