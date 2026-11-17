# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Search by file name**:
  - A search bar at the top of every page searches your whole Immich library by file name ("contains"; `%` and `_` are searched literally). The results open chronologically in the shared timeline with scrubber, lightbox, thumbnail cache, favorite, download and add-to-album actions.
  - At most the 5,000 newest results are loaded; a notice says when a search was cut. Back returns to the page you searched from at its scroll position, and the clear control resets the search. The text is kept in memory for the session only.
  - Read-only; needs the `asset.read` API key permission and Immich 3.2 or later (older servers show an "unsupported" message and every other view keeps working). The search sends only Immich's structured `filter`. Case sensitivity of the file name match was not verified against a live server.
  - Includes unsupported, unauthorized, permission, unavailable, offline and empty states in all supported languages.
  - A **Filters** panel in the search bar adds a date range (from the start of the first local day to the end of the last local day, either bound optional; a start after the end is rejected with a message) and a media type (All, Images or Videos). A search can consist of filters alone; all set filters apply together. Filters are kept in memory for the session and reset by the clear control and on sign-out. The date and media type conditions of the structured `filter` have not been verified against a live server.
- **Search Mode**:
  - An options button beside the search field opens a panel with one exclusive Search Mode choice: Filename, Description, Folder Path or OCR (recognized text). The choice is kept for the session and reset on sign-out.
  - Description and Folder Path use "contains" matching with `%` and `_` searched literally. OCR is disabled with an explanation when the Immich Server has OCR switched off; the server features are read once and held in memory, and a failure to read them never blocks a Search. Case sensitivity of the Description and Folder Path match was not verified against a live server.
- **Search display options and Rating**:
  - A **Display** group in the search panel restricts a Search to Favorites, to Unfiled assets (in no album) or, with **Archived**, to archived assets only; archived assets are left out of every Search otherwise. An **Open Archive** link beside the options switches to the Archive view like its navigation entry (no Back button); the text, Search Mode and filters stay in memory. Archived is the only way to combine archived assets with text or other filters.
  - A **Rating** filter selects one exact star level from one to five; selecting it again clears it.
  - Unverified against a live server: Search sends `visibility` explicitly (`timeline`, or `archive` with the Archived option) instead of relying on Immich's default, because that default was not verified to exclude archived assets; "no album" is sent as `hasAlbums: false`; Rating is sent as `rating: { eq: n }` and its exactness (and how unrated assets behave) was not verified.
- **Search camera filters**:
  - The Filters panel adds camera make, camera model and lens model. Each is chosen only from the suggestions Immich offers (typing narrows the list, free text that is not a suggestion is never applied), because Immich compares these values exactly. The model's suggestions are limited to the chosen make; choosing a different make clears the chosen model.
  - A failure to read suggestions shows a message with Retry inside that filter only (naming `asset.read` when the API key lacks it) and does not affect the rest of Search. Suggestions are read when a field is first used and held in memory for the session.
  - Adds a generic, read-only suggestions request (`GET /api/search/suggestions` with a suggestion type and optional narrowing by make, country or state) that the Location filters reuse. Not verified against a live server: the `make` narrowing of model suggestions, whether the `make`, `model` and `lensModel` equality conditions of the structured `filter` match the suggested values exactly (case and whitespace), and whether lens suggestions are meant to be narrowed by make.
- **Search Location filter**:
  - The Filters panel adds a Location with country and state or region. Each part is optional and chosen only from the suggestions Immich offers (typing narrows the list, free text that is not a suggestion is never applied), because Immich compares these values exactly. The state's suggestions are limited to the chosen country; choosing a different country clears the chosen state. A Location is not a Place.
  - Sent as `country` and `state` equality conditions of the structured `filter`, combined with every other filter. A failure to read suggestions is shown with Retry inside the filter only. The city part follows in a later step.
  - Not verified against a live server: whether the `country` and `state` suggestion values match the `eq` conditions exactly (case and whitespace), whether the `country` narrowing of state suggestions behaves as assumed, and that a state alone (without a country) matches as expected, since state names can repeat across countries.
