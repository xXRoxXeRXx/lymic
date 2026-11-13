## Problem Statement

Lymic users can browse their Immich Library chronologically, but cannot explore their Remote Assets geographically. They need a Map View with the familiar Immich Web interaction: clusters at wider zoom levels and Asset Thumbnails directly on the map as locations separate while zooming in. This must remain usable with 300,000 geotagged Remote Assets, including large groups at exactly the same location.

## Solution

Add a separate Map View rendered inside Lymic. Retrieve Remote Asset location markers from the existing Immich API, cluster them outside the UI thread, and show thumbnail pins for individual assets as zoom resolves clusters. Clicking a thumbnail opens the existing Lightbox experience. Groups that cannot separate expose a complete, incrementally loaded location result list.

Lymic supplies exactly one background map, using OpenFreeMap independently of Immich. The provider and styles are controlled by the application; users cannot choose providers or configure style URLs. Refresh location data once in the background whenever the user enters the Map View. Keep the current session's map visible during refresh. Do not add a refresh button or an overview/reset button.

## User Stories

1. As a Lymic user, I want a separate Map View in navigation, so that I can explore my Immich Library geographically.
2. As a Lymic user, I want Remote Assets with locations to appear regardless of which client uploaded them, so that the map represents my library rather than only Lymic uploads.
3. As a Lymic user, I want geotagged images and videos represented, so that both media types are discoverable by location.
4. As a Lymic user, I want assets without a valid location omitted, so that they are not placed at invented coordinates.
5. As a Lymic user, I want wider views to display clusters with asset counts, so that dense areas remain understandable.
6. As a Lymic user, I want zooming in to reveal Asset Thumbnails directly on the map, so that I can recognize individual assets without opening a separate list first.
7. As a Lymic user, I want individual thumbnail pins wherever assets are spatially resolved, so that I receive the familiar Immich Web presentation rather than a map of plain location dots.
8. As a Lymic user, I want clicking a cluster to zoom into its area, so that I can explore progressively smaller groups.
9. As a Lymic user, I want groups at identical coordinates to remain reachable through a location result list, so that maximum zoom does not hide assets permanently.
10. As a Lymic user, I want every asset in a location group to be reachable even when there are more than 10,000, so that selections are not silently truncated.
11. As a Lymic user, I want location results to load incrementally as I browse, so that a large group does not require every thumbnail and full asset detail to load at once.
12. As a Lymic user, I want a thumbnail click to open the existing Lightbox, so that viewing images and videos behaves consistently across the app.
13. As a Lymic user, I want previous/next in the Lightbox to stay within the selected result collection, so that navigation does not unexpectedly jump to unrelated locations.
14. As a Lymic user, I want existing Lightbox details and actions retained where applicable, so that I can view metadata, change Favorite status, download and add assets to an Album as I already can in the Photo Timeline.
15. As a Lymic user, I want closing the Lightbox to return me to my map context, so that I can continue exploring where I left off.
16. As a Lymic user, I want the initial map to show an area containing my asset clusters, so that I immediately have useful geographic context.
17. As a Lymic user, I want to zoom and pan to explore further or focus on my assets, so that ordinary map navigation is sufficient without an additional overview/reset button.
18. As a Lymic user, I want the map to refresh in the background each time I enter it, so that changes on my Immich Server become visible without a refresh button.
19. As a Lymic user, I want the session's existing map to remain visible during a refresh, so that revisiting the map does not unnecessarily replace it with a loading screen.
20. As a Lymic user, I want refreshing data to preserve the area I am exploring, so that background work does not unexpectedly move the map.
21. As a Lymic user, I want a clear initial loading, empty or failure state, so that I can distinguish an empty library from an unavailable server or denied map access.
22. As a Lymic user, I want a failed background refresh to retain previously loaded session data with a clear stale/error indication, so that I can still explore the available map.
23. As a Lymic user, I want a usable background map without setting up another account or API key, so that opening the Map View requires no additional provider setup.
24. As a Lymic user, I want Lymic to provide its map directly, so that I do not have to choose a provider or configure map styles.
25. As a Lymic user, I want light and dark map styles supported, so that the background map fits the app's theme.
26. As a Lymic user, I want map-provider failures reported separately from failures loading my assets, so that I understand which part of the map is unavailable.
27. As a Lymic user, I want library data and map session state isolated between servers and accounts, so that changing accounts does not expose another library's locations.
28. As a Lymic user, I want smooth zooming and panning with 300,000 located assets, so that my library size does not make geographic browsing unusable.
29. As a Lymic user, I want thumbnail loading to prioritize the area I am viewing, so that fast panning does not flood the server with requests for locations I have already left.
30. As a Lymic user, I want the feature to work with my unchanged Immich Server, so that I do not have to install a separate service or maintain a fork.
31. As a Lymic user, I want the map's first release to focus on browsing, so that I get the requested geographic interaction without additional filter controls.

## Implementation Decisions

