---
status: accepted
---

# Read-only Windows Virtual Library via Cloud Files API (Store channel only)

On Windows, Lymic provides an optional, read-only Virtual Library under the user's Pictures folder (`<Pictures>\Lymic`, resolved through the Known Folder API) using the Windows Cloud Files API. It projects Remote Assets as on-demand Virtual Placeholders under canonical date paths (`All Photos/YYYY/MM Month/<asset>`, `All Photos/Undated/<asset>`) and album directories (`Albums/<album>/<asset>`). The Virtual Library ships **only in the Microsoft Store (MSIX) release channel** ([ADR-0004](0004-microsoft-store-release-channel.md)). It is unavailable in the GitHub/NSIS channel, on macOS and on Linux, and there is no experimental NSIS path.

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
- `YYYY/MM Month` uses the asset's local capture time (`localDateTime`). Assets without a usable capture time, and those at exactly the Unix epoch instant (`1970-01-01T00:00:00`, what Immich reports for "no capture time"), appear under `All Photos/Undated/`; other assets of January 1970 stay in `All Photos/1970/01 January`.
- `All Photos` matches the Photo Timeline (no Archived, Trashed or locked-folder assets). `Albums/<album>` shows what Immich returns for that Album. Trashed Assets are never projected.
- Names are sanitized to NTFS rules. A short, stable suffix derived from the Immich ID is appended only on collision (`IMG_0001 (a1b2c3).jpg`), for asset and Album names alike.
- A Live Photo is projected as two placeholders in the same directory with a shared base name (`IMG_0001.HEIC` and `IMG_0001.MOV`).

**Hydration**
- Hydration downloads the original, never a preview variant, and progressively: the hydration policy is `Progressive`, so a reader waits only for the range it asked for. Each `FETCH_DATA` is served from the aligned start of that range (a `Range` request, falling back to the whole original and dropping the bytes before it when the server does not answer `206`) up to a read-ahead of 32 MiB past its end, then the download stops and the platform asks again for what the reader needs next. A seek in a video therefore does not wait for the bytes in between. The first field test ran with `Full`, under which the platform completes the open only after the last byte: a video started only after the whole original had been downloaded, which is why the policy was changed. The first field test with `Progressive` showed what a reader needs: Photos reads the first 64 KiB and a page at 1 MiB of the same file in parallel and gives up after about a second, so the first bytes of a range are handed to the platform as they arrive (not after a full 1 MiB block), a path resolves from the listing that created its placeholder (not by listing the directory again), and `CANCEL_FETCH_DATA` stops only the fetch of the range it names, because the platform shares one transfer key among all fetches of a file. What was not observed in Explorer: the offsets and lengths the platform asks for, overlapping `FETCH_DATA` calls of one file, and the real Immich `206` (see the verification checklist, C11). Thumbnails come from the thumbnail provider using the Immich thumbnail and never trigger Hydration.
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

The ADR was `proposed` until prototypes and a packaged build confirmed the assumptions that would change the design, and the maintainer decided to accept it (accepted 2026-11-23, see Gate results):

1. Delete and rename of Virtual Placeholders are rejected, including Explorer's Recycle Bin path.
2. The manifest-declared thumbnail provider renders thumbnails without triggering Hydration.
3. The sync root's behaviour on MSIX uninstall is known (no uninstall hook exists), and any leftover placeholders have an acceptable story.
4. A sync root nested in another sync root (for example OneDrive-redirected Pictures) is detected before registration.