- **Archive view**:
  - Adds an Archive entry after Favorites that shows your archived photos and videos in the shared timeline with scrubber, lightbox, thumbnail cache, favorite, download and add-to-album actions.
  - Read-only: Lymic never archives or unarchives assets. Needs the `asset.read` API key permission.
  - Includes loading, empty, error and offline states in all supported languages.
- **Trash view and Restore**:
  - Adds a Trash entry after Archive that shows the photos and videos in your Immich trash in the shared timeline with scrubber, lightbox and thumbnail cache, plus download.
  - Restore trashed items from the lightbox or with multi-selection, without a confirmation; restored items leave the view and return to your Immich library, and show up in Photos the next time you open it. Items that fail to restore stay selected so you can retry.
  - Lymic never moves assets to the trash, deletes them permanently or empties the trash.
  - Browsing needs the `asset.read` API key permission; Restore needs `asset.delete`. The trashed filter, the permission and the Restore call were verified against Immich v3.4.1 only; no minimum Immich version is claimed.
  - If one selected item can no longer be restored (for example because Immich already removed it permanently), the others are restored one by one and the failed item stays selected.
  - Known limitation, to be tracked separately: Synchronization treats a local file whose Remote Asset is in the Immich trash as already synced, so it is not uploaded again and stays in the trash.
  - Includes loading, empty, error, offline and Restore messages and a retention hint in all supported languages.

## [0.25.0] - 2026-11-13

### Added
- **Interactive clustered map browsing**:
  - Adds a Map view powered by MapLibre GL with clustered markers for geotagged Immich assets.
  - Supports browsing complete map cluster collections and location results with shared lightbox previewing.
  - Preserves map session state across refresh navigation and enables trackpad pinch zoom.
  - Adds local map fixture server and distinct map profiling workloads for verification.
- **Lightbox asset actions and details**:
  - Displays asset metadata and details directly inside the shared lightbox.
  - Adds favorite toggling, original asset downloading, and album assignment from the lightbox.
  - Extracts a reusable shared lightbox component from the timeline.
- **Timeline multi-selection actions**:
  - Adds multi-selection mode in the timeline with batch download support for selected assets.
- **Documentation**:
  - Updates `GLOSSARY.md` with download and favorite definitions, adds Microsoft Store badge to `README.md`, and refreshes OpenWiki docs.

### Fixed
- **Map and lightbox resilience**:
  - Handles malformed map coordinates gracefully and improves map viewing and localization.
  - Preserves partial or failed download safety on stream failure and hardens lightbox favorite navigation state and asset details.

## [0.24.0] - 2026-11-08

### Added
- **Photo and video timeline**:
  - Adds a dedicated timeline view grouping media into responsive month-based columnar buckets.
  - Implements a responsive masonry layout for photos and videos with dynamic column sizing.
  - Integrates an interactive date scrubber for rapid navigation across timeline months with throttling and spacing.
  - Introduces a media lightbox modal supporting full-screen preview, image pan/zoom, and HTML5 video playback with millisecond-precision durations.
  - Registers custom URI schemes (`lymic-thumb:` and `lymic-asset:`) for secure, sandboxed asset and thumbnail streaming into WebView2 and WebKit webviews.
  - Establishes a multi-tier local thumbnail and asset cache with offline fallback and background cache refreshes that never block user browsing.
- **Recognized people discovery**:
  - Adds a dedicated People view displaying faces recognized by the Immich server.
  - Implements lazy loading and incremental pagination for browsing large person libraries.
  - Caches person thumbnails with versioned cache keys tied to thumbnail paths to prevent stale facial avatars.
  - Adds person-filtered timeline browsing, enabling direct navigation from a person's avatar to their photos and videos.
