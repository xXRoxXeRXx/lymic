---
type: workflow
title: Synchronization Workflow
description: The end-to-end path from local media discovery to durable queue outcomes and Immich upload or duplicate completion.
tags: [sync, queue, uploads, recovery]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-53e081c93fba07219ed1e23f
    resource: repo://src-tauri/src/commands/sync.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Synchronization Workflow

Synchronization is an upload-oriented workflow: Lymic discovers local media in Watched Folders, identifies existing remote assets, uploads only missing assets, and records the completed outcome locally. It does not mirror remote changes back to the filesystem.

## From scan to durable work

A manual command, startup/recovery activity, folder changes, or watcher events can produce a scan. The media layer forms Upload Units—one asset or an eligible live-photo pair—with optional XMP sidecars. It reuses a cached hash only when the current size and modification time still match. Scan results are converted to queue assets, including a target-album ID resolved from the source folder, and persisted before an automatic runner is scheduled.

The durable queue is the recovery boundary. Queue entries record whether each unit is pending, synchronized, or failed; the job records running, paused, or idle state. A runner always consumes this persisted work, so recovered work can resume without a fresh folder scan.

## Processing and completion

The processor hashes when necessary, bulk-checks checksums with Immich, and treats known remote assets as duplicates rather than uploading them again. Missing units are streamed as multipart uploads. For either branch, a configured Target Album is part of the required outcome: remote IDs are retained, assignment is performed in batches, and an assignment failure leaves failed work instead of silently falling back to the root library.

Progress snapshots, current paths, transfer statistics, log entries, lifecycle events, and errors are emitted for the frontend and tray. Completion updates persisted queue/state records and makes failed entries available for retry.

## Pause, resume, retry, and failure

Pause is persisted immediately and becomes effective at a safe runner boundary. Resume wakes an in-memory runner when one exists; if it does not, the command starts recovery from pending persisted queue items rather than rescanning. A failed-item retry revalidates retained paths, discards entries that no longer apply, and requeues usable work.

Manual starts are protected by an in-memory compare-and-set guard. If setup, authentication, scanning, or processing fails, the command returns the durable status to idle and records an audit failure. Automatic scans can still enqueue work while a run is active or paused.

## Navigation

- [Local Persistence and Recovery](../data/local-state.md) explains the database boundary behind this workflow.
- [Watched Folders and Media Discovery](watched-folders.md) explains how scan inputs are produced.
- [Immich Integration](../integrations/immich-api.md) details duplicate checks, uploads, and albums.
- [Frontend State and Tauri IPC](../frontend/ui-and-ipc.md) covers its UI contract.
