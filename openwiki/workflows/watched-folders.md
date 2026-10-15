---
type: workflow
title: Watched Folders and Media Discovery
description: How Lymic configures local source folders, observes filesystem changes, and turns supported files into upload units.
tags: [folders, watcher, media, discovery]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-df6e066f6f018afe47017686
    resource: repo://docs/adr/0002-removable-watched-folders.md
  - id: openwiki-source-743fce1e1a7530f5eafe9a04
    resource: repo://src-tauri/src/commands/folders.rs
  - id: openwiki-source-fc674ee913e1089a29866209
    resource: repo://src-tauri/src/media/model.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Watched Folders and Media Discovery

A **Watched Folder** is a configured local directory that Lymic scans and observes for supported media. It can have one optional Target Album. Folder configuration is persistent even when the path is currently unavailable.

## Adding and removing folders

Adding a folder normalizes its path before persistence and rejects duplicates as well as parent/child overlaps. Existing available paths are registered with the watcher and scanned asynchronously; an unavailable path is persisted with deferred watch registration and a warning. The persisted configuration, not an active watcher registration, is the source of truth.

Removing a folder with active or queued work first requires explicit confirmation. The command removes the watcher before cancelling work and cleaning path-scoped queued/unsynchronized state, which prevents new work from being generated while removal proceeds. If cleanup fails, it attempts to restore watching.

## Watcher and discovery behavior

The watcher observes folder trees and supplies filesystem events to background processing. Debouncing coalesces changes; overflow is handled by requesting a rescan rather than trusting an incomplete event stream. Startup attempts to register all stored folders, logging an unavailable folder rather than deleting it.

The scanner recognizes supported media, records file failures, and builds `Asset` values containing path, size, modification time, optional Target Album, and later remote ID. Assets in the same directory with the same basename become a single **Upload Unit** only when they form an allowed image/video live-photo pair. Ambiguous or unsupported pairings remain independent units. Hash calculation streams the file to produce the SHA-1/Base64 value used by Immich.

## Removable-folder design status

The repository’s glossary and ADR define a **Removable Watched Folder** in terms of a stable volume identifier and volume-relative path, with remapping on reconnect. Current implementation sources and migrations do not yet contain that removable-folder model (there is no referenced `0006_removable_watched_folders.sql` migration), so treat the ADR as a documented architectural decision rather than an implemented runtime capability until source support lands.

## Navigation

- [Synchronization Workflow](synchronization.md) consumes scan results and Upload Units.
- [Local Persistence and Recovery](../data/local-state.md) covers persisted folders and cleanup state.
- [Frontend State and Tauri IPC](../frontend/ui-and-ipc.md) covers add/remove UI actions.