- **Album catalogue and album timeline**:
  - Adds an Album Catalogue view displaying accessible albums with cover thumbnails, titles, asset counts, and shared album indicators.
  - Integrates an album-specific timeline view grouping album media by local capture month into the shared timeline component.
  - Supports opening any accessible album in the timeline with seamless return navigation to the album catalogue.
  - Implements dedicated album caching and thumbnail loading with support for custom thumbnail protocols.
- **Release packaging and distribution**:
  - Adds Arch Linux AUR package (`lymic-bin`) support and automated release synchronization workflow.
  - Adds macOS Homebrew Cask automation script, workflow, and documentation.
- **Architecture and domain documentation**:
  - Records ADR-0006 for album timeline data sources and updates domain glossary.

### Fixed
- **Timeline protocol and cache resilience**:
  - Hardens custom URI schemes against invalid host lookups and WebView2 quirks.
  - Resolves authorization header propagation and cache isolation across server switches.
  - Fixes timeline refresh generation races so older fetch cycles cannot overwrite newer timeline state.

## [0.23.1] - 2026-11-05

### Fixed
- **Microsoft Store MSIX packaged startup and frontend delivery**:
  - Enables `custom-protocol` feature for Tauri in `src-tauri/Cargo.toml` so standalone release builds embed frontend assets and resolve `tauri.localhost` rather than defaulting to development `devUrl`.
  - Targets the dedicated Store binary (`lymic-desktop.exe`) in MSIX packaging scripts and release workflows.
  - Adds `internetClient` and `privateNetworkClientServer` capabilities to `AppxManifest.template.xml`.

## [0.23.0] - 2026-11-04

### Added
- **Canonical Snap Store distribution**:
  - Packages Lymic for Linux via Canonical Snap Store (`lymic`) under strict confinement using `core22` and the GNOME extension.
  - Declares interface plugs for network access, home directory access, external/removable storage, and password manager services (Secret Service API).
  - Configures WebKitGTK 4.1 layout bindings to prevent sandboxed runtime crashes.
  - Integrates automated Snapcraft configuration validation in TypeScript tests and PowerShell scripts.
  - Automates Snap package building, testing, and store publishing in GitHub Actions release workflow.
- **Product showcase and landing page**:
  - Adds responsive web landing page presenting key features, screenshots, system requirements, and platform download options for Windows and Linux.
  - Includes structured SEO metadata and FAQ schema.

### Changed
- **Modular architecture and desktop adapter separation**:
  - Splits the typed `DesktopAdapter` into dedicated domain adapters (`AuthAdapter`, `FoldersAdapter`, `SyncAdapter`, `SettingsAdapter`, `LoggingAdapter`, `UpdaterAdapter`).
  - Routes all Tauri plugins through desktop adapter seams for clean testing and boundary isolation.
  - Decouples Watched Folder registration behind a dedicated `WatchedFolderRegistration` application module.
  - Separates active synchronization status cards from Watched Folder management in the UI.
  - Decomposes synchronization processor stages and clarifies stage outcome contracts (`Completed`, `Failed`, `Cancelled`, `Skipped`).
  - Cleans up unused backend helpers, legacy font assets, obsolete CSS selectors, and unused locale keys.

### Fixed
- **Removable media and remount recovery**:
  - Preserves sync progress and restarts failed sync jobs automatically upon removable volume reconnection.
  - Improves removable volume listener concurrency and guards volume event subscriptions.
  - Resolves Escape key handling on removable volume prompt dialogs.
- **Database integrity and queue performance**:
  - Bounds durable queue windows and optimizes queue sequence allocation and indexing.
  - Ensures folder settings and queue records survive database migrations and salvages.
  - Safely discards SQLite connections when foreign key restoration fails.
- **Synchronization and IPC stability**:
  - Throttles progress IPC event emissions to reduce UI update pressure during rapid file transfers.
  - Isolates folder cancellations during active synchronization to avoid aborting unaffected folders.
  - Prevents race conditions during synchronization pause, queue drain, and runner handoff.
  - Unifies error reporting across sync runners and surfaces clear error states in the frontend.
  - Normalizes and validates Immich path segments and timestamps (h23 format).
