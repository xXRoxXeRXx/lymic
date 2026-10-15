---
type: operations
title: Development, Testing, and Operations
description: Practical commands and verification boundaries for developing, testing, and diagnosing Lymic.
tags: [development, testing, operations, audit]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-06ca3782a1ec6b853845b60b
    resource: repo://src-tauri/src/audit/mod.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Development, Testing, and Operations

Lymic is developed as a Node/Svelte frontend and a Rust/Tauri backend. Run the narrowest relevant tests while iterating, then use both toolchains’ checks before handing off a behavioral change.

## Local commands

```powershell
npm install
npm run tauri dev
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

`npm run tauri dev` provides the desktop smoke-test environment. `npm run check` runs SvelteKit synchronization and static checking; `npm test` runs Vitest. The Rust manifest supplies the Tauri backend and its test suite.

## Test boundaries

Frontend tests sit beside the route, controllers, subscription helper, and components. Rust unit and integration-style tests sit with modules such as database, media, synchronization, authentication, and app lifecycle. Prefer tests that observe the real boundary you changed: an IPC event contract for UI changes, persisted queue state for recovery changes, and request parsing/error classification for Immich changes.

High-risk behavior deserves explicit observable checks: migrations and repair, pause and resume after restart, retry and duplicate handling, target-album failures, credential redaction, and streamed large-file transfer.

## Runtime diagnostics

The audit subsystem is initialized at startup and appends structured events through a managed `AuditLogger`. If logging is unavailable or append fails, the backend emits a UI error log event instead of silently dropping the condition. Startup, synchronization, watcher registration, and database repair all create audit-relevant events, making the local log useful when investigating a lifecycle failure.

The application uses a tray-first desktop lifecycle: close hides the window, while actual process exit flushes audit output. Database repair preserves a quarantined copy and reports that only watched folders may be restored; troubleshoot recovered state with this distinction in mind.

## Navigation

- [Lymic Architecture Guide](../quickstart.md) links development tasks to their primary areas.
- [Desktop Runtime Architecture](../architecture/runtime.md) covers startup and tray lifecycle.
- [Local Persistence and Recovery](../data/local-state.md) covers migrations and repair.
- [Authentication and Credential Security](../security/authentication.md) covers secret-handling checks.
