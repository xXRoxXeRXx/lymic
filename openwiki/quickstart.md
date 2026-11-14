---
type: contributor quickstart
title: Lymic Codebase Quickstart
description: Task-oriented starting point for safely navigating Lymic's Tauri desktop client. Routes common changes to the runtime, UI and IPC, synchronization, local-state, Immich, security, and release guidance.
tags: [quickstart, contributors, development, architecture, tauri]
verified:
  - by: openwiki/0.7.2
    at: 2026-11-13T15:48:39.594Z
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
generated: { by: "openwiki/0.7.2", at: "2026-11-13T15:48:39.594Z" }
---

# Lymic Codebase Quickstart

Lymic is an upload-oriented desktop client: it discovers media in local **Watched Folders**, avoids transferring assets already known to Immich, and streams missing files from disk. It is not a two-way filesystem mirror: normal synchronization does not download remote assets or apply remote deletions or renames locally. Treat a change to that boundary as a product decision.

## Start a desktop development session

Install stable Rust, Node.js 18 or later, and the platform prerequisites required by Tauri. From the repository root:

```bash
npm ci
npm run tauri dev
```

`npm run dev` starts Vite only. Use `npm run tauri dev` for desktop smoke tests and for changes involving Rust, IPC, filesystem access, credentials, native plugins, or desktop configuration. The Rust application is in `src-tauri/`; the webview is in `src/`.

## Orient before changing behavior

Begin at the two composition boundaries:

- [`src-tauri/src/app/mod.rs`](../src-tauri/src/app/mod.rs) is the runtime composition root. It registers the asset and thumbnail protocols, plugins, and Tauri commands; initializes startup state; starts background work; sets up the tray; and queues window display except for an autostart launch with stored credentials. It also records event-loop readiness and flushes the shutdown audit event.
- [`src/routes/+page.svelte`](../src/routes/+page.svelte) is the webview composition entrypoint. It chooses the login interface until authentication succeeds, then owns navigation among photos, map, albums, people, dashboard, activity log, settings, and information. Selecting an album or person creates and refreshes its dedicated timeline controller; returning clears it and restores the originating view and scroll position.

On mount, the Svelte root starts the synchronization, watched-folder, update, and preferences lifecycles and removable-volume subscriptions. Authentication, folders, synchronization, and preferences hydrate independently; synchronization event subscriptions start only after sync hydration. Its unmount cleanup destroys the sync, watched-folder, update, and removable-volume controllers and stops the preferences lifecycle.

A Tauri command/event name and its serialized payload are a cross-language contract. For an IPC change, trace the Svelte caller, [`src/lib/contracts.ts`](../src/lib/contracts.ts), [`src/lib/tauri/desktop-adapter.ts`](../src/lib/tauri/desktop-adapter.ts), the Rust command or event producer, and focused tests together.

## Route the task

| Change or question | Read first | Then inspect |
| --- | --- | --- |
| Startup, recovery, tray/window behavior, native events, protocols, command registration, or background services | [Desktop Runtime Architecture](architecture/runtime.md) | `src-tauri/src/app/mod.rs` and the owning `src-tauri/src/app/` service |
| Component behavior, controller state, navigation, localization, command caller, or event subscriber | [Frontend UI and Desktop IPC](frontend/ui-and-ipc.md) | `src/routes/+page.svelte`, the owning controller, `src/lib/tauri/`, and its Rust boundary |
| SQLite state, migrations, queue recovery, checksums, folders, settings, or database repair | [Local State and Recovery](data/local-state.md) | `src-tauri/migrations/`, `src-tauri/src/db/`, and recovery callers |
| Login, server URL policy, API keys or Cloudflare credentials, keyring storage, logout, or redaction | [Authentication and Credential Boundaries](security/authentication.md) | authentication controller and commands, `src-tauri/src/auth.rs`, audit, and error paths |
| Immich endpoints, bulk duplicate checks, streaming upload, checksums, albums, thumbnails, or API compatibility | [Immich API Integration](integrations/immich-api.md) | `src-tauri/src/immich/`, credential policy, and sync completion |
| Manual, watcher, retry, or recovered sync; queueing, pause/resume, upload, duplicate handling, album assignment, or progress | [Synchronization](workflows/synchronization.md) | `src-tauri/src/sync/`, command, persistence, and sync controller |
| Folder admission, overlap, scanning, sidecars, watcher debounce/overflow, unavailable folders, or removable volumes | [Watched Folders, File Events, and Removable Media](workflows/watched-folders.md) | `watched_folders/`, `watcher/`, `media/`, `removable/`, persistence, and folder UI |
| Toolchains, focused/shared tests, test doubles, capabilities/configuration, CI, or desktop smoke testing | [Development, Verification, and Contract Tests](operations/development-and-testing.md) | the owning behavioral page and relevant configuration |
| Versions, updater/release channels, signing, bundles, Snap, Homebrew Cask, Arch, or release records | [Release and Distribution Pipeline](operations/release-distribution.md) | release workflow and packaging configuration |

For repository setup and vocabulary, consult the [README](../README.md), [CONTRIBUTING.md](../CONTRIBUTING.md), and [GLOSSARY.md](../GLOSSARY.md). Read the applicable ADR in [`docs/adr/`](../docs/adr/) before changing a resolved product or architecture decision.

## Safety guardrails

- The durable synchronization queue is the recovery boundary: resume recovered work from persisted queue state rather than rescanning folders. Persisted remote identity prevents an album-assignment retry from uploading the asset again.
- Keep hashing and multipart transfer streaming; do not introduce whole-file buffering. Watched Folder paths are normalized, and duplicates and parent/child overlaps are rejected. An unavailable folder remains configured for reconciliation rather than being silently removed.
- Keep command and event names/payloads synchronized across Svelte and Rust. Add database schema changes as new forward-only migrations; never edit an applied migration.
- Credentials belong in the native keyring, never SQLite, logs, errors, or UI events. Server URLs are HTTPS by default; local HTTP requires explicit loopback-only opt-in, and credentials in URLs, redirects, query strings, and fragments are rejected.

## Verify the affected boundary

Run the narrowest relevant tests while iterating. Before handoff, run applicable shared checks from the repository root:

```bash
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

These checks do not replace a desktop smoke test. Run `npm run tauri dev` for IPC, startup, native permissions, dialogs, watchers, credentials, updater, or Tauri configuration work. Use disposable folders and non-production credentials for synchronization scenarios; verify both the intended outcome and relevant failure or recovery paths, especially migrations, pause/retry, duplicates, target-album failures, watcher overflow, and removable-volume reconciliation.
