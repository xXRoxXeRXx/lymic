---
type: guide
title: Lymic Architecture Guide
description: Contributor entry point for routing Lymic changes to the owning runtime, frontend, synchronization, persistence, integration, security, and operations documentation. It defines the upload-only product boundary and the Rust/Svelte entrypoints to inspect first.
tags: [guide, architecture, contributors]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-16T12:54:13.301Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-31a0716bfc7f6379a14d2d70
    resource: repo://src/routes/%2Bpage.svelte
generated: { by: "openwiki/0.7.2", at: "2026-10-16T12:54:13.301Z" }
---

# Lymic Architecture Guide

Lymic is an upload-oriented desktop client that brings media from local **Watched Folders** into an Immich library. It is not a two-way filesystem mirror: it does not download remote assets or apply remote deletions or renames to local files. Preserve that boundary unless a product decision explicitly changes it.

## Orient yourself

The desktop application has two composition entrypoints:

- `src-tauri/src/app/mod.rs` is the Rust/Tauri composition root. It initializes host services, starts background work, sets up desktop integration, and registers the commands exposed to the webview.
- `src/routes/+page.svelte` is the Svelte UI composition entrypoint. It creates the UI controllers, hydrates their durable state and subscriptions on mount, and chooses the login or authenticated views.

The layers meet only through named Tauri commands and serialized events. A frontend change that invokes a command or consumes an event is therefore also an IPC-contract change; trace the controller and the corresponding Rust command or event producer rather than changing one side alone.

For product setup, see the repository [README](../README.md). Use the vocabulary in [GLOSSARY.md](../GLOSSARY.md), particularly **Watched Folder**, **Target Album**, **Upload Unit**, and **Album Assignment**.

## Route the change

| If you need to change… | Read first | Also inspect |
| --- | --- | --- |
| Startup, managed host services, background consumers, tray, window visibility, or command registration | [Desktop Runtime and Lifecycle](architecture/runtime.md) | `src-tauri/src/app/mod.rs` |
| A screen, controller state, command caller, or event subscription | [Frontend State and Tauri IPC](frontend/ui-and-ipc.md) | `src/routes/+page.svelte` and the owning controller |
| Discovery, checksums, queueing, pause/resume, retries, progress, or upload outcomes | [Synchronization Workflow](workflows/synchronization.md) | The sync controller and matching Rust sync command |
| Folder validation, filesystem watchers, removable media, or pairing | [Watched Folders and Removable Volumes](workflows/watched-folders.md) | Folder UI and command boundary |
| SQLite schema, migrations, durable queue/recovery state, or database repair | [Local Persistent State](data/local-state.md) | Migration plus its data accessor and recovery path |
| Immich requests, duplicate checks, streaming uploads, or target-album calls | [Immich API Integration](integrations/immich-api.md) | The affected synchronization completion path |
| Login, server URL policy, keyring credentials, or Cloudflare Access | [Authentication and Credential Security](security/authentication.md) | Authentication command and error/audit handling |
| Local setup, checks, focused tests, smoke tests, or diagnostics | [Development, Testing, and Operations](operations/development-and-testing.md) | The system page for the changed behavior |

## Cross-cutting guardrails

- Keep synchronization upload-only. The durable queue is the recovery boundary; do not turn a retry or recovery change into an implicit full rescan.
- Preserve local-data and transfer safety: credentials belong in the native keyring, not SQLite, logs, errors, or UI events; hashing and uploads must stream rather than buffer full files.
- Treat a Target Album as part of the queued outcome. Do not silently fall back to the root library when album assignment fails, and retain the remote asset identity so an assignment retry does not upload it again.
- Preserve folder lifecycle semantics: normalize paths and reject duplicate or overlapping Watched Folders; keep an unavailable folder configured for later reconciliation rather than removing it.
- Keep Tauri command names and event payloads synchronized with their Svelte callers. Add schema changes as new forward-only migrations; do not edit an applied migration.

## Verify at the right boundary

Use the focused tests described by the owning page while iterating. Before handoff, run the applicable frontend and Rust checks from the [operations guide](operations/development-and-testing.md). The package scripts provide `npm run check`, `npm test`, and the Tauri CLI through `npm run tauri`; start an end-to-end desktop smoke test with `npm run tauri dev`.

For high-risk changes, verify behavior—not only compilation—across the boundary involved: migration/recovery, pause and retry, duplicate detection, credential handling, target-album failures, or watcher overflow/remount behavior.