- Integrate a dedicated Map View and owning controller into the existing navigation and controller lifecycle. Add a domain-specific desktop adapter contract and matching Rust/Tauri surface for map data; keep frontend contracts, command registration and callers aligned.
- Render the map locally with MapLibre. Do not embed Immich Web or import Immich's map styles, tile-provider URLs, map enablement setting or other map presentation configuration. Immich remains the source of authorized Remote Asset data.
- Use OpenFreeMap as Lymic's fixed background-map provider with application-controlled light/dark styles. Users have access to exactly this map, with no provider selector, custom style URL fields or map-provider settings. Preserve required attribution. Immich credentials must never be forwarded to map-provider hosts.
- Treat the checked upstream implementation as evidence for UX and API behavior, not as a performance guarantee or a substitute for verifying Lymic's supported Immich contract. The checked marker endpoint is `GET /api/map/markers`, requires `map.read`, and returns a complete marker list without viewport filtering or pagination. Asset viewing continues to require its existing permissions. Show an actionable explanation if the API key lacks map access.
- Begin with a minimal marker snapshot and worker-based clustering. Retain only the ID/reference and coordinates needed by the map index; avoid materializing the full library's asset details in frontend reactive state. MapLibre's built-in worker clustering is the initial candidate; change the index ownership or transfer representation if measurement shows that memory or responsiveness targets require it. Do not add redundant indexes by default.
- Handle the potentially large marker response through a dedicated, explicitly bounded acquisition/decoding path. The existing generic successful-JSON response limit is 1 MiB and cannot accommodate this workload. Do not raise the limit for unrelated endpoints. Protect the UI thread from expensive conversion work and account for transient copies during Rust-to-WebView and worker transfers.
- Keep the marker snapshot/index in session memory only. Do not persist location snapshots to disk in version 1. Reuse the existing Asset Thumbnail disk cache; it is distinct from the location index. Release obsolete snapshots and indexes rather than accumulating one per visit.
- Perform one marker refresh per navigation entry into the Map View, rather than on every render, pan or zoom. A first entry loads data; later entries display the session snapshot while fetching its replacement. Suppress duplicate refreshes for the same entry and reject stale results after account changes or newer refresh generations. No manual refresh button or periodic polling is required.
- Start with a geographic extent containing the available asset clusters. Preserve the current camera during data refresh and routine navigation back to an already initialized session map. Do not add an overview/reset button. Persistent camera restoration across app restarts is not required by this spec.
- Match Immich Web's cluster-to-thumbnail transition. The switch depends on cluster membership at the current zoom, not a separately invented threshold that hides all thumbnails until a fixed zoom level. Individual assets that resolve out of clusters appear as thumbnail pins directly on the map.
- Restrict rendering and thumbnail requests to the viewport and a small practical margin. Clusters must summarize dense areas, and rendering/loading budgets must preserve thumbnail-pin behavior and reachability of all assets. Do not create 300,000 DOM markers, image elements or thumbnail requests. Tune cluster radius, maximum zoom and rendering technique against the agreed workload rather than blindly copying upstream constants.
- Cluster clicks zoom toward the cluster's extent or useful expansion level. If further zoom cannot separate a group, including identical coordinates at maximum useful zoom, offer the complete location result list. Do not copy the upstream 10,000-leaf selection cap. Traverse membership in bounded portions and retrieve only the details needed by the visible result page. A bounding rectangle alone must not silently introduce unrelated assets into an exact location/group selection.
- Reuse the existing Lightbox interaction by extracting shared presentation/behavior from the timeline as needed, rather than creating an inconsistent second viewer. Keep the selected result collection and its navigation order stable while the Lightbox is open; load additional members incrementally when required. Returning to the map preserves camera context and restores focus appropriately.
- Continue using the existing authenticated thumbnail and media protocols and bounded Thumbnail Cache, as required by ADR-0005. Keep API keys and Cloudflare Access credentials in Rust/native keyring. Do not transfer media as IPC base64 or preload the entire library's details. Preserve existing concurrency and cancellation/queue controls for visible media.
- Separate background-map resources from private asset requests. Support map workers and the provider's style, tile, glyph and sprite resources in the packaged Tauri app with appropriate CSP/protocol handling. Bundle application worker code locally; do not rely on development-only network allowances.
- Handle valid zero coordinates, invalid/non-finite coordinates, the antimeridian and high latitudes deliberately. Missing locations are omitted rather than assigned to an arbitrary point. Geographic grouping/counts must not duplicate assets when map wrapping displays multiple world copies.
- Keep initial loading, empty, map-permission, server/network and background-provider failure states distinguishable. Preserve the previous snapshot after a failed background refresh and make its stale status visible. Do not introduce a refresh button through error-state UI; entering the view again initiates the next refresh. Failed or deleted individual assets must not break the entire map.
- Clear private map data, pending requests and any old index on logout or account/server replacement. Worker lifecycle and hidden-view behavior must prevent obsolete account data from reappearing or consuming unbounded resources.
- Version 1 adds no location-index database schema or map-provider preferences. Leave the specific rendering primitive, internal transfer format and non-product tuning constants to implementation and measurement while preserving these observable requirements.