- **User interface and accessibility**:
  - Enhances modal dialog accessibility with proper focus trapping and ARIA attributes.
  - Retains folder exclusion rule drafts when saving fails.
  - Redacts sensitive credentials from UI error and retry logs.
  - Resets frontend user session state completely upon logout.
- **Store certification & security dependencies**:
  - Aligns Microsoft Store MSIX manifest by removing redundant `broadFileSystemAccess` capability.
  - Updates `zip` crate to 9.3.0 and `tauri-plugin-updater` to 2.14.1 (mitigating CVE-2026-115904, CVE-2026-115905, CVE-2026-115906).
  - Updates `rustls` to 0.23.46 and overrides `esbuild` for `svelte-i18n` security advisories.

## [0.22.0] - 2026-10-25

### Added
- **Microsoft Store distribution and MSIX packaging**:
  - Adds build-time release-channel selection (`LYMIC_RELEASE_CHANNEL=store` or `github`).
  - Completely omits and disables in-app update checks, dialogues, and GitHub updater calls when running in Microsoft Store mode.
  - Adds automated packaging script producing Windows x64 MSIX packages with the reserved Store identity (`xXRoxXeRXx.Lymic`) via Windows SDK `makeappx.exe`.
  - Adds manifest and package integrity validator ensuring identity, architecture, version, and restricted capabilities (`runFullTrust`, `broadFileSystemAccess`) match requirements.
  - Extends GitHub Actions release workflow to build, package, validate, and attach the Store MSIX package alongside existing GitHub NSIS installers.
  - Deploys public privacy and support pages on GitHub Pages (`https://xxroxxerxx.github.io/lymic/privacy/` and `https://xxroxxerxx.github.io/lymic/support/`).
  - Documents localized German and English Microsoft Store listing metadata, screenshot assets, and Partner Center capability justification.

### Fixed
- **Sync cancellation responsiveness**:
  - Prioritizes cancellation tokens in sync processor `await_or_cancellation` select branch to prevent race conditions during immediate aborts.

## [0.21.0] - 2026-10-18

### Added
- **Immich user profile avatar**:
  - Displays the authenticated user's profile avatar in the sidebar.
  - Caches user avatars locally in the application data directory with isolated namespaces per user and server.
  - Cleans up cached avatar files on logout and falls back gracefully to the standard user icon when no avatar is set, during loading errors, or when rate-limited.
- **Typed desktop adapter**:
  - Introduces a centralized, typed `DesktopAdapter` interface and composition root in the frontend to decouple controllers and UI components from direct Tauri invoke calls, improving testability and code organization.

### Changed
- **Synchronization service architecture**:
  - Encapsulates sync coordination and lifecycle management into an isolated `SynchronizationService` in the Rust core, keeping Tauri command adapters thin.
  - Extracts per-run processing (`ProcessorRunner`) and standardizes queue runner processor outcomes for cleaner lifecycle handling and resilient queue progression.
  - Aligns lifecycle state transitions and prevents regressions when stopping, pausing, or canceling synchronizations.

## [0.20.0] - 2026-10-16

### Added
- **Modernized sidebar navigation**:
  - Replaces top tabs with a persistent desktop sidebar providing seamless switching between Synchronization, Settings, Logs, and Info views.
  - Dedicated full-page view for activity logs with filtering and clearing controls.
  - Dedicated Info view displaying application version, repository links, and issue tracker shortcuts.
- **External link browser opening**:
  - Integrates `tauri-plugin-opener` to open documentation, repository, and issue tracker links directly in the system's default browser.

