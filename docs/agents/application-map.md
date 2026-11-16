# Lymic application map

Read this before changing Lymic application behaviour. It is the implementation-facing map; [the project documentation](../README.md) remains the user-facing reference.

## Product boundary

Lymic is a cross-platform Tauri desktop client that **uploads** media from local **Watched Folders** to one **Immich Server**. It discovers and watches **Media Assets**, avoids uploading **Duplicates** already known to Immich, and records durable progress locally.

It is not a bidirectional filesystem mirror: **Synchronization** does not automatically download **Remote Assets** or apply remote deletions or renames locally. A user-initiated **Download** from the Lightbox saves one **Remote Asset**'s original file to a chosen local path; it is separate from **Synchronization**.

The **Virtual Library** ([ADR-0003](../adr/0003-windows-virtual-cloud-library.md)) is a **planned, Windows-only** optional read-only projection of the **Immich Library** into the Windows filesystem via the Windows Cloud Files API (`cldapi.dll`), exposing **Virtual Placeholders** with on-demand **Hydration**. **It is not implemented on `master` or included in current builds** (there is no `cloud_files` module, `lymic-shell-ext` shell extension, Virtual Library IPC command, or Virtual Library UI/persistence in this codebase), and it is unavailable on macOS and Linux. Even as designed in [ADR-0003](../adr/0003-windows-virtual-cloud-library.md), the **Virtual Library** is strictly read-only and separate from **Synchronization**: Explorer delete, move, and write operations are denied so uploads and asset lifecycle remain exclusively in **Watched Folders** and **Synchronization**. Preserve that boundary unless the product decision explicitly changes it.

Use the defined terms in [`GLOSSARY.md`](../../GLOSSARY.md): **Watched Folder**, **Removable Watched Folder**, **Target Album**, **Album Assignment**, **Upload Unit**, **Synchronization**, **Download**, **Virtual Library**, **Virtual Placeholder**, **Hydration**, **Explore Overview**, **Place**, **Place Timeline**, **Memory**, and **Memory Timeline**. Target-album behaviour is governed by [ADR-0001](../adr/0001-folder-target-albums.md); removable-volume behaviour is governed by [ADR-0002](../adr/0002-removable-watched-folders.md); the planned read-only Windows Virtual Library design is recorded in [ADR-0003](../adr/0003-windows-virtual-cloud-library.md); Album Timelines and Place Timelines use paginated metadata search ([ADR-0006](../adr/0006-album-timeline-data-source.md), [ADR-0007](../adr/0007-place-timeline-metadata-search.md)).

## Runtime shape

```text
Svelte 5 UI (src/)
  invoke commands / subscribe to events
        │
        ▼
Tauri command layer (src-tauri/src/commands/)
        │
        ▼
Rust application services
  sync/  watched_folders/  watcher/  immich/  db/  audit/  app/
        │              │        │
        ▼              ▼        ▼
 Immich REST API    local FS   SQLite + native keyring
```

`src-tauri/src/app/mod.rs` is the runtime composition root. It registers every Tauri command, starts background work, creates the tray, and hides (rather than quits) the main window on close.

## Source map