## Testing Decisions

A reference-laptop benchmark and numerical FPS/memory acceptance targets are not required. The automated fixtures do not establish performance guarantees.

- The user confirmed the main test boundary: the visible Map View with a replaceable desktop adapter, covering cluster-to-thumbnail presentation, Lightbox opening/navigation and complete location results. Prefer this high-level boundary over tests that mirror internal helpers or assert a particular clustering library's internals.
- Use existing Svelte component tests with Testing Library/Vitest and controller tests with fake domain-specific desktop adapters as prior art. Extend the existing Lightbox behavior coverage so extraction preserves the Photo Timeline and Album Timeline experiences.
- At that boundary, verify loading and empty states; a thumbnail opening the correct asset; image/video support; Lightbox collection stability; exact-location groups above 10,000 members; failure retention; one background refresh per entry; no refresh/reset buttons or provider configuration; and account changes during in-flight loading.
- Use focused Rust integration/contract tests with the existing mock-HTTP-server pattern for marker acquisition, authorization failures, oversized/malformed responses, explicit response bounds and credential isolation. Verify the API contract against supported Immich versions rather than assuming that a development snapshot is universally available.
- Test cancellation/generation handling through observable outcomes: a late old-account response cannot replace the current map, and a failed replacement does not discard a successful session snapshot. Test membership completeness and duplicate suppression, not just a short first page.
- Exercise the real MapLibre renderer and locally bundled workers in the packaged Tauri WebView. Unit tests or jsdom simulations alone cannot establish actual cluster transitions, CSP compatibility, GPU performance or provider resource loading.
- Build repeatable 300,000-point datasets covering globally distributed assets, dense regional clusters and all assets at identical coordinates. Include antimeridian, high-latitude, zero-coordinate and invalid-location cases. The goal is 300,000 geotagged Remote Assets, not 300,000 assets of which only a small sample has coordinates.
- Run the repository's applicable Svelte/type, frontend behavior, Rust formatting/lint/test checks for the implemented change, plus the packaged desktop smoke checks above. Creating this specification does not itself implement or benchmark the feature.

## Out of Scope

- Importing or embedding the Immich Web UI, background-map configuration or map presentation settings.
- User-selectable background-map providers, custom style URLs, provider settings and support for user-configured/self-hosted background maps.
- Changes to the Immich Server, an Immich fork, or an additional map-index server/service.
- Time, Favorite, Album or Recognized Person filters in the Map View's first release.
- Place search, reverse geocoding, GPS editing or assigning coordinates to assets without locations.
- New manual refresh controls, overview/reset controls, or periodic polling.
- Persistent local location snapshots, offline-region downloads, a fully offline background map, and guaranteed camera restoration after app restart.
- Bidirectional filesystem synchronization or changes to Watched Folder, Target Album and Synchronization semantics.
- Replacing existing Lightbox capabilities or changing the Album Timeline data-source decision in ADR-0006.

## Further Notes

- The confirmed UX is an Immich-like map rendered by Lymic: count clusters become Asset Thumbnail pins as zoom resolves individual locations. A plain-dot map with thumbnails only in a side list does not satisfy the request.
- Lymic provides one fixed OpenFreeMap map with application-controlled styles. The user explicitly ruled out configurable providers or style URLs. OpenFreeMap's official documentation describes MapLibre integration and an account/API-key-free public service; retain attribution and review provider terms when integrating. Background-map requests reveal the requested map areas to that provider; private asset metadata and authentication remain separate.
- Upstream evidence is pinned to Immich commit `46eb2e3616f3a7638efb9d19040e21166dd6d2f1`. It demonstrates a complete marker fetch, worker-supported clustering, count and thumbnail marker layers, and a 10,000-leaf selection limit that Lymic must not inherit. The checked timeline API has geographic bounding-box support, but no fixed spatial aggregation budget; it is not a drop-in paginated marker endpoint.
- Upstream cluster-click behavior and constants are not mandatory replicas: the user explicitly chose zoom-on-cluster-click and complete results for inseparable groups. The required thumbnail transition follows upstream behavior.
- Sources: [Immich map implementation](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/web/src/lib/components/shared-components/map/Map.svelte), [Immich marker API DTO](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/dtos/map.dto.ts), [Immich marker controller](https://github.com/immich-app/immich/blob/46eb2e3616f3a7638efb9d19040e21166dd6d2f1/server/src/controllers/map.controller.ts), [MapLibre large-data guidance](https://maplibre.org/maplibre-gl-js/docs/guides/large-data/), [OpenFreeMap](https://openfreemap.org/), [OpenFreeMap integration](https://openfreemap.org/quick_start/), [OpenFreeMap terms](https://openfreemap.org/tos/).