### Changed
- **Unified desktop styling and design overhaul**:
  - Restyled login screen with clear input borders, field icons, and toggleable password/token visibility.
  - Redesigned target album selection modal, update notification dialog, removable volume setup prompt, and failed syncs modal with cohesive rounded containers, backdrop blur, and modern typography.
  - Renamed Dashboard view to Synchronization view and simplified the active synchronization status card layout.
  - Reorganized Settings view with account and logout controls positioned at the bottom, and widened the parallel upload selector to prevent chevron overlap.
  - Added localized text labels to folder action buttons.

### Fixed
- **Watched folder removal confirmation**:
  - Requires explicit user confirmation before removing watched folders to prevent accidental unwatching or transfer cancellations.

## [0.19.1] - 2026-10-15

### Fixed
- **Linux and macOS build compilation**:
  - Allow dead code on `VolumeEvent` variants on platforms where removable media monitors are currently stubbed.
  - Fix Clippy unit struct instantiation for macOS and Linux removable media monitors.

## [0.19.0] - 2026-10-15

### Added
- **Removable media support for watched folders**:
  - Discovers removable volumes such as SD cards and USB drives across Windows, macOS, and Linux, and notifies users when new media is inserted.
  - Adds a first-time setup prompt to configure removable storage as a watched folder with optional target album selection.
  - Recognizes remounted media by volume UUID/label and automatically remaps watched folder paths when drive letters or mount points change.
  - Adds resilient disconnection handling: pauses active uploads without corrupting sync state or marking missing files as deleted, and resumes pending items when the volume reconnects.
  - Automatically identifies and binds removable volumes when adding folders manually.
- **Target album assignment for watched folders**:
  - Allows watched folders to be mapped to an Immich target album so newly uploaded assets and server duplicates are automatically added to the selected album.
  - Enables creating new Immich albums directly within the folder setup dialog.
  - Displays assigned target album names in the watched folders list.
- **Shared locale catalog**:
  - Centralizes supported locale definitions and metadata into a shared catalog used across the frontend.

### Fixed
- **Update dialog positioning**:
  - Centers the update notification dialog on screen.
- **Album assignment error handling**:
  - Finalizes album assignment failures cleanly without silent fallbacks or orphaned queue records.
- **SQL path prefix matching**:
  - Hardens path prefix queries against substring collisions between sibling directory names.

## [0.18.0] - 2026-10-13

### Added
- **Multi-language internationalization (i18n)**:
  - Expands language support from English and German to 10 languages, adding Arabic, Bengali, French, Hindi, Portuguese, Russian, Spanish, and Simplified Chinese.
  - Fully localizes activity logs, tray menus, and backend system notifications across all supported languages.
- **Cloudflare Access service token support**:
  - Adds optional Cloudflare Access authentication (`CF-Access-Client-Id` and `CF-Access-Client-Secret`) headers for environments behind Cloudflare Zero Trust tunnels.
- **Loopback and local HTTP login**:
  - Allows opt-in connection to local unencrypted HTTP instances on localhost and private loopback addresses with explicit confirmation.
- **API key guidance dialog**:
  - Adds an in-app help modal on the login screen guiding users through creating and copying Immich API keys.

### Fixed
- **Database migration checksum recovery**:
  - Automatically heals and rebuilds corrupted or mismatched migration state while preserving watched folder configurations.
- **Interactive UI controls and layout**:
  - Organizes advanced login options into dedicated collapsible controls.
  - Ensures correct pointer cursor display and focus indicators on interactive buttons, selects, and dialog toggles.
  - Constrains the autostart toggle thumb within bounds.

## [0.17.0] - 2026-10-11

### Added
- **Dedicated page controllers**:
  - Decomposes the frontend into modular controllers (`auth`, `sync`, `preferences`, `update`, and `feedback`) with unit test coverage.

### Fixed
- **Atomic upload state reconciliation**:
  - Updates media unit state and sync item records atomically in a single database transaction upon upload completion.
- **Audit memory bounds**:
  - Bounds the in-memory audit queue with item and byte limits to prevent excessive memory usage under high event volume.
