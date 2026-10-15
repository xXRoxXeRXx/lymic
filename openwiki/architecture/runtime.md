---
type: architecture
title: Desktop Runtime Architecture
description: How Lymic composes its Svelte user interface, Tauri command surface, Rust services, and desktop lifecycle into one local media-upload application.
tags: [architecture, tauri, svelte, runtime]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-4e434927bce2240ac81949ac
    resource: repo://src-tauri/src/app/mod.rs
  - id: openwiki-source-d86d6ce7cb2c85ce2672a18b
    resource: repo://src-tauri/src/sync/service.rs
  - id: openwiki-source-31a0716bfc7f6379a14d2d70
    resource: repo://src/routes/%2Bpage.svelte
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Desktop Runtime Architecture

Lymic is a Tauri desktop application with a Svelte 5 frontend and a Rust backend. The browser-facing code owns presentation and interaction state; the Rust side owns desktop integration, local persistence, filesystem access, credentials, and calls to Immich. They meet at Tauri commands and application events rather than through a shared in-process model.

## Composition root and startup

`src-tauri/src/main.rs` starts the library application, while `app::run` is the composition root. It installs the desktop plugins, initializes the application state, starts background work, sets up the tray, registers every callable command, and routes window close events to the tray behavior. The command registry is therefore the authoritative backend IPC surface.

Startup initialization occurs before the background workers are spawned. It establishes the SQLite-backed state and recovery context; the background layer can then reconcile watched folders and resume durable synchronization work. A launch carrying `--autostart` starts hidden only when usable credentials are already stored. Ordinary launches show the main window after setup has returned control to the native event loop.

The app is deliberately tray-oriented: closing the main window hides it instead of exiting it. Exit is the point at which the audit logger is flushed.

## Frontend composition

`src/routes/+page.svelte` creates five long-lived controllers: authentication, synchronization, preferences, updater, and feedback. On mount it starts their lifecycles, hydrates backend state, and installs synchronization event subscriptions. The route chooses among login, dashboard, and settings views based on controller state; components receive state and callbacks rather than invoking backend commands directly.

This keeps UI-specific state such as dialogs, progress display, and error presentation in TypeScript while the backend remains the source of truth for credentials, folders, and durable sync status. Controller cleanup unsubscribes the frontend when the route unmounts.

## Backend service boundaries

The registered commands delegate to focused Rust areas:

- `commands/` adapts IPC inputs and responses for authentication, albums, folders, synchronization, and settings.
- `sync/` coordinates scanning, queue processing, pause/resume, progress, and completion.
- `watcher/` and `media/` discover local media and filesystem changes.
- `immich/` implements authenticated REST interactions and streamed upload behavior.
- `db/` owns SQLite initialization, migrations, folders, settings, synchronization state, and queue access.
- `auth` keeps secrets in the native keyring, while `audit` records structured local operational events.

The background service can accept scans while another run owns the synchronization lock: it writes discovered work to the durable queue first and only starts a runner when the coordinator is not paused and valid credentials can produce an Immich client. That ordering makes the database queue the handoff between filesystem activity and uploads.

## Cross-boundary contracts

Frontend code invokes named commands and subscribes to backend events. The backend registers command names in `app/mod.rs`; frontend controllers and the shared subscription helper consume those names and event payloads. Changes are cross-cutting: a command or event contract change normally requires coordinated backend, controller, component, and test changes.

Synchronization uses both request/response calls (for starting, pausing, resuming, reading status, and retrying failures) and events (for progress, snapshots, transfer statistics, lifecycle, logs, and errors). This split lets the UI hydrate a current snapshot after reload while receiving live updates during a transfer.

## Navigation

- [Frontend State and Tauri IPC](../frontend/ui-and-ipc.md) follows the UI-facing side of these contracts.
- [Synchronization Workflow](../workflows/synchronization.md) explains the queue and upload lifecycle.
- [Local Persistence and Recovery](../data/local-state.md) covers startup state and durable recovery.
- [Authentication and Credential Security](../security/authentication.md) covers the credential boundary used by startup and sync.
