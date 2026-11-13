# Immich map integration: source findings

Research date: 2026-11-12. This document records source evidence and implementation recommendations, not an accepted architecture decision or measured performance guarantee.

## Scope and evidence baseline

The intended feature is a separate Lymic navigation view for the authenticated user's Immich library, including geotagged photos and videos. The target is 300,000 geotagged Remote Assets and an unchanged Immich server. Further UX and acceptance decisions are being discussed.

The public GitHub commits API resolved Immich `main` to **`46eb2e3616f3a7638efb9d19040e21166dd6d2f1`** during research. The Immich links below use that immutable revision. This is a development snapshot, not a claim that Lymic or the user's server already supports its complete API. Older supported server versions need separate contract verification.

## Confirmed source findings

### Marker acquisition and server work

`GET /api/map/markers` requires `map.read` and returns one list. Its query DTO offers archive, favorite, creation-date, partner and shared-album options; it has no viewport, page or cursor input. The response carries ID, coordinates and optional place names. [Controller](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/controllers/map.controller.ts), [DTO](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/map.dto.ts).

The repository joins assets to EXIF rows with non-null coordinates, excludes deleted assets, applies ownership/visibility/filter conditions, sorts by creation time descending and executes without a limit. It selects no asset type restriction, so geotagged videos participate. The result is individual points, not server clusters or vector tiles. Its database query and response therefore grow with the matching library. Exact timings require measurement. [Map repository](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/repositories/map.repository.ts).

### Immich browser rendering and selection

Immich uses MapLibre through `svelte-maplibre`, supplies a worker URL, converts fetched markers to GeoJSON and enables clustering with radius 35 and maximum zoom 18. `MarkerLayer` supplies count elements for clusters and thumbnail images for individual points. Filter changes refetch the marker list. A cluster click requests at most **10,000** leaves, then derives selected IDs and bounds from those leaves. Viewport selection scans the marker array. These are specific implementation choices, not a 300,000-point benchmark. [Map component](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/web/src/lib/components/shared-components/map/Map.svelte).

The timeline panel receives those selected IDs, uses them as an asset filter, and queries a bounding box. Consequently, copying this selection flow would truncate exact cluster membership above 10,000. Replacing membership with a bounding rectangle alone would also change semantics: unrelated points can lie inside that rectangle. [Map timeline panel](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/web/src/routes/%28user%29/map/%5B%5Bphotos%3Dphotos%5D%5D/%5B%5BassetId%3Did%5D%5D/MapTimelinePanel.svelte).

### Viewport alternatives already available on the server

The metadata search endpoint supports bounded result sizes and newer cursor-based requests, but its checked filter schema contains neither a bounding box nor latitude/longitude predicates. It cannot be assumed to implement viewport marker loading. [Search DTO](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/search.dto.ts).

The **timeline** API does support `bbox=west,south,east,north` and optional `withCoordinates`; bucket listing and bucket loading require `asset.read`. This is a real alternative for geographically restricted asset browsing. It still returns time buckets and their assets, not spatial aggregation with a fixed result budget. [Timeline DTO](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/time-bucket.dto.ts), [controller](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/controllers/timeline.controller.ts).

The timeline repository combines an earth-distance bounding-circle prefilter with explicit latitude/longitude bounds. Crossing the antimeridian is handled with an OR longitude predicate, and the bbox DTO permits east < west. A world viewport or a dense location can still match the entire collection. [Asset repository](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/repositories/asset.repository.ts), [bbox DTO](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/bbox.dto.ts).

### Configuration and permissions

Immich exposes map enablement and light/dark style URLs as public configuration fields. Default styles use `tiles.immich.cloud`; custom styles are supported. The map component uses the configured URLs. Style JSON may refer to additional tiles, fonts and sprites, so their network requests and Tauri CSP/protocol behavior need integration testing. [Configuration schema/defaults](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/config.dto.ts).

Marker access uses `map.read`; reverse geocoding is separately guarded by `map.search`. Reverse geocoding is unnecessary just to render known coordinates. Timeline/asset browsing additionally requires `asset.read`; thumbnail and original access must retain their existing endpoint permissions. A configured API key can therefore allow photos while denying map markers. [Map controller](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/controllers/map.controller.ts), [permission constants](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/enum.ts).

## Recommendations, pending decisions and validation

MapLibre's official large-data guidance recommends reducing properties, clustering and considering tiling for larger datasets. It describes URL-based data loading and division into chunks as additional tools. These are useful techniques, not a documented guarantee for this workload. [MapLibre large-data guide](https://maplibre.org/maplibre-gl-js/docs/guides/large-data/).

For an unchanged Immich server, the strongest candidate is one minimal marker snapshot, a reusable spatial/cluster index outside the UI thread, and viewport-limited rendering. Avoid moving full asset details or 300,000 thumbnail elements through Svelte state. Store only coordinates and an ID/reference in the index; request details and thumbnails for visible selections. A MapLibre GeoJSON source already has worker infrastructure, so benchmark that simpler option before adding an independent index and duplicating memory. [MapLibre worker implementation](https://github.com/maplibre/maplibre-gl-js/blob/main/src/source/geojson_worker_source.ts).

Use MapLibre circle/symbol layers for large point sets. If thumbnail pins are desired, enforce a visible-thumbnail budget and load only those previews. Keep identical or very close coordinates grouped at maximum useful zoom, with a virtualized/paged location list; zooming alone cannot separate 300,000 photos at the same coordinate. Do not copy Immich's 10,000-leaf selection cap. Exact cluster membership should be traversed in bounded pages or queried through the local index, without eagerly fetching all asset details.

A local snapshot/cache could improve repeat startup, but introduces invalidation, account/server isolation and stale-location handling. It is not yet an accepted requirement. Do not infer that a full marker snapshot updates incrementally: the checked endpoint provides no change cursor. Progressive timeline bucket loading is a possible alternative, but exposes partial coverage and does not inherently bound dense-viewport work.

The following remain to be settled or proved:

- UX for cluster clicks, identical locations, preview pins, filters and refresh behavior.
- Supported Immich versions and their exact configuration, timeline and marker contracts.
- A representative 300,000-point benchmark on the actual Tauri WebView, including widely distributed points, all points at one location, antimeridian and high latitude cases.
- Cold server fetch time measured separately from local index construction, first useful display and repeat opening.
- Process memory across WebView, workers and attributable backend work, including transient copies during IPC/GeoJSON serialization; JavaScript heap alone is insufficient.
- The user accepted a **provisional** profile of a 16 GB machine with integrated graphics, at least 30 FPS during pan/zoom and no more than 500 MiB added process memory. These are acceptance targets, not measured results or a guarantee. Confirmed product decisions are tracked in [the design discussion](map-view-design.md).
- Error behavior for denied map permissions, disabled maps, unavailable style/tile hosts and stale/deleted assets, without exposing credentials to map resource hosts.
