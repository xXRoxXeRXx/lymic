# Map View design discussion

Status: product details synthesized into [the feature specification](map-view-spec.md), 2026-11-12. Application implementation has not started.

## Confirmed product decisions

- Lymic renders its own Map View as a separate navigation destination. Immich supplies Remote Asset data; its web application is not embedded.
- The scope is the authenticated user's Immich Library, including images and videos with a recorded location, regardless of upload origin.
- The Immich Server remains unchanged; no additional server component is required.
- Lymic owns its background-map configuration independently. Do not import Immich map styles, tile-provider URLs or map settings; Immich supplies asset data and location markers, not the background map.
- Lymic provides exactly one map using OpenFreeMap, with application-controlled light/dark styles. Users cannot choose a provider or configure style URLs; there are no map-provider settings.
- Design for 300,000 Remote Assets with coordinates, not merely 300,000 total assets.
- Cluster clicks zoom in. Locations that cannot be separated expose a complete, incrementally loaded result list. No fixed membership truncation is acceptable.
- Match Immich Web's map presentation: zoomed-out locations appear as count clusters; as zoom separates the points, individual Remote Assets appear as thumbnail pins directly on the map. Plain location dots with thumbnails only in a result list do not meet the requested behavior.
- Individual results open the existing Lightbox interaction. Previous/next stays within the selected result list.
- Version 1 has no additional filters.
- Retain the location index in session memory only; do not persist location snapshots to disk. The existing Thumbnail Cache is unaffected.
- Initially show an extent containing the asset clusters. Users explore by zooming and panning; do not add an overview/reset button. Persistent camera restoration across app restarts was not agreed.
- Refresh marker data once in the background each time the user enters the Map View. Preserve existing session data while refreshing. Do not add a refresh button; no periodic polling was requested.
- Provisional acceptance profile: a 16 GB laptop with integrated graphics, at least 30 FPS during pan/zoom and at most 500 MiB additional process memory attributable to the Map View. Measure distributed and concentrated 300,000-point datasets, and separate cold network transfer from local processing. These are agreed targets awaiting measurement.
- Test the visible Map View through a replaceable desktop adapter as the main integration boundary: clusters, thumbnail pins, Lightbox and complete location results. Complement this with focused Rust API-contract tests and a real Tauri performance benchmark; the user confirmed this test boundary.

## Evidence and architecture candidate

See [Immich source research](immich-map-research.md) for pinned upstream evidence. The candidate is MapLibre rendering with a minimal marker snapshot, clustering outside the UI thread, bounded visible points/previews and reuse of Lymic's authenticated media protocols. Index ownership and transfer format remain implementation choices to validate through measurement. Persistent location caching is out of scope for version 1.

## Implementation verification

- Preserve thumbnail pins while determining a viewport-based rendering/loading strategy that meets the agreed performance targets.
- Verify supported Immich API versions and missing map permissions. Do not import Immich map presentation configuration.
- Measure cold network transfer, local index construction and warm reopening separately; no absolute initial-load time was agreed.
- Check the fixed OpenFreeMap resources and packaged worker behavior in Tauri, preserving credential isolation.

The feature specification records observable acceptance requirements and leaves internal tuning choices to implementation and measurement.

Published as [GitHub issue #163](https://github.com/xXRoxXeRXx/lymic/issues/163) with the `ready-for-agent` label.

The user explicitly approved correcting the published parent issue to exclude configurable map providers. Issue #163 now matches the corrected local specification.

## Published implementation tickets

All five tickets are native sub-issues of #163, labeled `ready-for-agent`, with native GitHub blocking relationships:

1. [#164: Shared Lightbox](https://github.com/xXRoxXeRXx/lymic/issues/164) — no blockers.
2. [#165: Map View with real Remote Assets](https://github.com/xXRoxXeRXx/lymic/issues/165) — blocked by #164.
3. [#166: Background refresh on navigation entry](https://github.com/xXRoxXeRXx/lymic/issues/166) — blocked by #165.
4. [#167: Complete location-group browsing](https://github.com/xXRoxXeRXx/lymic/issues/167) — blocked by #165.
5. [#168: 300,000-asset performance acceptance](https://github.com/xXRoxXeRXx/lymic/issues/168) — blocked by #166 and #167.
