<script lang="ts">
  import { protocolSrc } from "../tauri/protocol-src";
  import { flushSync, onDestroy, untrack } from "svelte";
  import { Album, ChevronLeft, ChevronRight, Download, ExternalLink, Heart, Info, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { AlbumAssignmentResult, AssetDetails, TimelineAsset } from "../contracts";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import AddToAlbumModal from "./AddToAlbumModal.svelte";
  interface Props {
    asset: TimelineAsset;
    refreshGeneration?: number;
    detailsCache?: Map<string, AssetDetails | null>;
    updatingFavoriteAssetIds?: Record<string, true>;
    navigateAsset(delta: number, current: TimelineAsset): TimelineAsset | null | Promise<TimelineAsset | null>;
    onClose(): void;
    onFavoriteChange(assetId: string, favorite: boolean): void;
    serverUrl: string;
    onOpenExternalUrl(url: string): Promise<void>;
    getAssetDetails(assetId: string): Promise<AssetDetails | null>;
    setAssetFavorite(assetId: string, favorite: boolean): Promise<void>;
    chooseDownloadDestination(name: string): Promise<string | null>;
    downloadAsset(assetId: string, destination: string): Promise<void>;
    albumCatalogue?: TargetAlbumCatalogue;
    addAssetsToAlbum(assetIds: string[], albumId: string): Promise<AlbumAssignmentResult>;
  }
  let { asset, refreshGeneration = 0, detailsCache = new Map(), updatingFavoriteAssetIds = $bindable({}), navigateAsset, onClose, onFavoriteChange, serverUrl, onOpenExternalUrl, getAssetDetails, setAssetFavorite, chooseDownloadDestination, downloadAsset, albumCatalogue, addAssetsToAlbum }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  let selectedAsset = $state<TimelineAsset | null>(untrack(() => asset));
  let assetDetails = $state<AssetDetails | null>(null);
  const assetDetailsById = untrack(() => detailsCache);
  let isAssetDetailsLoading = $state(false);
  let isInfoOpen = $state(false);
  let favoriteOverrides = $state<Record<string, boolean>>({});
  let favoriteError = $state("");
  let downloadingAssetId = $state<string | null>(null);
  let downloadMessage = $state("");
  let downloadError = $state("");
  let isAddToAlbumOpen = $state(false);
  let isAddingToAlbum = $state(false);
  let addToAlbumMessage = $state("");
  let addToAlbumError = $state("");
  let addToAlbumMessageTimeout: ReturnType<typeof setTimeout> | null = null;
  let detailsGeneration = 0;
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

  function updateFavoriteState(assetId: string, isFavorite: boolean) {
    onFavoriteChange(assetId, isFavorite);
    favoriteOverrides = { ...favoriteOverrides, [assetId]: isFavorite };
    const currentDetails = assetDetailsById.get(assetId);
    if (currentDetails) {
      const updatedDetails = { ...currentDetails, isFavorite };
      assetDetailsById.set(assetId, updatedDetails);
      if (selectedAsset?.id === assetId) assetDetails = updatedDetails;
    }
    if (selectedAsset?.id === assetId) selectedAsset = { ...selectedAsset, isFavorite };
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
    isAddToAlbumOpen = true;
  }

  async function assignSelectedAssetToAlbum(albumId: string) {
    if (isAddingToAlbum || !selectedAsset) return;
    const assetId = selectedAsset.id;
    addToAlbumError = "";
    isAddingToAlbum = true;
    try {
      const result = await addAssetsToAlbum([assetId], albumId);
      if (result.succeeded !== 1 || result.failed !== 0) throw new Error("Album assignment was not completed");
      if (selectedAsset?.id === assetId) { isAddToAlbumOpen = false; showAddToAlbumMessage(); }
    } catch {
      if (selectedAsset?.id === assetId) addToAlbumError = $t("timeline_add_to_album_failed");
    } finally { isAddingToAlbum = false; }
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

  onDestroy(() => { detailsGeneration += 1; clearAddToAlbumMessage(); });

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
    onClose();
  }
  function closeFromLightboxBackdrop() {
    if (isInfoOpen) isInfoOpen = false; else closeLightbox();
  }
  let navigationGeneration = 0;
  async function navigate(delta: number) {
    if (!selectedAsset || downloadingAssetId || isAddingToAlbum) return;
    const current = selectedAsset;
    const generation = ++navigationGeneration;
    const nextAsset = await navigateAsset(delta, current);
    if (generation !== navigationGeneration || selectedAsset !== current) return;
    if (!nextAsset) return;
    flushSync(() => {
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
    if (event.key === "Escape") { event.preventDefault(); if (isInfoOpen) isInfoOpen = false; else closeLightbox(); }
    else if (event.key === "ArrowLeft") { event.preventDefault(); navigate(-1); }
    else if (event.key === "ArrowRight") { event.preventDefault(); navigate(1); }
  }
  function assetUrl(asset: TimelineAsset) {
    return `${serverUrl.replace(/\/$/, "")}/photos/${encodeURIComponent(asset.id)}`;
  }


  let initialGeneration = true;
  $effect(() => {
    refreshGeneration;
    untrack(() => {
      if (!initialGeneration) {
        detailsGeneration += 1;
        assetDetailsById.clear();
        favoriteOverrides = Object.fromEntries(
          Object.keys(updatingFavoriteAssetIds).map((id) => [id, favoriteOverrides[id]]),
        );
      }
      initialGeneration = false;
      if (selectedAsset) loadAssetDetails(selectedAsset);
    });
  });
</script>

<svelte:window onkeydown={handleKeydown} />

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
  <AddToAlbumModal isOpen={isAddToAlbumOpen} catalogue={albumCatalogue} isAssigning={isAddingToAlbum} assignmentError={addToAlbumError} onConfirm={(id) => void assignSelectedAssetToAlbum(id)} onCancel={() => { if (!isAddingToAlbum) { isAddToAlbumOpen = false; addToAlbumError = ""; } }} />
{/if}
