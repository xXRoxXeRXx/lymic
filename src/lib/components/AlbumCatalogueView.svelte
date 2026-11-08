<script lang="ts">
  import { ImageOff, RefreshCw, Users } from "lucide-svelte";
  import { t } from "svelte-i18n";
  import type { AlbumCatalogueItem } from "../contracts";
  import type { AlbumCatalogueViewState } from "../controllers/album-catalogue-controller.svelte";

  let { catalogue }: { catalogue: AlbumCatalogueViewState } = $props();
  let failedCoverIds = $state(new Set<string>());

  function coverSrc(album: AlbumCatalogueItem) {
    return `lymic-thumb://asset/${encodeURIComponent(album.albumThumbnailAssetId!)}`;
  }

  function hideCover(albumId: string) {
    failedCoverIds = new Set(failedCoverIds).add(albumId);
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-4" aria-label={$t("albums_title")}>
  <div class="flex items-center justify-end gap-3">
    {#if catalogue.isOffline}<p role="status" class="text-sm text-amber-700 dark:text-amber-300">{$t("albums_offline")}</p>{/if}
    <button type="button" class="rounded-lg p-2 text-slate-500 hover:bg-slate-200 hover:text-slate-900 disabled:cursor-wait disabled:opacity-60 dark:text-zinc-400 dark:hover:bg-zinc-800 dark:hover:text-zinc-100" aria-label={$t("albums_refresh")} aria-busy={catalogue.isRefreshing} disabled={catalogue.isRefreshing} onclick={() => void catalogue.refresh()}><RefreshCw class={`h-4 w-4 ${catalogue.isRefreshing ? "animate-spin" : ""}`} /></button>
  </div>
  {#if catalogue.isLoading}<p role="status">{$t("albums_loading")}</p>
  {:else if catalogue.error}<div role="alert"><p>{$t("albums_error")}</p><button type="button" onclick={() => void catalogue.refresh()}>{$t("albums_retry")}</button></div>
  {:else if catalogue.albums.length === 0}<p>{$t("albums_empty")}</p>
  {:else}<div class="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
    {#each catalogue.albums as album (album.id)}
      <article class="rounded-xl border border-slate-200 p-4 dark:border-zinc-700">
        <div class="mb-3 flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-slate-100 dark:bg-zinc-800">
          {#if album.albumThumbnailAssetId && !failedCoverIds.has(album.id)}
            <img class="h-full w-full object-cover" src={coverSrc(album)} alt={album.albumName} onerror={() => hideCover(album.id)} />
          {:else}<ImageOff class="h-12 w-12 text-slate-400" />{/if}
        </div>
        <div class="flex items-center gap-2"><h2 class="min-w-0 flex-1 truncate font-semibold">{album.albumName}</h2>{#if album.isShared}<Users class="h-4 w-4 shrink-0 text-slate-400" aria-label={$t("albums_shared")} />{/if}</div>
        <p class="text-sm text-slate-500 dark:text-zinc-400">{$t("albums_asset_count", { values: { count: album.assetCount } })}</p>
      </article>
    {/each}
  </div>{/if}
</div>
