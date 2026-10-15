---
type: workflow
title: Watched Folders and Removable Volumes
description: How Lymic persists folder roots, reconciles recursive filesystem watchers, scans discovered media, and handles currently implemented removable-volume detection.
tags: [folders, watcher, media, removable-volumes, synchronization]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T14:38:41.442Z
sources:
  - id: openwiki-source-5834cc73f881f690a60014d3
    resource: repo://src-tauri/migrations/0006_removable_watched_folders.sql
  - id: openwiki-source-3638749eb6f61a23896edb4d
    resource: repo://src-tauri/src/app/background.rs
  - id: openwiki-source-2fd48a041dfbcb39bc092be1
    resource: repo://src-tauri/src/app/startup.rs
  - id: openwiki-source-743fce1e1a7530f5eafe9a04
    resource: repo://src-tauri/src/commands/folders.rs
  - id: openwiki-source-a742e6a26921d17bb568bbf5
    resource: repo://src-tauri/src/media/paths.rs
  - id: openwiki-source-5eb94a640e50f33a69637199
    resource: repo://src-tauri/src/media/scanner.rs
  - id: openwiki-source-6e01ec2667abcaf79e009ae6
    resource: repo://src-tauri/src/removable/coordinator.rs
  - id: openwiki-source-99f87a0f929a26868c00b7b2
    resource: repo://src-tauri/src/removable/linux.rs
  - id: openwiki-source-b301e9a596cd5c9eb7699c6a
    resource: repo://src-tauri/src/removable/macos.rs
  - id: openwiki-source-0e4f903b254a62e959949c40
    resource: repo://src-tauri/src/removable/mod.rs
  - id: openwiki-source-ec68524fa9b39e10e5a570df
    resource: repo://src-tauri/src/removable/windows.rs
  - id: openwiki-source-766f3c8f72ae209e5fe092b7
    resource: repo://src-tauri/src/watcher/callback.rs
  - id: openwiki-source-d4a2613b04dbbd067d2632bc
    resource: repo://src-tauri/src/watcher/state.rs
  - id: openwiki-source-3f657ce02d82ba0de09609af
    resource: repo://src/lib/controllers/removable-volume-controller.svelte.ts
  - id: openwiki-source-9d08ef4075c63d08906a8b88
    resource: repo://src/lib/controllers/sync-controller.svelte.ts
generated: { by: "openwiki/0.7.2", at: "2026-10-15T14:38:41.442Z" }
---

# Watched Folders and Removable Volumes

A **watched folder** is a persisted local root, optionally associated with an Immich target album. Persistence is deliberately independent of filesystem availability: an offline root remains configured and is retried later. The folder list is the durable source of truth; `WatcherState` is only the current recursive `notify` registration.

## User entrypoints and registration

The dashboard's add button opens the native directory picker. After the user selects an album or no album, the frontend invokes the Tauri `add_folder` command and refreshes the displayed list. An empty or whitespace-only album ID is stored as no target album. `FolderList.svelte` renders the configured path, final path component, optional resolved album name, and a remove control; it does not expose watcher health or removable bindings.

`register_watched_folder` follows a deliberately ordered registration path:

```mermaid
flowchart TD
    Pick["Directory selected"] --> Add["add_folder"]
    Add --> Normalize["Normalize and check root conflicts"]
    Normalize --> Conflict{"Duplicate or overlap"}
    Conflict -->|yes| Reject["Return error"]
    Conflict -->|no| Available{"Directory exists"}
    Available -->|yes| Watch["Register recursive watcher"]
    Available -->|no| Deferred["Mark watch deferred"]
    Watch --> Persist["Insert watched folder"]
    Deferred --> Persist
    Persist --> Bound{"Removable binding supplied"}
    Bound -->|yes| Bind["Persist volume ID and relative path"]
    Bound -->|no| Enable["Enable sync controls"]
    Bind --> Enable
    Enable --> Scan{"Watcher registered now"}
    Scan -->|yes| Initial["Spawn scan and authenticated sync"]
    Scan -->|no| Wait["Deferred reconciler will scan on registration"]
```

This is the registration and deferred-availability lifecycle. A failure to persist after registering a watcher attempts to unwatch it; a failure to persist an optional removable binding removes the just-created folder and un-registers its watcher. A path that exists but is not a directory, or that cannot be accessed for a reason other than not found, is rejected. A not-found path is instead accepted, persisted, and logged as deferred.

### Path and overlap invariants

Normalization makes relative roots absolute, lexically removes `.` and `..`, and canonicalizes paths when possible. This retains a usable absolute lexical path for an unavailable folder while resolving aliases for an available one. Windows verbatim prefixes are removed; component comparison is case-insensitive on Windows. Before persistence, a candidate cannot be equal to, below, or above any configured root. The component-based test means a similarly prefixed sibling such as `Photos-Vacation` does not conflict with `Photos`.

The scanner repeats overlap protection for multi-root scans: it normalizes and sorts roots by component depth, retains the first parent, reports suppressed child roots, and deduplicates discovered file paths. This protects startup and recovery scans even if older database contents predate the command-side guard.

