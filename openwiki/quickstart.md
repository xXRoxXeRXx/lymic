---
type: guide
title: Lymic Architecture Guide
description: A contributor-oriented map to Lymic's desktop runtime, synchronization systems, persistence, integrations, and verification workflow.
tags: [guide, architecture, contributors]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Lymic Architecture Guide

Lymic is a cross-platform Tauri desktop client that brings media from local Watched Folders into one Immich library. It is upload-oriented: it does not download, mirror, rename, or apply remote deletions to local files.

## Start here

For user setup and product behavior, read the repository [README](../README.md). For implementation work, start from the Tauri composition root at `src-tauri/src/app/mod.rs` and the Svelte route at `src/routes/+page.svelte`. The project is split between a Svelte/TypeScript frontend and a Rust backend, connected by Tauri commands and events.

## Find the right system

| Task | Read first |
| --- | --- |
| Understand app startup, services, tray, or command registration | [Desktop Runtime Architecture](architecture/runtime.md) |
| Change a dashboard interaction, controller, command, or event | [Frontend State and Tauri IPC](frontend/ui-and-ipc.md) |
| Change scanning, pause/resume, retry, upload outcomes, or progress | [Synchronization Workflow](workflows/synchronization.md) |
| Change folder configuration, watcher behavior, or media pairing | [Watched Folders and Media Discovery](workflows/watched-folders.md) |
| Change schema, queue persistence, startup recovery, or repair | [Local Persistence and Recovery](data/local-state.md) |
| Change Immich calls, duplicates, uploads, or Target Albums | [Immich Integration](integrations/immich-api.md) |
| Change login, URL policy, secrets, or Cloudflare Access | [Authentication and Credential Security](security/authentication.md) |
| Run checks or investigate runtime records | [Development, Testing, and Operations](operations/development-and-testing.md) |

## Core guardrails

Use the canonical terms in `GLOSSARY.md`: Watched Folder, Target Album, Upload Unit, and Album Assignment. Keep the queue as the synchronization recovery boundary; keep credentials in the keyring; do not add full-file buffering to hashing or uploads; and keep command/event names synchronized with their Svelte callers.

For a local development run, install JavaScript dependencies and run `npm run tauri dev`. Before handoff, use the frontend checks and tests plus Rust formatting, clippy, and locked tests described in the operations guide.
