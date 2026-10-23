---
type: contributor guide
title: Lymic Codebase Guide
description: Contributor routing guide from Lymic's Tauri and Svelte entry points to the runtime, UI/IPC boundary, synchronization, folders, persistence, Immich, security, diagnostics, and development guidance.
tags: [guide, architecture, contributors]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-23T12:59:10.542Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-31a0716bfc7f6379a14d2d70
    resource: repo://src/routes/%2Bpage.svelte
generated: { by: "openwiki/0.7.2", at: "2026-10-19T15:10:01.321Z" }
---

# Lymic Codebase Guide

Lymic is a cross-platform, upload-oriented desktop client: it watches selected local folders, avoids uploading assets already present in Immich, and streams missing media from disk. It is **not** a two-way filesystem mirror: normal synchronization neither downloads remote assets nor applies remote deletions or renames locally. Preserve that product boundary unless an explicit product decision changes it.

## Start at the composition boundaries

There are two application entry points to trace before changing behavior:

- `src-tauri/src/app/mod.rs` is the Rust/Tauri composition root. It registers commands, starts background work, creates desktop integration, and owns the host-side lifecycle.
- `src/routes/+page.svelte` composes the Svelte application. It creates the concern-specific controllers and renders the login view when unauthenticated or the authenticated dashboard, activity log, settings, and information views.

The UI reaches the Rust host through named Tauri commands and receives native events through subscriptions. Treat a command name, serialized response, event name, or event payload as an IPC contract: trace both the owning Svelte controller and the Rust implementation or producer when changing either side.

On mount, the root starts the sync, update, and preferences lifecycles and removable-volume subscriptions. It then hydrates authentication, sync, and preferences independently; sync event subscriptions start only after sync hydration succeeds. Its cleanup destroys sync, update, and removable-volume controllers and stops the preferences lifecycle.

For product setup, start with the repository [README](../README.md). Use the terminology in [GLOSSARY.md](../GLOSSARY.md), especially *Watched Folder*, *Target Album*, *Upload Unit*, and *Album Assignment*.

## Task-routing map

Use this map to choose the owning documentation before editing. Each linked page explains the relevant state, contracts, failure behavior, and focused tests.

| Change or question | Read first | Trace with it |
| --- | --- | --- |
| Tauri startup/recovery, managed services, background tasks, tray, window visibility, native events, or command registration | [Desktop Runtime Architecture](architecture/runtime.md) | `src-tauri/src/app/mod.rs` and the owning app service |
| A view, controller, frontend state, Tauri command caller, native event subscriber, or localized UI behavior | [Frontend Controllers and IPC Boundary](frontend/ui-and-ipc.md) | `src/routes/+page.svelte`, the owning controller, component, and Rust command/event |
| Manual, recovered, or retry synchronization; scanning, checksums, duplicate checks, queueing, transfer progress, pause/resume, uploads, or completion | [Synchronization Pipeline](workflows/synchronization.md) | Sync controller, `src-tauri/src/sync/`, command layer, and queue persistence |
| Adding or validating folders; recursive discovery, media pairs or sidecars, watcher debounce/overflow, unavailable paths, or removable volumes | [Watched Folders and Media Discovery](workflows/watched-folders.md) | Folder UI and commands, `src-tauri/src/media/`, `src-tauri/src/watcher/`, and persistence |
| SQLite schema, migrations, folders, cached checksums, queue/job recovery state, settings, cache locations, or retention | [Local State and Recovery](data/local-state.md) | `src-tauri/migrations/`, `src-tauri/src/db/`, and the recovery path |
| Immich authentication requests, profile retrieval, bulk existence checks, multipart uploads, checksums, albums, Cloudflare headers, or API failures | [Immich API Integration](integrations/immich-api.md) | `src-tauri/src/immich/`, affected sync completion, and credential policy |
| Login validation, server URL/loopback HTTP policy, API-key or Cloudflare credentials, keyring storage, logout, profile cache, or secret handling | [Authentication and Credential Safety](security/authentication.md) | Auth controller and command, `src-tauri/src/auth.rs`, audit/error paths |
| Audit event structure, log redaction, asynchronous writer behavior, rotation, diagnostic UI, or logging failure | [Audit Logging and Operational Diagnostics](operations/audit-logging.md) | `src-tauri/src/audit/`, error reporting, and the affected service |
| Toolchain setup, checks, unit tests, smoke tests, capability/configuration changes, builds, or release/update behavior | [Development, Testing, and Release Operations](operations/development-and-testing.md) | The system page for the behavior changed |

## Change-safety checkpoints

- Keep synchronization upload-only. The durable queue is the recovery boundary: recovered work resumes from queue state rather than forcing a folder rescan.
- Retain transfer and album safety. Hashing and uploads stream files rather than buffering whole files. A Target Album is captured on the queue entry; on assignment failure do not silently fall back to the root library, and retain the remote asset identity so a retry does not upload it again.
- Retain folder lifecycle rules. Normalize paths, reject duplicate or parent/child-overlapping Watched Folders, and leave an unavailable folder configured for watcher reconciliation rather than removing it.
- Keep credentials in the native keyring and out of SQLite, logs, errors, and UI events. Follow the HTTPS-by-default and loopback-only opt-in policy when touching login or HTTP clients.
- Keep frontend and backend IPC in lockstep. Add persistence changes as new forward-only migrations; do not edit an applied migration.

## Verify at the affected boundary

Run the focused tests named by the owning page while iterating, then use the applicable repository checks:

```powershell
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

`npm run tauri dev` starts an end-to-end desktop smoke-test session; `npm run tauri build` produces a production build. For high-risk work, test observable behavior across the boundary—not merely compilation—including migration/recovery, pause and retry, duplicate detection, credential handling, Target Album failures, and watcher overflow or removable-volume reconciliation.
