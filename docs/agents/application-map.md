# Lymic application map

Read this before changing Lymic application behaviour. It is the implementation-facing map; [the project documentation](../README.md) remains the user-facing reference.

## Product boundary

Lymic is a cross-platform Tauri desktop client that **uploads** media from local **Watched Folders** to one Immich server. It discovers and watches local files, avoids uploading assets already known to Immich, and records durable progress locally.

It is not a bidirectional filesystem mirror: it neither downloads remote assets nor applies remote deletions or renames locally. Preserve that boundary unless the product decision explicitly changes it.

Use the defined terms in [`GLOSSARY.md`](../../GLOSSARY.md): **Watched Folder**, **Removable Watched Folder**, **Target Album**, **Album Assignment**, and **Upload Unit**. Target-album behaviour is governed by [ADR-0001](../adr/0001-folder-target-albums.md); removable-volume behaviour is governed by [ADR-0002](../adr/0002-removable-watched-folders.md).

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
| UI composition | `src/routes/+page.svelte` | Creates controllers, hydrates them, and selects login, dashboard, or settings views. |
| UI state and IPC callers | `src/lib/controllers/` | One controller per concern: auth, sync, preferences, update, and feedback. |
| UI components and translations | `src/lib/components/`, `src/lib/i18n/` | Present controller state; locale files are the UI strings. |
| UI event bridge | `src/lib/tauri/sync-event-subscription.ts` | Stable frontend subscriptions and event payload expectations. |
| Tauri surface | `src-tauri/src/commands/`, `src-tauri/src/app/mod.rs` | Command inputs, responses, and registration. |
| Sync orchestration | `src-tauri/src/sync/` | Scan results, durable queue, pause/resume, retries, upload processing, progress. |
| Watched Folder registration | `src-tauri/src/watched_folders/` | Watched Folder validation, watcher registration, persistence with compensation, and initial sync trigger. |
| File discovery | `src-tauri/src/media/` | Supported extensions, pairing, sidecars, path handling, and scanning. |
| Filesystem watching | `src-tauri/src/watcher/` | Recursive watcher lifecycle, debouncing, overflow recovery, unavailable-folder handling. |
| Immich integration | `src-tauri/src/immich/` | Authenticated client, endpoint calls, streaming multipart uploads, album calls. |
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

The durable queue is the recovery boundary: `RUNNING`, `PAUSED`, and `IDLE` live in `sync_jobs`; `PENDING`, `SYNCED`, and `FAILED` entries live in `sync_queue`. Resume recovered work from the queue rather than scanning folders again.

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

### Tauri IPC

Commands are registered in `app/mod.rs`; implementations live in `commands/`. The command groups are `auth`, `albums`, `folders`, `sync`, and `settings`. When changing a command, update its controller caller and tests in the same change.

The backend emits `sync-progress`, `sync-progress-percent`, `sync-started`, `sync-transfer-stats`, `sync-progress-snapshot`, `sync-paused`, `sync-resumed`, `sync-error`, `trigger-sync`, `log-message`, and `sync-idle`. Keep event names and serialized payloads backward-compatible within the application release.

### Immich

The core API calls are `GET /api/users/me`, `POST /api/assets/bulk-upload-check`, and `POST /api/assets`; album behaviour is in `immich/album.rs`. Requests use `x-api-key`; bulk checks and uploads use the SHA-1/Base64 checksum contract. Changes to endpoints or payloads require checking the supported Immich API contract rather than guessing.

### Persistence

Add schema changes as a new, forward-only file in `src-tauri/migrations/`. Keep migration, db accessor, queue/recovery behaviour, and tests aligned. Do not mutate an applied migration.

## Change guide

| If changing… | Inspect together |
| --- | --- |
| A UI action or displayed state | Component, owning controller, Tauri command, event type/subscription, locale strings, relevant test. |
| Sync semantics | `sync/`, `db/queue.rs`, `db/sync_state.rs`, `commands/sync.rs`, and the frontend sync controller. |
| Folder lifecycle | `watched_folders/`, `commands/folders.rs`, `watcher/`, `media/paths.rs`, scanner, persistence, and folder UI. |
| Media recognition or pairing | `media/extensions.rs`, `media/model.rs`, `media/scanner.rs`, upload representation, and tests. |
| An Immich request | `immich/client.rs` plus the endpoint module, credential policy, audit redaction, and error handling. |
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
