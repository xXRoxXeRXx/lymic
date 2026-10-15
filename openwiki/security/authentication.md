---
type: security
title: Authentication and Credential Security
description: How Lymic validates Immich servers, protects credentials in the native keyring, and keeps secrets out of diagnostics.
tags: [security, authentication, keyring, immich]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-15T07:26:54.164Z
sources:
  - id: openwiki-source-d0aa70773e481ea79113c4ad
    resource: repo://src-tauri/src/auth.rs
  - id: openwiki-source-0fc5ba992451ba2dccaef07f
    resource: repo://src-tauri/src/commands/auth.rs
  - id: openwiki-source-302fefd6d827abadc8b2086e
    resource: repo://src-tauri/src/immich/client.rs
generated: { by: "codex", at: "2026-10-15T07:26:54.164Z" }
---

# Authentication and Credential Security

Lymic stores connection credentials in the operating system’s native keyring, not in its SQLite database. The stored configuration contains the server URL, API key, local-HTTP opt-in, and optional Cloudflare Access pair; its debug representation explicitly redacts the API key and Cloudflare secret.

## Login and logout

The login command trims optional Cloudflare values, creates a temporary `ImmichClient`, validates the connection through the current-user endpoint, and only then writes credentials to the keyring. Logout deletes the keyring entry; a missing entry is already considered a successful logged-out state. The UI can separately query authentication status, stored server URL, and current user name.

Audit events use a derived actor identity and sanitize failures using the submitted URL, API key, and Cloudflare values, avoiding those secret values in logs and user-facing errors.

## Server URL policy

`ImmichClient` requires HTTPS by default. HTTP is accepted only with explicit opt-in and only for `localhost` or loopback IP addresses. URLs must contain a host and may not contain embedded credentials, query strings, or fragments. The client normalizes the configured base path to an API base and disables redirects, preventing an API-key request from being redirected to a different or downgraded target.

Requests pass the API key in `x-api-key`. Cloudflare Access is enabled only when both client ID and secret are supplied and valid; the secret header is marked sensitive.

## Navigation

- [Immich Integration](../integrations/immich-api.md) covers the authenticated requests this client performs.
- [Local Persistence and Recovery](../data/local-state.md) distinguishes database state from keyring-held credentials.
- [Desktop Runtime Architecture](../architecture/runtime.md) shows where stored credentials affect startup and synchronization.
