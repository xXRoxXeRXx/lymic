<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { t } from "svelte-i18n";
  import * as maplibregl from "maplibre-gl";
  import type { GeoJSONSource, Map as MapLibreMap, Marker } from "maplibre-gl";
  import mapWorkerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
  import "maplibre-gl/dist/maplibre-gl.css";
  import type { MapViewState } from "../controllers/map-controller.svelte";
  import type { AssetDesktopAdapter, AlbumAssignmentDesktopAdapter, UpdateDesktopAdapter } from "../tauri/desktop-adapter";
  import type { TimelineAsset } from "../contracts";
  import type { TargetAlbumCatalogue } from "../controllers/target-album-catalogue.svelte";
  import Lightbox from "./Lightbox.svelte";

  interface Props {
    mapState: MapViewState;
    active: boolean;
    serverUrl: string;
    desktop: AssetDesktopAdapter & AlbumAssignmentDesktopAdapter & Pick<UpdateDesktopAdapter, "openExternalUrl">;
    albumCatalogue: TargetAlbumCatalogue;
  }
  let { mapState, active, serverUrl, desktop, albumCatalogue }: Props = $props();
  let container: HTMLDivElement;
  let map: MapLibreMap | undefined;
  let providerError = $state(false);
  let selectedAsset = $state<TimelineAsset | null>(null);
  let selectionGeneration = 0;
  const pageSize = 100;
  let collection = $state<{ source: GeoJSONSource; id: number; count: number } | null>(null);
  let page = $state(0);
  let pageIds = $state<string[]>([]);
  let pageLoading = $state(false);
  let pageError = $state(false);
  let pageGeneration = 0;
  let selectedIndex = -1;
  let deferredMarkers = false;
  let deferredStyle = false;
  let resultButton: HTMLButtonElement | null = null;
  let mapFocus: HTMLElement | null = null;

  async function loadPage(index: number) {
    const current = collection;
    if (!current) return;
    const generation = ++pageGeneration;
    pageLoading = true;
    pageError = false;
    try {
      const leaves = await current.source.getClusterLeaves(current.id, pageSize, index * pageSize);
      if (generation !== pageGeneration || collection !== current) return;
      page = index;
      pageIds = leaves.map((leaf) => String(leaf.properties?.id));
    } catch {
      if (generation === pageGeneration) pageError = true;
    } finally {
      if (generation === pageGeneration) pageLoading = false;
    }
  }
  async function assetAt(index: number): Promise<TimelineAsset | null> {
    const current = collection;
    if (!current || index < 0 || index >= current.count) return null;
    try {
      const leaves = await current.source.getClusterLeaves(current.id, 1, index);
      const id = leaves[0]?.properties?.id;
      if (id == null || collection !== current) return null;
      const details = await desktop.getAssetDetails(String(id));
      if (!details || collection !== current) return null;
      return toAsset(String(id), details);
    } catch { return null; }
  }
  async function navigateCollection(delta: number): Promise<TimelineAsset | null> {
    const current = collection;
    if (!current) return null;
    for (let index = selectedIndex + delta; index >= 0 && index < current.count; index += delta) {
      const asset = await assetAt(index);
      if (collection !== current) return null;
      if (asset) { selectedIndex = index; return asset; }
    }
    return null;
  }
  async function openResult(index: number, button: HTMLButtonElement) {
    resultButton = button;
    const asset = await assetAt(index);
    if (asset) { selectedIndex = index; selectedAsset = asset; }
  }
  function closeResults() {
    pageGeneration++;
    collection = null;
    pageIds = [];
    if (deferredStyle) { deferredStyle = false; deferredMarkers = false; map?.setStyle(styleUrl()); }
    else if (deferredMarkers) { deferredMarkers = false; setMarkers(); }
    mapFocus?.focus();
  }
  const styleUrl = () => `https://tiles.openfreemap.org/styles/${document.documentElement.classList.contains("dark") ? "dark" : "liberty"}`;

  let initialized = false;
  let lastMarkers = untrack(() => mapState.markers);
  let worker: Worker | undefined;
  const pins = new Map<string, Marker>();
  const pendingImages = new Map<string, HTMLImageElement>();
  const activeImages = new Map<string, HTMLImageElement>();
  const thumbnailConcurrency = 8;
  function pumpThumbnails() {
    while (activeImages.size < thumbnailConcurrency && pendingImages.size) {
      const next = pendingImages.entries().next().value;
      if (!next) break;
      const [id, image] = next;
      pendingImages.delete(id);
      activeImages.set(id, image);
      const complete = () => {
        if (activeImages.get(id) !== image) return;
        activeImages.delete(id);
        pumpThumbnails();
      };
      image.onload = complete;
      image.onerror = complete;
      image.src = thumbnailUrl(id);
    }
  }
  function removePin(id: string, pin: Marker) {
    pendingImages.delete(id);
    const image = activeImages.get(id);
    if (image) { activeImages.delete(id); image.onload = null; image.onerror = null; image.removeAttribute("src"); }
    pin.remove();
    pins.delete(id);
  }
  let workerGeneration = 0;
  let sourceUpdating = false;

  function thumbnailUrl(id: string) {
    const convert = (globalThis as typeof globalThis & { __TAURI_INTERNALS__?: { convertFileSrc?: (path: string, protocol: string) => string } }).__TAURI_INTERNALS__?.convertFileSrc;
    return convert ? `${convert("", "lymic-thumb").replace(/\/$/, "")}/asset/${encodeURIComponent(id)}` : `lymic-thumb://asset/${encodeURIComponent(id)}`;
  }
  function toAsset(id: string, details: NonNullable<Awaited<ReturnType<typeof desktop.getAssetDetails>>>): TimelineAsset {
    return { id, width: details.imageWidth ?? 0, height: details.imageHeight ?? 0, type: details.assetType === "VIDEO" ? "VIDEO" : "IMAGE", takenAt: null, cameraMake: details.cameraMake, cameraModel: details.cameraModel, fileSize: details.fileSize, duration: null, isFavorite: details.isFavorite };
  }
  async function openAsset(id: string) {
    const generation = ++selectionGeneration;
    try {
      const details = await desktop.getAssetDetails(id);
      if (generation !== selectionGeneration) return;
      if (details) selectedAsset = toAsset(id, details);
    } catch { /* A removed asset should not break the rest of the map. */ }
  }
  function updatePins() {
    if (!map || map.isMoving() || !map.getLayer("map-points") || !active) return;
    const found = new Set<string>();
    for (const feature of map.queryRenderedFeatures({ layers: ["map-points"] })) {
      const id = String(feature.properties.id);
      if (found.has(id) || feature.geometry.type !== "Point") continue;
      found.add(id);
      if (pins.has(id)) continue;
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-label", $t("map_open_asset"));
      button.className = "map-thumbnail";
      const image = document.createElement("img");
      image.alt = "";
      image.loading = "lazy";
      pendingImages.set(id, image);
      button.append(image);
      button.onclick = () => void openAsset(id);
      pins.set(id, new maplibregl.Marker({ element: button }).setLngLat(feature.geometry.coordinates as [number, number]).addTo(map));
    }
    for (const [id, pin] of pins) if (!found.has(id)) removePin(id, pin);
    pumpThumbnails();
  }
  function removePins() { for (const [id, pin] of pins) removePin(id, pin); }
  function setMarkers() {
    if (!map || !map.getSource("assets")) return;
    worker?.terminate();
    const generation = ++workerGeneration;
    worker = new Worker(new URL("../workers/map-data.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event) => {
      if (!map || generation !== workerGeneration) return;
      const source = map.getSource("assets") as GeoJSONSource | undefined;
      if (!source) return;
      sourceUpdating = true;
      void source.setData(event.data.data).catch(() => { providerError = true; }).finally(() => {
        if (generation === workerGeneration) sourceUpdating = false;
      });
      if (!initialized && mapState.loaded) {
        if (mapState.markers.length) {
          map.fitBounds(event.data.bounds as [number, number, number, number], { padding: 60, maxZoom: 12, duration: 0 });
        }
        initialized = true;
      }
      worker?.terminate();
      worker = undefined;
    };
    worker.postMessage(mapState.markers);
  }
  onMount(() => {
    maplibregl.setWorkerUrl(mapWorkerUrl);
    map = new maplibregl.Map({ container, style: styleUrl(), center: [0, 0], zoom: 1, maxZoom: 21, renderWorldCopies: false });
    const themeObserver = new MutationObserver(() => { if (collection) { deferredStyle = true; return; } removePins(); map?.setStyle(styleUrl()); });
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    map.addControl(new maplibregl.NavigationControl(), "top-right");
    map.on("error", () => { providerError = true; });
    map.on("style.load", () => {
      if (!map) return;
      map.addSource("assets", { type: "geojson", data: { type: "FeatureCollection", features: [] }, cluster: true, clusterRadius: 50, clusterMaxZoom: 21 });
      map.addLayer({ id: "map-clusters", type: "circle", source: "assets", filter: ["has", "point_count"], paint: { "circle-color": "#2563eb", "circle-radius": 20, "circle-stroke-color": "#fff", "circle-stroke-width": 2 } });
      map.addLayer({ id: "map-counts", type: "symbol", source: "assets", filter: ["has", "point_count"], layout: { "text-field": ["get", "point_count_abbreviated"], "text-size": 12 }, paint: { "text-color": "#fff" } });
      map.addLayer({ id: "map-points", type: "circle", source: "assets", filter: ["!", ["has", "point_count"]], paint: { "circle-radius": 1, "circle-opacity": 0 } });
      setMarkers();
    });
    map.on("click", "map-clusters", async (event) => {
        const feature = event.features?.[0];
        if (!map || sourceUpdating || !feature || feature.geometry.type !== "Point") return;
        const source = map.getSource("assets") as GeoJSONSource;
        const id = Number(feature.properties.cluster_id);
        const zoom = await source.getClusterExpansionZoom(id);
        if (zoom <= 21 && zoom > map.getZoom()) {
          map.easeTo({ center: feature.geometry.coordinates as [number, number], zoom });
        } else {
          mapFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          if (worker) { workerGeneration++; worker.terminate(); worker = undefined; deferredMarkers = true; }
          collection = { source, id, count: Number(feature.properties.point_count) };
          void loadPage(0);
        }
      });
    map.on("movestart", removePins);
    map.on("moveend", updatePins);
    map.on("sourcedata", updatePins);
    return () => { themeObserver.disconnect(); selectionGeneration++; workerGeneration++; worker?.terminate(); removePins(); map?.remove(); map = undefined; };
  });
  $effect(() => {
    const markers = mapState.markers;
    if (markers !== lastMarkers) { lastMarkers = markers; if (collection) deferredMarkers = true; else { removePins(); setMarkers(); } }
  });
  $effect(() => {
    if (active) { queueMicrotask(() => { map?.resize(); updatePins(); }); }
    else { mapState.leave(); removePins(); }
  });
</script>

<div class="relative h-full min-h-0 flex-col" style:display={active ? "flex" : "none"}>
  {#if mapState.loading && !mapState.loaded}<p role="status">{$t("map_loading")}</p>{/if}
  {#if mapState.error}<p role="alert">{mapState.error === "permission" ? $t("map_permission") : $t("map_error")}{mapState.stale ? ` ${$t("map_stale")}` : ""}</p>{/if}
  {#if mapState.loaded && !mapState.markers.length}<p role="status">{$t("map_empty")}</p>{/if}
  {#if providerError}<p role="alert">{$t("map_provider_error")}</p>{/if}
  <div bind:this={container} class="min-h-0 flex-1 rounded-xl" aria-label={$t("map_title")}></div>
  {#if collection}
    <section class="absolute inset-4 z-20 overflow-auto rounded-xl bg-white p-4 shadow-xl dark:bg-gray-900" aria-label={$t("map_location_results")}>
      <div class="flex items-center justify-between"><h2>{$t("map_location_results")} ({collection.count})</h2><button type="button" onclick={closeResults}>{$t("map_close_results")}</button></div>
      {#if pageLoading}<p role="status">{$t("map_loading")}</p>{/if}
      {#if pageError}<p role="alert">{$t("map_error")}</p>{/if}
      <div class="grid grid-cols-4 gap-2 sm:grid-cols-6">
        {#each pageIds as id, index}
          <button type="button" aria-label={$t("map_open_asset")} onclick={(event) => void openResult(page * pageSize + index, event.currentTarget)}><img src={thumbnailUrl(id)} alt="" loading="lazy" class="aspect-square w-full object-cover" /></button>
        {/each}
      </div>
      <div class="flex items-center gap-3"><button type="button" disabled={page === 0 || pageLoading} onclick={() => void loadPage(page - 1)}>{$t("map_previous_page")}</button><span>{page + 1} / {Math.ceil(collection.count / pageSize)}</span><button type="button" disabled={(page + 1) * pageSize >= collection.count || pageLoading} onclick={() => void loadPage(page + 1)}>{$t("map_next_page")}</button><button type="button" disabled={pageLoading || (page + 1) * pageSize >= collection.count} onclick={() => void loadPage(Math.ceil(collection!.count / pageSize) - 1)}>{$t("map_last_page")}</button></div>
    </section>
  {/if}
  {#if selectedAsset}
    <Lightbox asset={selectedAsset} navigateAsset={(delta) => collection ? navigateCollection(delta) : null} onClose={() => { selectedAsset = null; queueMicrotask(() => resultButton?.focus()); }} onFavoriteChange={() => {}} {serverUrl} onOpenExternalUrl={desktop.openExternalUrl} getAssetDetails={desktop.getAssetDetails} setAssetFavorite={desktop.setAssetFavorite} chooseDownloadDestination={desktop.chooseDownloadDestination} downloadAsset={desktop.downloadAsset} {albumCatalogue} addAssetsToAlbum={desktop.addAssetsToAlbum} />
  {/if}
</div>

<style>
  :global(.map-thumbnail) { width: 46px; height: 46px; border: 2px solid white; border-radius: 6px; overflow: hidden; background: #ddd; cursor: pointer; box-shadow: 0 1px 5px #0008; }
  :global(.map-thumbnail img) { width: 100%; height: 100%; object-fit: cover; }
</style>
