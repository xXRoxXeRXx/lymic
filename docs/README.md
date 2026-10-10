# Lymic Project Documentation

## 1. Overview

Lymic is an unofficial, cross-platform desktop client for uploading and synchronizing local media with an [Immich](https://immich.app/) server. It monitors selected folders, finds supported media, determines which assets already exist on Immich, and uploads only the missing assets.

Lymic is designed as an **upload-oriented client**, not a two-way filesystem mirror. It does not mirror remote deletions, rename local files to match Immich, download remote assets, or manage albums as part of its normal synchronization workflow.

> **Disclaimer:** Lymic is a community project and is not affiliated with, maintained by, or endorsed by the Immich team. Keep independent backups of irreplaceable media.

## 2. Key Capabilities

- Watch multiple local folders recursively for new or modified media.
- Scan folders at startup and when a folder is added.
- Debounce filesystem events before processing them.
- Cache SHA-1/Base64 values and file metadata locally; reuse a value only after the current size and mtime match, avoiding rehashing unchanged files.
- Check batches of assets against Immich before uploading to avoid duplicate transfers.
- Stream multipart uploads from disk, which keeps memory use low for large files.
- Resume work through a durable local synchronization queue.
- Pause and resume synchronization safely.
- Show synchronization status, progress, errors, and logs in the desktop UI.
- Retry failed files.
- Support native credential storage, system-tray controls, desktop notifications, optional autostart, signed in-app updates, English, German, and light/dark appearance preferences.

## 3. Requirements

### To use the application

1. An accessible Immich server using **HTTPS** with a valid TLS configuration.
2. An Immich API key.
3. Local folders containing media that the operating-system account running Lymic can read.

The client rejects server URLs that contain embedded credentials, a query string, or a fragment. HTTP redirects are intentionally disabled. These protections mean a reverse proxy must expose the final HTTPS endpoint directly.

### To build from source

- [Rust](https://www.rust-lang.org/tools/install), current stable toolchain.
- [Node.js](https://nodejs.org/) 18 or later. The release workflow currently uses Node.js 22.
- Platform-specific [Tauri prerequisites](https://tauri.app/start/prerequisites/).
- On Ubuntu release builds, GTK 3, WebKitGTK 4.1, AppIndicator, librsvg, and `patchelf` are required; see [the workflow](../.github/workflows/release.yml) for the exact packages.

## 4. Connecting to Immich

Create an API key in Immich under **Account Settings → API Keys**, then enter the server URL and key in Lymic's login screen.

The existing project guidance recommends these scopes:

| Scope | Purpose |
| --- | --- |
| `asset.upload` | Upload new media assets. |
| `asset.read` | Check whether an asset already exists. |
| `server_info.read` | Validate server compatibility. |

Lymic normally does not need deletion or album-write privileges. Connection validation calls Immich's user endpoint, so ensure the key and Immich version allow the account lookup used during sign-in.

Credentials are stored in the operating system's native keyring (for example, Keychain, Credential Manager, or Secret Service), not in the project's SQLite database.

## 5. Synchronization Lifecycle

For each configured watched folder, Lymic follows this general flow:

1. **Discover** — Recursively scan the folder for supported media. File-watcher events initiate later incremental work; an event queue overflow causes a full rescan.
2. **Identify pairs and sidecars** — Associate supported live-photo/motion pairs and optional XMP sidecars where available.
3. **Hash** — Reuse a cached SHA-1/Base64 only when the current size and mtime match the stored metadata; stream changed or uncached files to calculate SHA-1/Base64.
4. **Check** — Send SHA-1/Base64 values to Immich's bulk upload check endpoint in batches. Assets already known to Immich are skipped.
5. **Queue and upload** — Persist required work in the local queue, then stream missing files as multipart uploads with configured concurrency.
6. **Record results** — Persist completion or failure details, update UI progress, and make failed work available for retry.

The persistent queue survives restarts. A pause request is durable and takes effect at safe processing boundaries; resuming continues queued work rather than requiring a full restart of the application.

### Folder behavior

- Duplicate folders and overlapping folder hierarchies are rejected to prevent ambiguous ownership and duplicate scanning.
- A folder that is temporarily unavailable remains configured. Lymic retries its watcher registration periodically (currently every 30 seconds).
- Closing the main window hides Lymic; use the system tray's quit action to exit the application.

### Media behavior and limits

Supported media is determined by the backend's extension list rather than by a single fixed short list. Common image, video, and Immich-compatible formats are supported.

Live/motion pairing is intentionally specific: files must share a stem and match one of these combinations:

- `HEIC` or `HEIF` image with `MOV` video
- `JPG` or `JPEG` image with `MP4` video

XMP sidecars are optional. The upload code caps an XMP sidecar at 10 MiB.

## 6. Architecture

Lymic is a Tauri v2 desktop application with a Svelte frontend and Rust backend.

```text
Svelte 5 UI
  │  Tauri IPC commands and application events
  ▼
Rust application layer
  ├── authentication and Immich HTTP client
  ├── folder watcher and background tasks
  ├── sync scanner, hash cache, queue, and upload processor
  ├── SQLite persistence and migrations
  ├── audit logging
  └── system tray and desktop plugins
  │
  ├── Native keyring (API key)
  ├── SQLite database (local state)
  └── Immich REST API (remote assets)
```

### Frontend

The single-page Svelte/SvelteKit interface provides sign-in, watched-folder management, synchronization controls, error/retry state, preferences, and event-driven progress. It uses TypeScript, Vite, Tailwind CSS, Lucide icons, and `svelte-i18n`.

The frontend persists the user's theme preference in browser local storage. Available application locales are English and German.

### Backend

The Rust backend is organized under `src-tauri/src/`. The application layer initializes state and services, registers Tauri commands, and emits events consumed by the UI. Major areas include:

| Area | Responsibility |
| --- | --- |
| `app/` | Startup, IPC command registration, background tasks, tray integration, and application events. |
| `sync/` | Scanning, hash-cache use, batch checks, queue processing, progress, pause/resume, and retry behavior. |
| `immich/` | Secure HTTP client, upload checks, and streamed multipart asset uploads. |
| `db/` and `migrations/` | SQLite initialization, migrations, watched folders, queue state, settings, and recovery. |
| `watcher/` | Recursive filesystem watching, debounce handling, overflow recovery, and unavailable-folder retries. |
| `media/` | Supported file extensions and media/pair classification. |
| `audit/` | Structured audit-log writing and rotation. |

## 7. APIs and Data Flow

### Immich REST endpoints

Lymic communicates with Immich using these primary endpoints:

| Endpoint | Use |
| --- | --- |
| `GET /api/users/me` | Validate the authenticated user/session during sign-in. |
| `POST /api/assets/bulk-upload-check` | Determine which locally discovered assets are already present. |
| `POST /api/assets` | Upload an asset through streamed multipart form data. |

Requests authenticate with the `x-api-key` header. The bulk check and upload `x-immich-checksum` header use SHA-1/Base64; requests may include an `assetData` file, a live-photo partner as `livePhotoData`, and an XMP sidecar when present.

### Tauri IPC and events

The UI invokes backend commands for authentication, folder configuration, synchronization start/pause/resume/status, failed-file retry, upload concurrency, localization, autostart, and database-recovery notices. The backend emits state, progress, idle, error, log, and tray-triggered synchronization events.

## 8. Local Data, Security, and Recovery

Lymic keeps application state in SQLite in Tauri's platform-specific application-data location. The database uses WAL mode and contains information such as watched folders, file metadata and checksums, queue state, remote asset identifiers, synchronization settings, and failed-item context.

If database initialization detects corruption, Lymic quarantines the damaged database, recreates state, and attempts to salvage watched folders. This is a recovery aid, not a substitute for backing up configuration.

Audit records are written as JSON Lines (`.jsonl`) in application data. Logs rotate across five files of up to 5 MiB each. Treat application data and audit logs as potentially sensitive because they can contain local paths, checksums, remote IDs, and failure details.

Security controls include:

- Native keyring storage for credentials.
- HTTPS-only server URLs.
- Refusal of credentials in URLs, redirects, query strings, and fragments.
- Streaming I/O to avoid retaining whole media files in memory.
- A restrictive Tauri content-security policy for the packaged application.

## 9. Development Guide

### Install dependencies

```bash
npm install
```

Use `npm ci` when installing exactly from the lockfile in a clean automation environment.

### Run the frontend only

```bash
npm run dev
```

This starts Vite on the development URL configured in [`src-tauri/tauri.conf.json`](../src-tauri/tauri.conf.json).

### Run the desktop application

```bash
npm run tauri dev
```

Tauri starts the Vite frontend and launches the native desktop shell.

### Build a production bundle

```bash
npm run tauri build
```

The configuration builds the static frontend before bundling native installers for the current platform.

### Quality checks

Run these commands before submitting a change:

```bash
# Frontend type and Svelte checks
npm run check

# Frontend tests
npm test

# Rust formatting
cargo fmt --manifest-path src-tauri/Cargo.toml --check

# Rust linting
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Rust tests, honoring Cargo.lock
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

The release workflow runs all of the checks above, including `npm test`, before bundling a release.

## 10. Repository Map

```text
.
├── src/                         # Svelte UI, routes, styles, and i18n
├── static/                      # Static frontend assets
├── src-tauri/
│   ├── src/                     # Rust application and synchronization backend
│   ├── migrations/              # SQLite schema migrations
│   ├── capabilities/            # Tauri capability permissions
│   ├── icons/                   # Native application icons
│   ├── Cargo.toml               # Rust package and dependencies
│   └── tauri.conf.json          # Tauri application/build configuration
├── .github/workflows/release.yml# Tagged release automation
├── package.json                 # Frontend dependencies and npm scripts
├── README.md                    # Project introduction and quick start
└── docs/README.md               # This detailed project documentation
```

## 11. Build and Release Process

Publishing is defined in [`.github/workflows/release.yml`](../.github/workflows/release.yml). A push of a tag matching `v*` triggers a matrix build for macOS Apple Silicon, Ubuntu 22.04, and Windows. The workflow installs Node.js and stable Rust, runs all checks, then invokes the Tauri GitHub Action to create a signed draft release. The action uploads installer artifacts, their signatures, and `latest.json`; Windows update metadata deliberately selects the NSIS installer.

Update the version consistently in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` before creating a release tag.

### In-app update signing

Tauri validates update signatures before installation. Generate an updater key pair once, outside the repository, and preserve both the private key and its password securely:

```bash
npm run tauri signer generate
```

The public key is embedded in `src-tauri/tauri.conf.json`. Store the private key in the GitHub repository secret `TAURI_SIGNING_PRIVATE_KEY` and its password, when one was set, in `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit either value. Losing the private key or password prevents signing future updates for already installed clients.

Create a `vX.Y.Z` tag only after the three version fields are synchronized. The workflow leaves the GitHub release as a draft. Review generated release notes, installers, signature files, and `latest.json` for `windows-x86_64`, `linux-x86_64`, and `darwin-aarch64`, then publish the draft manually. Only published stable releases are returned by the update endpoint; drafts and pre-releases are intentionally excluded.

The first updater-enabled release can update only installations that already include this updater public key. Existing `0.14.0` installations must install the first updater-enabled release manually from GitHub. Linux in-app updates apply to AppImage packages only. macOS artifacts also require Apple code signing and notarization for Gatekeeper trust; those credentials and steps are separate from Tauri updater signing.

## 12. Troubleshooting

| Symptom | Check |
| --- | --- |
| Cannot sign in | Confirm the final server URL is HTTPS, has no redirect, credentials, query string, or fragment, and that the API key has the required access. |
| Folder cannot be watched | Confirm the folder exists and the application has permission to read it. Temporarily unavailable folders remain configured and are retried. |
| Assets are skipped | The bulk upload check may have found matching assets already on Immich. Review logs and server state before forcing another upload. |
| Uploads fail or stall | Inspect the UI error state and local audit logs, verify network access and server health, then retry failed files. |
| Application appears to close | Closing its window hides it to the system tray. Quit from the tray menu. |
| Local state is reset | Check application-data backups and audit logs. Database recovery may have quarantined a corrupt database and recreated it. |

## 13. License and Contribution

Lymic is distributed under the [MIT License](../LICENSE). Follow the project's existing contribution guidance in [`CONTRIBUTING.md`](../CONTRIBUTING.md) when it is available in your checkout.
