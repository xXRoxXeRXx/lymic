---
status: proposed
---

# Read-only Windows Virtual Library via Cloud Files API (Store channel only)

On Windows, Lymic plans to provide an optional, read-only Virtual Library under the user's Pictures folder (`<Pictures>\Lymic`, resolved through the Known Folder API) using the Windows Cloud Files API. It projects Remote Assets as on-demand Virtual Placeholders under canonical date paths (`All Photos/YYYY/MM Month/<asset>`, `All Photos/Undated/<asset>`) and album directories (`Albums/<album>/<asset>`). The Virtual Library ships **only in the Microsoft Store (MSIX) release channel** ([ADR-0004](0004-microsoft-store-release-channel.md)). It is unavailable in the GitHub/NSIS channel, on macOS and on Linux, and there is no experimental NSIS path.

The Store package supplies Package Identity and Store signing, so the sync root is registered with `StorageProviderSyncRootManager` and the thumbnail provider is a COM server declared in the package manifest. No registry-based shell registration, installer-time COM registration or Sparse Package is needed. Callbacks, placeholder creation and Hydration use the Cloud Filter API (`cldapi.dll`).

The Virtual Library is read-only by design. This is a product decision, not an API limit: the Cloud Files API supports bidirectional sync, but Lymic keeps uploads, asset lifecycle and Target Album membership strictly within Watched Folders and Synchronization.

## Considered Options

- **Bidirectional read-write virtual root with Explorer drag-and-drop ingestion**: Allow pasting files directly into Explorer folders and uploading them to Immich. Rejected because detecting Live Photo pairs via arbitrary timing windows (e.g. 10-second debounce) is fragile, Explorer file locking creates race conditions, mixing ad-hoc writes with the Watched Folder queue violates Lymic's Synchronization architecture, and an Explorer delete would have to map to a remote deletion that Lymic never performs. May be revisited as a separate decision.
- **Multi-view projection including Persons and Places**: Project virtual hierarchies for `Persons/<person>/` and `Places/<country>/<city>/`. Rejected because multiple NTFS placeholders for the same Remote Asset cause placeholder explosion, redundant metadata tracking, and cache confusion.
- **Sparse Package with Windows App Identity and in-process Explorer extensions**: Rejected. It would need a commercial code-signing certificate for public releases and would host custom COM shell extensions in `explorer.exe`. The Store channel already provides Package Identity and signing.
- **Registry-based registration for the GitHub/NSIS installer**: Register the sync root and an unsigned thumbnail DLL in the registry under `SyncRootManager`. Rejected: an unsigned DLL loaded into `explorer.exe` is exposed to Smart App Control and antivirus blocking, and the NSIS installer is already unsigned.
- **Permitting standard Explorer file deletion**: Rejected because Windows Explorer does not differentiate between local cache eviction and remote deletion. Delete and rename of Virtual Placeholders are vetoed, and users manage disk space through "Free up space" or Immich.

## Decisions

**Availability and registration**
- The feature is gated on actual Package Identity at runtime (for example `GetCurrentPackageFullName`) in addition to `LYMIC_RELEASE_CHANNEL=store`. It is unavailable in `tauri dev` and in any unpackaged run. Developers use a locally registered MSIX build.
- The sync root path is resolved with the Known Folder API, not `%USERPROFILE%\Pictures`. Lymic detects an enclosing sync root (for example Pictures redirected into OneDrive) and asks the user to choose another folder instead of failing. The effective path is shown in settings.
- A Watched Folder may not be inside, equal to, or a parent of the Virtual Library root. Lymic rejects it with an explicit error and refuses to enable the Virtual Library while such a Watched Folder exists.

**Write behaviour**
- Delete and rename of Virtual Placeholders are vetoed. The Cloud Files API cannot prevent the user from creating new local files in the sync root; Lymic ignores them and never uploads them.

**Projection**
- Placeholders are created lazily per directory through `FETCH_PLACEHOLDERS` when Explorer first lists it. Reconciliation only diffs directories that were already populated.
- `YYYY/MM Month` uses the asset's local capture time (`localDateTime`). Assets without a usable capture time appear under `All Photos/Undated/`.
- `All Photos` matches the Photo Timeline (no Archived, Trashed or locked-folder assets). `Albums/<album>` shows what Immich returns for that Album. Trashed Assets are never projected.
- Names are sanitized to NTFS rules. A short, stable suffix derived from the Immich ID is appended only on collision (`IMG_0001 (a1b2c3).jpg`), for asset and Album names alike.
- A Live Photo is projected as two placeholders in the same directory with a shared base name (`IMG_0001.HEIC` and `IMG_0001.MOV`).

