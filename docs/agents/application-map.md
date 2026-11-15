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
| UI composition | `src/routes/+page.svelte` | Creates controllers, hydrates them, and selects login, photo timeline, explore, favorites, map, albums, people, dashboard, logs, settings, or info views, plus the sub-views reached only from other views (All Places, All Memories, and the album, person, place and memory timelines). It owns the Back origins (`personOrigin`, `placeOrigin`, `memoryOrigin`) and the per-view scroll positions. The Explore Overview, its controllers and views are described under [Explore](#explore). |
| UI state and IPC callers | `src/lib/controllers/` | One controller per concern: auth, timeline, favorites timeline, sync, watched folders, preferences, update, and feedback, plus the Explore controllers: explore (composes the independent People, Places and Memories sections and refreshes them when stale), people, places, memories (overview, at most 12 newest) and all-memories (paged), which share one error classifier (`explore-error.ts`). |
| UI components and translations | `src/lib/components/`, `src/lib/i18n/` | Present controller state; locale files are the UI strings. `remote-asset-actions.svelte.ts` is the single owner of Remote Asset detail caching, stale-request handling, favorite overrides on details, and Download (single file or archive) for the Lightbox, timeline selection, and Map View. |
| Frontend IPC contracts and adapters | `src/lib/contracts.ts`, `src/lib/tauri/desktop-adapter.ts` | Shared frontend DTOs/state types and domain-split Tauri command adapters. |
| UI event bridge | `src/lib/tauri/sync-event-subscription.ts` | Stable frontend subscriptions and event payload expectations. |
| Tauri surface | `src-tauri/src/commands/`, `src-tauri/src/app/mod.rs`, `src-tauri/src/app/thumbnail_protocol.rs`, `src-tauri/src/app/asset_protocol.rs` | Command inputs, responses, custom streaming URI schemes (`lymic-thumb://`, `lymic-asset://`), and registration. |
| Sync orchestration | `src-tauri/src/sync/` | Scan results, durable queue, pause/resume, retries, upload processing, progress. |
| Watched Folder lifecycle | `src-tauri/src/watched_folders/` | One interface for the **Watched Folder** lifecycle: registration (validation, watcher registration, persistence with compensation, initial sync trigger), **Exclusion Pattern** updates (normalise, persist, rescan that folder), and removal. |
| Removable-volume monitoring | `src-tauri/src/removable/` | Windows uses `WindowsRemovableMediaMonitor` and `WM_DEVICECHANGE` to detect removable volumes. `MacOsRemovableMediaMonitor` and `LinuxRemovableMediaMonitor` are currently no-op stubs, so removable-volume arrival and remount reconciliation are not active on macOS or Linux. |
| File discovery | `src-tauri/src/media/` | Supported extensions, pairing, sidecars, path handling, and scanning. |
| Filesystem watching | `src-tauri/src/watcher/` | Recursive watcher lifecycle, debouncing, overflow recovery, unavailable-folder handling. |
| Immich integration | `src-tauri/src/immich/` | Authenticated client, endpoint calls, streaming multipart uploads, album calls, and the read-only Explore calls (`places.rs`, `memory.rs`, and the city-filtered Place Timeline search in `timeline.rs`). `ImmichSession` (`session.rs`) is how the album, timeline, map, people, places and memories commands and the thumbnail and asset protocols obtain the client: its `run` returns credential-free errors, so those callers never redact by hand. `get_current_user` uses it too. Login validates credentials that are not stored yet, so it builds its own client and redacts both its audit reason and the error it returns through `AuthConfig::redact`. Synchronization (`sync/service.rs`) obtains its client from the session too, for start, retry and resume alike, so there is one construction path and one place invalid server configuration is detected. |
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

Commands are registered in `app/mod.rs`; implementations live in `commands/`. The command groups are `auth`, `albums`, `timeline`, `people`, `places`, `memories`, `folders`, `sync`, and `settings`. `get_favorites_timeline_buckets` and `get_favorites_timeline_bucket_assets` belong to the timeline surface and supply the Favorites Timeline with favorite buckets and per-bucket Remote Assets. `get_asset_details` belongs to the timeline surface and supplies the Lightbox with a Remote Asset's original filename, favorite status, and optional EXIF details. `set_asset_favorite` changes a Remote Asset's favorite status. `download_asset` streams a Remote Asset's original file to the user-chosen Download path. `download_archive` retrieves Immich's ZIP archive, opens the native save dialog with Immich's safe suggested filename, and streams it to the selected path. `add_assets_to_album` adds one or more Remote Assets to an existing Album and reports successful and failed assignments. The Explore commands are read-only: `get_places` (`commands/places.rs`) lists Places; `get_place_timeline` (`commands/timeline.rs`, argument `city`) returns a Place's complete Timeline Buckets and assets; `get_memories` (`size`, clamped to 1-100) returns the newest Memories with their assets, and `get_memories_page` (`page`, 1-based, and `size`) returns a `MemoriesPage` for All Memories (both in `commands/memories.rs`). The People section reuses `get_people` and the `get_person_timeline_*` commands; there is no Explore-specific people command. Shared frontend IPC contracts live in `src/lib/contracts.ts`, and domain-specific adapter interfaces live in `src/lib/tauri/desktop-adapter.ts`. When changing a command, update its contract, domain adapter interface, owning caller (controller or component), and tests in the same change.

The backend emits `sync-progress`, `sync-progress-percent`, `sync-started`, `sync-transfer-stats`, `sync-progress-snapshot`, `sync-paused`, `sync-resumed`, `sync-error`, `log-message`, and `sync-idle`. Keep event names and serialized payloads backward-compatible within the application release.

### Immich

The core API calls are `GET /api/users/me`, `POST /api/assets/bulk-upload-check`, `POST /api/assets`, `GET /api/timeline/buckets`, `GET /api/timeline/bucket`, `GET /api/assets/{id}`, `PUT /api/assets/{id}` (favorite status), `GET /api/search/cities` (Places, one Remote Asset per city, `asset.read`; `immich/places.rs`), `POST /api/search/metadata` with `city`, `withExif`, `order=desc`, `page` and `size=1000` (Place Timeline, `asset.read`, ADR-0007), `GET /api/memories?size=N&order=desc` (Memories, `memory.read`; `immich/memory.rs`), `GET /api/assets/{id}/thumbnail`, `GET /api/assets/{id}/original`, `POST /api/download/archive` (ZIP downloads), and `PUT /api/albums/{id}/assets` (album assignment); album behaviour is in `immich/album.rs` and timeline behaviour is in `immich/timeline.rs`. `GET /api/memories` has no offset or page parameter, so `get_memories_page` requests `size = page * pageSize + 1` Memories, drops the earlier pages, and treats the one extra Memory as the sign that a next page exists; paging counts Memories as Immich returns them, before Memories without dated assets (which cannot be shown) are dropped; a Memory's cover is the first asset that appears in its timeline. Memory responses embed their assets with EXIF and are read with a 32 MiB response cap instead of the default cap. Do not use `/api/search/explore`. Requests use `x-api-key`; bulk checks and uploads use the SHA-1/Base64 checksum contract. Changes to endpoints or payloads require checking the supported Immich API contract rather than guessing.

### Explore

The Explore Overview is read-only against the Immich Library, has no persisted state, and is separate from Synchronization. Navigation shows "Explore" directly after Photos; Photos stays the view opened after login.

| Piece | Files | Notes |
| --- | --- | --- |
| Overview | `ExploreView.svelte`, `ExploreSection.svelte`, `explore-controller.svelte.ts` | Sections in order People, Places, Memories. Each owns loading, error with Retry, offline (cached data stays visible) and empty state. An empty section without an error is hidden; a placeholder replaces all three when none is visible. The overview refreshes when opened and, while it is the open view, after Synchronization settles (`refreshIfActive`). When it settles while another view is open, the overview is marked stale and refreshes on return (`refreshIfStale`). |
| People section | `people-controller.svelte.ts`, `RecognizedPersonTile.svelte` | Reuses `get_people` and shows as many people as fit in one row; "View all" opens the People view. A key lacking `person.read` surfaces as a permission-denied state naming `person.read`, in the section and in the People view. |
| Places | `places-controller.svelte.ts`, `PlaceTile.svelte`, `PlacesView.svelte` | `get_places` (`GET /api/search/cities`). The overview shows as many tiles as fit in one row; "All Places" shows all. Tile thumbnails use `lymic-thumb://asset/...` and the Thumbnail Cache. |
| Place Timeline | `place-timeline-controller.svelte.ts` | `get_place_timeline` in the shared `TimelineView`, titled with the city name. The whole result loads before the scrubber appears; a failed page fails the load. |
| Memories | `memories-controller.svelte.ts`, `MemoryCard.svelte`, `memory-title.ts` | `get_memories` with `size=12`; the controller also sorts newest first and caps at 12 in case the server ignores `size` and `order`. A Memory's cover is the first asset that appears in its timeline. Titles: "N years ago" for an on-this-day Memory on its anniversary (compared with the user's local today), the person's name for a birthday Memory, a medium date otherwise and for unknown types. |
| All Memories | `all-memories-controller.svelte.ts`, `AllMemoriesView.svelte` | `get_memories_page`, 24 per page, further pages load while scrolling; a failed further page keeps the pages already shown and offers retry. |
| Memory Timeline | `memory-timeline-controller.svelte.ts` | Needs no request: the Memory already carries its assets grouped into Timeline Buckets, which the shared `TimelineView` shows. |

