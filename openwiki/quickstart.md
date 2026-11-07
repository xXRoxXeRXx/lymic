---
type: contributor quickstart
title: Lymic Codebase Quickstart
description: Entry point for safely changing Lymic, an upload-only Tauri and Svelte desktop client for Immich. It identifies the composition boundaries, local development commands, verification gates, and the documentation that owns common changes.
tags: [quickstart, contributors, development, architecture, tauri]
verified:
  - by: openwiki/0.7.2
    at: 2026-11-06T15:30:15.009Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-31a0716bfc7f6379a14d2d70
    resource: repo://src/routes/%2Bpage.svelte
generated: { by: "openwiki/0.7.2", at: "2026-11-06T15:30:15.009Z" }
---

# Lymic Codebase Quickstart

Lymic is a cross-platform desktop client that discovers media in local **Watched Folders**, avoids transferring assets already known to one Immich server, and streams missing media from disk. It is deliberately **upload-only**: normal synchronization does not download remote assets, apply remote deletions or renames to local files, or manage albums. Treat a change to that boundary as a product decision, not an incidental synchronization enhancement.

## Set up and run

Install current stable Rust, Node.js 18 or later, and the platform prerequisites required by Tauri. From the repository root, install the lockfile-resolved frontend dependencies and start the desktop application:

```bash
npm ci
npm run tauri dev
```

`npm run dev` starts only the Vite/SvelteKit frontend; use `npm run tauri dev` when the change involves the native host, IPC, filesystem, credentials, or another desktop integration. The Rust crate is rooted at `src-tauri/Cargo.toml`, while the webview source is rooted at `src/`.

## Orient before editing

There are two composition boundaries worth tracing first:

- `src-tauri/src/app/mod.rs` constructs the Tauri host: it initializes startup state, starts background work and tray integration, registers commands, and handles window and process lifecycle.
- `src/routes/+page.svelte` composes the webview. It creates the concern-specific controllers, renders login when unauthenticated and the authenticated dashboard, activity log, settings, and information views otherwise.

The UI reaches Rust through named Tauri commands and consumes native events through subscriptions. A command name, serialized request or response, event name, and event payload are a cross-language contract. Trace the Svelte controller, frontend adapter/contracts, Rust command, event producer, and focused tests together when changing one side.

At mount, the root starts the sync, watched-folder, update, and preferences lifecycles plus removable-volume subscriptions. It hydrates authentication, folders, sync, and preferences independently; sync subscriptions begin only after sync hydration. Cleanup destroys the sync, watched-folder, update, and removable-volume controllers and stops the preferences lifecycle.

## Route the task to its owner

Use the following map before implementation. The destination pages contain the detailed lifecycle, failure, state, and test guidance; this page intentionally does not duplicate them.

| Change or question | Owning page | Inspect with it |
| --- | --- | --- |
| Startup and recovery, managed services, background work, tray/window behavior, command registration, or native events | [Runtime Architecture](architecture/runtime.md) | `src-tauri/src/app/mod.rs` and the owning app service |
| A component, controller, frontend state, command caller, event subscriber, localization, or UI lifecycle | [Frontend UI and IPC](frontend/ui-and-ipc.md) | `src/routes/+page.svelte`, the controller, `src/lib/tauri/`, and the Rust boundary |
| SQLite data, migrations, queue/job recovery, cached hashes, folders, settings, or local-data repair | [Local State and Recovery](data/local-state.md) | `src-tauri/migrations/`, `src-tauri/src/db/`, and recovery callers |
| Login, server URL policy, API key or Cloudflare credentials, keyring storage, logout, profile cache, or redaction | [Authentication and Credential Boundaries](security/authentication.md) | Auth controller and commands, `src-tauri/src/auth.rs`, and error/audit paths |
| Immich requests, bulk duplicate checks, streaming uploads, checksums, albums, API compatibility, or transfer failures | [Immich API Integration](integrations/immich-api.md) | `src-tauri/src/immich/`, the affected synchronization completion path, and credential policy |
| Manual, watcher, retry, or recovery sync; admission, queueing, pause/resume, upload, duplicate, album, and progress behavior | [Synchronization Pipeline](workflows/synchronization.md) | `src-tauri/src/sync/`, commands, persistence, and the sync controller |
| Folder registration, path overlap, discovery, sidecars, watcher debounce/overflow, unavailable folders, or removable volumes | [Watched Folder and Removable Media Lifecycle](workflows/watched-folders.md) | `watched_folders/`, `watcher/`, `media/`, `removable/`, persistence, and folder UI |
| Toolchains, focused and shared checks, smoke testing, configuration/capabilities, or CI | [Development and Testing](operations/development-and-testing.md) | The owning behavioral page and `vite.config.js`, Tauri configuration, or capabilities when relevant |
| Versions, updater/distribution channels, signed artifacts, Store, Snap, or release records | [Release and Distribution](operations/release-distribution.md) | Release workflow and packaging configuration |

For project setup and vocabulary, see the repository [README](../README.md), [CONTRIBUTING.md](../CONTRIBUTING.md), and [GLOSSARY.md](../GLOSSARY.md). Read applicable ADRs in [`docs/adr/`](../docs/adr/) when the task changes a resolved product or architecture decision.

## Preserve these cross-cutting boundaries

- Keep synchronization upload-only. Its durable queue is the recovery boundary: recovery resumes persisted work rather than requiring a new folder scan.
- Keep file hashing and multipart transfer streaming; do not introduce full-file buffering. Preserve the saved remote identity so an album-assignment retry does not upload an asset again.
- Treat Tauri IPC as a coordinated interface. Keep command and event contracts synchronized in Svelte and Rust, and add database changes as new forward-only migrations rather than modifying an applied migration.
- Do not move credentials into SQLite or expose them through logs, errors, or UI events. Follow the HTTPS-default and explicit loopback-only HTTP policy.

## Verify at the affected boundary

Run the narrowest relevant tests while iterating. Before handoff, run the applicable shared checks from the repository root:

```bash
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

These checks do not replace a desktop smoke test. Run `npm run tauri dev` for IPC, startup, native permissions, dialogs, watchers, credentials, updater, or Tauri-configuration work. Use disposable folders and non-production credentials for sync scenarios, and verify both the intended result and the relevant failure or recovery path—for example migration recovery, pause/retry, duplicate handling, target-album failure, watcher overflow, or removable-volume reconciliation.
