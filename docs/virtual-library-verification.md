# Virtual Library manual verification checklist (Windows adapter)

This is the one runnable checklist for the Windows adapter of the Virtual Library ([ADR-0003](adr/0003-windows-virtual-cloud-library.md), part of [#266](https://github.com/xXRoxXeRXx/lymic/issues/266); it replaces the per-ticket records of #278 to #284). The application map points here ([`docs/agents/application-map.md`](agents/application-map.md), "Verification"). Windows calls in `src-tauri/src/cloud_files/adapter/` and in `lymic-shell-ext` cannot be unit tested, so everything that depends on Explorer, the registry or a registered package is recorded here and only here.

**Status: no step has a kept record (log, count, version or screenshot).** What is known comes from three sources, and the Result column says which: the maintainer's field tests (below), a qualitative smoke test; the maintainer's later answer in the session on 2026-11-23, that they checked all ADR-0003 gate items individually on the Package Identity build and that everything passed ("Passed (maintainer-reported, 2026-11-23; no per-step record)"); and the gate prototype. A step that says "Not run" was named by none of them. The author of the code cannot run the checklist (Windows Server 2022 without Developer Mode cannot register an MSIX package), so nothing in this file was observed by the author. A maintainer-reported pass carries no details, numbers or versions beyond the date and "Windows 11"; to turn one into a record, re-run the step and fill in the Result.

## What is known from the field tests

Reported by the maintainer from tests of a locally registered Package Identity build (identity `Lymic.Dev`, `exe` COM server variant, Windows 11, 2026-11-23, see [`docs/store/local-msix-build.md`](store/local-msix-build.md)). Taken from the maintainer's reports, not independently re-run. The Windows build number and the Lymic commit of each test were not recorded, and no log was attached to the "everything works" report.

Observed:

- The sync root is registered (the `SyncRootManager` key exists with `NamespaceCLSID`, `DisplayNameResource`, `IconResource`, `Flags=34`, `ProtectionMode` and an AUMID; in the first packaged run it had no `ThumbnailProvider` value).
- Placeholders are real: reparse tag `0x9000601a`, attributes `RECALL_ON_DATA_ACCESS | OFFLINE | SPARSE` (and `ARCHIVE`).
- Folder listing is lazy (`FETCH_PLACEHOLDERS` per directory) and Hydration of originals works, in `Albums` and in `All Photos`.
- The first packaged run found: generic file icons and no cloud status in Explorer; a Hydration storm (1393 placeholders, 6.75 GB, hydrated after a few clicks by an unknown process); a console window from the console-subsystem COM host; "Make sure Lymic is running on your PC" (`0x8007016A`) until the Virtual Library was disabled and enabled again; thumbnails not served for misses under `All Photos`. Each has a change in the review and field-report fixes (reader policy, content-indexing attribute, provider status and icon, windows-subsystem host, connection watchdog, per-directory thumbnail serving). None of these was re-checked step by step.
- In the last test the maintainer reported, in their words, that it "looks very good and everything works perfectly" with cloud symbols, thumbnails, opening files and the folder tree. This is a smoke test, not a run of this checklist: it does not say how many thumbnails appeared, whether any placeholder was Hydrated by browsing, or which of the fixes mattered.

Observed only in the gate prototype (`vl-gates-prototype`, package `Lymic.GatesProto`, Windows 11, build and date not recorded; comments on [#266](https://github.com/xXRoxXeRXx/lymic/issues/266)), not in this code:

- Explorer Delete and Rename of a placeholder raised `NOTIFY_DELETE` and `NOTIFY_RENAME` from `explorer.exe` and were vetoed with `STATUS_CLOUD_FILE_ACCESS_DENIED`.
- Registration with `com:SurrogateServer` failed with `0x80070490`; with `com:ExeServer` it succeeded. Why is unknown.
- The prototype's thumbnail provider was never invoked.

### Gate items: maintainer-reported pass, no per-step record

On 2026-11-23 the maintainer answered in the session that they checked each of the following individually on the Package Identity build and that everything passed. They gave no per-step details, numbers or logs, and the Windows build and Lymic commit were not recorded. These are the evidence for the "Gate results" of ADR-0003, which was accepted on that basis:

- Shift+Delete and dragging a placeholder to the Recycle Bin are vetoed (F5, F6). Single Delete and Rename were seen vetoed in the prototype (F1, F3); they are not listed in the confirmation, so F1 to F4 stay "prototype only".
- A new local file in the folder is ignored and never uploaded (F13, F14).
- A sync root nested in another sync root is detected before registration (A7).
- MSIX uninstall behaviour (gate [#277](https://github.com/xXRoxXeRXx/lymic/issues/277), I1). What Windows leaves behind was not written down, so the guidance for a leftover folder after an uninstall stays as it is.
- Thumbnails without Hydration (E3).
- Offline and 401 notices (D1, D2, D5, D6).
- Disable and logout cleanup (H1 to H5, H12 to H14).
- The autostart `StartupTask` (I2, I3).

Not named in the confirmation and therefore not individually recorded: the checks of the reader log (C6, E4), the localized texts, the edge cases of the cleanup (H6 to H11, H15, H16), the failure variants D3, D4, D7 to D12, reconciliation (G1 to G17) and the remaining steps marked "Not run".

## How to run

Prepare a local MSIX build as in [`docs/store/local-msix-build.md`](store/local-msix-build.md), start Lymic from its Start menu entry (never the raw `.exe`) and use a throw-away Immich account and a folder with nothing valuable in it. Keep the Immich access log (or a proxy) open.

| Item | Value |
| --- | --- |
| Windows edition and build | |
| Package identity and version (`Get-AppxPackage`) | |
| COM server variant (`exe` or `surrogate`) and handler variant | |
| Immich server version | |
| Lymic commit | |
| Pictures redirected into OneDrive on this machine | |
| Explorer view mode (thumbnails need Medium icons or larger) | |
| Date and who ran it | |

Where the evidence is, with `<family>` the package family name (`Get-AppxPackage -Name Lymic.Dev`):

| What | Where |
| --- | --- |
| App diagnostic log: every listing, lookup, callback, `FETCH_DATA` with the reader, connection and watchdog events, panics (bounded at 512 KiB, previous file kept as `.old`) | `%LOCALAPPDATA%\Packages\<family>\LocalState\lymic-virtual-library.log` |
| Thumbnail provider log: `CreateInstance`, `Initialize`, `GetThumbnail` | `%LOCALAPPDATA%\Packages\<family>\LocalState\lymic-shell-ext.log` |
| Thumbnail cache and miss requests | `%LOCALAPPDATA%\Packages\<family>\LocalState\shell-thumbnails` |
| Audit log (the `logs` folder of the app data directory, which a package redirects) | `%LOCALAPPDATA%\Packages\<family>\LocalCache\Roaming\com.lymic.desktop\logs` |
| Registration | `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s` |

Result column: write PASS, FAIL or the observed behaviour, and a note for anything unexpected. "Field" means covered, in part, by the field tests above; it is not a PASS of the step. "Passed (maintainer-reported ...)" is the confirmation described above. Date / Windows column: the date and the Windows version of the run that produced the result.

If a step fails, send `lymic-virtual-library.log` and `lymic-shell-ext.log` from the same test. No `FETCH_PLACEHOLDERS` or `FETCH_DATA` line for a failing click together with `connection is lost` or no `connected the sync root` line points at the connection; `listing '...' failed: NotFound: the timeline reports N asset(s)` points at the month search window; `listed '...': 0 entries` or `skipped` lines show an empty plan; `gave up on` lines show unresolvable thumbnail requests.

## A. Availability, enabling and registration

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| A1 | Run the unpackaged executable (`npm run tauri dev`, or `lymic-desktop.exe` from `target`). Open Settings. | No Virtual Library section. | Not run | |
| A2 | Start the registered package from its Start menu entry. Open Settings. | The section is shown, disabled, with the folder `<Pictures>\Lymic` (Known Folder, also when Pictures is redirected). | Field: section shown and enabled (implied by the test); default folder not recorded | 2026-11-23, Win 11 |
| A3 | Make sure `<Pictures>\Lymic` is not inside another sync root. Switch the toggle on. Restart Lymic. | No error. The folder is created. The toggle stays on after the restart. | Field: enabling registered the sync root; persistence across restart not recorded | 2026-11-23, Win 11 |
| A4 | `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s` | An entry `Lymic.VirtualLibrary!<your SID>!Library` whose `UserSyncRoots` value is the folder. Record every value, `IconResource` and any `ThumbnailProvider` value (expected `{9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41}`) included. If it is missing, the handler was not taken from the manifest: stop and report. | Field: entry exists; first run had no `ThumbnailProvider` value; `UserSyncRoots` and the values after the later fixes not recorded | 2026-11-23, Win 11 |
| A5 | Inspect `AppxManifest.xml` of the registered package. | It declares the `windows.cloudFiles` thumbnail handler and the `com:ExeServer` `LymicShellExt.exe`; `Register` in A3 succeeded with it. | Field: `Register` succeeded with the `exe` server; manifest not inspected; surrogate variant not run in this code (the prototype's failed with `0x80070490`) | 2026-11-23, Win 11 |
| A6 | While enabled, try Change folder. | Refused with a localized message asking to disable the Virtual Library first. | Not run | |
| A7 | Disable. Choose a folder inside OneDrive (or any other cloud sync folder) with Change folder, then switch on. Also try a Pictures folder redirected into OneDrive. | Refused with a localized message that asks for another folder and names the other sync root. The toggle stays off. No new `SyncRootManager` entry and no folder created. **Gate 4.** | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| A8 | Choose a folder that contains another sync root (for example your user profile folder, which contains `OneDrive`). Switch on. | Refused the same way. Nothing is registered. | Not run | |
| A9 | Repeat A7 in German and Arabic. | The message is in the chosen language, the path is readable, the right-to-left text is not garbled. | Not run | |
| A10 | If it can be told, note whether the registry or `CfGetSyncRootInfoByPath` caught A7. | Record. Reading `HKLM\...\SyncRootManager` from inside the package has not been confirmed to see other providers' roots. | Not run | |
| A11 | Try to add the Virtual Library folder, a folder inside it and its parent as a Watched Folder; then add an ordinary Watched Folder and run a Synchronization. | The three are refused with the overlap message. The ordinary Watched Folder synchronizes as before and never reads the Virtual Library folder (Process Monitor on the folder if possible). | Not run | |

## B. Browsing and projection

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| B1 | Open the folder in Explorer. | `All Photos` and `Albums` with cloud status icons, nothing else; the folder is in the navigation pane as a sync root with Lymic's icon; the Status column shows a state. | Field: cloud symbols and the folder tree reported fine in the last test; the first run had generic icons and no cloud status; navigation pane entry and Status column not individually checked | 2026-11-23, Win 11 |
| B2 | Note the Immich request count, then open `All Photos`. | The years appear. Only that listing (`timeline/buckets`) was requested; no month and no album was listed. | Field: lazy `FETCH_PLACEHOLDERS` per directory observed; request counts not recorded | 2026-11-23, Win 11 |
| B3 | Open a year, then a month. | Months, then files. Names, sizes and dates match the Immich web app; the month is the capture's local month; only that month was searched. | Field: listing works; the comparison with the web app was not recorded | 2026-11-23, Win 11 |
| B4 | Find a Live Photo and a name collision. | The Live Photo is two files with one base name (`IMG_0001.HEIC`, `IMG_0001.MOV`). A collision shows ` (<6 characters>)` on every colliding name. | Not run | |
| B5 | Open `All Photos\Undated`, `Albums` and one album. | Assets without a capture time are under `Undated`. Owned and shared albums are folders showing their assets. | Field: `Albums` and `All Photos` listed; `Undated` and shared albums not individually reported | 2026-11-23, Win 11 |
| B6 | After a fresh enable or Refresh Library, look for a `1970` folder and `Garfild.3gp`. | No `1970` folder unless the library holds a photo of January 1970 that is not at exactly `1970-01-01T00:00:00`; `Garfild.3gp` (the exact epoch, Immich's "no capture time") is in `Undated`. Field: not run. | Not run | |
| B6a | Upload a photo (or set a capture date in Immich) of 15 January 1970 and refresh; open `All Photos`. | `1970` exists and `1970 January` holds that photo; `Undated` still holds `Garfild.3gp`. Without such a photo `1970` is absent again after the next refresh. NOT YET RUN: the projection is tested against a fake Immich server; whether Immich really reports a real 1970 capture time as its local time was not observed. | Not run | |
| B7 | Open an album or month with more than 1000 assets. | All assets appear (placeholders are handed over in batches of 1000). | Not run | |
| B8 | Open `All Photos`, a year and a month with many files; read the log. | `FETCH_PLACEHOLDERS` and `listed '<path>': N entries` lines for each; the folders show their items. | Not run | |
| B9 | Inspect the Immich access log for the whole session. | Only `GET` requests and the `POST` metadata searches. No write to any asset. | Not run | |

## C. Placeholders, Hydration and the connection

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| C1 | `Get-Item` / `fsutil reparsepoint query` on a placeholder. | Reparse tag `0x9000601a`, attributes `RECALL_ON_DATA_ACCESS`, `OFFLINE`, `SPARSE`. After a fresh enable also `NotContentIndexed`. | Field: tag and attributes observed in the first run; `NotContentIndexed` not recorded | 2026-11-23, Win 11 |
| C2 | Open a placeholder (Photos or Paint). | The file opens; its status icon becomes locally available. | Field: opening originals works in `Albums` and `All Photos` | 2026-11-23, Win 11 |
| C3 | Open a file under `All Photos` and one under `Albums`; read the log. | Both open and both have a `FETCH_DATA` line. A `0x8007016A` error is explained by the lines before it (`connection is lost`). | Field: opening worked in the last test; whether the connection was lost again was not recorded; the first run had `0x8007016A` until re-enable | 2026-11-23, Win 11 |
| C4 | Copy a hydrated file elsewhere and compare its SHA-256 with the original downloaded from the Immich web app. | Identical. | Not run | |
| C5 | Open a video of more than 1 GiB; watch Lymic's memory. | It plays or copies completely; memory does not grow with the file size. | Not run | |
| C6 | Browse and click as in the first run, then read `FETCH_DATA` lines. Count hydrated placeholders (`Get-ChildItem` attributes). | Only files someone opened are hydrated. Reads by `SearchProtocolHost.exe`, `SearchFilterHost.exe`, `SearchIndexer.exe` and `dllhost.exe` from `System32`/`SysWOW64` are `refused`. If another process hydrates, its name is in the log: add it to `readers.rs`. | Not run (the first run hydrated 1393 placeholders, 6.75 GB, by an unknown process) | |
| C7 | Double-click a photo and copy a file in Explorer. | Both work (`explorer.exe` and unknown readers are not refused). | Not run | |
| C8 | Quit Lymic and start it again with the toggle on. Without touching Settings, open an unhydrated placeholder. | `connected the sync root` is in the log; the file hydrates. With Lymic stopped, a click gives Explorer's error (expected). | Not run | |
| C9 | Switch the toggle off (confirm), then on again for the same folder. | Re-enabling is not mistaken for a nested root and works. | Not run | |
| C10 | Look for the watchdog: end nothing, just leave Lymic running a few minutes with the log open. | `connection is lost` never appears while nothing happened; if it does, it is followed by a successful reconnect. | Not run | |
| C11 | Open a large video (several hundred MB), seek to the end at once, then back to the start; read the `FETCH_DATA` lines (offset and length) and the Immich access log. | Policy `Progressive`: the video starts within seconds, not after the whole download. Each `FETCH_DATA` has the offset and length the player read; the Immich log shows `GET /assets/<id>/original` with `Range: bytes=<n>-` answered `206` (or `200` when a server or proxy ignores ranges, then the app skips the bytes it does not need), and each download ends about 32 MiB after the range. The seek to the end does not wait for the bytes in between. Record the offsets and lengths. NOT YET RUN. | Not run | |

## D. Failures and notices (#281)

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| D1 | Stop (or block) the Immich server. Open an unhydrated placeholder and time it. | Fails within about 15 seconds with a network-style error (`STATUS_CLOUD_FILE_NETWORK_UNAVAILABLE`); Explorer does not hang; the access log shows one attempt, no retry loop. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| D2 | Same situation, Lymic window open on any view. | Exactly one notice: a banner and one activity log entry ("Could not open ... because Immich could not be reached."), no "Sign in again" button. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| D3 | Still offline: open ten different placeholders, then one of them several times. | Every open fails promptly; no second notice and no second log entry. | Not run | |
| D4 | Dismiss the banner, restore the server, open a placeholder (it hydrates), stop the server and open another. Also stay offline for more than five minutes and fail again. | A new notice after the success; one more after five quiet minutes. | Not run | |
| D5 | Restore the server. Revoke the API key (or log in with a revoked key) and open an unhydrated placeholder. | Fails promptly (`STATUS_CLOUD_FILE_AUTHENTICATION_FAILED`), no loop; one notice "... your Immich sign-in is no longer valid" with "Sign in again". Record the exact error text. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| D6 | Press "Sign in again". Enter a valid key; repeat once with "Not now". | The login form for the same server appears; nothing was logged out and the Virtual Library stays enabled and untouched; after a valid key the same placeholder hydrates; "Not now" closes the form and changes nothing. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| D7 | Delete (or trash and empty) a photo in Immich without refreshing. Open its unhydrated placeholder. | Fails promptly with a "file not found" style error (record the text); one notice naming the file. Record whether the platform accepts `STATUS_OBJECT_NAME_NOT_FOUND`; if not, the fallback is `STATUS_CLOUD_FILE_INVALID_REQUEST` in `ntstatus` in `adapter/callbacks.rs`. | Not run | |
| D8 | After D7, press Refresh Library. | The placeholder is gone. | Not run | |
| D9 | Start a large download, then close the opening program (or cancel the copy). | The download stops within a moment; the log says a download was cancelled by the reader; no notice. If it says a cancel arrived for a download not in flight and the download continues, the transfer keys do not match: record it. | Not run | |
| D10 | Take Immich offline in the middle of a large download. | Fails within about 30 seconds (no data), network error, one notice; the partial file is not marked complete. | Not run | |
| D11 | Switch the language to German and Arabic and trigger the notices of D2, D5 and D7. | Banner, log entry, "Sign in again", "Not now" and the login explanation are in the chosen language. | Not run | |
| D12 | With the server stopped, open an unbrowsed folder twice, then restore the server and open it again. | Each listing fails (the same notice episode, not a second one); after the server is back the listing works and the folder was left unpopulated in between. | Not run | |

## E. Thumbnails (#282, gate 2)

The provider is `lymic-shell-ext`, declared in the manifest; it answers from a cache the app fills and must never Hydrate. Use a library of real images, the view set to Large icons.

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| E1 | Open a never-opened month of 20 or more photos. | Thumbnails appear for the unhydrated placeholders within a few seconds. Record how many of the first screen have one. | Field: thumbnails shown in the last test (qualitative); counts not recorded; the first run had none | 2026-11-23, Win 11 |
| E2 | Immich access log for E1. | `assets/<id>/thumbnail?format=JPEG` requests from Lymic; no `assets/<id>/original` request caused by browsing. | Not run | |
| E3 | In the folder of E1, check status icons and attributes. | Every file nobody opened is still a placeholder (`RECALL_ON_DATA_ACCESS`/offline): **browsing with thumbnails Hydrated nothing**. This is gate 2. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| E4 | Read `lymic-shell-ext.log` for the files of E1. | `CreateInstance`, `Initialize ok: ... identity byte(s)`, `GetThumbnail ...: served from the cache`. `nothing cached (NotCached); answered with the generic image and asked the app` means Explorer asked before the app cached it. `the cached image cannot be decoded` means the format is unreadable (WebP without a codec?). Also read the app log's `FETCH_DATA` lines for readers during E1. | Not run | |
| E5 | `reg query "HKCR\CLSID\{9a3c3e0e-5b7d-4f4d-8f0e-2d6c1f6b7a41}" /s` | Record whether the class is registered (a packaged class may not show; then skip). | Not run | |
| E6 | Task Manager during E1. | `LymicShellExt.exe` appears while thumbnails are drawn and is gone about 30 seconds after browsing stops; no console window. | Not run | |
| E7 | Scroll through a folder of more than 100 photos. | The first 48 are present at once; the rest arrive within seconds without F5 (`thumbnails of '<path>': ... continue in the background`, `thumbnail requests for '<dir>'` lines). Record how many needed F5. | Not run | |
| E8 | After a miss (grey square), wait. | The app serves the request and the item updates (`SHCNE_UPDATEITEM`) without F5. If the grey square stays, record it (a thumbnail cache reset is the workaround). Include `Garfild.3gp` (a video). | Not run | |
| E9 | Open one photo (hydrate it). | That photo shows Explorer's own thumbnail from the local content; the others stay unhydrated. | Not run | |
| E10 | Upload a new photo in Immich, Refresh Library, look at the month. | The placeholder appears and gets a thumbnail; record whether F5 was needed. | Not run | |
| E11 | Stop Immich (cached thumbnails exist) and browse an already visited folder. | Cached thumbnails still show; no hang. | Not run | |
| E12 | Switch the Virtual Library off; check `...\LocalState\shell-thumbnails`. Repeat with logout. | The directory is gone both times. | Not run | |
| E13 | If A4 or E1 fail: package with `-ComServer surrogate` (DLL built with `--features surrogate-dll`) and repeat A4, E1 and E4; only if A4 still shows no `ThumbnailProvider` value try `-CloudFilesHandlers empty`. | Record what changed. If `Register` fails (`VIRTUAL_LIBRARY_START_FAILED`, `0x80070490`) with a manifest, that declaration is the cause: record it and package without it. | Not run | |

## F. Read-only enforcement (#279, gate 1)

Use a hydrated and an unhydrated placeholder, a file and a folder, with the Immich access log open. The log lines to look for start with `virtual library: refused a ...` (delete, rename inside the root, rename out of it, rename into the Recycle Bin); which callback Shift+Delete and the Recycle Bin drag raise was not recorded.

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| F1 | Select a placeholder file, press Delete (or right-click, Delete). | Refused with an error; the file stays; log `refused a delete`. | Prototype only: vetoed with `STATUS_CLOUD_FILE_ACCESS_DENIED`; not run against this code | Prototype, Win 11, date not recorded |
| F2 | Same for a folder placeholder (a month, an album). | Refused; the folder and its content stay. | Not run | |
| F3 | Rename a file placeholder (F2). | Refused; the old name stays; log `refused a rename inside the Virtual Library folder`. | Prototype only (as F1); not run against this code | Prototype, Win 11, date not recorded |
| F4 | Rename a folder placeholder. | Refused. | Not run | |
| F5 | Shift+Delete on a placeholder. Record which callback was logged. | Refused; the file stays. If it is deleted or nothing is refused: FAIL, and a finding for gate 1. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| F6 | Drag a placeholder onto the Recycle Bin. | Refused; log `refused a rename into the Recycle Bin` or `refused a delete`; nothing in the Recycle Bin. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| F7 | Drag a placeholder to a folder outside the Virtual Library, same volume and another drive. | Refused (same volume is a rename; another volume is copy then delete). Record whether a copy was made (a copy is the user's own file and allowed). The placeholder stays. | Not run | |
| F8 | Drag a placeholder to another folder inside the Virtual Library. | Refused. | Not run | |
| F9 | Cut a placeholder (Ctrl+X) and paste it elsewhere. | Refused; the placeholder stays. | Not run | |
| F10 | Select several placeholders and press Delete. | All refused; none disappears. | Not run | |
| F11 | Restore from the Recycle Bin, if anything vetoed ever got there. | Record (only possible if F5 to F7 failed). | Not run | |
| F12 | Right-click a hydrated file, Free up space. | Still works (dehydration is not vetoed). | Not run | |
| F13 | Create a new file in the folder (New, Text Document; copy a photo into the root and into `All Photos`). | The file is created and not blocked; Lymic does not touch it and makes no Immich request for it. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| F14 | Wait two minutes, then check the Immich access log and Lymic's activity log. | The files of F13 are never uploaded or mentioned; no `POST /assets`, `PUT`, `PATCH` or `DELETE`. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| F15 | Delete or rename the file of F13 itself. | Record: it is not a placeholder, so no callback is expected and it is allowed. | Not run | |
| F16 | Make a junction (`mklink /J`) inside an ordinary Watched Folder pointing at the Virtual Library folder, then synchronize. | Record whether the scanner enters it (it does not follow symbolic links; junctions are untested). A scan that hydrates a placeholder is a FAIL. | Not run | |

Code-level checks done by reading and unit tests, not by Explorer: projection and Hydration make only `GET` requests and the read-only `POST /search/metadata` (`walking_the_whole_projection_sends_only_reads_to_immich`); nothing in `cloud_files` or `virtual_library` references the upload queue, Synchronization or Watched Folders; the overlap rule is enforced both ways (`virtual_library_root`, `watched_folders/registration.rs`, `commands/virtual_library.rs`).

## G. Reconciliation and Refresh Library (#280)

Preparation: enable on a fresh folder (directories populated before #280 are not recorded and not reconciled). Open `All Photos`, one year, one month with a few photos, `Albums` and one album. Leave one other album and one other month unopened.

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| G1 | `sqlite3 <package app data>\lymic.db "select path from virtual_library_populated_directories"` | One row per opened directory (`""` for the root, `All Photos`, `All Photos/<year>`, `All Photos/<year>/<MM Month>`, `Albums`, `Albums/<album>`), none for unopened ones, none for a directory whose listing failed. | Not run | |
| G2 | Upload a photo to Immich that belongs to the opened month and album; Settings, Virtual Library, **Refresh Library**. | The button shows "Refreshing..." and returns; the placeholder appears in the month and album (F5 if needed) with the right name, size and dates. | Not run | |
| G3 | In Immich, remove that photo from the album only. Refresh. | It leaves the album folder, stays in the month. | Not run | |
| G4 | In Immich, move a photo to the trash. Refresh. | The placeholder is gone from its month and every album; no error; nothing in the Recycle Bin. | Not run | |
| G5 | Hydrate a photo, change it in Immich so its size changes. Refresh. | New size; the local copy is dropped (cloud only); opening downloads the new original. | Not run | |
| G6 | Hydrate a photo, modify it locally (pending sync), remove the asset in Immich. Refresh. | The modified file is not deleted. | Not run | |
| G7 | Put `notes.txt` into an opened month and a folder `Mine` (with a file) into `Albums`. Refresh. | Both survive; no upload. | Not run | |
| G8 | Delete an opened Immich album containing a hydrated photo. Refresh. Repeat with a `notes.txt` placed in the album folder first. | The folder disappears with its placeholders and its row; with `notes.txt` the folder and the file stay, the placeholders go, the row stays. | Not run | |
| G9 | Remove in Immich the unopened album, add a photo to an unopened month. Refresh; watch the access log. | Neither was listed (no `search/metadata` for them); the month shows current content when first opened. | Not run | |
| G10 | Stop Immich. Refresh. Then start it and refresh again. | An error naming the refresh with the reason; nothing disappeared; works afterwards. | Not run | |
| G11 | Revoke the API key (or log out) and refresh. | An error; nothing disappeared. | Not run | |
| G12 | Quit Lymic, add a photo in Immich, start Lymic and wait about a minute. | The photo appears: the startup run works. Note how long. | Not run | |
| G13 | Add another photo and wait 10 minutes with Lymic in the tray. | It appears without any action: the timer works. | Not run | |
| G14 | Make a run slow; press Refresh twice while it runs and let the timer fall into it. | Only one run is active at any time; the presses wait and return after one follow-up run. | Not run | |
| G15 | Hydrate and pin a photo, remove it in Immich, refresh. | Record. Expected: an in-sync pinned placeholder is deleted like any other. | Not run | |
| G16 | Remove a photo in Immich and refresh; look for `refused a delete` in the log. Also try deleting a placeholder in Explorer while no refresh runs. | The removal works (Lymic's own deletion is acknowledged). If `refused a delete` appears for it, the registration of own removals does not reach the callback (check path spelling). Explorer's delete is still refused. | Not run | |
| G17 | Change the folder (disable, choose another, enable) and open a directory. | `virtual_library_populated_directories` holds only directories of the new folder. | Not run | |
| G18 | Disable the Virtual Library (confirm) while a Refresh Library run is in progress (slow Immich), then enable again. | The cleanup waits for the run and then removes the folder content; the run creates no placeholder in the folder after the cleanup looked at it. NOT YET RUN (the exclusion is unit tested with a fake guard; the timing on a real folder was not observed). | Not run | |
| G19 | Make the database refuse the write of a populated directory (lock it), open a directory, then release it and refresh. | The log and a notice report that the directory could not be recorded; after the next run the directory is reconciled like any other. NOT YET RUN. | Not run | |

Also look for: whether reading a directory placeholder's identity works (a directory that cannot be read counts as not ours and stays, which would keep a removed album's folder forever); whether reading thousands of identities per run is fast enough; whether `CF_UPDATE_FLAG_DEHYDRATE` with a new size works on a hydrated placeholder; whether `remove_dir` on a never-populated directory placeholder triggers `FETCH_PLACEHOLDERS` (it should not).

## H. Cleanup on disable and logout (#283)

Cleanup decisions are unit tested with a fake filesystem and host; the Windows calls (`adapter/teardown.rs`: `CfGetPlaceholderInfo` on an attributes-only handle, `StorageProviderSyncRootManager.Unregister`, `CfDisconnectSyncRoot`) never ran. What it assumes: a hydrated or pinned original is still a placeholder with the stored identity and the in-sync state (H4, H5); reading that state hydrates nothing and calls no Immich (H7); a user file is not a placeholder (H4, H5); unregistering and deleting work while nothing serves the folder (H5, H10). What `Unregister` does to placeholders is **not established** (the prototype saw unhydrated placeholders vanish and hydrated ones become plain files, unpackaged, Windows Server only): the cleanup scans before it unregisters, so the outcome only decides who removes the files. Record which (H5).

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| H1 | Enable. Browse to a month, open (hydrate) two photos, "Always keep on this device" on a third. Put `mine.txt` in a month folder and `top.txt` in the root; make a root folder `Mine` with a file; edit and save one hydrated photo in place. | Two photos show locally available, the third pinned. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H2 | Switch the toggle off and decline. | The dialog says hydrated and pinned originals are removed and files not created by Lymic stay with their folder. After declining nothing changed: still on, still served, files open. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H3 | Switch off and confirm. | Toggle off. A notice names the folder and lists `mine.txt`, `top.txt`, `Mine` (or its file) and the edited photo as kept. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H4 | Inspect the folder and `reg query ...SyncRootManager /s`; with Process Monitor note whether `lymic-desktop.exe` or Windows removed the hydrated files (`DeleteFile`). | The `Lymic.VirtualLibrary!<SID>!Library` entry is gone. Unhydrated, hydrated and pinned placeholders and empty Lymic folders are gone. `mine.txt` with its month folder and parents, `top.txt` with the root, `Mine` with its file, and the edited photo with its folders remain. Record who removed the hydrated files. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H5 | Open the kept files. | They open with their content (nothing truncated). | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H6 | During H3 watch the Immich access log. | No request for any original and no listing. | Not run | |
| H7 | Repeat H1 and H3 with no user files, no edited photo, no `Mine`. | No notice; the root folder is removed too. | Not run | |
| H8 | After H7: switch on, then off at once. | Both work; no notice; folder created and removed. | Not run | |
| H9 | Switch on, browse, end `lymic-desktop.exe` in Task Manager. Start Lymic, switch off and confirm. | The cleanup completes as in H4 although the previous process served the sync root. | Not run | |
| H10 | Switch on, browse, switch off and confirm, then immediately on and off again. | The second run finds nothing and does not error. | Not run | |
| H11 | Keep one hydrated file open in another program (the delete is refused) and switch off. | A notice lists that item with the Windows reason and says to turn the Virtual Library on and off again. Everything else is removed and the sync root unregistered. After closing the program, on and off removes it. | Not run | |
| H12 | Log out while enabled and decline. | The logout question also names that hydrated and pinned originals are removed; declining changes nothing. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H13 | Repeat H1, log out and confirm. | Same cleanup as H4 and the same notice as H3 (shown on the login screen); after login the Virtual Library is off and the folder not served. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H14 | Log out while the Virtual Library is off. | The plain logout question only; nothing removed from the folder. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| H15 | Repeat H3 in German and Arabic. | Dialogs and notices in the chosen language; right-to-left text and path not garbled. | Not run | |
| H16 | Switch on and off five times with Task Manager details open. | Memory does not grow by a callback table per cycle. | Not run | |

## I. Package lifecycle: uninstall (gate 3) and autostart

### I1. Uninstall (gate #277)

The maintainer reports that they checked the uninstall behaviour on the Package Identity build and that it passed (2026-11-23, no per-step record: what Windows leaves behind was not written down, so the rows below are not individually recorded). The cleanup does not rely on it. To record it, with the Virtual Library on, some photos hydrated and pinned and a user file in the folder, uninstall the package without switching it off, then record:

| Item | Observed | Date / Windows |
| --- | --- | --- |
| Is the `SyncRootManager` registration still there? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |
| Does the folder still show in Explorer with cloud icons? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |
| Do unhydrated placeholders open, fail or stay? What error? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |
| Do hydrated and pinned files still open? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |
| Can the leftover folder be deleted in Explorer? Any error? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |
| After reinstalling, enabling and disabling: is everything Lymic created removed? | Not recorded (gate reported passed) | 2026-11-23, Windows 11 |

Until it is recorded, the guidance for a user who sees a leftover Lymic folder after an uninstall is: delete it in Explorer (nothing is deleted in Immich), or reinstall Lymic and switch the Virtual Library on and off again so its cleanup runs. The CHANGELOG says the same. An in-app note is not possible: Lymic is not running after an uninstall, and no uninstall hook exists.

### I2 to I5. Autostart (#284)

The manifest declares a `windows.startupTask` (`TaskId="LymicStartup"`, `Enabled="false"`) instead of the Run-key login item. See [`docs/store/local-msix-build.md`](store/local-msix-build.md), "Autostart inside the package".

| # | Step | Expected | Result | Date / Windows |
| --- | --- | --- | --- | --- |
| I2 | Turn autostart on in Settings. | Task Manager, Startup apps lists "Lymic" as Enabled; Settings, Apps, Startup shows it. | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| I3 | Sign out and in again. | Lymic starts hidden in the tray when signed in to Immich, with the window when there are no stored credentials (depends on `ActivationKind::StartupTask`; the failure is benign). | Passed (maintainer-reported, 2026-11-23; no per-step record) | 2026-11-23, Windows 11 |
| I4 | Turn the task off in Task Manager; reopen Settings; turn it on in Lymic. | The toggle shows off; turning it on reports the "turn it back on in Windows Settings" error. | Not run | |
| I5 | With the Virtual Library on, toggle autostart off and on. | The hint "Photos you have not opened yet only work while Lymic is running" shows while autostart is off and disappears when it is on. | Not run | |

## Known gaps this checklist will not close

- The Windows error path of Hydration failures (completing `FETCH_DATA` with a failure status, `CANCEL_FETCH_DATA`) has no unit test; D1 to D12 are its only coverage. What is unit tested: the classification of Immich failures, the failure to Cloud Files status mapping, the timeouts, the episode rule, the cancel registry and the notice and re-login wiring.
- Directories populated before reconciliation existed (#280) are not recorded and not reconciled.
- Files whose size Immich does not report even after asking for the asset are left out of their directory (logged).
- Unhydrated placeholders work only while Lymic runs; the connection watchdog (`cloud_files/watchdog.rs`) is a guess at the cause of `0x8007016A` and is not proven to fix it.
- Whether automatic dehydration works (Storage Sense, cloud content) is not covered.
- Hydration policy: `Progressive` since the field test showed that `Full` starts a video only after the whole original is downloaded. The adapter serves the range the platform asks for plus a 32 MiB read-ahead (`hydration.rs`, unit tested against a fake server that honours, ignores or refuses ranges). Not observed in Explorer: the offsets and lengths of `FETCH_DATA`, two overlapping `FETCH_DATA` calls of one file, that the platform's own background reads are not refused by the reader policy (`readers.rs`, tuned while the policy was `Full`), and the real Immich `206` and the proxy case. C11 records them.
- The disable and logout cleanup deletes what the scan found before unregistering the sync root; it does not look at each file again right before deleting it. A review proposed that look, but what the platform answers for a placeholder after the sync root is unregistered is not established (ADR-0003), and a look that fails or finds a plain file would stop a cleanup that works. It was not adopted; a file the user edits in the moment between the scan and the delete is deleted.