All Places and All Memories have no navigation entry. Back returns to the view the user came from, tracked in `+page.svelte`: `personOrigin` (`people` or `explore`) for a Recognized Person's Photo Timeline, `placeOrigin` (`explore` or `places`) for a Place Timeline, and `memoryOrigin` (`explore` or `all-memories`) for a Memory Timeline. The Back label and the restored scroll position follow the origin; Explore, People, All Places and All Memories each remember their own scroll position, and sign-out resets the origins and positions.

Failure codes are credential-free strings that the controllers classify: `places_*` and `memories_*` (`*_authentication_required`, `*_permission_denied:<hint>`, `*_unavailable`, `*_invalid_response`); recognized people keep their `recognized_people_*` codes. One classifier (`explore-error.ts`) maps `*_permission_denied:` to the permission-denied state and `*_authentication_required` and `*_unavailable` to the unavailable state for People, Places, Memories and All Memories; anything else is a generic error. A Place Timeline failure shows the shared timeline load error. Lightbox actions (Favorite, Download, Add to Album) come from the same `remote-asset-actions.svelte.ts` owner as every other timeline.

Required API key permissions: `asset.read` (Places, Place Timelines), `person.read` (People section) and `memory.read` (Memories); thumbnails and the Lightbox continue to need `asset.view`. A key lacking one of them disables only that section. Immich Web's "Recently added" section is deliberately not implemented.

Not verified against a live Immich server:

- The birthday Memory response shape is a guess. `immich/memory.rs` reads the person's name from `data.personName`, falling back to `data.name`; with neither, the birthday Memory gets the date title.
- ADR-0007's same-named-city finding comes from reading Immich's server source, not from a running server, and names no Immich version.
- Memories need a recent Immich release. No minimum version is documented and the Memory response shape has not been checked against several releases; absent optional fields are tolerated.

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
