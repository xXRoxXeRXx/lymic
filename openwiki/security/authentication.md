---
type: security architecture
title: Authentication and Credential Boundaries
description: How the desktop client validates and stores Immich credentials, constructs authenticated clients, and handles logout, startup, and synchronization failures.
tags: [security, authentication, credentials, keyring, immich]
verified:
  - by: openwiki/0.7.2
    at: 2026-11-13T15:48:39.594Z
sources:
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-00ff4b2512b6dbfa268cbfa4
    resource: repo://src-tauri/capabilities/default.json
  - id: openwiki-source-3638749eb6f61a23896edb4d
    resource: repo://src-tauri/src/app/background.rs
  - id: openwiki-source-4e434927bce2240ac81949ac
    resource: repo://src-tauri/src/app/mod.rs
  - id: openwiki-source-67c8c914a2c873ee8a3d870d
    resource: repo://src-tauri/src/audit/event.rs
  - id: openwiki-source-d0aa70773e481ea79113c4ad
    resource: repo://src-tauri/src/auth.rs
  - id: openwiki-source-5c8cfd03344143eaad8d7595
    resource: repo://src-tauri/src/commands/albums.rs
  - id: openwiki-source-0fc5ba992451ba2dccaef07f
    resource: repo://src-tauri/src/commands/auth.rs
  - id: openwiki-source-c374d892616f6c4a01eace43
    resource: repo://src-tauri/src/commands/profile_cache.rs
  - id: openwiki-source-68f595f7eb6172e10ca02c4c
    resource: repo://src-tauri/src/commands/timeline.rs
  - id: openwiki-source-302fefd6d827abadc8b2086e
    resource: repo://src-tauri/src/immich/client.rs
  - id: openwiki-source-d86d6ce7cb2c85ce2672a18b
    resource: repo://src-tauri/src/sync/service.rs
  - id: openwiki-source-5d0ac05ba13520bfac82dec7
    resource: repo://src/lib/controllers/auth-controller.svelte.test.ts
  - id: openwiki-source-55591e62b629b2a76b404b16
    resource: repo://src/lib/controllers/auth-controller.svelte.ts
  - id: openwiki-source-aca69707d11c38fb1aa256e1
    resource: repo://src/lib/tauri/desktop-adapter.ts
generated: { by: "openwiki/0.7.2", at: "2026-11-13T15:48:39.594Z" }
---

# Authentication and Credential Boundaries

Lymic authenticates to Immich with an API key and can attach an optional Cloudflare Access service-token pair. The renderer supplies these values to registered Tauri commands; the Rust backend is the boundary that validates a connection before writing the complete configuration to the operating system's native keyring. Profile data, thumbnails, the sync queue, and audit records are separate stores, not alternate credential stores.

## Credential ownership and retrieval

`AuthConfig` holds the server URL, API key, local-HTTP opt-in, and optional Cloudflare client ID and secret. The configuration is serialized into one fixed native-keyring entry. A missing entry means logged out, and deleting an already-missing entry succeeds. Defaults for deserializing older configurations leave local HTTP and Cloudflare Access disabled.

Credential reads have a process-local two-second cache. It can cache either a configuration or the logged-out result, and successful stores and deletions update it immediately. This reduces keyring reads; the keyring remains the persistent authentication authority. The authenticated-client cache is likewise derived process state rather than a session authority.

Treat the complete configuration as secret-bearing. `AuthConfig`'s `Debug` output redacts the API key and Cloudflare client secret, but callers must not put a configuration or credential component in diagnostics, screenshots, or support material.

## Login, persistence, and logout

The Svelte controller uses `AuthDesktopAdapter`; its Tauri implementation invokes `login`, `logout`, `get_auth_status`, `get_server_url`, and `get_current_user`. At startup, `hydrate()` obtains the keyring-backed authentication status, then the saved server URL and profile. A profile lookup failure is non-fatal to that frontend authentication state.

Before IPC, the controller requires a URL and API key, trims Cloudflare values, and refuses a partial Cloudflare pair. The command repeats Cloudflare normalization because IPC input is untrusted. It constructs a temporary `ImmichClient`, calls `validate_connection()`, and stores credentials only after that succeeds. Validation requests `GET users/me` with `x-api-key`; a successful response must provide a nonblank name or email. The saved local-HTTP setting is derived from the accepted client. Profile-cache clearing after a successful login is best effort.

```mermaid
sequenceDiagram
    participant UI as Svelte controller
    participant IPC as Tauri auth command
    participant Client as Immich client
    participant Server as Immich server
    participant Keyring as Native keyring
    participant ProfileCache as Profile cache

    UI->>UI: Validate fields and token pair
    UI->>IPC: login request
    IPC->>Client: Construct temporary client
    Client->>Client: Validate URL and headers
    Client->>Server: GET users/me with API key
    Server-->>Client: Current user or error
    alt Connection is valid
        IPC->>Keyring: Store configuration
        IPC->>ProfileCache: Best effort clear
        IPC-->>UI: Success
        UI->>UI: Clear entered secrets and load profile
    else Construction or validation fails
        IPC-->>UI: Error without keyring write
    end
    UI->>IPC: confirmed logout
    IPC->>Keyring: Delete credentials
    IPC->>ProfileCache: Clear after deletion succeeds
    IPC-->>UI: Success or error
```

*Login validates the proposed connection before persistence; logout clears profile data only after credential deletion succeeds.*