| Area | Start here | Responsibility |
| --- | --- | --- |
| UI composition | `src/routes/+page.svelte` | Creates controllers, hydrates them, and selects login, photo timeline, explore, favorites, archive, trash, map, albums, people, dashboard, logs, settings, or info views, plus the sub-views reached only from other views (All Places, All Memories, and the album, person, place and memory timelines), each wrapped in `BackBarView.svelte` (the Back button above the content). It owns the Back origins (`personOrigin`, `placeOrigin`, `memoryOrigin`) and the per-view scroll positions. Archive is a main view: its navigation entry sits directly after Favorites, and it has no Back bar, no Back origin and no per-view scroll position (see [Archive Timeline](#archive-timeline)). Trash is a main view too: its navigation entry sits directly after Archive, and it has no Back bar, no Back origin and no per-view scroll position (see [Trash Timeline](#trash-timeline)). The Explore Overview, its controllers and views are described under [Explore](#explore). |
| UI state and IPC callers | `src/lib/controllers/` | One controller per concern: auth, timeline, favorites timeline, archive timeline, trash timeline, sync, watched folders, preferences, update, and feedback, plus the Explore controllers: explore (composes the independent People, Places and Memories sections and refreshes them when stale), people, places, memories (overview, at most 12 newest) and all-memories (paged), which share one error classifier (`explore-error.ts`); places and memories also share one list-loading helper (`explore-list-controller.svelte.ts`: generation guard, offline fallback, classified error). |
| UI components and translations | `src/lib/components/`, `src/lib/i18n/` | Present controller state; locale files are the UI strings. `remote-asset-actions.svelte.ts` is the single owner of Remote Asset detail caching, stale-request handling, favorite overrides on details, and Download (single file or archive) for the Lightbox, timeline selection, and Map View. `remote-asset-restore.ts` is the single owner of Restore for the Lightbox and timeline selection (see [Trash Timeline](#trash-timeline)). |
| Frontend IPC contracts and adapters | `src/lib/contracts.ts`, `src/lib/tauri/desktop-adapter.ts` | Shared frontend DTOs/state types and domain-split Tauri command adapters. |
| UI event bridge | `src/lib/tauri/sync-event-subscription.ts` | Stable frontend subscriptions and event payload expectations. |
| Tauri surface | `src-tauri/src/commands/`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/thumbnail_protocol.rs`, `src-tauri/src/app/asset_protocol.rs` | Command inputs, responses, custom streaming URI schemes (`lymic-thumb://`, `lymic-asset://`), and registration. |
| Sync orchestration | `src-tauri/src/sync/` | Scan results, durable queue, pause/resume, retries, upload processing, progress. |
| Watched Folder lifecycle | `src-tauri/src/watched_folders/` | One interface for the **Watched Folder** lifecycle: registration (validation, watcher registration, persistence with compensation, initial sync trigger), **Exclusion Pattern** updates (normalise, persist, rescan that folder), and removal. |
| Removable-volume monitoring | `src-tauri/src/removable/` | Windows uses `WindowsRemovableMediaMonitor` and `WM_DEVICECHANGE` to detect removable volumes. `MacOsRemovableMediaMonitor` and `LinuxRemovableMediaMonitor` are currently no-op stubs, so removable-volume arrival and remount reconciliation are not active on macOS or Linux. |
| File discovery | `src-tauri/src/media/` | Supported extensions, pairing, sidecars, path handling, and scanning. |
| Filesystem watching | `src-tauri/src/watcher/` | Recursive watcher lifecycle, debouncing, overflow recovery, unavailable-folder handling. |
| Immich integration | `src-tauri/src/immich/` | Authenticated client, endpoint calls, streaming multipart uploads, album calls, and the read-only Explore calls (`places.rs`, `memory.rs`, and the city-filtered Place Timeline search in `timeline.rs`), and `trash.rs`, the restore write path for Trashed Assets (the only Immich write the Trash Timeline performs). The 401/403/other mapping of the Places and Memories calls lives in one function (`explore_status_error` in `response.rs`), and the endpoint tests share one fake server (`test_support.rs`, `#[cfg(test)]`). `ImmichSession` (`session.rs`) is how the album, timeline, map, people, places and memories commands and the thumbnail and asset protocols obtain the client: its `run` returns credential-free errors, so those callers never redact by hand. `get_current_user` uses it too. Login validates credentials that are not stored yet, so it builds its own client and redacts both its audit reason and the error it returns through `AuthConfig::redact`. Synchronization (`sync/service.rs`) obtains its client from the session too, for start, retry and resume alike, so there is one construction path and one place invalid server configuration is detected. |
| Persistence | `src-tauri/src/db/`, `src-tauri/migrations/` | SQLite initialization, schema evolution, folders, state, queue, and settings. |
| Lifecycle and desktop UI | `src-tauri/src/app/` | Startup recovery, events, tray, locale, background tasks, app state. |
| Credentials and audit | `src-tauri/src/auth.rs`, `src-tauri/src/audit/` | Native-keyring secrets and structured local audit records. |

## Synchronization lifecycle

1. **Discover:** startup, a manual sync, adding a folder, or watcher events produce a scan. Watcher overflow triggers a rescan.
2. **Classify:** the media layer forms an **Upload Unit** from one asset or an allowed live-photo pair and finds an optional XMP sidecar.
3. **Hash:** unchanged files reuse a cached checksum only when current size and mtime match. Others are streamed while hashing.
4. **Check:** checksum batches go to Immich's bulk-upload check; existing assets are not uploaded.
5. **Queue and process:** missing assets are durably enqueued, then streamed as multipart uploads with configured parallelism.
6. **Assign and record:** when a Watched Folder has a Target Album, both uploaded and duplicate remote assets are assigned to it. Queue outcomes, remote IDs, failures, and progress are persisted.

The durable queue is the recovery boundary: `RUNNING`, `PAUSED`, `FAILED`, and `IDLE` live in `sync_jobs`; `PENDING`, `SYNCED`, and `FAILED` entries live in `sync_queue`. The Synchronization lifecycle coordinates job transitions and runner claims. On restart, pending work without a runner is recovered as paused; resume it from the queue rather than scanning folders again.

## Invariants to preserve

- Watched Folder paths are normalized; duplicates and parent/child overlaps are rejected.
- An unavailable folder remains configured and is retried by watcher reconciliation; it is not silently removed.
- Pause is durable and becomes effective at a safe queue boundary. Resume must be safe after restart.
- A Target Album is copied to the queue entry at enqueue time. Never fall back silently to the root library when its album operation fails.
- Persist `remote_id` after upload so an album-assignment retry does not re-upload the asset.
- Credentials (API key and optional Cloudflare Access token) stay in the native keyring, never SQLite, logs, errors, or UI events.
- Server URLs are HTTPS by default. HTTP is allowed only after explicit opt-in for loopback hosts; credentials in URLs, redirects, query strings, and fragments are rejected.
- Upload and hashing I/O stream files; do not introduce full-file buffering.
- Keep Tauri command names and event names/payloads synchronized with their Svelte callers. `sync-event-subscription.ts` is the frontend event contract.

## Boundary contracts

The map controller (`src/lib/controllers/map-controller.svelte.ts`) owns the session marker snapshot; `MapView.svelte` renders it and opens the shared Lightbox. The registered `get_map_markers` command uses `GET /api/map/markers` through the domain-specific map desktop adapter.

### Tauri IPC

Commands are registered in `app/mod.rs`; implementations live in `commands/`. The command groups are `auth`, `albums`, `timeline`, `people`, `places`, `memories`, `folders`, `sync`, and `settings`. `get_favorites_timeline_buckets` and `get_favorites_timeline_bucket_assets` belong to the timeline surface and supply the Favorites Timeline with favorite buckets and per-bucket Remote Assets. `get_archive_timeline_buckets` and `get_archive_timeline_bucket_assets` (argument `timeBucket`) are the two read-only commands of the Archive Timeline; they mirror the Favorites pair and are routed through `ImmichSession::with_stored_credentials`. `get_trash_timeline_buckets` and `get_trash_timeline_bucket_assets` (argument `timeBucket`) are the two read-only commands of the Trash Timeline, and `restore_trashed_assets` (argument `assetIds`) is the one Immich write it performs; all three are in `commands/timeline.rs` and routed through `ImmichSession::with_stored_credentials`. `get_asset_details` belongs to the timeline surface and supplies the Lightbox with a Remote Asset's original filename, favorite status, and optional EXIF details. `set_asset_favorite` changes a Remote Asset's favorite status. `download_asset` streams a Remote Asset's original file to the user-chosen Download path. `download_archive` retrieves Immich's ZIP archive, opens the native save dialog with Immich's safe suggested filename, and streams it to the selected path. `add_assets_to_album` adds one or more Remote Assets to an existing Album and reports successful and failed assignments. The Explore commands are read-only: `get_places` (`commands/places.rs`) lists Places; `get_place_timeline` (`commands/timeline.rs`, argument `city`) returns a Place's complete Timeline Buckets and assets; `get_memories` (`size`, clamped to 1-100) returns the newest Memories with their assets, and `get_memories_page` (`page`, 1-based, and `size`) returns a `MemoriesPage` for All Memories (both in `commands/memories.rs`). The People section reuses `get_people` and the `get_person_timeline_*` commands; there is no Explore-specific people command. Shared frontend IPC contracts live in `src/lib/contracts.ts`, and domain-specific adapter interfaces live in `src/lib/tauri/desktop-adapter.ts`. When changing a command, update its contract, domain adapter interface, owning caller (controller or component), and tests in the same change.

The backend emits `sync-progress`, `sync-progress-percent`, `sync-started`, `sync-transfer-stats`, `sync-progress-snapshot`, `sync-paused`, `sync-resumed`, `sync-error`, `log-message`, and `sync-idle`. Keep event names and serialized payloads backward-compatible within the application release.

### Immich

The core API calls are `GET /api/users/me`, `POST /api/assets/bulk-upload-check`, `POST /api/assets`, `GET /api/timeline/buckets`, `GET /api/timeline/bucket` (the Archive Timeline calls both with `visibility=archive`, see [Archive Timeline](#archive-timeline); the Trash Timeline calls both with `isTrashed=true`, see [Trash Timeline](#trash-timeline)), `GET /api/assets/{id}`, `PUT /api/assets/{id}` (favorite status), `POST /api/trash/restore/assets` (Restore, see [Trash Timeline](#trash-timeline)), `GET /api/search/cities` (Places, one Remote Asset per city, `asset.read`; `immich/places.rs`), `POST /api/search/metadata` with `city`, `withExif`, `order=desc`, `page` and `size=1000` (Place Timeline, `asset.read`, ADR-0007), `GET /api/memories?size=N&order=desc` (Memories, `memory.read`; `immich/memory.rs`), `GET /api/assets/{id}/thumbnail`, `GET /api/assets/{id}/original`, `POST /api/download/archive` (ZIP downloads), and `PUT /api/albums/{id}/assets` (album assignment); album behaviour is in `immich/album.rs` and timeline behaviour is in `immich/timeline.rs`. `GET /api/memories` has no offset or page parameter, so `get_memories_page` requests `size = page * pageSize + 1` Memories, drops the earlier pages, and treats the one extra Memory as the sign that a next page exists; paging counts Memories as Immich returns them, before Memories without dated assets (which cannot be shown) are dropped; a Memory's cover is the first asset that appears in its timeline. Memory responses embed their assets with EXIF and are read with a 32 MiB response cap instead of the default cap. Do not use `/api/search/explore`. Requests use `x-api-key`; bulk checks and uploads use the SHA-1/Base64 checksum contract. Changes to endpoints or payloads require checking the supported Immich API contract rather than guessing.

### Explore

The Explore Overview is read-only against the Immich Library, has no persisted state, and is separate from Synchronization. Navigation shows "Explore" directly after Photos; Photos stays the view opened after login.

| Piece | Files | Notes |
| --- | --- | --- |
| Overview | `ExploreView.svelte`, `ExploreSection.svelte`, `explore-controller.svelte.ts` | Sections in order People, Places, Memories. Each owns loading, error with Retry, offline (cached data stays visible) and empty state. An empty section without an error is hidden; a placeholder replaces all three when none is visible. The overview refreshes when opened and, while it is the open view, after Synchronization settles (`refreshIfActive`). When it settles while another view is open, the overview is marked stale and refreshes on return (`refreshIfStale`). |
| People section | `people-controller.svelte.ts`, `RecognizedPersonTile.svelte` | Reuses `get_people` and shows as many people as fit in one row; "View all" opens the People view. A key lacking `person.read` surfaces as a permission-denied state naming `person.read`, in the section and in the People view. |
| Places | `places-controller.svelte.ts`, `PlaceTile.svelte`, `PlacesView.svelte` | `get_places` (`GET /api/search/cities`). The overview shows as many tiles as fit in one row; "All Places" shows all. Tile thumbnails use `lymic-thumb://asset/...` and the Thumbnail Cache. |
| Place Timeline | `place-timeline-controller.svelte.ts` | `get_place_timeline` in the shared `TimelineView`, titled with the city name. The whole result loads before the scrubber appears; a failed page fails the load. |
| Memories | `memories-controller.svelte.ts`, `MemoryCard.svelte`, `memory-title.ts` (`localizedMemoryTitle` binds it to svelte-i18n) | `get_memories` with `size=12`; the controller also sorts newest first and caps at 12 in case the server ignores `size` and `order`. A Memory's cover is the first asset that appears in its timeline. Titles: "N years ago" for an on-this-day Memory on its anniversary (compared with the user's local today), the person's name for a birthday Memory, a medium date otherwise and for unknown types. |
| All Memories | `all-memories-controller.svelte.ts`, `AllMemoriesView.svelte` | `get_memories_page`, 24 per page, further pages load while scrolling; a failed further page keeps the pages already shown and offers retry. |
| Memory Timeline | `memory-timeline-controller.svelte.ts` | Needs no request: the Memory already carries its assets grouped into Timeline Buckets, which the shared `TimelineView` shows. |

All Places and All Memories have no navigation entry. Back returns to the view the user came from, tracked in `+page.svelte`: `personOrigin` (`people` or `explore`) for a Recognized Person's Photo Timeline, `placeOrigin` (`explore` or `places`) for a Place Timeline, and `memoryOrigin` (`explore` or `all-memories`) for a Memory Timeline. The Back label and the restored scroll position follow the origin; Explore, People, All Places and All Memories each remember their own scroll position, and sign-out resets the origins and positions.

Failure codes are credential-free strings that the controllers classify: `places_*` and `memories_*` (`*_authentication_required`, `*_permission_denied:<hint>`, `*_unavailable`, `*_invalid_response`); recognized people keep their `recognized_people_*` codes. One classifier (`explore-error.ts`) maps `*_permission_denied:` to the permission-denied state and `*_authentication_required` and `*_unavailable` to the unavailable state for People, Places, Memories and All Memories; anything else is a generic error. A Place Timeline failure shows the shared timeline load error. Lightbox actions (Favorite, Download, Add to Album) come from the same `remote-asset-actions.svelte.ts` owner as every other timeline.

Required API key permissions: `asset.read` (Places, Place Timelines), `person.read` (People section) and `memory.read` (Memories); thumbnails and the Lightbox continue to need `asset.view`. A key lacking one of them disables only that section. Immich Web's "Recently added" section is deliberately not implemented.

Not verified against a live Immich server:

- The birthday Memory response shape is a guess. `immich/memory.rs` reads the person's name from `data.personName`, falling back to `data.name`; with neither, the birthday Memory gets the date title.
- ADR-0007's same-named-city finding comes from reading Immich's server source, not from a running server, and names no Immich version.
- Memories need a recent Immich release. No minimum version is documented and the Memory response shape has not been checked against several releases; absent optional fields are tolerated.

### Archive Timeline

The Archive Timeline shows the authenticated user's Archived Assets in the shared `TimelineView` (Timeline Scrubber, Lightbox, Thumbnail Cache). It is read-only: Lymic never archives or unarchives a Remote Asset, and the Lightbox and selection offer only Favorite, Download and Add to Album from `remote-asset-actions.svelte.ts`. It is separate from Synchronization, Watched Folders and the Virtual Library, and persists nothing.

| Piece | Files | Notes |
| --- | --- | --- |
| Navigation and view | `src/routes/+page.svelte` | An "Archive" entry (lucide `Archive` icon, label `archive_title`) directly after Favorites. `openArchive()` switches to `currentView = "archive"` and calls `archive.refresh()`, as Favorites does; reopening the active view is a no-op. It is a main view: no `BackBarView`, no Back origin, no per-view scroll position. Loading, empty, error and offline messages are the `archive_loading`, `archive_empty`, `archive_error` and `archive_offline` locale keys. |
| Controller | `archive-timeline-controller.svelte.ts` | `createTimelineController` over `ArchiveTimelineDesktopAdapter` (`getArchiveTimelineBuckets`, `getArchiveTimelineBucketAssets`). Created and hydrated with the other controllers in `session-controller.svelte.ts`, which also clears it with the other per-user state on login and sign-out. The shared timeline contract test runs it like the Favorites Timeline. |
| Tauri commands | `commands/timeline.rs`, registered in `app/mod.rs` | `get_archive_timeline_buckets` and `get_archive_timeline_bucket_assets` (`time_bucket`), both read-only. |
| Immich client | `immich/timeline.rs` | `get_archive_timeline_buckets` and `get_archive_timeline_bucket_assets` reuse `fetch_timeline_buckets_with_query` and `fetch_timeline_bucket_assets_with_query`; see below. |

Immich calls (both need only `asset.read`; thumbnails and the Lightbox continue to need `asset.view`):

- `GET /api/timeline/buckets?size=MONTH&visibility=archive`
- `GET /api/timeline/bucket?size=MONTH&timeBucket=<bucket>&visibility=archive`

The requests carry no `isFavorite`, `isArchived`, `isTrashed` or `withPartners` parameter; the deprecated archived flag and metadata search are not used. Unlike the Favorites Timeline, a bucket's assets are not defaulted to favorites: `isFavorite` comes from the response. Buckets are sorted newest first. Failures are credential-free strings (`Failed to fetch archive timeline buckets: <status>`, `Failed to fetch archive timeline bucket: <status>`, `Invalid ... response`) and show the shared timeline load error. The one exception is HTTP 403: it returns the credential-free code `archive_permission_denied:asset.read` (the shared `explore_status_error` format, applied only to the Archive requests), which `archive-timeline-controller.svelte.ts` classifies with `classifyExploreError(…, "archive")` and exposes as `permissionDenied`, so the view shows `archive_permission_error` (naming the missing `asset.read` permission) instead of `archive_error`, with Retry still available. No minimum Immich version is gated, so a server that rejects the parameter surfaces as that load error.

#### Verification status of the Immich assumptions

None of these was verified against a live Immich server; no server was available while documenting. They stay UNVERIFIED until someone runs the manual check below. The documentation evidence column comes from the public Immich OpenAPI spec and server source on `main` at the time of writing (spec version `3.4.0-rc.0`). It guides the manual check, is not live verification, and names no minimum release.

| Assumption | Status | Documentation evidence only |
| --- | --- | --- |
| The parameter is named `visibility` and takes the value `archive` on both timeline endpoints. | UNVERIFIED (live) | `GET /timeline/buckets` and `GET /timeline/bucket` document a `visibility` query parameter with enum `archive`, `timeline`, `hidden`, `locked`, requiring `asset.read`. Older servers used `isArchived`; their behaviour with `visibility` is unknown. |
| `visibility=archive` excludes trashed and locked-folder assets. | UNVERIFIED (live) | The server query filters `asset.visibility = <visibility>` and, unless `isTrashed=true`, `asset.deletedAt is null`. Locked assets have visibility `locked`, so they should not match `archive`, and trashed assets should be removed by the `deletedAt` condition. |
| The Photo Timeline and Favorites Timeline already exclude Archived Assets. | UNVERIFIED (live); the documentation evidence points the other way | Lymic sends no `visibility` to those two timelines. In the server source an omitted `visibility` applies `withDefaultVisibility`, which matches `archive` and `timeline`, so those timelines may already include Archived Assets (Immich Web passes `visibility=timeline` to hide them). If a live check confirms this, an Archived Asset shows in both the Photo Timeline and the Archive Timeline. #230 leaves those timelines unchanged, so open a separate issue. |
| Extra: the `size=MONTH` parameter that every Lymic timeline request sends is accepted. | UNVERIFIED (live) | The `3.4.0-rc.0` spec lists no `size` parameter on either timeline endpoint. This is existing behaviour shared by all timelines, noted because it surfaces in the same check. |

Manual check against a live Immich server: prepare a test library with one archived image, one archived video, one archived-then-trashed asset, one asset in the locked folder, one asset that is both favorite and archived, and one plain timeline asset. Call both endpoints with `curl -H "x-api-key: <key>"`, once with `visibility=archive` and once without. Confirm that the request is accepted, that the bucket counts match the archived set only, that the trashed and locked-folder assets are absent, and whether the request without `visibility` returns the archived assets. Then open Lymic's Archive, Photos and Favorites views and compare. Record the result and the Immich version here, replace each UNVERIFIED with the observed outcome, and open an issue for any difference.

### Trash Timeline

The Trash Timeline shows the authenticated user's Trashed Assets in the shared `TimelineView` (Timeline Scrubber, Lightbox, Thumbnail Cache). Lymic performs exactly one write to the Immich trash: Restore. It never moves a Remote Asset into the trash, never deletes one permanently, never empties the trash, and offers no "restore all". The Lightbox and multi-selection offer only Restore and Download (`allowFavoriteAndAlbum={false}` hides Favorite and Add to Album). It is separate from Synchronization, Watched Folders and the Virtual Library: viewing or restoring touches no local file, Watched Folder or Synchronization state, and nothing is persisted.

| Piece | Files | Notes |
| --- | --- | --- |
| Navigation and view | `src/routes/+page.svelte` | A "Trash" entry (lucide `Trash2` icon, label `trash_title`) directly after Archive. `openTrash()` switches to `currentView = "trash"` and calls `trash.refresh()`; reopening the active view is a no-op. It is a main view: no `BackBarView`, no Back origin, no per-view scroll position, no count badge. An effect calls `trash.clear()` when the view is left, so the assets and any selection are dropped and returning shows a fresh load. Loading, empty, error and offline messages are `trash_loading`, `trash_empty`, `trash_error` and `trash_offline`; `trash_hint` is a fixed hint (no number) that Immich removes trashed items after the server's retention period. Lymic does not read that setting. |
| Controller | `trash-timeline-controller.svelte.ts` | `createTimelineController` over `TrashTimelineDesktopAdapter` (`getTrashTimelineBuckets`, `getTrashTimelineBucketAssets`). Created and hydrated with the other controllers in `session-controller.svelte.ts`, which also clears it on login and sign-out. The shared timeline contract test runs it like the Favorites Timeline. Browsing has no permission-specific state: any failure, including HTTP 403, shows the shared `trash_error` with Retry. |
| Restore | `remote-asset-restore.ts`, `TimelineView.svelte`, `Lightbox.svelte` | `createRemoteAssetRestore` makes one `restoreTrashedAssets` request for the given Remote Assets, removes the restored ones from the timeline at once, and keeps every asset not confirmed as restored. It never rejects. `TimelineView` takes `restoreTrashedAssets` and `allowFavoriteAndAlbum` props; the Lightbox gets an `onRestore` callback and `allowFavorite=false`. No confirmation dialog. From the Lightbox a successful Restore closes it. From multi-selection the selection shrinks to the assets that failed, with `trash_restore_success`, `trash_restore_partial` (some failed) or `trash_restore_none` (nothing restored; both name how many failed, and the Lightbox uses `trash_restore_none` too). A refused Restore shows `trash_restore_permission`, a failed request `trash_restore_failed` (no count); the view stays usable. |
| Tauri commands | `commands/timeline.rs`, registered in `app/mod.rs` | `get_trash_timeline_buckets` and `get_trash_timeline_bucket_assets` (`time_bucket`), both read-only; `restore_trashed_assets` (`asset_ids`, rejects an empty list or an empty ID) returns `TrashRestoreResult { restoredAssetIds, failedAssetIds }`. |
| Immich client | `immich/timeline.rs`, `immich/trash.rs` | The browse pair reuses `fetch_timeline_buckets_with_query` and `fetch_timeline_bucket_assets_with_query`; Restore is `restore_trashed_assets` in `trash.rs`. |

Immich calls as Lymic sends them (browsing needs `asset.read`; thumbnails and the Lightbox continue to need `asset.view`; Restore is assumed to need `asset.delete`):

- `GET /api/timeline/buckets?size=MONTH&isTrashed=true`
- `GET /api/timeline/bucket?size=MONTH&timeBucket=<bucket>&isTrashed=true`
- `POST /api/trash/restore/assets` with `{"ids":["<id>", ...]}`, reading `{"count": n}`
- `GET /api/assets/{id}`, read back only after a partial count, reading `isTrashed`

The browse requests carry no `isFavorite`, `visibility`, `isArchived` or `withPartners` parameter. Buckets are sorted newest first, and `isFavorite` comes from the response. Failures of the browse calls are credential-free strings (`Failed to fetch trash timeline buckets: <status>`, `Failed to fetch trash timeline bucket: <status>`, `Invalid ... response`) and show the shared timeline load error. No minimum Immich version is gated, so a server that rejects the filter surfaces as that load error.

Restore sends all selected IDs in one request (IDs must be unique and path-safe). The result is interpreted from `count` alone: `count` equal to the number requested means every asset was restored, `0` means none was, and more than requested is an error. Because `count` does not name the failed assets, a partial count triggers one `GET /api/assets/{id}` per requested asset, and an asset counts as restored only when the response says `isTrashed` is false; any doubt (request failure, non-success status, unreadable body) counts as still trashed. The frontend additionally treats anything not confirmed as restored as failed. Request-level failures are credential-free codes and never forward the response body: `trash_restore_authentication_required` (401), `trash_restore_permission_denied:asset.delete` (403, classified by `classifyExploreError(…, "trash_restore")`) and `trash_restore_unavailable` (any other status or a transport error).

#### Verification status of the Immich assumptions

None of this was verified against a live Immich server; no server was available while implementing or documenting. Every row stays UNVERIFIED until someone runs the manual check below. Lymic's own tests use a fake server that returns the shapes Lymic assumes, so they cannot confirm any of it.

| Assumption | Status | Notes |
| --- | --- | --- |
| The trashed filter is the query parameter `isTrashed` with the value `true` on both timeline endpoints, and `size=MONTH` is accepted with it. | UNVERIFIED (live) | Name and value are assumed. The [Archive Timeline](#archive-timeline) notes list documentation evidence that the server hides trashed assets unless `isTrashed=true`; that is source reading, not a live check. |
| Restore is `POST /api/trash/restore/assets` with body `{"ids":[...]}` and returns `{"count": n}`, the number of assets restored, with no per-asset result. | UNVERIFIED (live) | Call, body and result shape are assumed. A different endpoint, field name or result shape makes Restore fail with `trash_restore_unavailable` or an "Invalid trash restore response" error. |
| After a partial count, `GET /api/assets/{id}` reports `isTrashed` (false once restored). | UNVERIFIED (live) | The field name and that it is false after a Restore are assumed. If it is absent or differs, assets that were in fact restored are reported as failed and stay visible until the next refresh. |
| Restore needs the API key permission `asset.delete`. | UNVERIFIED (live) | Assumed; the 403 message names `asset.delete`. If the server needs another permission the message is misleading. Lymic does not check permissions up front. |
| The Photo Timeline and Favorites Timeline already exclude Trashed Assets. | UNVERIFIED (live) | Lymic sends no trashed filter to those two timelines and relies on Immich's default. If they include Trashed Assets, a Trashed Asset shows there as well as in the Trash Timeline; open a separate issue. The Archive Timeline's trashed exclusion is likewise unverified (see above). |
| Synchronization still works for a local file in a Watched Folder whose Remote Asset is in the trash. | UNVERIFIED, unchecked risk | Synchronization resolves duplicates through a checksum search on Immich, which may hide trashed assets by default, so such a local file might not resolve to its Remote Asset: it might be uploaded again, or duplicate resolution or Target Album assignment might fail. This feature does not change Synchronization. |

Manual check against a live Immich server. Record the result and the Immich version here, replace each UNVERIFIED with the observed outcome, fix or open an issue for anything that differs from the assumptions:

1. Prepare a test library with one plain image, one video, and three assets moved to the Immich trash (one of them a favorite). Use one API key with `asset.read`, `asset.view` and `asset.delete`, and one without `asset.delete`.
2. Call `GET /api/timeline/buckets?size=MONTH&isTrashed=true` and `GET /api/timeline/bucket?size=MONTH&timeBucket=<bucket>&isTrashed=true` with `curl -H "x-api-key: <key>"`. Confirm that the parameter and `size=MONTH` are accepted and that only the trashed assets are returned. Repeat both calls without `isTrashed` (and the Favorites calls with `isFavorite=true`) and confirm the trashed assets are absent. Open Lymic's Photos, Favorites and Trash views and compare.
3. Call `POST /api/trash/restore/assets` with `{"ids":["<trashed id>"]}`. Record the exact status code and body. Repeat with one trashed ID plus one unknown or already restored ID and record what a partial `count` looks like and whether the response names assets. Then `GET /api/assets/{id}` and confirm `isTrashed` is present, false for a restored asset and true for one still in the trash.
4. Repeat the Restore with the key without `asset.delete`. Record the status code and the permission the server names, and confirm Lymic shows `trash_restore_permission`. Confirm browsing still works with that key.
5. In Lymic, open Trash, Restore one asset from the Lightbox and several from multi-selection (including, if possible, one that cannot be restored). Confirm the restored assets leave the view, the failed ones stay selected, and the restored assets appear in Photos.
6. Synchronization risk: keep a local file in a Watched Folder whose Remote Asset has been moved into the Immich trash (and, in a second run, with a Target Album set), run a sync, and record whether duplicate resolution or the Target Album assignment fails or the file is uploaded again. If it fails, open a separate issue; it is not fixed here.

### Persistence

Add schema changes as a new, forward-only file in `src-tauri/migrations/`. Keep migration, db accessor, queue/recovery behaviour, and tests aligned. Do not mutate an applied migration.

## Change guide

| If changing… | Inspect together |
| --- | --- |
| A UI action or displayed state | Component, owning controller, Tauri command, event type/subscription, locale strings, relevant test. |
| Sync semantics | `sync/`, `db/queue.rs`, `db/sync_state.rs`, `commands/sync.rs`, and `controllers/sync-controller.svelte.ts`. |
| Folder lifecycle | `watched_folders/`, `commands/folders.rs`, `watcher/`, `media/paths.rs`, scanner, persistence, `controllers/watched-folders-controller.svelte.ts`, and folder UI. |
| Media recognition or pairing | `media/extensions.rs`, `media/model.rs`, `media/scanner.rs`, upload representation, and tests. |
| Explore: Places, Memories, Place or Memory Timelines, Back origins | `+page.svelte`, `explore-controller.svelte.ts`, the places, memories, all-memories and timeline controllers, `immich/places.rs`, `immich/memory.rs`, `immich/timeline.rs`, `commands/{places,memories,timeline}.rs`, contracts, desktop adapter, locale strings, ADR-0007. |
| An Immich request | `immich/client.rs` plus the endpoint module, `immich/session.rs` (credential redaction), credential policy, and error handling. |
| Album behaviour | `immich/album.rs`, `sync/album_batch.rs`, queue schema, folder UI, and ADR-0001. |
| Settings, locale, tray, or startup | Owning controller plus `app/locale.rs`, `app/tray.rs`, and `app/startup.rs` as applicable. |

## Verification

Run the narrowest relevant tests while iterating, then run the applicable checks before handoff:

```powershell
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Use `npm run tauri dev` for an end-to-end desktop smoke test. Migrations, pause/recovery, credential handling, queue retry, duplicate handling, and Target Album failure paths are high-risk changes: verify their observable behavior rather than only compilation.

## Supporting references

- [`GLOSSARY.md`](../../GLOSSARY.md): canonical vocabulary.
- [`docs/adr/`](../adr/): resolved architectural decisions; read the ADRs that touch the change.
- [`docs/README.md`](../README.md): product, operational, security, and release detail.
- [`README.md`](../../README.md): repository entry point and local setup.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md): contribution expectations.