- **Ambiguous watcher event reconciliation**:
  - Recognizes ambiguous filesystem watcher events and triggers directory rescans instead of queueing unverified events.
- **Immich response error propagation**:
  - Correctly surfaces network failures and truncated error bodies during Immich API requests.
- **XMP sidecar directory caching**:
  - Caches directory scans to avoid redundant disk reads when discovering companion sidecars.
- **Database facade encapsulation**:
  - Narrows internal module re-exports to enforce clean architectural boundaries.
- **NSIS installer branding**:
  - Configures the custom application icon for the Windows NSIS installer.

## [0.16.5] - 2026-10-11

### Fixed
- **Login-screen startup deadlock**:
  - Releases the locale mutex before refreshing localized tray labels, preventing a non-reentrant lock from blocking the WebView during startup.

## [0.16.4] - 2026-10-11

### Changed
- **WebView startup diagnostics**:
  - Records secrets-free audit phases for each initial frontend operation, including update, authentication, autostart, and listener setup.

## [0.16.3] - 2026-10-11

### Fixed
- **Release pipeline lint failure**:
  - Uses `inspect_err` for tray setup diagnostics so the Rust Clippy check passes with warnings denied.

## [0.16.2] - 2026-10-11

### Fixed
- **Windows startup hang**:
  - Schedules main-window display on the Tauri event loop instead of calling native window APIs during setup.
  - Adds tray, event-loop, and window-display audit phases to precisely diagnose any remaining startup stalls.

## [0.16.1] - 2026-10-11

### Fixed
- **Deferred startup window display**:
  - Defers revealing the main window until database initialization, background recovery, and system tray setup complete successfully.
- **Startup phase diagnostics**:
  - Records granular audit events across startup phases to trace initialization progress and diagnose startup failures.

## [0.16.0] - 2026-10-10

### Added
- **Tray pause and resume controls**:
  - Allows pausing and resuming active synchronizations directly from the system tray menu.

### Changed
- **Compact tray transfer status**:
  - Formats the tray tooltip to present remaining item count, transferred size, upload speed, and ETA in a compact single line.

### Fixed
- **Payload-scaled upload timeouts**:
  - Dynamically scales HTTP upload timeouts based on file size to prevent timeouts on large media files or slower connections.
- **Silent autostart launch**:
  - Launches minimized to the system tray on system autostart for authenticated users without showing the main window upfront.
- **Queue state synchronization**:
  - Keeps tray actions synchronized with queue state and prevents redundant or conflicting sync triggers while paused.

## [0.15.0] - 2026-10-10

### Added
- **Signed in-app updates**:
  - Checks GitHub Releases silently at startup and manually from Settings.
  - Shows release notes and download progress before an explicitly confirmed install and restart.
  - Blocks update installation while synchronization is running or paused.
  - Produces signed updater artifacts and update metadata for published GitHub releases.

## [0.14.0] - 2026-10-10

### Added
- **Sync transfer speed and ETA statistics**:
  - Calculates real-time upload speed and remaining time estimation during active synchronizations, displayed both on the dashboard and in the system tray tooltip.

### Changed
- **Quieter activity logging**:
  - Removed redundant pipeline initialization messages from the UI activity log during sync execution.

### Fixed
- **Sync cancellation on folder removal**:
  - Cancels active and scheduled sync runs and removes queued assets when a watched folder is removed from configuration.
- **Hidden file and directory skipping**:
  - Automatically filters out dot-directories, dotfiles, and Windows hidden-attribute files or folders during filesystem scanning.
- **Pre-epoch file modification timestamp caching**:
  - Correctly preserves negative UNIX epoch timestamps in the hash cache for photos with metadata dated prior to 1970.
- **Detailed network error cause reporting**:
  - Preserves underlying network and HTTP error details during Immich server connectivity checks and media uploads.
- **Dynamic bundled app version display**:
  - Retrieves and displays the runtime application version in the UI footer using the Tauri API.

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