Logout derives its audit actor from existing credentials before attempting deletion. On success it clears the profile cache and thumbnail cache. If deletion fails, cache cleanup does not run and the controller retains its authenticated state; after a successful, confirmed logout it clears its API-key, local-HTTP, Cloudflare, display-name, and avatar state before its logout hook.

## Client construction and transport policy

Every `ImmichClient` constructor—whether for login, a command using stored credentials, or sync—centralizes the credential-bearing transport rules:

- HTTPS is required by default. HTTP is permitted only when `allow_local_http` is enabled and the host is exactly `localhost` or an IPv4 or IPv6 loopback address. LAN hosts, `.local` names, public hosts, and other schemes are rejected.
- A server URL needs a host and may not contain userinfo, a query, or a fragment. The client normalizes the path to an Immich `/api/` base while retaining an existing trailing `/api` and deployment path prefixes.
- Reqwest redirect handling is disabled, so a credential-bearing request cannot follow a redirect to a different target.

The `LOCAL_HTTP_NOT_ALLOWED` error is recognized by the controller and shown as a localized login error. Enabling local HTTP is an explicit acceptance that the API key is sent over that loopback connection.

Cloudflare Access is off when both optional values are absent, including whitespace-only values after normalization. If supplied, the client ID and secret must be a complete pair and valid HTTP header values. The constructor installs `cf-access-client-id` and a sensitive `cf-access-client-secret` as default headers, so they accompany validation and later requests from that client; API calls add the Immich API key as `x-api-key` per request.

Commands such as albums, timeline, map, people, asset, and thumbnail operations call `ImmichClient::from_stored_credentials()`. That helper reads the current credentials first, reuses an `Arc<ImmichClient>` only when the entire `AuthConfig` matches its cached configuration, and otherwise constructs and caches a client with the corresponding derived account ID. Consequently, a logout is observed before such a command can obtain an authenticated client, and a changed configuration cannot reuse the prior client's cache entry.

Synchronization deliberately takes a separate path: after it reads credentials for a run, it constructs an `ImmichClient` from that snapshot rather than using the shared command-client cache. This still applies the same constructor policy, including the persisted local-HTTP setting and Cloudflare pair.

## Authentication failure during sync and startup

Sync admission persists discovered work and claims the lifecycle before it obtains credentials. If credentials are absent or unreadable, or client construction rejects the stored server configuration, the service marks the run `FAILED`, updates the sync snapshot and tray action when possible, and writes a `sync.aborted` audit event. Durable queued work is retained, so it can be recovered rather than discarded. The same failure handling applies to manual and background sources; manual admission also emits a sync error to the UI.

Background startup starts a startup-sync task, waits two seconds, loads and scans watched folders, then submits the result with `SyncSource::Startup`. It therefore reaches the same credential gate after scan admission. A missing login or invalid persisted configuration can leave the startup run failed with queued work retained; it does not make the desktop login command fabricate a session.

## Profile isolation and audit boundaries

Profile metadata and an optional raw avatar are stored under the application cache directory, not in the keyring. Cache reads are account-bound and can additionally be user-bound. After a current-user refresh, the backend rereads credentials and compares the derived account ID before resolving or persisting the result; a session change returns `Session changed during profile refresh` rather than saving data from the old session into the new one.

Profile-image retrieval rejects declared or actual bodies over 10 MiB. Cached data becomes a data URL only when its signature is JPEG, PNG, WebP, or GIF; unrecognized bytes, including SVG, are not rendered as avatars.

Login and logout audit events use a domain-separated SHA-256 actor identifier derived from the normalized server URL and API key instead of storing those raw values. Failed-login and profile-refresh diagnostics redact submitted credential values; the audit sanitizer removes line breaks, collapses whitespace, and bounds retained error text.

## Capability and server-permission boundaries

The `default` Tauri capability applies to the `main` window. It declares `core:default`, narrowly listed dialog permissions, restart permission, and the updater and opener defaults. This desktop capability controls the application's Tauri/plugin surface; it does not grant access to Immich data or expand an API key's server-side scope.

Immich enforces API-key scopes. The documented workflow needs `user.read` for login/current-user lookup, `asset.read` and `asset.upload` for synchronization, and `album.read`, `album.create`, and `albumAsset.create` for album features. Normal operation does not require asset deletion or album update; grant only scopes needed for enabled features.

## Change and test guidance

When changing authentication code:

1. Validate a proposed client before storing credentials, and keep URL, header, and redirect policy in `ImmichClient::new`.
2. Treat credential and client caches as derived optimizations. Credential mutations must update the credential cache; command clients must read stored credentials before reuse.
3. Keep sync's credential gate and durable failure path aligned with all sync sources, including startup and recovery.
4. Clear profile and thumbnail caches only after successful credential deletion, and retain the account-change guard around profile persistence.
5. Redact the URL and every credential component before diagnostics or audit records.

Focused tests cover legacy defaults and debug redaction, loopback-only HTTP, URL and Cloudflare-header validation, API-base normalization, frontend login/session behavior, profile cache account/user isolation, rejected SVG avatars, the profile-refresh session-change guard, and durable sync failure behavior.

## Related documentation

- [Runtime architecture](../architecture/runtime.md) explains desktop startup and background services.
- [UI and IPC](../frontend/ui-and-ipc.md) describes the renderer-to-Tauri boundary.
- [Immich API integration](../integrations/immich-api.md) covers authenticated API operations.
- [Quickstart](../quickstart.md) covers initial connection setup.
- [Synchronization](../workflows/synchronization.md) describes queue and transfer behavior.
