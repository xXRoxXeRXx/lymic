---
type: integration
title: Immich Integration
description: Lymic's authenticated boundary to Immich for identity checks, duplicate detection, streamed uploads, and target-album membership.
tags: [immich, api, uploads, albums]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-27cf4cf217241afe3fc02fc8
    resource: repo://src-tauri/src/immich/bulk_check.rs
  - id: openwiki-source-d21a18714fae9df82df58bf5
    resource: repo://src-tauri/src/immich/upload.rs
  - id: openwiki-source-a1c1326bfb806284fbf8343f
    resource: repo://src-tauri/src/sync/album_batch.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Immich Integration

The `immich/` module isolates Lymic’s REST interactions behind `ImmichClient`. Synchronization creates an authenticated client from keyring-held configuration, then uses it for identity validation, duplicate checks, uploads, asset lookup, and album actions.

## Duplicate checks and upload decisions

Before upload, Lymic sends checksums through the bulk-check API. The response parser requires a complete one-for-one response for requested hashes. A duplicate may include an Immich asset identifier; if it does not and album assignment is needed, Lymic performs a checksum search to resolve the unique remote asset. Unauthorized and forbidden bulk-check responses are classified as non-retryable; other server failures are retryable.

Accepted units are uploaded as multipart requests. Asset files, an optional live-photo video, and an optional XMP sidecar are streamed from disk; the request construction records transfer progress without loading the full asset into memory. An upload response must contain a nonempty remote asset ID.

## Target albums are a required outcome

A Watched Folder can carry a Target Album. Its identifier is copied into each queue entry when work is enqueued, so changing the folder later cannot change in-flight work. Both uploaded assets and duplicates are eligible for assignment. The assignment layer batches pending items by album, adds their remote IDs, and only completes the queue items after the album action succeeds.

There is no silent fallback to the root library. If an album operation fails, the affected entries become failed work with a reason. The queue retains the remote ID after a successful upload, allowing a later album-assignment retry without a second upload.

## Navigation

- [Authentication and Credential Security](../security/authentication.md) covers client construction and secret handling.
- [Synchronization Workflow](../workflows/synchronization.md) places these calls in the end-to-end pipeline.
- [Watched Folders and Media Discovery](../workflows/watched-folders.md) explains the source of Target Album configuration.
