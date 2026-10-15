---
type: persistence
title: Local Persistence and Recovery
description: The SQLite and keyring state that lets Lymic retain folder configuration, synchronization progress, queue outcomes, and recovery context across launches.
tags: [persistence, sqlite, recovery, sync]
sources:
  - id: openwiki-source-941c10286ffa11a6881678ef
    resource: repo://src-tauri/migrations/0004_sync_queue.sql
  - id: openwiki-source-2fd48a041dfbcb39bc092be1
    resource: repo://src-tauri/src/app/startup.rs
  - id: openwiki-source-c33d47a5d8372d5284fcc388
    resource: repo://src-tauri/src/db/init.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Local Persistence and Recovery

Lymic uses an application-data SQLite database for durable operational state and the platform keyring for credentials. The database is the recovery boundary for synchronization: restarting does not require rescanning to reconstruct queued work.

## Stored state and ownership

SQLite stores watched folders, per-path synchronization state, a single durable synchronization job, its ordered queue entries, settings, and migration history. `sync_state` caches hashes only when the stored size and modification time match the current asset; it also retains a remote identifier and failure details. `sync_jobs` records the aggregate runner state (`RUNNING`, `PAUSED`, or `IDLE`), and `sync_queue` holds the individual `PENDING`, `SYNCED`, or `FAILED` outcomes.

Queue rows carry the target-album identifier and remote identifier needed to finish the intended outcome without re-uploading an asset. By contrast, API keys and optional Cloudflare credentials do not go into SQLite: authentication stores them in the native keyring.

## Schema evolution

Migrations in `src-tauri/migrations/` evolve the database. They are applied through SQLx at initialization, so a schema change must be introduced as a new forward-only migration; an already applied migration must not be edited. The original schema creates watched folders and cached sync state, later migrations add failure details, settings, the durable job and queue, and album/remote fields on queued entries.

## Startup recovery

Startup initializes the audit logger and watcher machinery, opens and migrates the database, restores the durable job snapshot, loads configured folders, and places the pool and coordinator into Tauri-managed state. A recovered `PAUSED` snapshot initializes the coordinator as paused, so discovery can preserve new queue work without accidentally restarting uploads.

For a damaged database, initialization performs SQLite verification and treats corruption and migration checksum mismatches as recoverable. It quarantines the unusable database (including WAL/SHM files), creates a fresh database, and attempts to salvage watched-folder records. Other retained data—settings, queue history, and the hash cache—is intentionally not promised after repair, and the application surfaces a recovery notice.

## Operational invariants

- The durable queue, rather than a new scan, is the source for resumed work.
- Updates to grouped synchronization state use transactions so a stored outcome is coherent.
- Removing a folder discards only unsynchronized/queued work in its scope; synchronized records remain available for duplicate and hash decisions.
- SQLite uses WAL and a bounded connection pool so readers can remain responsive while writes serialize.

## Navigation

- [Synchronization Workflow](../workflows/synchronization.md) describes how the queue is produced and consumed.
- [Watched Folders and Media Discovery](../workflows/watched-folders.md) covers configured folder state.
- [Authentication and Credential Security](../security/authentication.md) covers the keyring-held complement to this database.
- [Development, Testing, and Operations](../operations/development-and-testing.md) covers migration and recovery verification.
