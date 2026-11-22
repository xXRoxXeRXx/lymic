# Virtual Library manual verification record

**Status: NOT YET RUN.** Every result below is empty on purpose.

This is the manual verification record for the Windows adapter tracer bullet ([#278](https://github.com/xXRoxXeRXx/lymic/issues/278), part of [#266](https://github.com/xXRoxXeRXx/lymic/issues/266), [ADR-0003](adr/0003-windows-virtual-cloud-library.md)): enabling the Virtual Library, registering the sync root, browsing the projected tree in Explorer, hydrating an original, and refusing a folder inside another sync root.

## What was and was not observed

The code for this flow was written and its platform-neutral parts are covered by unit tests (`cargo test`: placeholder mapping, identity encoding, nested-root decision with a fake operating system, block alignment, population and hydration orchestration with fakes, the settings commands with a fake service, the streaming original download against a local fake server). The Windows calls themselves (`src-tauri/src/cloud_files/adapter/`: `StorageProviderSyncRootManager.Register`, `CfConnectSyncRoot`, the `FETCH_PLACEHOLDERS` and `FETCH_DATA` callbacks, `CfGetSyncRootInfoByPath`, the registry read) compile but **cannot be unit tested**, and **nothing was observed in Explorer by the author of this change**. The machine it was written on is Windows Server 2022 without Developer Mode and cannot register an MSIX package, so no packaged run, no registration and no callback has ever executed. Do not read any acceptance criterion of #278 as verified until a person has run this checklist and filled in the results.

The behaviour of the Windows API that the design leans on (registration and `CfConnectSyncRoot` work, listing a folder fires `FETCH_PLACEHOLDERS` for that folder only and hydrates nothing, opening a file fires `FETCH_DATA`) was observed by the maintainer in the gate prototype (`vl-gates-prototype`), not in this code.

## Environment to record

| Item | Value |
| --- | --- |
| Windows edition and build | |
| Local MSIX registered per [`docs/store/local-msix-build.md`](store/local-msix-build.md) (identity name, version) | |
| Immich server version | |
| Lymic commit | |
| Pictures folder redirected into OneDrive on this machine | |
| Date and who ran it | |

## Checklist

Fill the **Result** column with PASS, FAIL or the observed behaviour, and add a note for anything unexpected.

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| 1 | Run the unpackaged executable (`npm run tauri dev`, or `lymic-desktop.exe` from `target`). Open Settings. | No Virtual Library section. | |
| 2 | Start the registered package from its Start menu entry. Open Settings. | The Virtual Library section is shown, disabled, with the folder `<Pictures>\Lymic` (Known Folder, also when Pictures is redirected). | |
| 3 | Make sure the folder `<Pictures>\Lymic` is not inside another sync root. Switch the toggle on. | No error. The folder is created. The toggle stays on after restarting Lymic. | |
| 4 | Check the registration: `reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager" /s` | An entry `Lymic.VirtualLibrary!<your SID>!Library` whose `UserSyncRoots` value is the folder. | |
| 5 | Open the folder in Explorer. | It shows `All Photos` and `Albums` with cloud status icons, and nothing else. The folder is listed in the Explorer navigation pane as a sync root. | |
| 6 | Before opening any subfolder, note the request count in the Immich access log (or a proxy). Open `All Photos`. | The years appear. Only the requests for that listing (`timeline/buckets`) were made; no month and no album was listed. | |
| 7 | Open a year, then a month. | The months, then the files appear. Names, sizes and dates match the Immich web app. The month is the capture's local month. Only that month was searched. | |
| 8 | Find a Live Photo and a name collision in a month or album. | The Live Photo is two files with one base name (`IMG_0001.HEIC` and `IMG_0001.MOV`). A collision shows the ` (<6 characters>)` suffix on every colliding name. | |
| 9 | Open `All Photos\Undated` and `Albums`, then one album. | Assets without a capture time are under `Undated`. Owned and shared albums appear as folders; an album folder shows its assets. | |
| 10 | In a month view with Large icons (thumbnails on), watch the Immich access log and the file status icons. | Record whether Explorer reads file content to draw thumbnails. There is no thumbnail provider yet (separate ticket); any `assets/<id>/original` request caused only by browsing is a finding, not a pass. | |
| 11 | Open a placeholder (for example in Photos or Paint). | The file opens. Its status icon changes to locally available. | |
| 12 | Copy a hydrated file elsewhere and compare its SHA-256 with the original downloaded from the Immich web app. | Identical. | |
| 13 | Open a large video (more than 1 GiB if possible). Watch Lymic's memory in Task Manager. | The video plays or copies completely. Memory does not grow with the file size. | |
| 14 | Open an album or month with more than 1000 assets. | All assets appear (placeholders are handed over in batches of 1000). | |
| 15 | Switch the toggle off, then on again for the same folder. | Re-enabling is not mistaken for a nested root and works. | |
| 16 | Quit Lymic and start it again with the toggle on. Without touching Settings, open an unhydrated placeholder. | It hydrates (the sync root is served again on app start). | |
| 17 | Switch the toggle off. Open an unhydrated placeholder, and a hydrated one. | The unhydrated one fails to open (record the Explorer error). The hydrated one still opens. The registration and placeholders stay (cleanup is #283). | |
| 18 | While enabled, try Change folder. | Refused with a localized message asking to disable the Virtual Library first. | |
| 19 | Disable the toggle. Choose, with Change folder, a folder inside OneDrive (or inside any other cloud sync folder), then switch on. | Refused with a localized message that asks for another folder and names the other sync root. The toggle stays off. No new entry appears under `SyncRootManager` and the folder is not created. | |
| 20 | Choose a folder that contains another sync root (for example your user profile folder, which contains `OneDrive`). Switch on. | Refused the same way. Nothing is registered. | |
| 21 | Repeat 19 in at least German and Arabic (language setting). | The message is shown in the chosen language, the path is readable, and the right-to-left text is not garbled. | |
| 22 | While the Virtual Library is enabled, stop the Immich server (or block it) and open an unhydrated placeholder. | The read fails with a network error and does not hang or retry in a loop. | |
| 23 | Restore the server, then give Lymic an invalid API key (log in with a revoked key) and open an unhydrated placeholder. | The read fails with an authentication error; no loop. | |
| 24 | Open a folder you have not browsed while the server is stopped, then restore the server and open it again. | The first listing fails; the second one works (a failed listing leaves the folder unpopulated). | |
| 25 | Inspect the Immich access log for the whole session. | Only `GET` requests (plus the `POST` metadata searches). No write to any asset. | |
| 26 | Inspect `AppxManifest.xml` in the registered package. | No `windows.cloudFiles` extension and no COM server declaration (the gate prototype's `Register` failed with 0x80070490 with one). | |
| 27 | On a machine where the nested-root check can be exercised only through the registry (or only through `CfGetSyncRootInfoByPath`), note which one caught it, if it can be told. | Record. Reading `HKLM\...\SyncRootManager` from inside the package has not been confirmed to see other providers' roots. | |

## Read-only enforcement (#279)

**Status: NOT YET RUN.** Same environment table as above.

What was observed and what was not. In the gate prototype (`vl-gates-prototype`) the maintainer saw, on real Windows 11, Explorer Delete and Explorer Rename of a placeholder raise `NOTIFY_DELETE` and `NOTIFY_RENAME` from `explorer.exe`, and vetoed them with `STATUS_CLOUD_FILE_ACCESS_DENIED` through `CfExecute` `ACK_DELETE` and `ACK_RENAME`. That code was ported into `adapter/callbacks.rs` (`on_notify_delete`, `on_notify_rename`) and the refusal decision lives in `cloud_files/write_guard.rs`, unit tested. **Nothing below has been run against the ported code**, and these paths have **never been observed at all**, even in the prototype: Shift+Delete, dragging to the Recycle Bin, and creating a new local file in the folder. The author of this change cannot run Explorer or register a package (Windows Server 2022, no Developer Mode). Which callback each of the unobserved paths raises is unknown; the Lymic diagnostic output (standard error, lines starting `virtual library: refused a ...`) tells which callback fired and with which target.

Do the steps with the Virtual Library enabled and the Immich access log (or a proxy) open. Use a hydrated and an unhydrated placeholder, a file and a folder.

| # | Step | Expected | Result |
| --- | --- | --- | --- |
| R1 | Select a placeholder file in Explorer, press Delete (or right-click, Delete). | Refused with an error; the file stays. Lymic logs `refused a delete`. | |
| R2 | Same for a folder placeholder (for example a month, an album). | Refused; the folder and its content stay. | |
| R3 | Rename a file placeholder (F2). | Refused; the old name stays. Lymic logs `refused a rename inside the Virtual Library folder`. | |
| R4 | Rename a folder placeholder. | Refused. | |
| R5 | Press Shift+Delete on a placeholder (permanent delete, skips the Recycle Bin). Record which callback Lymic logged. | Refused; the file stays. If it is deleted, or no refusal is logged, that is a FAIL and a finding for the ADR gate 1. | |
| R6 | Drag a placeholder onto the Recycle Bin on the desktop. | Refused; Lymic logs `refused a rename into the Recycle Bin` or `refused a delete`. Nothing appears in the Recycle Bin. | |
| R7 | Drag a placeholder to another folder outside the Virtual Library (same volume), then to another drive. | Refused (move on the same volume is a rename, to another volume it is copy then delete). Record whether the copy is made and whether the delete is refused; a copy is the user's own file and is allowed. The placeholder must stay. | |
| R8 | Drag a placeholder to another folder inside the Virtual Library (for example from one month to another). | Refused. | |
| R9 | Cut a placeholder (Ctrl+X) and paste it elsewhere. | Refused; the placeholder stays. | |
| R10 | Select several placeholders and press Delete. | All refused; none disappears. | |
| R11 | Restore from the Recycle Bin, if something that was vetoed ever got there (only possible if R5-R7 failed). | Record. | |
| R12 | Right-click a hydrated file, Free up space. | Still works (dehydrate is not vetoed). | |
| R13 | Create a new file in the Virtual Library folder (for example New, Text Document, and copy a photo into the root and into `All Photos`). | The file is created and is not blocked. Lymic does not touch it and makes no request to Immich for it. | |
| R14 | Wait at least two minutes, then check the Immich access log and Lymic's activity log. | The files from R13 are never uploaded and never mentioned. No `POST /assets`, `PUT`, `PATCH` or `DELETE` of any kind. | |
| R15 | Delete or rename the file from R13 itself. | Record: is it refused or allowed? (It is not a placeholder, so no callback is expected, and it is allowed.) | |
| R16 | Try to add the Virtual Library folder (and a folder inside it, and its parent) as a Watched Folder; then add an ordinary Watched Folder and run a Synchronization. | The three are refused with the overlap message. The ordinary Watched Folder synchronizes as before and never reads the Virtual Library folder (check with Process Monitor on the folder if possible). | |
| R17 | Make a junction or symbolic link inside an ordinary Watched Folder that points at the Virtual Library folder (`mklink /J`), then synchronize. | Record whether the scanner enters it. The scanner does not follow symbolic links by design; junctions on Windows have not been tested. A scan that hydrates a placeholder is a FAIL. | |
| R18 | Inspect the Immich access log for the whole R session. | Only `GET` requests and the `POST` metadata searches. | |

Code-level checks done by reading and by unit tests (not by running Explorer): the projection and hydration make only `GET` requests and the read-only `POST /search/metadata` (`walking_the_whole_projection_sends_only_reads_to_immich`); nothing in `cloud_files` or `virtual_library` references the upload queue, Synchronization or Watched Folders; Watched Folder registration, enabling and moving the root all refuse an overlap, in both directions (`virtual_library_root`, `watched_folders/registration.rs`, `commands/virtual_library.rs`, each with tests).

## Known gaps this record will not close

- Delete and rename of placeholders are vetoed in code (#279) but have never been run against this code; see the section below. Do not read them as passing.
- Placeholders are not refreshed after the first listing (#280); a changed Immich Library shows after the next listing only if the directory was never populated.
- No in-app notice and no re-login offer on failure (#281); the platform error is all a person sees.
- Disabling does not unregister the sync root or remove files (#283).
- No thumbnails from Immich and no autostart hint yet; unhydrated placeholders work only while Lymic runs.
- Files whose size Immich does not report even after asking for the asset are left out of their directory (logged to standard error only).
