<script lang="ts">
  import { protocolSrc } from "../tauri/protocol-src";
  import { flushSync, onDestroy, untrack } from "svelte";
  import { Album, ArchiveRestore, ChevronLeft, ChevronRight, Download, ExternalLink, Heart, Info, Trash2, X } from "lucide-svelte";
  import { locale, t } from "svelte-i18n";
  import { toBcp47Locale } from "../i18n";
  import type { AlbumAssignmentResult, AssetDetails, AssetTag, TimelineAsset } from "../contracts";
  import TagColorDot from "./TagColorDot.svelte";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import type { RemoteAssetActions } from "./remote-asset-actions.svelte";
  import { restoreFeedbackFor, restoreFeedbackMessage, type RemoteAssetRestoreOutcome } from "./remote-asset-restore";
  import AddToAlbumModal from "./AddToAlbumModal.svelte";
  import MoveToTrashDialog from "./MoveToTrashDialog.svelte";
  import { trashFeedbackFor, trashFeedbackMessage, type RemoteAssetTrashOutcome } from "./remote-asset-trash";
  interface Props {
    asset: TimelineAsset;
    assetActions: RemoteAssetActions;
    updatingFavoriteAssetIds?: Record<string, true>;
    navigateAsset?: (delta: number, current: TimelineAsset) => TimelineAsset | null | Promise<TimelineAsset | null>;
    onClose(): void;
    onFavoriteChange(assetId: string, favorite: boolean): void;
    serverUrl: string;
    onOpenExternalUrl(url: string): Promise<void>;
    setAssetFavorite(assetId: string, favorite: boolean): Promise<void>;
    albumCatalogue?: TargetAlbumCatalogue;
    addAssetsToAlbum(assetIds: string[], albumId: string): Promise<AlbumAssignmentResult>;
    /** False hides the Favorite toggle, for views whose assets are read-only. */
    allowFavorite?: boolean;
    /** Offers Restore for a Trashed Asset; reports what happened and never rejects. */
    onRestore?(assetId: string): Promise<RemoteAssetRestoreOutcome>;
    /** Offers Move to Trash (after a confirmation the Lightbox asks for); reports what happened and never rejects. */
    onTrash?(assetId: string): Promise<RemoteAssetTrashOutcome>;
    /** Shows the Tags of the asset as chips; selecting one closes the Lightbox first, then calls this to open that Tag's Tag Timeline. */
    onSelectTag?(tag: AssetTag): void;
  }
  let { asset, assetActions, updatingFavoriteAssetIds = $bindable({}), navigateAsset, onClose, onFavoriteChange, serverUrl, onOpenExternalUrl, setAssetFavorite, albumCatalogue, addAssetsToAlbum, allowFavorite = true, onRestore, onTrash, onSelectTag }: Props = $props();
  const numberLocale = $derived(toBcp47Locale($locale));
  let selectedAsset = $state<TimelineAsset | null>(untrack(() => asset));
  let assetDetails = $state<AssetDetails | null>(null);
  let isAssetDetailsLoading = $state(false);
  let isInfoOpen = $state(false);
  let favoriteError = $state("");
  let downloadingAssetId = $state<string | null>(null);
  let downloadMessage = $state("");
  let downloadError = $state("");
  let isDestroyed = false;
  let isAddToAlbumOpen = $state(false);
  let isAddingToAlbum = $state(false);
  let addToAlbumMessage = $state("");
  let addToAlbumError = $state("");
  let isRestoring = $state(false);
  let restoreError = $state("");
  let isTrashConfirmOpen = $state(false);
  let isTrashing = $state(false);
  let trashError = $state("");
  let addToAlbumMessageTimeout: ReturnType<typeof setTimeout> | null = null;
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
    return assetActions.isFavorite(asset);
  }

  function updateFavoriteState(assetId: string, isFavorite: boolean) {
    onFavoriteChange(assetId, isFavorite);
    if (selectedAsset?.id === assetId) {
      const updatedDetails = assetActions.cachedDetails(assetId);
      if (updatedDetails) assetDetails = updatedDetails;
      selectedAsset = { ...selectedAsset, isFavorite };
    }
  }

  function loadAssetDetails(asset: TimelineAsset) {
    favoriteError = "";
    const cached = assetActions.cachedDetails(asset.id);
    if (cached !== undefined) {
      assetDetails = cached;
      isAssetDetailsLoading = false;
      return;
    }
    assetDetails = null;
    isAssetDetailsLoading = true;
    void assetActions.loadDetails(asset.id)
      .then(({ details, stale }) => {
        if (stale || isDestroyed || selectedAsset?.id !== asset.id) return;
        assetDetails = details;
        isAssetDetailsLoading = false;
      })
      // Details enhance the Info drawer; a failure must not disrupt asset viewing.
      .catch(() => {
        if (!isDestroyed && selectedAsset?.id === asset.id) isAssetDetailsLoading = false;
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
    downloadMessage = "";
    downloadError = "";
    downloadingAssetId = assetId;
    const isCurrent = () => selectedAsset?.id === assetId;
    try {
      const outcome = await assetActions.download([assetId], () => !isDestroyed && isCurrent());
      if (outcome === "saved") downloadMessage = $t("timeline_download_saved");
    } catch {
      if (isCurrent()) downloadError = $t("timeline_download_failed");
    } finally {
      downloadingAssetId = null;
    }
  }

  async function restoreAsset() {
    if (!selectedAsset || !onRestore || isRestoring) return;
    const assetId = selectedAsset.id;
    restoreError = "";
    isRestoring = true;
    try {
      const outcome = await onRestore(assetId);
      if (isDestroyed || selectedAsset?.id !== assetId || outcome.restoredAssetIds.includes(assetId)) return;
      restoreError = restoreFeedbackMessage(restoreFeedbackFor(outcome, 1), $t, numberLocale);
    } finally {
      if (!isDestroyed) isRestoring = false;
    }
  }

  function openTrashConfirm() {
    if (!selectedAsset || !onTrash || isBusy) return;
    trashError = "";
    isTrashConfirmOpen = true;
  }

  function cancelTrash() {
    if (!isTrashing) isTrashConfirmOpen = false;
  }

  async function trashAsset() {
    if (!selectedAsset || !onTrash || isTrashing) return;
    const assetId = selectedAsset.id;
    trashError = "";
    isTrashing = true;
    try {
      const outcome = await onTrash(assetId);
      if (isDestroyed) return;
      isTrashConfirmOpen = false;
      if (outcome.movedAssetIds.includes(assetId)) return;
      trashError = trashFeedbackMessage(trashFeedbackFor(outcome, 1), $t, numberLocale);
    } finally {
      if (!isDestroyed) isTrashing = false;
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

  onDestroy(() => { isDestroyed = true; clearAddToAlbumMessage(); });

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
    if (isBusy) return;
    onClose();
  }
  const isBusy = $derived(downloadingAssetId !== null || isAddingToAlbum || isRestoring || isTrashing);
  function selectTag(tag: AssetTag) {
    if (!onSelectTag || isBusy) return;
    onClose();
    onSelectTag(tag);
  }
  function closeFromLightboxBackdrop() {
    if (isInfoOpen) isInfoOpen = false; else closeLightbox();
  }
  let navigationGeneration = 0;
  async function navigate(delta: number) {
    if (!selectedAsset || !navigateAsset || isBusy) return;
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
      restoreError = "";
      trashError = "";
      isTrashConfirmOpen = false;
      isAddToAlbumOpen = false;
      clearAddToAlbumMessage();
      addToAlbumError = "";
    });
    loadAssetDetails(nextAsset);
  }
  function handleKeydown(event: KeyboardEvent) {
    if (isAddToAlbumOpen || isTrashConfirmOpen) return;
    if (event.key === "Escape") { event.preventDefault(); if (isInfoOpen) isInfoOpen = false; else closeLightbox(); }
    else if (event.key === "ArrowLeft") { event.preventDefault(); navigate(-1); }
    else if (event.key === "ArrowRight") { event.preventDefault(); navigate(1); }
  }
  function assetUrl(asset: TimelineAsset) {
    return `${serverUrl.replace(/\/$/, "")}/photos/${encodeURIComponent(asset.id)}`;
  }


  // Loads details initially and again whenever the shared details are invalidated, e.g. by a timeline refresh.
  $effect(() => {
    assetActions.generation;
    untrack(() => {
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
          {#if onTrash}
            <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("trash_move")} aria-label={$t("trash_move")} aria-busy={isTrashing} disabled={isBusy} onclick={openTrashConfirm}><Trash2 class="h-5 w-5 {isTrashing ? 'animate-pulse' : ''}" /></button>
          {/if}
          {#if onRestore}
            <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("trash_restore")} aria-label={$t("trash_restore")} aria-busy={isRestoring} disabled={isRestoring} onclick={() => void restoreAsset()}><ArchiveRestore class="h-5 w-5 {isRestoring ? 'animate-pulse' : ''}" /></button>
          {/if}
          {#if albumCatalogue}
            <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("timeline_add_to_album")} aria-label={$t("timeline_add_to_album")} disabled={isAddingToAlbum} onclick={openAddToAlbum}><Album class="h-5 w-5" /></button>
          {/if}
          {#if allowFavorite}
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={isFavorite(selectedAsset) ? $t("timeline_remove_favorite") : $t("timeline_add_favorite")} aria-label={isFavorite(selectedAsset) ? $t("timeline_remove_favorite") : $t("timeline_add_favorite")} aria-pressed={isFavorite(selectedAsset)} disabled={Boolean(updatingFavoriteAssetIds[selectedAsset.id])} onclick={() => void toggleFavorite()}><Heart class="h-5 w-5 {isFavorite(selectedAsset) ? 'fill-current' : ''}" /></button>
          {/if}
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80" title={$t("timeline_info")} aria-label={$t("timeline_info")} aria-pressed={isInfoOpen} onclick={() => isInfoOpen = !isInfoOpen}><Info class="h-5 w-5" /></button>
          <button type="button" class="rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" title={$t("timeline_close_lightbox")} aria-label={$t("timeline_close_lightbox")} disabled={isBusy} onclick={closeLightbox}><X class="h-5 w-5" /></button>
        </div>
        {#if navigateAsset}
          <button type="button" class="absolute left-2 top-1/2 z-30 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" aria-label={$t("timeline_previous_asset")} disabled={isBusy} onclick={() => navigate(-1)}><ChevronLeft class="h-5 w-5" /></button>
          <button type="button" class="absolute right-2 top-1/2 z-30 -translate-y-1/2 rounded-full bg-black/60 p-3 text-white hover:bg-black/80 disabled:opacity-50" aria-label={$t("timeline_next_asset")} disabled={isBusy} onclick={() => navigate(1)}><ChevronRight class="h-5 w-5" /></button>
        {/if}
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
                {#if assetDetails.tags && assetDetails.tags.length > 0}
                  <section class="space-y-3">
                    <h2 class="font-semibold text-white">{$t("timeline_info_tags")}</h2>
                    <ul class="flex flex-wrap gap-2">
                      {#each assetDetails.tags as tag (tag.id)}
                        <li>
                          <button type="button" class="inline-flex items-center gap-1.5 rounded-full bg-white/15 px-3 py-1 text-white hover:bg-white/25 disabled:opacity-50" title={tag.value} aria-label={$t("timeline_tag_open", { values: { path: tag.value } })} disabled={isBusy || !onSelectTag} onclick={() => selectTag(tag)}>
                            <TagColorDot color={tag.color} />
                            <span>{tag.name}</span>
                          </button>
                        </li>
                      {/each}
                    </ul>
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
          {#if restoreError}<p class="rounded bg-red-950/90 px-3 py-2 text-red-200" role="alert">{restoreError}</p>{/if}
          {#if trashError}<p class="rounded bg-red-950/90 px-3 py-2 text-red-200" role="alert">{trashError}</p>{/if}
          {#if addToAlbumMessage}<p class="rounded bg-emerald-950/90 px-3 py-2 text-emerald-200" role="status">{addToAlbumMessage}</p>{/if}
        </div>
      </div>
    </dialog>
  </div>
{/if}

{#if albumCatalogue}
  <AddToAlbumModal isOpen={isAddToAlbumOpen} catalogue={albumCatalogue} isAssigning={isAddingToAlbum} assignmentError={addToAlbumError} onConfirm={(id) => void assignSelectedAssetToAlbum(id)} onCancel={() => { if (!isAddingToAlbum) { isAddToAlbumOpen = false; addToAlbumError = ""; } }} />
{/if}

{#if onTrash}
  <MoveToTrashDialog isOpen={isTrashConfirmOpen} count={1} isMoving={isTrashing} onConfirm={() => void trashAsset()} onCancel={cancelTrash} />
{/if}
