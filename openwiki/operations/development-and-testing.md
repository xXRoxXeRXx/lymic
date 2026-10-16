---
type: operations
title: Development, Testing, and Operations
description: Repository-specific local development, verification, desktop smoke-test, and diagnostic guidance for Lymic. Scope checks to the frontend, Rust backend, or end-to-end boundary changed.
tags: [development, testing, operations, diagnostics]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-16T12:54:13.301Z
sources:
  - id: openwiki-source-ab40d61d1bfa656ad091dbd7
    resource: repo://docs/agents/application-map.md
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-ca67060e890937010b96de80
    resource: repo://src-tauri/Cargo.toml
  - id: openwiki-source-4e434927bce2240ac81949ac
    resource: repo://src-tauri/src/app/mod.rs
  - id: openwiki-source-06ca3782a1ec6b853845b60b
    resource: repo://src-tauri/src/audit/mod.rs
  - id: openwiki-source-5dd5e1d51325d70994289153
    resource: repo://src-tauri/src/audit/writer.rs
  - id: openwiki-source-e852d51f343149f2ec60ea2b
    resource: repo://src-tauri/src/db/tests.rs
  - id: openwiki-source-2f74d91a723df4e84be6c65f
    resource: repo://src-tauri/src/removable/tests.rs
generated: { by: "openwiki/0.7.2", at: "2026-10-16T12:54:13.301Z" }
---

# Development, Testing, and Operations

Lymic is a Tauri desktop application with a SvelteKit/TypeScript frontend and Rust backend. Work from the repository root. During iteration, run the smallest test that exercises the behavior under change; before handoff, run the applicable frontend and Rust gates and perform a desktop smoke test when the UI, IPC, startup, filesystem, or service integration is affected.

## Prepare and run

The documented prerequisites are a current stable Rust toolchain, Node.js 18 or later, and the operating-system dependencies required by Tauri. Install the JavaScript dependencies once after cloning:

```powershell
npm install
```

Use the Tauri CLI script for the integrated desktop application. It starts the Vite frontend through Tauri and is the appropriate local smoke-test entrypoint:

```powershell
npm run tauri dev
```

The package manifest also provides the following scripts. Use the named script rather than reconstructing its underlying tool invocation.

| Need | Command | What it does |
| --- | --- | --- |
| Frontend-only development server | `npm run dev` | Runs `vite dev`. |
| Production frontend bundle | `npm run build` | Runs `vite build`. |
| Serve a built frontend bundle | `npm run preview` | Runs `vite preview`. |
| Static Svelte/TypeScript check | `npm run check` | Synchronizes SvelteKit and runs `svelte-check` with `tsconfig.json`. |
| Watch static checks | `npm run check:watch` | Runs the same synchronization and checker in watch mode. |
| Frontend tests | `npm test` | Runs Vitest once (`vitest run`). |
| Tauri subcommands | `npm run tauri <subcommand>` | Invokes the Tauri CLI; for example, `npm run tauri build`. |

## Verification by change boundary

Run the narrowest relevant frontend or Rust test while changing code, then run these repository-wide gates for the layers touched:

```powershell
npm run check
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

The Cargo commands deliberately name `src-tauri/Cargo.toml`; do not run them against an assumed root Rust manifest. `--locked` on the test command verifies the checked-in dependency resolution. `cargo fmt --check` and Clippy with `-D warnings` are handoff gates, not substitutes for behavior tests.

Choose tests based on the boundary changed:

- **Frontend state, rendering, or localization:** test the owning component and controller, then run `npm test` and `npm run check`. If a UI action crosses IPC, verify both the controller caller and the command/event contract rather than testing the component alone.
- **Tauri commands or event payloads:** inspect the command registration, implementation, frontend caller, and the stable subscription helper together. Keep command names plus event names and serialized payloads compatible with their Svelte consumers.
- **Database, migration, queue, or recovery:** add or update Rust tests that use the migrated test database. Exercise observable durable outcomes—especially pause/restart recovery, queue order and deduplication, failure/retry behavior, and repair—not merely a successful query.
- **Removable media and watcher lifecycle:** test arrival, removal, remount, watcher reconciliation, and the subsequent scan using the coordinator boundary. A removal must not silently discard the persisted folder binding; reconnect behavior and failed queue recovery need separate coverage.
- **Immich, authentication, or upload changes:** test request/error behavior and redaction at the integration boundary. For a real-server smoke test, use deliberately non-production credentials and verify the affected upload, duplicate, or Target Album path without exposing secrets in output.

Migrations, pause/resume after restart, credentials, queue retry, duplicate handling, Target Album failures, large-file streaming, and database repair are high-risk. Their acceptance criteria are observable state and user-facing error handling, not compilation alone. Detailed synchronization behavior belongs in [Synchronization](../workflows/synchronization.md), credential policy in [Authentication and Credential Security](../security/authentication.md), and local recovery semantics in [Local Persistence and Recovery](../data/local-state.md).

## Desktop smoke-test checklist

Start with `npm run tauri dev`, then verify the smallest end-to-end path that includes the changed boundary. For changes to startup or desktop behavior, verify the application window/tray behavior rather than assuming a running development server proves it: the backend composition root initializes application state and background work, registers IPC commands, installs close-to-tray handling, and writes a shutdown audit record only when the process actually exits.

For a synchronization-facing change, a useful smoke test is: authenticate against a safe server, add or select a test Watched Folder, trigger the relevant scan or synchronization action, observe UI progress/errors, and confirm the expected durable result. When testing failure paths, also verify recovery after a restart or removable-volume reconnection as applicable. Do not turn an unavailable folder or a failed album assignment into a “success” merely because the process stayed alive.

## Runtime diagnostics

Audit records are local JSON Lines files under the application data directory's `logs` directory, named `audit-0.jsonl` through `audit-4.jsonl`. The writer retains five slots and rotates when a file would exceed 5 MiB. This is the first artifact to inspect for startup, watcher, synchronization, and repair lifecycle failures; correlate it with the UI's `log-message` error when the application is open.

The audit logger is asynchronous and bounded. When its delivery queue is saturated, events can be dropped and the UI is notified; when capacity recovers, it reports how many records were dropped. A missing managed logger or an append failure likewise becomes a UI error rather than being silently ignored. At process exit, the runtime appends the shutdown event and attempts a flush. Therefore use the tray's quit action or otherwise exit the process when validating final audit output—closing the main window only hides it.

If a local database fails validation or migration, diagnose it through the recovery notice and the persisted backup rather than deleting files speculatively. Repair creates a fresh database and may restore Watched Folder configuration, but it does not establish that prior queue/history state was recovered. See [Local Persistence and Recovery](../data/local-state.md) for the data and recovery boundary, and [Desktop Runtime Architecture](../architecture/runtime.md) for startup and tray lifecycle details.

## Contribution expectations and related pages

Contributions should include tests for code that needs them, pass the relevant test suite, and be formatted/linted. Use [Lymic Architecture Guide](../quickstart.md) to locate an area before changing it. The [Desktop Runtime Architecture](../architecture/runtime.md), [Synchronization](../workflows/synchronization.md), [Local Persistence and Recovery](../data/local-state.md), and [Authentication and Credential Security](../security/authentication.md) pages own the corresponding system behavior; this page owns how to develop, verify, smoke-test, and diagnose it.