## Scanning and watcher reconciliation

Folder scans run on a blocking worker through `WalkDir`. They recurse below the root, accept only supported media extensions, and collect path, size, and modification time into assets. Hidden names and the NAS metadata directories `@eaDir`, `@SynoResource`, and `#recycle` are excluded; Windows hidden attributes are also excluded. Per-entry traversal and metadata errors become file failures rather than aborting all discovery. The scan result is passed into the synchronization workflow only when authentication permits it.

At startup, Lymic attempts to register every persisted root. An unavailable one is retained and reported rather than removed. A background task then polls all configured roots every 30 seconds. For each root, reconciliation stops an existing watch when it is unavailable, registers an available but unregistered root recursively, and leaves an already-registered root alone. Newly registered roots are logged, scanned, and sent through authenticated synchronization.

The filesystem callback forwards only create and modify events to a bounded channel. The consumer coalesces events for two seconds and uses path scanning to build a deduplicated asset/failure batch. Remove and access events are ignored. An ambiguous event, a `notify` backend error, or channel saturation marks a rescan as required; after the debounce period, the consumer clears its incremental buffers and scans all persisted roots instead. This favors a complete reconciliation over trusting a lossy event stream.

## Removing a root and cancelling work

Removal is a two-step UI interaction when the root has active coordinator work or stored sync work. The first `remove_folder` call returns `requiresSyncCancellation`; the frontend shows a warning and repeats the command with `confirmCancellation: true` only after confirmation. Without work, removal proceeds immediately.

For a known root, the backend un-registers the watcher **before** cancelling in-memory work and deleting path-scoped queued and unsynchronized state, so the watcher cannot create fresh work while cleanup proceeds. The database cleanup uses path components, preserving sibling roots. If cleanup fails, the command tries to reconcile the watcher back to the root's current availability. If deleting the persisted folder fails, it likewise tries to restore watching and logs an error if restoration fails. Once removal completes, the tray sync affordance is enabled only if at least one persisted folder remains.

## Removable volumes: implemented boundary and current gap

The schema has a one-to-one `removable_watched_folders` binding keyed by watched-folder ID. It stores a stable `volume_id` and a volume-relative path and cascades away when the watched folder is deleted. The reusable registration function can create this binding after persisting a folder and compensates by removing the folder if binding persistence fails, but the current public `add_folder` command and dashboard add flow never provide a binding. Consequently, the stored binding is an implementation foundation rather than a user-reachable “watch this card” feature.

On Windows, a dedicated device-message thread reacts to removable-drive arrival and removal notifications. It ignores fixed drives; for a removable drive it derives a volume GUID, mount path, and display name, then sends a `VolumeEvent`. At application startup the monitor feeds a volume-event consumer:

```mermaid
sequenceDiagram
    participant Monitor as Windows device monitor
    participant Consumer as Background consumer
    participant Store as SQLite bindings
    participant Backend as Tauri backend
    participant UI as Volume controller
    Monitor->>Consumer: arrival with volume metadata
    Consumer->>Store: find bindings by volume ID
    alt no binding
        Consumer->>Backend: emit removable-volume-detected
        Backend->>UI: event payload
        UI->>UI: queue one prompt and optional notification
    else binding exists
        Consumer->>Consumer: suppress discovery prompt
    end
    Monitor->>Consumer: removal
    Consumer->>Consumer: no operational action yet
```

This shows the currently implemented detection behavior, not automatic remapping. A known volume suppresses the discovery event, but arrival does not reconstruct its absolute root, re-register its watcher, or scan it. Removal is explicitly a no-op pending watcher unregistration and queue-state handling. macOS and Linux currently select default monitors whose `start` method does nothing, so this runtime detection path is Windows-specific.

The frontend listens for `removable-volume-detected`, suppresses duplicates and volume IDs dismissed during its lifetime, queues remaining prompts, and sends a desktop notification only while the app is in the background and notification permission is already granted. It unsubscribes when destroyed.

## Change and test guide

Keep command-side root validation and scanner-side overlap handling aligned: the former prevents new conflicting configuration while the latter makes bulk scans safe for existing data. Changes to watcher event classification must retain the overflow-to-full-rescan fallback. Changes to removal ordering must keep unwatching ahead of cancellation/cleanup and preserve best-effort watcher restoration on failure.

Focused Rust tests cover duplicate, parent, child, and prefix-sibling conflict cases; watcher registration/deferred/re-registration behavior; hidden and NAS metadata exclusion; scan overlap deduplication; watcher queue overflow; and unknown-versus-known removable-volume arrivals. Frontend controller tests cover the `add_folder` payload and the remove-confirmation retry. For synchronization of scanned results and upload semantics, see [Synchronization Workflow](synchronization.md). Related persistence and IPC details are in [Local Persistence and Recovery](../data/local-state.md) and [Frontend State and Tauri IPC](../frontend/ui-and-ipc.md).
