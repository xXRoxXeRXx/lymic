---
type: contributor quickstart
title: Lymic Codebase Guide
description: Entry point for safely changing Lymic, an upload-oriented Tauri and Svelte desktop client for Immich. It routes common work to the architecture, workflow, security, data, integration, and operations guidance that owns it.
tags: [quickstart, contributors, development, architecture, tauri]
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-4e434927bce2240ac81949ac
    resource: repo://src-tauri/src/app/mod.rs
  - id: openwiki-source-31a0716bfc7f6379a14d2d70
    resource: repo://src/routes/%2Bpage.svelte
generated: { by: "openwiki/0.7.2", at: "2026-11-09T18:02:27.157Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-11-09T18:02:27.157Z
---

# Lymic Codebase Guide

Lymic is a cross-platform desktop client that discovers media in local **Watched Folders**, avoids transferring assets already known to Immich, and streams missing media from disk. It is deliberately **upload-oriented**, not a two-way filesystem mirror: normal synchronization does not download remote assets, apply remote deletions or renames to local files, or manage albums. Treat a change to that boundary as a product decision, not an incidental synchronization enhancement.

## Start here

Install current stable Rust, Node.js 18 or later, and Tauri's platform prerequisites. From the repository root:

```bash
npm ci
npm run tauri dev
```

`npm run dev` starts only Vite/SvelteKit. Use `npm run tauri dev` for a desktop smoke test and whenever the change touches Rust, IPC, the filesystem, credentials, native plugins, or desktop configuration. The Rust application is in `src-tauri/`; the webview is in `src/`.

Two composition boundaries provide the best initial trace:

- `src-tauri/src/app/mod.rs` is the runtime composition root. It registers custom media protocols, Tauri plugins and commands; initializes startup state; launches background work and the tray; hides the main window on close; and records event-loop readiness and shutdown audit events. Autostart starts hidden only when stored credentials exist.
- `src/routes/+page.svelte` is the webview composition entrypoint. It creates domain controllers and renders login when unauthenticated; authenticated navigation includes photos, albums, people, dashboard, activity log, settings, and information views.

The webview invokes named Tauri commands and subscribes to native events. A command or event name and its serialized payload are a cross-language contract: trace the Svelte controller, `src/lib/contracts.ts`, `src/lib/tauri/desktop-adapter.ts`, Rust command, event producer, and focused tests together.

On mount, the root starts sync, watched-folder, update, and preference lifecycles plus removable-volume subscriptions. Authentication, folders, sync, and preferences hydrate independently; sync event subscriptions begin after sync hydration. On unmount, it destroys the sync, watched-folder, update, and removable-volume controllers and stops the preferences lifecycle.

## Route the task

| Change or question | Read first | Then inspect |
| --- | --- | --- |
| Startup/recovery, managed services, tray/window behavior, protocol or command registration, native events | [Desktop Runtime Architecture](architecture/runtime.md) | `src-tauri/src/app/mod.rs` and the owning app service |
| Component behavior, controller state, UI navigation, command caller, event subscriber, or localization | [Frontend UI and IPC Boundary](frontend/ui-and-ipc.md) | `src/routes/+page.svelte`, owning controller, `src/lib/tauri/`, and Rust boundary |
| SQLite data, migrations, queue recovery, cached checksums, folders, settings, or repair | [Local State and Recovery](data/local-state.md) | `src-tauri/migrations/`, `src-tauri/src/db/`, and recovery callers |
| Login, server URL policy, API key or Cloudflare credentials, keyring storage, logout, profile cache, or redaction | [Authentication and Credential Safety](security/authentication.md) | Auth controller and commands, `src-tauri/src/auth.rs`, audit and error paths |
| Immich requests, duplicate checks, streaming uploads, checksums, albums, API compatibility, thumbnails, or transfer failures | [Immich API Integration](integrations/immich-api.md) | `src-tauri/src/immich/`, credential policy, and synchronization completion path |
| Manual, watcher, retry, or recovered synchronization; queueing, pause/resume, upload, duplicate, album assignment, or progress | [Synchronization](workflows/synchronization.md) | `src-tauri/src/sync/`, command, persistence, and sync controller |
| Folder registration, path overlap, discovery, sidecars, watcher debounce/overflow, unavailable folders, or removable volumes | [Watched Folders](workflows/watched-folders.md) | `watched_folders/`, `watcher/`, `media/`, `removable/`, persistence, and folder UI |
| Structured local audit records, observability, or audit retention | [Audit Logging](operations/audit-logging.md) | `src-tauri/src/audit/` and the operation that emits the event |
| Toolchains, focused/shared checks, test doubles, capabilities/configuration, CI, or desktop smoke tests | [Development and Testing](operations/development-and-testing.md) | The owning behavioral page and relevant configuration |
| Versions, updater/distribution channels, signed artifacts, Store, Snap, Homebrew, AUR, or release records | [Release and Distribution](operations/release-distribution.md) | Release workflow and packaging configuration |

For project setup and vocabulary, see the repository [README](../README.md), [CONTRIBUTING.md](../CONTRIBUTING.md), and [GLOSSARY.md](../GLOSSARY.md). Read the applicable ADRs in [`docs/adr/`](../docs/adr/) before changing a resolved product or architecture decision.

## Preserve these boundaries

- The durable synchronization queue is the recovery boundary: recovery resumes persisted work rather than requiring a new folder scan. Persisted remote identity prevents a retry of album assignment from uploading the asset again.
- Keep hashing and multipart transfer streaming; do not introduce full-file buffering. Watched Folder paths are normalized, and duplicates or parent/child overlaps are rejected. An unavailable folder remains configured for reconciliation rather than being silently removed.
- Treat IPC as a coordinated interface. Keep Tauri command and event contracts synchronized in Svelte and Rust; add schema changes as new forward-only migrations rather than editing an applied migration.
- Credentials belong in the native keyring, never SQLite, logs, errors, or UI events. Server URLs are HTTPS by default; local HTTP requires explicit loopback-only opt-in, and credentials in URLs, redirects, query strings, and fragments are rejected.

## Verify at the affected boundary

Run the narrowest relevant tests while iterating. Before handoff, run the applicable shared checks from the repository root:

```bash
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

These checks do not replace a desktop smoke test. Run `npm run tauri dev` for IPC, startup, native permissions, dialogs, watchers, credentials, updater, or Tauri-configuration work. Use disposable folders and non-production credentials for synchronization scenarios; verify both the intended result and the relevant failure or recovery path, especially for migrations, pause/retry, duplicates, target-album failures, watcher overflow, and removable-volume reconciliation.