**Hydration**
- Hydration always downloads the full original, never a preview variant, and not progressively. Thumbnails come from the thumbnail provider using the Immich thumbnail and never trigger Hydration.
- Offline server, expired token or a remotely removed asset fail the read with a matching Cloud Files error, without retry loops, plus one in-app notice. A 401 offers re-login. A removed asset disappears on the next reconciliation.

**Lifecycle**
- Reconciliation runs on startup, every 10 minutes, and through a manual "Refresh Library" action in settings.
- Disabling the Virtual Library or logging out deregisters the sync root and removes everything Lymic created, including hydrated and pinned originals, after a confirmation that says so. Files that Lymic did not create and that are not placeholders stay in place, together with their folder, and the dialog reports this.
- Unhydrated placeholders only work while Lymic runs. Enabling the Virtual Library shows a hint when autostart is off.

**Module boundaries**
- A platform-neutral `virtual_library` module in the Tauri backend holds the pure projection rules (date buckets, Undated, collision suffix, NTFS sanitizing, Live Photo pairs) so they are testable on any OS.
- A Windows-only `cloud_files` module (`cfg(windows)`) holds sync root registration, callbacks, placeholder creation and Hydration, following the pattern of `removable/`.
- The thumbnail provider is a separate `lymic-shell-ext` crate in the workspace, declared in the MSIX manifest.

## Acceptance gates

This ADR stays `proposed` until prototypes confirm the assumptions that would change the design:

1. Delete and rename of Virtual Placeholders are rejected, including Explorer's Recycle Bin path.
2. The manifest-declared thumbnail provider renders thumbnails without triggering Hydration.
3. The sync root's behaviour on MSIX uninstall is known (no uninstall hook exists), and any leftover placeholders have an acceptable story.
4. A sync root nested in another sync root (for example OneDrive-redirected Pictures) is detected before registration.

Two items are implementation tasks, not gates: autostart inside the MSIX package, and any Store certification requirements specific to Cloud Files apps. The autostart task is decided (#284): the Run-key login item is not expected to work with Package Identity because HKCU writes are virtualized, so the manifest declares a `windows.startupTask` that the in-app toggle drives through the `StartupTask` API in packaged runs. Its runtime behaviour is still unverified in a registered package; see `docs/store/local-msix-build.md`.

## Consequences

- Only part of it exists, and none of it is shipped as a working Virtual Library: this remains a proposed, Windows-only, Store-only capability. What exists is a settings section (the persisted enable toggle, which has no projection effect, and the root folder; visible only when the backend reports the Virtual Library available at runtime, that is on Windows, in the `store` channel and with real Package Identity), the root folder (default `Lymic` in the Pictures Known Folder, changeable with the folder picker), the overlap rule (a Watched Folder may not be inside, equal to or a parent of the root; registration and enabling refuse it), and a platform-neutral, read-only projection of the Immich Library (`virtual_library`, listing and resolving paths with NTFS-safe unique names). The tracer bullet of the Windows adapter (`cloud_files`, #278) now consumes that projection: enabling checks for a nested sync root and refuses it, registers the sync root, creates placeholders per directory and hydrates the full original. It is written but not yet observed in Explorer ([verification record](../virtual-library-verification.md)).
- Still missing: delete and rename vetoes, reconciliation, hydration failure notices, the `lymic-shell-ext` crate and its MSIX manifest declarations. Disabling and logout now unregister the sync root and remove what Lymic created (#283, `cloud_files/cleanup.rs`); gate 3 (MSIX uninstall) is still unobserved, so that cleanup decides what is Lymic's from the placeholder state and identity before unregistering and never deletes anything else, which keeps it safe whatever Windows leaves behind.
- When implemented, it adds the `virtual_library` and `cloud_files` modules, the `lymic-shell-ext` crate, and new declarations in the MSIX manifest template used by `scripts/package-msix.ps1`.
- Explorer provides native on-demand Hydration, cloud status icons, and "Free up space" / "Always keep on this device" actions, while the feature stays strictly separate from Synchronization.
- GitHub/NSIS users get no Virtual Library. This limits the feature's reach until the NSIS channel gets a signed path, which needs its own decision.
