<script lang="ts">
  import { t } from "svelte-i18n";
  import type { SearchCamera } from "../contracts";
  import type { SearchController } from "../controllers/search-controller.svelte";
  import { setCameraPart } from "../search-filters/camera";
  import SearchFilterSuggestion from "./SearchFilterSuggestion.svelte";
  import SearchFilterHeading from "./SearchFilterHeading.svelte";

  /** The camera Search Filter: make, model and lens model, each chosen from Immich's suggestions; the model's suggestions are limited by the chosen make. */
  let { search }: { search: SearchController } = $props();

  const camera = $derived(search.filters.camera);

  function set(part: keyof SearchCamera) {
    return (value: string | undefined) => { search.filters.camera = setCameraPart(search.filters.camera, part, value); };
  }
</script>

<div role="group" aria-labelledby="search-filter-camera-label" class="space-y-2">
  <SearchFilterHeading id="search-filter-camera-label" text={$t("search_filter_camera")} />
  <div class="grid gap-2 sm:grid-cols-3">
    <SearchFilterSuggestion
      {search}
      id="search-filter-camera-make"
      label={$t("search_filter_camera_make")}
      request={{ type: "cameraMake" }}
      value={camera?.make}
      onChange={set("make")}
    />
    <SearchFilterSuggestion
      {search}
      id="search-filter-camera-model"
      label={$t("search_filter_camera_model")}
      request={camera?.make ? { type: "cameraModel", make: camera.make } : { type: "cameraModel" }}
      value={camera?.model}
      onChange={set("model")}
    />
    <SearchFilterSuggestion
      {search}
      id="search-filter-camera-lens"
      label={$t("search_filter_camera_lens")}
      request={{ type: "cameraLensModel" }}
      value={camera?.lensModel}
      onChange={set("lensModel")}
    />
  </div>
</div>
