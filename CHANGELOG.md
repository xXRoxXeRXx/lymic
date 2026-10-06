# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.13.0] - 2026-10-06

### Added
- **Byte-level sync progress tracking**:
  - Tracks synchronization progress across queue blocks based on total and completed bytes (`totalBytes`, `completedBytes`), providing accurate byte-weighted progress calculations in both the backend coordinator and the frontend dashboard.
- **XMP sidecar naming variant discovery**:
  - Automatically identifies sidecar metadata files using both stem-based (`photo.xmp`) and appended (`photo.jpg.xmp`) naming conventions, including case-insensitive matching on non-Windows platforms.

### Changed
- **Modular Svelte UI architecture**:
  - Decomposed the monolithic main page into modular Svelte components (`DashboardView`, `FolderList`, `ActivityLog`, `SettingsView`, `LoginView`, `FailedSyncsModal`) with shared TypeScript types and dedicated unit tests.
- **Pausable sync queue processing windows**:
  - Bounded sync execution into small, inspectable batch windows, allowing responsive pause and resume operations without stalling active thread pools or corrupting queue checkpoints.
- **Upload pipeline hash caching**:
  - Reuses cached file hashes from the local database during queue execution to avoid redundant re-hashing of unchanged media files.
- **Typed frontend activity log events**:
  - Replaced unstructured log strings with typed, structured event payloads (`UiLogEvent`) for clearer auditability and consistent UI rendering.

### Fixed
- **NAS and system metadata exclusion during folder scans**:
  - Excludes Synology and NAS metadata directories (`@eaDir`, `@SynoResource`, `#recycle`) as well as hidden dot-directories from media scans to prevent unwanted files and phantom sync errors.
- **Automatic removal of missing files on retry**:
  - Cleanly discards queue entries for files that no longer exist on disk when retrying failed synchronizations instead of triggering repeated errors.

## [0.12.0] - 2026-10-04

### Fixed
- **Immich bulk check payload bounding**:
  - Limited the chunk size for batch duplicate checks to 100 units (`BULK_CHECK_CHUNK_SIZE`), ensuring requests stay within Immich server body size limits and preventing bulk sync errors during large synchronizations.
- **Unix platform test compatibility**:
  - Corrected database file lock simulation in unit tests on Unix environments by toggling permissions rather than relying on Windows-exclusive share locks.
- **macOS system tray compiler warning**:
  - Eliminated unused variable warning for the system tray handle on macOS targets.

## [0.11.0] - 2026-10-04

### Added
- **Pausable persistent sync queue**:
  - SQLite-backed sync queue (`sync_queue`) that preserves sync state across app restarts.
  - Ability to pause and resume ongoing synchronizations directly from the UI.
  - Automatically recovers interrupted or running sync queues as paused upon startup.
- **Configurable upload parallelism**:
  - Configurable worker count (1 to 10 concurrent uploads, default: 3) in Settings.
  - Preferences persisted in the local database (`app_settings`).
- **Live Photo and Motion Photo pair uploads**:
  - Automatic detection and pairing of photo assets with corresponding motion video clips (e.g. `.heic`/`.jpg` with `.mov`).
  - Multipart upload streaming both image and live photo video parts to Immich in a single operation.
- **Dark mode theme preferences**:
  - Support for System, Light, and Dark themes selectable in Settings.
  - Theme preference saved to local storage with smooth transitions.
- **Database auto-recovery and quarantine**:
  - Automatic detection of SQLite database corruption via `quick_check` and error codes.
  - Quarantines corrupted database files (`.db`, `.db-wal`, `.db-shm`) into timestamped backups.
  - Automatically salvages watched folders before database recreation.
  - Emits desktop notifications, audit log records, and startup notifications in the UI.
- **Detailed sync retry and failure inspectability**:
  - Records granular failure reasons for failed uploads (`sync_failure_reason`).
  - UI modal listing failed sync items with reasons and retry capability.
- **Expanded media format support**:
  - Recognizes all asset file extensions supported by Immich (including RAW formats, DNG, WebP, MP4, MKV, etc.).
- **User profile display in settings**:
  - Displays currently authenticated Immich user details fetched via `/api/users/me`.
- **Security & UX improvements**:
  - Confirmation dialog before user logout.
  - Disables sync actions when no folders are selected or watched.
  - Prevents default browser context menus and web interaction defaults (drag & drop into window).
  - Version label in the global footer.

### Changed
- **Modularized Tauri backend**:
  - Reorganized Rust codebase into clean modules (`api`, `auth`, `config`, `db`, `media`, `sync`, `watcher`).
  - Decoupled sync pipeline coordination, media scanning, and watcher callbacks.
- **Documentation**:
  - Added comprehensive project architecture and operations guide in `docs/README.md`.

## [0.10.0] - 2026-10-04

### Added
- **Structured audit logging**:
  - Local, privacy-conscious audit logs for administrative and security-relevant actions with automated rotation.
- **Login verification**:
  - Validates server URL and API key using the authenticated current user endpoint (`/api/users/me`).

### Fixed
- Fixed build warnings related to system tray menu construction on macOS.

### CI/CD
- Upgraded CI workflows to `actions/checkout@v5` and `actions/setup-node@v5`.

## [0.9.0] - 2026-10-03

### Added
- **SQLite migrations**:
  - Schema migrations and connection pool settings for SQLite.
- **XMP sidecar streaming**:
  - Streaming and size limits for sidecar files during upload.

### Fixed
- Fixed race conditions and watcher event overflow handling during directory scans.
- Hardened error propagation across sync and account actions.
- Deduplicated overlapping watched folders.
- Enforced secure HTTPS protocol for Immich server URLs.

### Changed
- Updated dependencies to SvelteKit 3 and Node.js 22 in CI pipelines.
- Hardened Content Security Policy (CSP) and Tauri capabilities.

## [0.8.0] - 2026-04-25

### Added
- Initial release of Lymic desktop sync client for Immich.
- Automated filesystem watcher for local media folders.
- Fast duplicate detection via SHA-1 file hashing.
- Background system tray support.
- Multi-language UI (German and English).
