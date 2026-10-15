---
type: integration
title: Frontend State and Tauri IPC
description: How Svelte controllers own interface state and coordinate named Tauri commands with live backend events.
tags: [frontend, svelte, tauri, ipc]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-11d0a4fc3a0d3b69fb50a64d
    resource: repo://src-tauri/src/app/events.rs
  - id: openwiki-source-53e081c93fba07219ed1e23f
    resource: repo://src-tauri/src/commands/sync.rs
  - id: openwiki-source-88425a733aa79bac537f50d8
    resource: repo://src/lib/tauri/sync-event-subscription.ts
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Frontend State and Tauri IPC

The Svelte frontend treats backend state as authoritative and keeps interaction and presentation state in controllers. The root route wires controllers into views; components receive values and callbacks rather than making their own IPC calls.

## Controller-owned UI state

The synchronization controller owns dashboard-facing state: the current synchronization status, progress, active filename, transfer estimates, watched folders, failed entries, logs, and folder-dialog state. It hydrates folders and the current snapshot through commands, then reacts to events. Authentication, preferences, update, and feedback each have separate controllers, composed in `+page.svelte`.

A representative user action is “sync”: the controller invokes `start_sync`, while the backend validates that folders exist, prevents overlapping manual runs, marks the persistent job `RUNNING`, obtains credentials, scans folders, and runs the synchronization pipeline. Pause and resume use snapshot-returning commands so the UI can immediately reflect durable status.

## Event contract

`sync-event-subscription.ts` is the frontend subscription boundary. It subscribes to progress, percentage, start, transfer-statistics, snapshot, pause, resume, error, trigger, log, and idle events, and returns cleanup that removes listeners when the controller is destroyed. The helper checks active lifecycle state so a late asynchronous listener does not remain attached after unmount.

The backend constructs structured UI log payloads and emits named events. Event names and serialized payload shapes are a compatibility contract between Rust and TypeScript. Update both sides—and their tests—when changing a command, an event name, or a payload.

## Lifecycle and failure behavior

The route starts controller lifecycles on mount, hydrates data, then starts subscriptions after the synchronization controller has initialized. Errors during a request are reported to the feedback controller; failed synchronization state is refreshed from the backend rather than inferred only from a rejected invocation. When a user logs out, the synchronization controller clears its user-visible state and returns the route to the dashboard view.

## Navigation

- [Desktop Runtime Architecture](../architecture/runtime.md) covers composition and registered command ownership.
- [Synchronization Workflow](../workflows/synchronization.md) covers what the sync commands execute.
- [Watched Folders and Media Discovery](../workflows/watched-folders.md) follows add/remove folder actions.
- [Authentication and Credential Security](../security/authentication.md) covers login and logout.