Two items are implementation tasks, not gates: autostart inside the MSIX package, and any Store certification requirements specific to Cloud Files apps. The autostart task is decided (#284): the Run-key login item is not expected to work with Package Identity because HKCU writes are virtualized, so the manifest declares a `windows.startupTask` that the in-app toggle drives through the `StartupTask` API in packaged runs. Store certification requirements are not covered here.

## Gate results

The checklist is [`docs/virtual-library-verification.md`](../virtual-library-verification.md). **No gate has a kept record** (no logs, counts, Windows build or Lymic commit). Two kinds of result exist:

- **Written detail, gate prototype** (`Lymic.GatesProto`, Windows 11, comments on [#266](https://github.com/xXRoxXeRXx/lymic/issues/266)): a single Delete and a Rename of a placeholder raised `NOTIFY_DELETE` and `NOTIFY_RENAME` from `explorer.exe` and were vetoed with `STATUS_CLOUD_FILE_ACCESS_DENIED`; registration and `CfConnectSyncRoot` worked with the `exe` COM server and failed with the surrogate server (`0x80070490`); the prototype's thumbnail provider was never invoked; the nested-root check passed but not against a OneDrive-redirected Pictures folder; gate 3 was not run.
- **Maintainer-reported, no per-step record.** The maintainer's field tests of a locally registered Package Identity build (`Lymic.Dev`, `exe` COM server, Windows 11, 2026-11-23): the sync root registers, placeholders are real, listing and Hydration of originals work, and in the last reported test it "looks very good and everything works perfectly" (cloud symbols, thumbnails, opening files, folder tree), a smoke test and not a run of the checklist. In addition, the maintainer's direct answer in the session on 2026-11-23 ("Ja, alles geprüft") confirms that all gate items were checked individually on the Package Identity build and passed. It named: Shift+Delete and drag-to-Recycle-Bin veto, a new local file ignored and never uploaded, the nested sync root, MSIX uninstall behaviour, thumbnails without Hydration, offline and 401 notices, disable and logout cleanup, and autostart. It gave no details.

Per gate:

1. **Delete and rename veto, Recycle Bin path.** Single Delete and Rename: prototype only. Shift+Delete and drag-to-Recycle-Bin veto: reported passed, no record.
2. **Thumbnails without Hydration.** Thumbnails are shown in the last field test; no Hydration from browsing is reported passed, no record. The reader log was not kept.
3. **MSIX uninstall behaviour.** Reported passed; what Windows leaves behind was not written down. The cleanup does not depend on it, and the guidance for a leftover folder (delete it, or reinstall and switch the Virtual Library on and off) stands.
4. **Nested sync root.** Reported passed, no record; the prototype's check alone did not cover a OneDrive-redirected Pictures folder.

**Accepted on 2026-11-23** on the strength of the maintainer's own report above. No per-step record was kept. The checklist steps behind it (A7, D1, D2, D5, D6, E3, E4, F1 to F6, F13, F14, H1 to H5, H12 to H14, I1 to I3) can be re-run later to fill in the Result column; a failure there reopens this decision.

The first packaged runs found defects the gates did not predict: generic icons and no cloud status in Explorer, a Hydration storm (1393 placeholders, 6.75 GB hydrated after a few clicks by an unknown process), a console window from the console-subsystem COM host, "Make sure Lymic is running" (`0x8007016A`) until the Virtual Library was disabled and enabled again, and thumbnails missing for `All Photos`. They were addressed in the review and field-report fixes (reader policy, content-indexing attribute, provider status and icon, windows-subsystem host, connection watchdog, per-directory thumbnail serving). This did not change the decisions above.

## Consequences

- It is a Windows-only, Store-only capability, available only with real Package Identity. The settings section (enable toggle, root folder, Refresh Library) is visible only when the backend reports it available at runtime. The overlap rule holds in both directions: a Watched Folder may not be inside, equal to or a parent of the root.
- The platform-neutral `virtual_library` module holds the projection rules (`All Photos`, `Albums`, NTFS-safe unique names, Live Photo pairs, the exact Unix epoch instant as `Undated`), listed lazily per directory. The Windows-only `cloud_files` module holds the adapter (registration, callbacks, placeholders, Hydration with typed failures and notices, the delete and rename veto, reconciliation and Refresh Library, the connection watchdog, the reader policy, the diagnostic and listing logs, and the cleanup on disable and logout). The `lymic-shell-ext` crate is the thumbnail provider, a COM `IThumbnailProvider` declared in the MSIX manifest as a `com:ExeServer` (the `com:SurrogateServer` DLL is the packaged alternative, `package-msix.ps1 -ComServer surrogate`; why the prototype's surrogate declaration failed `Register` is unknown). It serves a cache of Immich JPEG thumbnails that the app fills when it lists a directory and never opens a placeholder's data, so by design it does not cause Hydration. See [the application map](../agents/application-map.md) and [`docs/store/local-msix-build.md`](../store/local-msix-build.md).
- Disabling and logout unregister the sync root and remove what Lymic created, including hydrated and pinned originals; files Lymic did not create stay with their folder. The cleanup decides what is Lymic's from the placeholder state and identity before unregistering and never deletes anything else, which keeps it safe whatever Windows leaves behind after an uninstall.
- Evidence of a run is in `LocalState\lymic-virtual-library.log`, `LocalState\lymic-shell-ext.log` and the audit log under `LocalCache\Roaming\com.lymic.desktop\logs` of the package.
- Explorer provides native on-demand Hydration, cloud status icons, and "Free up space" / "Always keep on this device" actions, while the feature stays strictly separate from Synchronization.
- GitHub/NSIS users get no Virtual Library. This limits the feature's reach until the NSIS channel gets a signed path, which needs its own decision.
