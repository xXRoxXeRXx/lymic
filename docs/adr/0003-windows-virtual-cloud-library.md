---
status: proposed
---

# Read-only Windows Virtual Library via Cloud Files API

On Windows, Lymic plans to provide an optional, read-only Virtual Library under `%USERPROFILE%\Pictures\Lymic` using the Windows Cloud Filter API (`cldapi.dll`), projecting Remote Assets (including paired image and Live Photo Video Virtual Placeholders) as on-demand Virtual Placeholders under canonical date paths (`All Photos/YYYY/MM Month/<asset>`) and album directories (`Albums/<album>/<asset>`). Explorer delete, move, and write operations on virtual projections will be denied (`STATUS_ACCESS_DENIED`), keeping asset lifecycle, uploads, and Target Album membership strictly within Watched Folders and Synchronization. Thumbnails will be served by a lightweight COM `IThumbnailProvider` DLL registered under `SyncRootManager` in the Windows registry without Sparse Packages or commercial code-signing certificates.

## Considered Options

- **Bidirectional read-write virtual root with Explorer drag-and-drop ingestion**: Allow pasting files directly into Explorer folders and uploading them to Immich. Rejected because detecting Live Photo pairs via arbitrary timing windows (e.g. 10-second debounce) is fragile, Explorer file locking creates race conditions, and mixing ad-hoc writes with the Watched Folder queue violates Lymic's Synchronization architecture.
- **Multi-view projection including Persons and Places**: Project virtual hierarchies for `Persons/<person>/` and `Places/<country>/<city>/`. Rejected because multiple NTFS placeholders for the same Remote Asset cause placeholder explosion, redundant metadata tracking, complex path sanitization for person/place names, and cache confusion.
- **Sparse Package with Windows App Identity and in-process Explorer extensions**: Package Lymic with a Sparse Package manifest and sign it with a Windows code-signing certificate to declare modern shell extension handlers. Rejected because public releases would be blocked without an expensive commercial code-signing certificate, and hosting custom COM shell extensions in `explorer.exe` risks desktop shell instability. A minimal COM thumbnail provider registered directly in the registry under `SyncRootManager` achieves cached thumbnail rendering without packaging overhead.
- **Permitting standard Explorer file deletion**: Allow the user to press Delete in Explorer to remove Virtual Placeholders or directories. Rejected because Windows Explorer does not differentiate between local cache eviction and remote deletion. Deletion attempts are synchronously blocked with `STATUS_ACCESS_DENIED` to prevent accidental loss of Remote Assets; users manage local disk space through native Cloud Files commands ("Free up space") or Immich.

## Consequences

- Not yet implemented on `master` or shipped in current builds; this remains a proposed, Windows-only capability (unavailable on macOS and Linux).
- When implemented, it will add a Windows-only `cloud_files` module in Tauri and a minimal companion COM DLL (`lymic-shell-ext`) registered via the Windows installer.
- Incremental reconciliation will run on application startup, periodically (every 10 minutes), and via a manual "Refresh Library" action in settings, pruning locally cached Virtual Placeholders when assets are removed remotely.
- Explorer will provide native on-demand Hydration, cloud status icons, and standard "Free up space" / "Always keep on this device" context menu actions out of the box, while remaining strictly separate from Synchronization.
- Disabling the Virtual Library or logging out will deregister the sync root (`CfUnregisterSyncRoot`) and clean up the virtual directory structure.
